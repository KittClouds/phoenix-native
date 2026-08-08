use phoenix_scene_compiler::{compile_caps_layout, CapsNode};
use phoenix_scene_contract::{CapsRole, VisualNodeKind};
use std::time::{Duration, Instant};

const NODES: usize = 25_000;
const CHUNKS: usize = 250;
const TRIALS: usize = 40;
const P95_GATE: Duration = Duration::from_millis(20);

fn main() {
    let nodes = fixture();
    let mut timings = Vec::with_capacity(TRIALS);
    for _ in 0..TRIALS {
        let started = Instant::now();
        let layout = compile_caps_layout(&nodes)
            .unwrap_or_else(|error| panic!("CAPS benchmark fixture: {error}"));
        std::hint::black_box(layout);
        timings.push(started.elapsed());
    }
    timings.sort_unstable();
    let median = percentile(&timings, 50);
    let p95 = percentile(&timings, 95);
    let max = timings.last().copied().unwrap_or(Duration::ZERO);
    println!(
        "CAPS nested Klein mosaic: nodes={NODES} chunks={CHUNKS} trials={TRIALS} \
         median={median:?} p95={p95:?} max={max:?} gate={P95_GATE:?}"
    );
    assert!(
        p95 <= P95_GATE,
        "CAPS layout p95 {p95:?} exceeded {P95_GATE:?}"
    );
}

fn fixture() -> Vec<CapsNode> {
    let mut nodes = Vec::with_capacity(NODES);
    nodes.push(CapsNode {
        stable_id: 1,
        role: CapsRole::Episode,
        semantic_kind: VisualNodeKind::Episode,
        parent_slot: None,
        sibling_rank: 0,
        sibling_count: 1,
        membership_count: 1,
    });
    for chunk in 0..CHUNKS {
        nodes.push(CapsNode {
            stable_id: 2 + chunk as u64,
            role: CapsRole::Chunk,
            semantic_kind: VisualNodeKind::Chunk,
            parent_slot: Some(0),
            sibling_rank: chunk as u32,
            sibling_count: CHUNKS as u32,
            membership_count: 1,
        });
    }
    let entities = NODES - CHUNKS - 1;
    for entity in 0..entities {
        let chunk = entity % CHUNKS;
        let sibling_rank = entity / CHUNKS;
        let sibling_count = (entities - chunk).div_ceil(CHUNKS);
        nodes.push(CapsNode {
            stable_id: 10_000 + entity as u64,
            role: CapsRole::Entity,
            semantic_kind: VisualNodeKind::EntityOther,
            parent_slot: Some((chunk + 1) as u32),
            sibling_rank: sibling_rank as u32,
            sibling_count: sibling_count as u32,
            membership_count: 1,
        });
    }
    nodes
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let index = sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(index).copied().unwrap_or(Duration::ZERO)
}
