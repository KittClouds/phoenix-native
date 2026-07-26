use graph_model::{
    EdgeId, EdgeVisual, GraphDiff, GraphRevision, GraphSnapshot, NodeId, NodeVisual,
};
use graph_render_wgpu::SceneState;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let mut state = SceneState::default();
    let node_count = 25_000usize;
    let edge_count = 150_000usize;
    let nodes: Vec<_> = (0..node_count)
        .map(|index| NodeVisual {
            id: NodeId(index as u64 + 1),
            position: [index as f32, (index % 97) as f32, 0.0],
            radius: 1.0,
            color: [0.2, 0.6, 0.9, 1.0],
            kind: 0,
            flags: 0,
        })
        .collect();
    let edges: Vec<_> = (0..edge_count)
        .map(|index| EdgeVisual {
            id: EdgeId(index as u64 + 1),
            source: NodeId((index % node_count) as u64 + 1),
            target: NodeId(((index + 17) % node_count) as u64 + 1),
            width: 1.0,
            color: [0.4, 0.5, 0.7, 0.3],
            kind: 0,
            flags: 0,
        })
        .collect();
    let snapshot = GraphSnapshot::new(GraphRevision(1), nodes, edges);
    state
        .set_snapshot(&snapshot)
        .unwrap_or_else(|error| panic!("{error}"));

    let started = Instant::now();
    for revision in 2..102u64 {
        let mut diff = GraphDiff::new(GraphRevision(revision));
        diff.updated_nodes.reserve(node_count / 100);
        for index in 0..node_count / 100 {
            let id = NodeId((index * 97 % node_count) as u64 + 1);
            let mut node = *state
                .node_at_slot(state.node_slot(id).unwrap_or_default())
                .unwrap_or_else(|| panic!("fixture node {id} missing"));
            node.position[2] = revision as f32;
            diff.updated_nodes.push(node);
        }
        black_box(
            state
                .apply_diff(diff)
                .unwrap_or_else(|error| panic!("{error}")),
        );
    }
    let elapsed = started.elapsed();
    println!(
        "100 x 1% updates on {node_count} nodes / {edge_count} edges: {:?} ({:.3} ms/update)",
        elapsed,
        elapsed.as_secs_f64() * 10.0
    );
}
