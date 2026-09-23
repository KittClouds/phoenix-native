#[path = "engines.rs"]
pub(super) mod engines;
#[path = "prefetch.rs"]
mod prefetch;
#[path = "presentation.rs"]
pub(super) mod presentation;
use presentation::Phase;
#[path = "voices.rs"]
pub(super) mod voices;
use phoenix_audio::device::WaveOutput;
use phoenix_reader_session::*;
use phoenix_tts_native::Cancellation;
use phoenix_workspace::DocumentLease;
use prefetch::{Generator, Job};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub enum Command {
    Assign {
        segment: u32,
        character: String,
        voice: VoiceChoice,
    },
    Play,
    Pause,
    Stop,
    Previous,
    Next,
    Bookmark,
    ReturnBookmark,
}
#[derive(Clone, Default, PartialEq)]
pub struct Status {
    pub phase: Phase,
    pub message: String,
    pub voice_name: String,
    pub source_ranges: Arc<[ByteRange]>,
    pub segment: u32,
    pub segments: usize,
    pub chapter: u32,
    pub chapters: usize,
    pub seconds: u64,
    pub playing: bool,
    pub finished: bool,
    pub requested: bool,
    pub buffered_seconds: u64,
    pub rebufferings: u32,
    pub generated_during_playback: u32,
    pub device_starvations: u32,
}
pub struct Bridge {
    pub commands: SyncSender<Command>,
    pub status: Arc<Mutex<Status>>,
    cancel: Cancellation,
    join: Option<thread::JoinHandle<()>>,
}
impl Bridge {
    pub fn shutdown(mut self) {
        self.cancel.cancel();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
    pub fn send(&self, command: Command) {
        if matches!(command, Command::Stop) {
            self.cancel.cancel();
        }
        let _ = self.commands.try_send(command);
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.cancel.cancel();
        let _ = self.commands.try_send(Command::Stop);
    }
}
#[derive(Deserialize)]
pub(super) struct Config {
    #[serde(default)]
    pub worker: PathBuf,
    #[serde(default)]
    pub model: PathBuf,
    #[serde(default)]
    pub dll_directory: PathBuf,
    pub storage: PathBuf,
    #[serde(default)]
    pub voices: Vec<voices::VoiceSpec>,
    #[serde(default)]
    cast: Option<CastProfile>,
    #[serde(default)]
    pub supertonic: Option<engines::CpuConfig>,
}
impl Config {
    pub fn load_voices(&mut self) -> anyhow::Result<()> {
        if self.supertonic.is_some() {
            voices::add_cpu_voices(&mut self.voices);
        }
        voices::load_library(&self.storage, &mut self.voices)
    }
}

#[allow(dead_code)] // Shared by the standalone controller smoke.
pub fn start(workspace: PathBuf, lease: Arc<DocumentLease>, plain: bool) -> Bridge {
    start_with_voice(workspace, lease, plain, None)
}
pub(super) fn start_with_voice(
    workspace: PathBuf,
    lease: Arc<DocumentLease>,
    plain: bool,
    voice: Option<VoiceChoice>,
) -> Bridge {
    let (tx, rx) = mpsc::sync_channel(16);
    let status = Arc::new(Mutex::new(Status {
        phase: Phase::Preparing,
        message: "Opening saved revision…".into(),
        ..Default::default()
    }));
    let shared = status.clone();
    let cancel = Cancellation::default();
    let token = cancel.clone();
    let join = thread::spawn(move || {
        let result = run(workspace, lease, plain, voice, rx, &shared, &token);
        let mut status = shared.lock().unwrap();
        status.playing = false;
        status.requested = false;
        status.finished = true;
        status.phase = if result.is_err() && !token.is_cancelled() {
            Phase::Failed
        } else {
            Phase::Stopped
        };
        status.message = match result {
            Ok(()) if status.message.starts_with("Cast saved") => status.message.clone(),
            Ok(()) => "Stopped · position saved".into(),
            Err(_) if token.is_cancelled() => "Preparation cancelled. Listen when ready.".into(),
            Err(e) => format!("Reader stopped: {e:#}"),
        };
    });
    Bridge {
        commands: tx,
        status,
        cancel,
        join: Some(join),
    }
}
fn run(
    workspace: PathBuf,
    lease: Arc<DocumentLease>,
    plain: bool,
    selected_voice: Option<VoiceChoice>,
    rx: Receiver<Command>,
    shared: &Mutex<Status>,
    cancel: &Cancellation,
) -> anyhow::Result<()> {
    let config_path = workspace.with_extension("reader.json");
    anyhow::ensure!(
        std::fs::metadata(&config_path)?.len() <= 1_048_576,
        "Reader configuration is too large"
    );
    let mut config: Config = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|e| anyhow::anyhow!("Configure {}: {e}", config_path.display()))?,
    )?;
    let workspace_id = *blake3::hash(workspace.to_string_lossy().as_bytes()).as_bytes();
    let plan = if plain {
        plan_plain_chapter(
            &lease.content,
            DocumentBinding::from_lease(workspace_id, &lease)?,
        )?
    } else {
        plan_markdown(
            workspace_id,
            &lease,
            PlannerConfig {
                max_segment_bytes: 512,
                ..Default::default()
            },
        )?
        .plan
    };
    if plan
        .spec()
        .segments
        .iter()
        .any(|s| s.spoken.end - s.spoken.start > 2048)
    {
        anyhow::bail!("Paragraph too long; split the saved document into shorter paragraphs");
    }
    let snapshots = SnapshotStore::open(config.storage.join("snapshots"))?;
    snapshots.retain(&lease)?;
    snapshots.retain_plan(&plan)?;
    shared.lock().unwrap().message = "Verifying local narrator files…".into();
    config.load_voices()?;
    if config.cast.is_none() {
        config.cast =
            VoiceLibrary::open(config.storage.join("voices"))?.load_cast(&lease.content, &plan)?;
    }
    let bundle = engines::Bundles::for_plan(&config, &plan, selected_voice, cancel)?;
    let voices::PreparedVoices {
        voices,
        slots,
        table,
    } = voices::prepare(
        &config.voices,
        selected_voice,
        config.cast.as_ref(),
        &lease.content,
        &plan,
        &bundle,
    )?;
    let identity = table.identity(0)?.clone();
    let voice_binding = table.fingerprint();
    let mut id = blake3::Hasher::new();
    id.update(&plan.id());
    id.update(&voice_binding);
    let session_id = *id.finalize().as_bytes();
    let mut sessions = SessionStore::open(config.storage.join("sessions"))?;
    let (session, restored) = match sessions.load(session_id, &plan) {
        Ok(s) => (s, true),
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            (ReaderSession::new(session_id, &plan, voice_binding)?, false)
        }
        Err(e) => return Err(e.into()),
    };
    let cache = AudioCache::open(config.storage.join("cache"), 1024 * 1024 * 1024)?;
    let provider = engines::Providers::new(bundle, config.storage.clone())?;
    let mut runtime = if restored {
        ReaderRuntime::restore(
            WaveOutput::open_default()?,
            plan,
            session_id,
            identity,
            &sessions,
        )?
    } else {
        ReaderRuntime::new(WaveOutput::open_default()?, plan, session, identity)?
    };
    runtime.bind_voices(table)?;
    let mut generator = Generator::new(provider, cache);
    let mut active = false;
    let mut priming = true;
    let mut started = false;
    let mut rebufferings = 0;
    let mut generated_during_playback = 0;
    let clock = Instant::now();
    let mut generation_epoch = 1u64;
    let mut inflight = None;
    let mut selected = None;
    let mut rtf = 1.0f64;
    let mut paint_segment = None;
    let mut source_ranges: Arc<[ByteRange]> = Arc::from([]);
    let mut last_status = Instant::now() - Duration::from_secs(1);
    loop {
        if cancel.is_cancelled() {
            break;
        }
        while let Ok(command) = rx.try_recv() {
            match command {
                Command::Assign {
                    segment,
                    ref character,
                    voice,
                } => {
                    generator.cancel();
                    if runtime.state() == PlaybackState::Playing {
                        runtime.pause(true)?;
                    }
                    let narrator = selected_voice
                        .or_else(|| config.cast.as_ref().map(|c| c.narrator))
                        .unwrap_or(VoiceChoice::of(&config.voices[0].profile)?);
                    voices::assign_passage(
                        &config.storage,
                        config.cast.as_ref(),
                        &lease.content,
                        runtime.plan(),
                        segment,
                        character,
                        voice,
                        narrator,
                    )?;
                    shared.lock().unwrap().message =
                        "Cast saved - reload the saved document to apply.".into();
                    cancel.cancel();
                    break;
                }
                Command::Stop => {
                    cancel.cancel();
                    break;
                }
                Command::Play => {
                    active = true;
                    if runtime.state() == PlaybackState::Paused && !priming {
                        runtime.pause(false)?;
                    }
                }
                Command::Pause => {
                    active = false;
                    if runtime.state() == PlaybackState::Playing {
                        runtime.pause(true)?;
                    }
                    runtime.checkpoint(&mut sessions, clock.elapsed().as_millis() as u64, true)?;
                }
                Command::Bookmark => {
                    runtime.bookmark(1)?;
                    runtime.checkpoint(&mut sessions, clock.elapsed().as_millis() as u64, true)?;
                }
                Command::ReturnBookmark => {
                    if let Some(p) = runtime.session().bookmarks().get(&1) {
                        selected = Some((p.segment, p.source_frame));
                    }
                }
                Command::Previous | Command::Next => {
                    let plan = runtime.plan();
                    let current = plan.segment(runtime.session().position().segment)?.chapter;
                    let chapter = if matches!(command, Command::Next) {
                        (current + 1).min(plan.spec().chapters.len() as u32 - 1)
                    } else {
                        current.saturating_sub(1)
                    };
                    selected = plan
                        .spec()
                        .segments
                        .iter()
                        .position(|s| s.chapter == chapter)
                        .map(|s| (s as u32, 0));
                }
            }
            if matches!(
                command,
                Command::Previous | Command::Next | Command::ReturnBookmark
            ) && selected.is_some()
            {
                generation_epoch = generation_epoch
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("generation epoch exhausted"))?;
                generator.cancel();
                priming = true;
                if runtime.state() == PlaybackState::Playing {
                    runtime.pause(true)?;
                }
                runtime.checkpoint(&mut sessions, clock.elapsed().as_millis() as u64, true)?;
            }
        }
        if cancel.is_cancelled() {
            break;
        }
        if let Ok(receipt) = generator.rx.try_recv() {
            inflight = None;
            if receipt.epoch == generation_epoch {
                let audio = receipt.result.map_err(anyhow::Error::msg)?;
                if receipt.generated {
                    if runtime.state() == PlaybackState::Playing {
                        generated_during_playback += 1;
                    }
                    let observed = receipt.seconds / (audio.manifest().frames as f64 / 24000.0);
                    rtf = (rtf * 0.75 + observed * 0.25).clamp(0.1, 5.0);
                }
                if let Some((segment, frame)) = selected.take() {
                    anyhow::ensure!(receipt.segment == segment, "seek receipt segment");
                    runtime.seek(segment, frame, audio)?;
                    runtime.pause(true)?;
                } else if matches!(runtime.state(), PlaybackState::NeedsAudio(_)) {
                    runtime.attach(runtime.epoch(), receipt.segment, audio)?;
                    runtime.pause(true)?;
                } else {
                    runtime.enqueue(runtime.epoch(), receipt.segment, audio)?;
                }
            }
        }
        let target = ((3.0 + rtf * 12.0).clamp(8.0, 30.0) * 24000.0) as u64;
        let end = runtime.plan().spec().segments.len() as u32;
        if active
            && priming
            && selected.is_none()
            && runtime.state() == PlaybackState::Paused
            && (runtime.buffered_frames() >= target
                || runtime.ahead_count() == PREFETCH_SEGMENTS
                || runtime.next_prefetch_segment() >= end)
        {
            runtime.pause(false)?;
            priming = false;
            started = true;
        }
        if active && !priming {
            runtime.tick()?;
            if matches!(runtime.state(), PlaybackState::NeedsAudio(_)) {
                priming = true;
                if started {
                    rebufferings += 1;
                }
            }
        }
        if inflight.is_none() {
            let needed = selected.map(|(s, _)| s).or_else(|| {
                if !active {
                    return None;
                }
                match runtime.state() {
                    PlaybackState::NeedsAudio(s) => Some(s),
                    PlaybackState::Playing | PlaybackState::Paused
                        if runtime.buffered_frames() < target
                            && runtime.ahead_count() < PREFETCH_SEGMENTS =>
                    {
                        let s = runtime.next_prefetch_segment();
                        (s < end).then_some(s)
                    }
                    _ => None,
                }
            });
            if let Some(segment) = needed {
                let text = runtime
                    .plan()
                    .segment(segment)?
                    .spoken
                    .slice(&runtime.plan().spec().spoken)?
                    .to_owned();
                generator.submit(Job {
                    epoch: generation_epoch,
                    segment,
                    plan: runtime.plan().id(),
                    key: runtime.required_key(segment)?,
                    text,
                    voice: Arc::clone(&voices[usize::from(slots[segment as usize])]),
                    cancel: Cancellation::default(),
                })?;
                inflight = Some(segment);
            }
        }
        runtime.checkpoint(&mut sessions, clock.elapsed().as_millis() as u64, false)?;
        if last_status.elapsed() >= Duration::from_millis(100) {
            let p = runtime.session().position();
            let segment = runtime.plan().segment(p.segment)?;
            if paint_segment != Some(p.segment) {
                let mut ranges = Vec::new();
                runtime
                    .plan()
                    .project(segment.spoken, |range| ranges.push(range))?;
                source_ranges = ranges.into();
                paint_segment = Some(p.segment);
            }
            let label = if active && priming {
                "Buffering".to_owned()
            } else {
                format!("{:?}", runtime.state())
            };
            *shared.lock().unwrap() = Status {
                phase: if active && priming {
                    Phase::Buffering
                } else {
                    match runtime.state() {
                        PlaybackState::Playing => Phase::Playing,
                        PlaybackState::Paused => Phase::Paused,
                        PlaybackState::Completed => Phase::Completed,
                        PlaybackState::NeedsAudio(_) => Phase::Ready,
                        _ => Phase::Stopped,
                    }
                },
                message: format!("{label} · saved revision {} · 1×", lease.revision.0),
                voice_name: voices[usize::from(slots[p.segment as usize])].name.clone(),
                source_ranges: Arc::clone(&source_ranges),
                segment: p.segment,
                segments: end as usize,
                chapter: segment.chapter,
                chapters: runtime.plan().spec().chapters.len(),
                seconds: p.source_frame / 24000,
                playing: runtime.state() == PlaybackState::Playing,
                finished: false,
                requested: active && runtime.state() != PlaybackState::Completed,
                buffered_seconds: runtime.buffered_frames() / 24000,
                rebufferings,
                generated_during_playback,
                device_starvations: runtime.device_starvations(),
            };
            last_status = Instant::now();
        }
        thread::sleep(Duration::from_millis(10));
    }
    runtime.cancel()?;
    runtime.checkpoint(&mut sessions, clock.elapsed().as_millis() as u64, true)?;
    // Generator drop cancels and joins its owned request before stores close.
    Ok(())
}
