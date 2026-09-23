use phoenix_hybrid_space::{layout, CapsRole, HybridLane, HybridNode};
use std::time::Instant;

fn main() {
    let mut nodes = Vec::with_capacity(25_000);
    for i in 0..25_000 {
        let (role, parent) = match i {
            0 => (CapsRole::Document, None),
            1..=100 => (CapsRole::Chapter, Some(0)),
            _ => (CapsRole::Paragraph, Some(1 + (i % 100) as u32)),
        };
        nodes.push(HybridNode {
            stable_id: i + 1,
            role,
            parent_slot: parent,
            lane: HybridLane::Structure,
            sibling_rank: 0,
            sibling_count: 1,
            degree: 2,
        });
    }
    for _ in 0..3 {
        std::hint::black_box(layout(&nodes).unwrap());
    }
    let mut samples = [0_u128; 40];
    for sample in &mut samples {
        let start = Instant::now();
        std::hint::black_box(layout(&nodes).unwrap());
        *sample = start.elapsed().as_micros();
    }
    samples.sort_unstable();
    println!(
        "Hybrid regions: nodes={} median_us={} p95_us={}",
        nodes.len(),
        samples[19],
        samples[37]
    );
}
