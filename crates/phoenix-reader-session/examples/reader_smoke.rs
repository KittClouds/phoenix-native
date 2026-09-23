//! Real-device completed-cache vertical slice; silent fixture is not TTS quality evidence.
use phoenix_audio::device::WaveOutput;
use phoenix_reader_session::*;
use phoenix_tts_contract::*;
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let text = "# First\n\nHello reader.\n\n# Second\n\nGoodbye reader.";
    let lease = DocumentLease {
        entry_id: EntryId(1),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(text.as_bytes()),
        content: Arc::from(text),
    };
    let plan = plan_markdown([1; 32], &lease, PlannerConfig::default())?.plan;
    let identity = SynthesisIdentity {
        provider: [1; 32],
        runtime: [2; 32],
        model: [3; 32],
        tokenizer: [4; 32],
        codec: [5; 32],
        voice: [6; 32],
        reference_audio: None,
        reference_transcript: None,
        direction: [7; 32],
        generation_config: [8; 32],
        transformations: [9; 32],
        postprocessing: [10; 32],
        seed: 0,
        format: AudioFormat::PCM24,
    };
    let mut cache = AudioCache::open(root.path().join("cache"), 8 * 1024 * 1024)?;
    for (segment, planned) in plan.spec().segments.iter().enumerate() {
        let spoken = planned.spoken.slice(&plan.spec().spoken)?;
        let binding = Binding {
            request: segment as u64 + 1,
            epoch: 1,
            plan: plan.id(),
            segment: segment as u32,
            audio_key: identity.audio_key(spoken)?,
        };
        let envelope = |sequence, event| Envelope {
            binding,
            sequence,
            event,
        };
        let mut writer = cache.begin(binding, identity.clone(), spoken, 12_000)?;
        writer.push(
            envelope(
                0,
                Event::Started {
                    provider: identity.provider,
                    format: AudioFormat::PCM24,
                },
            ),
            &[],
        )?;
        let mut frame = 0;
        let mut sequence = 1;
        while frame < 12_000 {
            let frames = (12_000 - frame).min(BLOCK_FRAMES as u64);
            writer.push(
                envelope(
                    sequence,
                    Event::AudioChunk {
                        first_frame: frame,
                        frames: frames as u32,
                    },
                ),
                &vec![0; frames as usize * 2],
            )?;
            frame += frames;
            sequence += 1;
        }
        writer.finish(
            envelope(
                sequence,
                Event::Completed {
                    frames: frame,
                    reason: FinishReason::Normal,
                },
            ),
            None,
        )?;
    }
    let session = ReaderSession::new([11; 32], &plan, identity.voice)?;
    let mut runtime = ReaderRuntime::new(
        WaveOutput::open_default()?,
        plan.clone(),
        session,
        identity.clone(),
    )?;
    let mut store = SessionStore::open(root.path().join("sessions"))?;
    let start = Instant::now();
    let mut paused = false;
    loop {
        if start.elapsed() > Duration::from_secs(15) {
            return Err("Reader device timeout".into());
        }
        match runtime.tick()? {
            PlaybackState::NeedsAudio(segment) => {
                let audio = cache.get(runtime.required_key(segment)?)?;
                runtime.attach(runtime.epoch(), segment, audio)?;
            }
            PlaybackState::Playing => {
                if !paused && runtime.session().position().source_frame > 2000 {
                    runtime.pause(true)?;
                    let position = runtime.session().position();
                    thread::sleep(Duration::from_millis(100));
                    runtime.tick()?;
                    assert_eq!(position, runtime.session().position());
                    runtime.bookmark(1)?;
                    runtime.checkpoint(&mut store, start.elapsed().as_millis() as u64, true)?;
                    runtime.pause(false)?;
                    paused = true;
                }
            }
            PlaybackState::Completed => break,
            other => return Err(format!("unexpected {other:?}").into()),
        }
        runtime.checkpoint(&mut store, start.elapsed().as_millis() as u64, false)?;
        thread::sleep(Duration::from_millis(5));
    }
    runtime.checkpoint(&mut store, start.elapsed().as_millis() as u64, true)?;
    let expected = runtime.session().position();
    drop(runtime);
    let restored = store.load([11; 32], &plan)?;
    assert_eq!(expected, restored.position());
    assert!(restored.bookmarks().contains_key(&1));
    let mut resumed = ReaderRuntime::new(WaveOutput::open_default()?, plan, restored, identity)?;
    let segment = expected.segment;
    resumed.attach(
        resumed.epoch(),
        segment,
        cache.get(resumed.required_key(segment)?)?,
    )?;
    assert_eq!(resumed.tick()?, PlaybackState::Completed);
    println!("PASS Reader real device: chapters, pause/resume, bookmark, checkpoint, reopen exact artifact; elapsed {:?}", start.elapsed());
    Ok(())
}
