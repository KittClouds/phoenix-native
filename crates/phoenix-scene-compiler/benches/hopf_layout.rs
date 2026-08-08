use phoenix_scene_compiler::{compile_hopf_positions, HopfNode};
use phoenix_scene_contract::CapsRole;
use std::time::{Duration, Instant};

const NODES: usize = 25_000;
const TRIALS: usize = 40;
const P95_GATE: Duration = Duration::from_millis(25);

fn main() {
    let nodes = fixture();
    let mut timings = Vec::with_capacity(TRIALS);
    for _ in 0..TRIALS {
        let started = Instant::now();
        let positions = compile_hopf_positions(&nodes)
            .unwrap_or_else(|error| panic!("Hopf benchmark fixture: {error}"));
        std::hint::black_box(positions);
        timings.push(started.elapsed());
    }
    timings.sort_unstable();
    let median = percentile(&timings, 50);
    let p95 = percentile(&timings, 95);
    let max = timings.last().copied().unwrap_or(Duration::ZERO);
    println!(
        "Hopf S3 fiber layout: nodes={NODES} trials={TRIALS} median={median:?} \
         p95={p95:?} max={max:?} gate={P95_GATE:?}"
    );
    assert!(
        p95 <= P95_GATE,
        "Hopf layout p95 {p95:?} exceeded {P95_GATE:?}"
    );
}

fn fixture() -> Vec<HopfNode> {
    (0..NODES)
        .map(|slot| HopfNode {
            stable_id: slot as u64 + 1,
            semantic_slot: (slot % 23) as u16,
            role: CapsRole::Entity,
            parent_slot: None,
            sibling_rank: 0,
            sibling_count: 1,
            degree: (slot % 64) as u32,
        })
        .collect()
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let index = sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(index).copied().unwrap_or(Duration::ZERO)
}
