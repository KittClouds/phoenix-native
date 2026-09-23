//! Isolated voice samples: never select a narrator, alter book position or paint
//! document ranges. One cancellable sample owns its device and sample cache.
use super::{worker, PhoenixShell};
use gpui::Context;
use phoenix_audio::device::{PlaybackDevice, WaveOutput};
use phoenix_reader_session::{AudioCache, VoiceChoice};
use phoenix_tts_native::{Cancellation, Request, VoiceAsset};
use std::{
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SAMPLE: &str =
    "The rain had stopped. She opened the old book and smiled. At last, the journey could begin.";
pub(super) struct Audition {
    token: Cancellation,
    events: mpsc::Receiver<String>,
    join: Option<thread::JoinHandle<anyhow::Result<()>>>,
}
impl Audition {
    pub fn cancel(&self) {
        self.token.cancel();
    }
}
impl Drop for Audition {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl PhoenixShell {
    pub(super) fn preview_reader_voice(&mut self, choice: VoiceChoice, cx: &mut Context<Self>) {
        if self.reader.retiring {
            self.reader.notice = "Finishing the previous listening session…".into();
            cx.notify();
            return;
        }
        if let Some(audition) = &self.reader.audition {
            audition.cancel();
            self.reader.notice = "Stopping sample…".into();
            cx.notify();
            return;
        }
        // Retire the book's model before starting an audition model. Merely
        // pausing audio leaves Breeze resident and can exhaust a 12 GB GPU.
        self.invalidate_reader_document(cx);
        let previous = self.reader.bridge.take();
        self.reader.status.finished = true;
        self.reader.status.requested = false;
        self.reader.status.playing = false;
        self.reader.status.phase = worker::presentation::Phase::Paused;
        let workspace = self.kernel.workspace_path().to_path_buf();
        let token = Cancellation::default();
        let cancel = token.clone();
        let (tx, events) = mpsc::sync_channel(4);
        let join = thread::spawn(move || {
            if let Some(previous) = previous {
                previous.shutdown();
            }
            anyhow::ensure!(!cancel.is_cancelled(), "Sample cancelled");
            sample(workspace, choice, &cancel, tx)
        });
        self.reader.audition = Some(Audition {
            token,
            events,
            join: Some(join),
        });
        self.reader.notice = "Preparing a short voice sample… First use can take a moment.".into();
        cx.notify();
    }
    pub(super) fn poll_voice_audition(&mut self, cx: &mut Context<Self>) {
        let Some(audition) = &self.reader.audition else {
            return;
        };
        while let Ok(message) = audition.events.try_recv() {
            self.reader.notice = message;
            cx.notify();
        }
        if !audition.join.as_ref().is_some_and(|j| j.is_finished()) {
            return;
        }
        let mut audition = self.reader.audition.take().unwrap();
        let cancelled = audition.token.is_cancelled();
        let result = audition.join.take().unwrap().join();
        self.reader.notice = match result {
            _ if cancelled => "Sample stopped.".into(),
            Ok(Ok(())) => "Sample finished. Choose Use voice & listen to hear your book.".into(),
            Ok(Err(e)) => format!("Could not preview this voice: {e:#}"),
            Err(_) => "The sample worker stopped unexpectedly. You can retry.".into(),
        };
        if std::mem::take(&mut self.reader.audition_then_listen) {
            self.reader_primary(cx);
        }
        cx.notify();
    }
}
fn sample(
    workspace: PathBuf,
    choice: VoiceChoice,
    cancel: &Cancellation,
    tx: mpsc::SyncSender<String>,
) -> anyhow::Result<()> {
    let path = workspace.with_extension("reader.json");
    anyhow::ensure!(
        std::fs::metadata(&path)?.len() <= 1_048_576,
        "Configuration bounds"
    );
    let mut config: worker::Config = serde_json::from_slice(&std::fs::read(path)?)?;
    config.load_voices()?;
    let spec = config
        .voices
        .iter()
        .find(|v| VoiceChoice::of(&v.profile).ok() == Some(choice))
        .ok_or_else(|| anyhow::anyhow!("Voice is no longer installed"))?;
    let bundle = worker::engines::Bundles::open(&config, &[spec], cancel)?;
    let instruction = if spec.profile.reference.is_some() {
        spec.profile.default_delivery.clone()
    } else if !spec.profile.default_delivery.is_empty() {
        format!(
            "{} {}",
            spec.profile.description, spec.profile.default_delivery
        )
    } else {
        spec.profile.description.clone()
    };
    let asset = match (&spec.profile.reference, &spec.asset) {
        (Some(r), Some(path)) => {
            let asset = VoiceAsset::open(path, r.encoded, r.model, r.codec)?;
            anyhow::ensure!(
                *blake3::hash(asset.transcript().as_bytes()).as_bytes() == r.transcript,
                "Reference transcript mismatch"
            );
            Some(asset)
        }
        (None, None) => None,
        _ => anyhow::bail!("Reference voice files are missing"),
    };
    let mut provider = worker::engines::Providers::new(bundle, config.storage.clone())?;
    let mut cache = AudioCache::open(config.storage.join("samples"), 64 * 1024 * 1024)?;
    let request = Request {
        epoch: 1,
        plan: *blake3::hash(b"phoenix.voice-sample/v1").as_bytes(),
        segment: 0,
        text: SAMPLE,
        instruction: &instruction,
        seed: spec.profile.seed,
        max_frames: 1_440_000,
    };
    let key = if let Some(style) = &spec.supertonic_style {
        provider
            .cpu
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CPU provider unavailable"))?
            .generate(request, style, &mut cache, cancel, |_| Ok(()))?
    } else {
        provider
            .breeze
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Breeze provider unavailable"))?
            .generate_voiced_streamed(request, asset.as_ref(), &mut cache, cancel, |_| Ok(()))?
    };
    let audio = cache.get(key)?;
    anyhow::ensure!(!cancel.is_cancelled(), "Sample cancelled");
    let _ = tx.try_send(format!("Playing sample · {}", spec.profile.name));
    let mut device = WaveOutput::open_default()?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut block = [0i16; 2048];
    for bytes in audio.pcm().chunks(block.len() * 2) {
        while !device.available()? {
            check(cancel, deadline)?;
            thread::sleep(Duration::from_millis(10));
        }
        check(cancel, deadline)?;
        for (sample, pair) in block.iter_mut().zip(bytes.chunks_exact(2)) {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
        device.submit(&block[..bytes.len() / 2])?;
    }
    while device.position()? < audio.manifest().frames {
        check(cancel, deadline)?;
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn check(cancel: &Cancellation, deadline: Instant) -> anyhow::Result<()> {
    anyhow::ensure!(!cancel.is_cancelled(), "Sample cancelled");
    anyhow::ensure!(Instant::now() < deadline, "Sample playback timed out");
    Ok(())
}
