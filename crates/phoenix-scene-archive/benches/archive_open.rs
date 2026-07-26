use phoenix_scene_archive::{ArchiveManifold, PhoenixSceneArchiveV1};
use std::path::Path;
use std::time::Instant;

const TRIALS: usize = 30;

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("phoenix-comparison-v1.psa");
    let mut timings = Vec::with_capacity(TRIALS);
    for _ in 0..TRIALS {
        let started = Instant::now();
        let archive = PhoenixSceneArchiveV1::open(&path).unwrap_or_else(|error| {
            panic!("open frozen scene archive {}: {error}", path.display())
        });
        let pages = archive
            .open_manifold(ArchiveManifold::Hybrid)
            .unwrap_or_else(|error| panic!("open frozen Hybrid pages: {error}"));
        std::hint::black_box(pages.positions.len());
        timings.push(started.elapsed());
    }
    timings.sort_unstable();
    let median = timings[TRIALS / 2];
    let p95 = timings[(TRIALS * 95 / 100).min(TRIALS - 1)];
    let max = timings[TRIALS - 1];
    println!(
        "PhoenixSceneArchiveV1 mmap + directory + five-page Hybrid verification: \
         trials={TRIALS} median={median:?} p95={p95:?} max={max:?}"
    );
}
