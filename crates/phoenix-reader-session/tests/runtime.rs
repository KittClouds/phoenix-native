mod support;
use phoenix_audio::device::PlaybackDevice;
use phoenix_reader_session::*;
use std::{cell::RefCell, io, rc::Rc};
use support::*;

#[test]
fn restored_cursor_is_not_republished_until_it_changes() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut store = SessionStore::open(root.path()).unwrap();
    store.checkpoint(&session, &plan, 0, true).unwrap();
    let mut runtime = ReaderRuntime::restore(
        Device(Rc::new(RefCell::new(State::default()))),
        plan,
        [7; 32],
        identity(),
        &store,
    )
    .unwrap();
    assert!(!runtime.checkpoint(&mut store, 2000, true).unwrap());
    assert!(!runtime.checkpoint(&mut store, 3000, false).unwrap());
}

#[test]
fn device_backpressure_bounds_submission_without_skipping_frames() {
    use phoenix_tts_contract::{AudioFormat, Event, FinishReason};
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let binding = binding("Hello.");
    let mut writer = cache.begin(binding, identity(), "Hello.", 4096).unwrap();
    writer
        .push(
            event(
                binding,
                0,
                Event::Started {
                    provider: identity().provider,
                    format: AudioFormat::PCM24,
                },
            ),
            &[],
        )
        .unwrap();
    for n in 0..2 {
        writer
            .push(
                event(
                    binding,
                    n + 1,
                    Event::AudioChunk {
                        first_frame: n * 2048,
                        frames: 2048,
                    },
                ),
                &[0; 4096],
            )
            .unwrap();
    }
    writer
        .finish(
            event(
                binding,
                3,
                Event::Completed {
                    frames: 4096,
                    reason: FinishReason::Normal,
                },
            ),
            None,
        )
        .unwrap();
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(binding.audio_key).unwrap())
        .unwrap();
    r.tick().unwrap();
    r.tick().unwrap();
    assert_eq!(state.borrow().queued, 2048);
    assert_eq!(r.session().position().source_frame, 0);
    state.borrow_mut().presented = 2048;
    r.tick().unwrap();
    assert_eq!(state.borrow().queued, 4096);
    assert_eq!(r.session().position().source_frame, 2048);
    state.borrow_mut().presented = 4096;
    assert_eq!(r.tick().unwrap(), PlaybackState::Completed);
}

#[test]
fn speed_change_preserves_source_position_and_flushes_old_output() {
    use phoenix_tts_contract::{AudioFormat, Event, FinishReason};
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let binding = binding("Hello.");
    let mut writer = cache.begin(binding, identity(), "Hello.", 24_000).unwrap();
    writer
        .push(
            event(
                binding,
                0,
                Event::Started {
                    provider: identity().provider,
                    format: AudioFormat::PCM24,
                },
            ),
            &[],
        )
        .unwrap();
    let pcm: Vec<u8> = (0..24_000)
        .flat_map(|n| {
            let phase = n as f32 * std::f32::consts::TAU * 240.0 / 24_000.0;
            ((phase.sin() * 16000.0) as i16).to_le_bytes()
        })
        .collect();
    let mut sequence = 1;
    for (index, chunk) in pcm.chunks(4096).enumerate() {
        writer
            .push(
                event(
                    binding,
                    sequence,
                    Event::AudioChunk {
                        first_frame: (index * 2048) as u64,
                        frames: (chunk.len() / 2) as u32,
                    },
                ),
                chunk,
            )
            .unwrap();
        sequence += 1;
    }
    writer
        .finish(
            event(
                binding,
                sequence,
                Event::Completed {
                    frames: 24_000,
                    reason: FinishReason::Normal,
                },
            ),
            None,
        )
        .unwrap();
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut runtime = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    runtime
        .attach(runtime.epoch(), 0, cache.get(binding.audio_key).unwrap())
        .unwrap();
    runtime.tick().unwrap();
    state.borrow_mut().presented = 1024;
    let old_epoch = runtime.epoch();
    runtime.set_speed(1300).unwrap();
    assert!(runtime.epoch() > old_epoch);
    assert_eq!(runtime.speed_milli(), 1300);
    assert_eq!(runtime.session().position().source_frame, 1024);
    assert_eq!(state.borrow().queued, 0);
    runtime.pause(false).unwrap();
    runtime.tick().unwrap();
    assert!(state.borrow().queued > 0);
    assert_eq!(runtime.session().position().source_frame, 1024);
}

#[derive(Default)]
struct State {
    queued: u64,
    presented: u64,
    paused: bool,
    resets: u32,
    fail: bool,
}
struct Device(Rc<RefCell<State>>);
impl PlaybackDevice for Device {
    fn available(&mut self) -> io::Result<bool> {
        Ok(self.0.borrow().queued - self.0.borrow().presented < 2048)
    }
    fn submit(&mut self, samples: &[i16]) -> io::Result<()> {
        self.0.borrow_mut().queued += samples.len() as u64;
        Ok(())
    }
    fn position(&mut self) -> io::Result<u64> {
        if self.0.borrow().fail {
            Err(io::Error::other("device disconnected"))
        } else {
            Ok(self.0.borrow().presented)
        }
    }
    fn pause(&mut self, paused: bool) -> io::Result<()> {
        self.0.borrow_mut().paused = paused;
        Ok(())
    }
    fn reset(&mut self) -> io::Result<()> {
        let mut s = self.0.borrow_mut();
        s.queued = 0;
        s.presented = 0;
        s.resets += 1;
        Ok(())
    }
}

#[test]
fn presentation_pause_seek_bookmark_restart_and_stale_audio() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let mut cache = AudioCache::open(root.path().join("cache"), 2 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r =
        ReaderRuntime::new(Device(state.clone()), plan.clone(), session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(key).unwrap()).unwrap();
    r.tick().unwrap();
    assert_eq!(
        r.session().position().source_frame,
        0,
        "queued is not presented"
    );
    state.borrow_mut().presented = 2;
    r.pause(true).unwrap();
    assert_eq!(r.session().position().source_frame, 2);
    r.bookmark(9).unwrap();
    let mut store = SessionStore::open(root.path().join("sessions")).unwrap();
    assert!(r.checkpoint(&mut store, 1000, true).unwrap());
    assert!(!r.checkpoint(&mut store, 1001, true).unwrap());
    let restored = store.load([7; 32], &plan).unwrap();
    assert_eq!(restored.bookmarks()[&9].source_frame, 2);
    let old_epoch = r.epoch();
    r.seek(0, 1, cache.get(key).unwrap()).unwrap();
    assert!(r.attach(old_epoch, 0, cache.get(key).unwrap()).is_err());
    r.tick().unwrap();
    assert_eq!(state.borrow().queued, 3);
    r.cancel().unwrap();
    assert_eq!(state.borrow().queued, 0);
    assert_eq!(r.state(), PlaybackState::Stopped);
    drop(r);
    let mut resumed =
        ReaderRuntime::new(Device(state.clone()), plan, restored, identity()).unwrap();
    resumed
        .attach(resumed.epoch(), 0, cache.get(key).unwrap())
        .unwrap();
    resumed.tick().unwrap();
    assert_eq!(state.borrow().queued, 2);
    state.borrow_mut().presented = 2;
    assert_eq!(resumed.tick().unwrap(), PlaybackState::Completed);
    assert_eq!(resumed.session().position().source_frame, 4);
}

#[test]
fn device_failure_flushes_and_cannot_advance_position() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(key).unwrap()).unwrap();
    r.tick().unwrap();
    state.borrow_mut().fail = true;
    assert!(r.tick().is_err());
    assert_eq!(r.state(), PlaybackState::Failed);
    assert_eq!(state.borrow().queued, 0);
    assert_eq!(r.session().position().source_frame, 0);
}

#[test]
fn segment_progression_requests_only_next_segment() {
    let root = tempfile::tempdir().unwrap();
    let lease = lease("Hello. Goodbye.", 1);
    let plan = plan_markdown([1; 32], &lease, PlannerConfig::default())
        .unwrap()
        .plan;
    assert_eq!(plan.spec().segments.len(), 2);
    let mut cache = AudioCache::open(root.path(), 4 * 1024 * 1024).unwrap();
    let keys: Vec<_> = plan
        .spec()
        .segments
        .iter()
        .map(|s| write_cache(&mut cache, s.spoken.slice(&plan.spec().spoken).unwrap()))
        .collect();
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(keys[0]).unwrap()).unwrap();
    r.tick().unwrap();
    state.borrow_mut().presented = 4;
    assert_eq!(r.tick().unwrap(), PlaybackState::NeedsAudio(1));
    r.attach(r.epoch(), 1, cache.get(keys[1]).unwrap()).unwrap();
    r.tick().unwrap();
    state.borrow_mut().presented = 4;
    assert_eq!(r.tick().unwrap(), PlaybackState::Completed);
    assert_eq!(r.session().position().segment, 1);
}

#[test]
fn prefetch_queues_across_boundaries_without_reset_or_early_highlight() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan_markdown(
        [1; 32],
        &lease("Hello. Goodbye. Again.", 1),
        PlannerConfig::default(),
    )
    .unwrap()
    .plan;
    let mut cache = AudioCache::open(root.path(), 4 * 1024 * 1024).unwrap();
    let keys: Vec<_> = plan
        .spec()
        .segments
        .iter()
        .map(|s| write_cache(&mut cache, s.spoken.slice(&plan.spec().spoken).unwrap()))
        .collect();
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(keys[0]).unwrap()).unwrap();
    assert!(r
        .enqueue(r.epoch(), 2, cache.get(keys[2]).unwrap())
        .is_err());
    r.enqueue(r.epoch(), 1, cache.get(keys[1]).unwrap())
        .unwrap();
    r.enqueue(r.epoch(), 2, cache.get(keys[2]).unwrap())
        .unwrap();
    assert_eq!(r.buffered_frames(), 12);
    let resets = state.borrow().resets;
    r.tick().unwrap();
    assert_eq!(
        state.borrow().queued,
        12,
        "all three segments queued before first ends"
    );
    assert_eq!(r.session().position().segment, 0);
    state.borrow_mut().presented = 5;
    r.pause(true).unwrap();
    assert_eq!(r.session().position().segment, 1);
    assert_eq!(r.session().position().source_frame, 1);
    r.pause(false).unwrap();
    state.borrow_mut().presented = 10;
    r.tick().unwrap();
    assert_eq!(r.session().position().segment, 2);
    assert_eq!(r.session().position().source_frame, 2);
    assert_eq!(state.borrow().resets, resets);
    state.borrow_mut().presented = 12;
    assert_eq!(r.tick().unwrap(), PlaybackState::Completed);
}

#[test]
fn seek_discards_prefetch_and_rejects_late_epoch() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan_markdown(
        [1; 32],
        &lease("Hello. Goodbye.", 1),
        PlannerConfig::default(),
    )
    .unwrap()
    .plan;
    let mut cache = AudioCache::open(root.path(), 4 * 1024 * 1024).unwrap();
    let first = write_cache(
        &mut cache,
        plan.segment(0)
            .unwrap()
            .spoken
            .slice(&plan.spec().spoken)
            .unwrap(),
    );
    let second = write_cache(
        &mut cache,
        plan.segment(1)
            .unwrap()
            .spoken
            .slice(&plan.spec().spoken)
            .unwrap(),
    );
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state.clone()), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(first).unwrap()).unwrap();
    let old = r.epoch();
    r.enqueue(old, 1, cache.get(second).unwrap()).unwrap();
    r.tick().unwrap();
    r.seek(0, 2, cache.get(first).unwrap()).unwrap();
    assert_eq!(r.ahead_count(), 0);
    assert_eq!(state.borrow().queued, 0);
    assert!(r.enqueue(old, 1, cache.get(second).unwrap()).is_err());
    r.tick().unwrap();
    assert_eq!(state.borrow().queued, 2);
}

#[test]
fn prefetch_has_a_hard_eight_segment_bound() {
    let root = tempfile::tempdir().unwrap();
    let plan = plan_markdown(
        [1; 32],
        &lease(&"Hello. ".repeat(12), 1),
        PlannerConfig::default(),
    )
    .unwrap()
    .plan;
    let mut cache = AudioCache::open(root.path(), 4 * 1024 * 1024).unwrap();
    let key = write_cache(
        &mut cache,
        plan.segment(0)
            .unwrap()
            .spoken
            .slice(&plan.spec().spoken)
            .unwrap(),
    );
    let state = Rc::new(RefCell::new(State::default()));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut r = ReaderRuntime::new(Device(state), plan, session, identity()).unwrap();
    r.attach(r.epoch(), 0, cache.get(key).unwrap()).unwrap();
    for segment in 1..=8 {
        r.enqueue(r.epoch(), segment, cache.get(key).unwrap())
            .unwrap();
    }
    assert!(r.enqueue(r.epoch(), 9, cache.get(key).unwrap()).is_err());
    assert_eq!(r.ahead_count(), 8);
}

#[test]
fn cast_binding_rejects_wrong_session_and_old_voice_audio() {
    let plan = plan(&lease("Hello.", 1));
    let mut changed = identity();
    changed.voice = [90; 32];
    let make_table = || UtteranceVoices::new(&plan, vec![changed.clone()], vec![0]).unwrap();
    let table = make_table();
    let session = ReaderSession::new([7; 32], &plan, table.fingerprint()).unwrap();
    let wrong = ReaderSession::new([8; 32], &plan, [33; 32]).unwrap();
    let device = || Device(Rc::new(RefCell::new(State::default())));
    let mut runtime = ReaderRuntime::new(device(), plan.clone(), wrong, identity()).unwrap();
    assert!(runtime.bind_voices(make_table()).is_err());
    let mut runtime = ReaderRuntime::new(device(), plan.clone(), session, identity()).unwrap();
    runtime.bind_voices(table).unwrap();
    assert_eq!(
        runtime.required_key(0).unwrap(),
        changed.audio_key("Hello.").unwrap()
    );
    assert!(runtime.bind_voices(make_table()).is_err());
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path(), 4 * 1024 * 1024).unwrap();
    let old = write_cache(&mut cache, "Hello.");
    assert!(runtime
        .attach(runtime.epoch(), 0, cache.get(old).unwrap())
        .is_err());
}

#[test]
fn cast_session_can_play_checkpoint_and_resume_the_exact_audio() {
    let plan = plan(&lease("Hello.", 1));
    let table = || UtteranceVoices::new(&plan, vec![identity()], vec![0]).unwrap();
    let voices = table();
    let session = ReaderSession::new([91; 32], &plan, voices.fingerprint()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let mut store = SessionStore::open(root.path().join("sessions")).unwrap();
    let state = Rc::new(RefCell::new(State::default()));
    let mut runtime = ReaderRuntime::new(Device(state), plan.clone(), session, identity()).unwrap();
    runtime.bind_voices(voices).unwrap();
    runtime
        .attach(runtime.epoch(), 0, cache.get(key).unwrap())
        .unwrap();
    runtime.tick().unwrap();
    runtime.pause(true).unwrap();
    runtime.checkpoint(&mut store, 1, true).unwrap();
    drop(runtime);
    let mut restored = ReaderRuntime::restore(
        Device(Rc::new(RefCell::new(State::default()))),
        plan.clone(),
        [91; 32],
        identity(),
        &store,
    )
    .unwrap();
    restored.bind_voices(table()).unwrap();
    restored
        .attach(restored.epoch(), 0, cache.get(key).unwrap())
        .unwrap();
}
