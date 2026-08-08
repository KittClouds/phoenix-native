use graph_model::GraphRevision;
use graph_render_wgpu::SceneState;
use phoenix_scene_archive::{
    EdgeRecord, ManifoldPageSet, NodeIdentityRecord, NodeStyleRecord, PositionRecord,
    TopologyRecord,
};
use std::hint::black_box;
use std::time::Instant;

const NODE_COUNT: usize = 25_000;
const SWITCH_COUNT: usize = 200;

fn main() {
    let identities: Vec<_> = (0..NODE_COUNT)
        .map(|slot| NodeIdentityRecord {
            id: slot as u64 + 1,
        })
        .collect();
    let styles = vec![
        NodeStyleRecord {
            color: [0.2, 0.7, 0.6, 0.9],
            radius: 0.4,
            kind: 0,
            flags: 0,
        };
        NODE_COUNT
    ];
    let topology: Vec<TopologyRecord> = Vec::new();
    let edges: Vec<EdgeRecord> = Vec::new();
    let manifolds: [Vec<PositionRecord>; 6] = std::array::from_fn(|manifold| {
        (0..NODE_COUNT)
            .map(|slot| PositionRecord {
                position: [
                    slot as f32 * 0.001,
                    (slot % 97) as f32 + manifold as f32,
                    manifold as f32 * 4.0,
                ],
            })
            .collect()
    });
    let initial = ManifoldPageSet {
        identities: &identities,
        styles: &styles,
        topology: &topology,
        edges: &edges,
        positions: &manifolds[0],
    };
    let mut state = SceneState::default();
    state
        .set_archive_pages(GraphRevision(1), &initial)
        .unwrap_or_else(|error| panic!("{error}"));
    let node_capacity = state.node_capacity_slots();
    let edge_capacity = state.edge_capacity_slots();

    let mut samples = [0_u128; SWITCH_COUNT];
    for (switch, sample) in samples.iter_mut().enumerate() {
        let started = Instant::now();
        state
            .update_packed_positions(black_box(&manifolds[switch % manifolds.len()]))
            .unwrap_or_else(|error| panic!("{error}"));
        *sample = started.elapsed().as_micros();
    }
    samples.sort_unstable();
    let p95 = samples[(SWITCH_COUNT * 95).div_ceil(100) - 1];
    assert_eq!(state.node_capacity_slots(), node_capacity);
    assert_eq!(state.edge_capacity_slots(), edge_capacity);
    println!(
        "{SWITCH_COUNT} packed switches / {NODE_COUNT} nodes: p95={p95}us max={}us",
        samples[SWITCH_COUNT - 1]
    );
}
