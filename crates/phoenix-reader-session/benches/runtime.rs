#[path = "../tests/support/mod.rs"]
mod support;
use criterion::{criterion_group, criterion_main, Criterion};
use phoenix_audio::device::PlaybackDevice;
use phoenix_reader_session::*;
use std::{cell::Cell, io, rc::Rc};
use support::*;

struct Device {
    clock: Rc<Cell<u64>>,
    queued: u64,
}
impl PlaybackDevice for Device {
    fn available(&mut self) -> io::Result<bool> {
        Ok(true)
    }
    fn submit(&mut self, pcm: &[i16]) -> io::Result<()> {
        self.queued += pcm.len() as u64;
        Ok(())
    }
    fn position(&mut self) -> io::Result<u64> {
        Ok(self.clock.get())
    }
    fn pause(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn reset(&mut self) -> io::Result<()> {
        self.queued = 0;
        self.clock.set(0);
        Ok(())
    }
}
fn bench(c: &mut Criterion) {
    let root = tempfile::tempdir().unwrap();
    let plan = plan(&lease("Hello.", 1));
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let clock = Rc::new(Cell::new(0));
    let session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    let mut runtime = ReaderRuntime::new(
        Device {
            clock: clock.clone(),
            queued: 0,
        },
        plan,
        session,
        identity(),
    )
    .unwrap();
    c.bench_function("cached_runtime_verified_replay_4_frames", |b| {
        b.iter(|| {
            runtime.seek(0, 0, cache.get(key).unwrap()).unwrap();
            runtime.tick().unwrap();
            clock.set(4);
            assert_eq!(runtime.tick().unwrap(), PlaybackState::Completed);
        })
    });
}
criterion_group!(benches, bench);
criterion_main!(benches);
