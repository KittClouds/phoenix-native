use phoenix_scene_compiler::{
    compile_caps_positions, compile_siegel_positions, compile_transit_positions, CapsNode,
};
use phoenix_scene_contract::{CapsRole, VisualNodeKind};
use std::time::{Duration, Instant};

fn main() {
    let mut nodes = Vec::with_capacity(25_000);
    nodes.push(CapsNode {
        stable_id: 1,
        role: CapsRole::Document,
        semantic_kind: VisualNodeKind::Document,
        parent_slot: None,
        sibling_rank: 0,
        sibling_count: 1,
        membership_count: 1,
    });
    for rank in 0..24_999 {
        nodes.push(CapsNode {
            stable_id: rank + 2,
            role: CapsRole::Chapter,
            semantic_kind: VisualNodeKind::Chapter,
            parent_slot: Some(0),
            sibling_rank: rank as u32,
            sibling_count: 24_999,
            membership_count: 1,
        });
    }
    for (name, kernel) in [
        ("caps", compile_caps_positions as fn(&[CapsNode]) -> _),
        ("siegel", compile_siegel_positions),
        ("transit", compile_transit_positions),
    ] {
        for _ in 0..3 {
            std::hint::black_box(kernel(&nodes).unwrap());
        }
        let mut timings = Vec::with_capacity(40);
        for _ in 0..40 {
            let start = Instant::now();
            std::hint::black_box(kernel(&nodes).unwrap());
            timings.push(start.elapsed());
        }
        timings.sort_unstable();
        println!(
            "{name}: nodes={} median_us={} p95_us={}",
            nodes.len(),
            timings[19].as_micros(),
            timings[37].as_micros()
        );
        assert!(
            timings[37] < Duration::from_millis(25),
            "{name} exceeded 25ms layout budget"
        );
    }
}
