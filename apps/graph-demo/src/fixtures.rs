use anyhow::{bail, Context};
use graph_model::{
    EdgeId, EdgeVisual, GraphDiff, GraphRevision, GraphSnapshot, NodeId, NodeVisual,
};
use graph_render_wgpu::SceneState;
use hashbrown::HashSet;
use memchr::memchr;
use memmap2::MmapOptions;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::fs::File;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum FixtureTopology {
    Uniform,
    Clustered,
    HubAndSpoke,
    LongChain,
    Communities,
}

pub fn generate(
    topology: FixtureTopology,
    node_count: usize,
    edge_count: usize,
    seed: u64,
) -> GraphSnapshot {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut nodes = Vec::with_capacity(node_count);
    let mut edges = Vec::with_capacity(edge_count);
    match topology {
        FixtureTopology::Uniform => uniform_nodes(&mut nodes, node_count, &mut rng),
        FixtureTopology::Clustered => clustered_nodes(&mut nodes, node_count, &mut rng),
        FixtureTopology::HubAndSpoke => {
            hub_nodes(&mut nodes, &mut edges, node_count, &mut rng);
        }
        FixtureTopology::LongChain => chain_nodes(&mut nodes, &mut edges, node_count),
        FixtureTopology::Communities => community_nodes(&mut nodes, node_count, &mut rng),
    }
    fill_edges(&nodes, &mut edges, edge_count, &mut rng);
    GraphSnapshot::new(GraphRevision(1), nodes, edges)
}

pub fn load_json(path: &Path) -> anyhow::Result<GraphSnapshot> {
    let file =
        File::open(path).with_context(|| format!("opening graph fixture {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("reading graph fixture metadata {}", path.display()))?;
    if metadata.len() == 0 {
        bail!("graph fixture {} is empty", path.display());
    }
    // SAFETY: The mapping is read-only and lives no longer than `file`. External mutation of
    // fixture files while loading is outside the demo contract and still cannot violate memory
    // safety because no typed references into the mapping escape this function.
    let mapped = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("memory-mapping graph fixture {}", path.display()))?;
    let object_start = memchr(b'{', &mapped)
        .with_context(|| format!("graph fixture {} has no JSON object", path.display()))?;
    if mapped[..object_start]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace() && *byte != 0xEF && *byte != 0xBB && *byte != 0xBF)
    {
        bail!("graph fixture {} has invalid leading bytes", path.display());
    }
    let snapshot: GraphSnapshot = serde_json::from_slice(&mapped[object_start..])
        .with_context(|| format!("parsing graph fixture {}", path.display()))?;
    snapshot.validate()?;
    Ok(snapshot)
}

pub fn animated_diff(
    state: &SceneState,
    next_revision: GraphRevision,
    ratio: f32,
    rng: &mut ChaCha8Rng,
) -> GraphDiff {
    let node_count = state.node_count();
    let desired = ((node_count as f32 * ratio).ceil() as usize)
        .clamp(usize::from(node_count != 0), node_count);
    let mut selected = HashSet::with_capacity(desired);
    while selected.len() < desired {
        let slot = rng.gen_range(0..state.node_capacity_slots());
        if state.node_at_slot(slot as u32).is_some() {
            selected.insert(slot);
        }
    }
    let mut diff = GraphDiff::new(next_revision);
    diff.updated_nodes.reserve(desired);
    for slot in selected {
        let Some(mut node) = state.node_at_slot(slot as u32).copied() else {
            continue;
        };
        node.position[0] += rng.gen_range(-1.5..1.5);
        node.position[1] += rng.gen_range(-1.5..1.5);
        node.color[0] = (node.color[0] + rng.gen_range(-0.04..0.04)).clamp(0.05, 1.0);
        diff.updated_nodes.push(node);
    }
    diff
}

fn uniform_nodes(nodes: &mut Vec<NodeVisual>, count: usize, rng: &mut ChaCha8Rng) {
    let extent = (count as f32).sqrt() * 5.0;
    for index in 0..count {
        nodes.push(node(
            index,
            [
                rng.gen_range(-extent..extent),
                rng.gen_range(-extent..extent),
                rng.gen_range(-extent * 0.2..extent * 0.2),
            ],
            rng.gen_range(1.0..3.5),
            [
                rng.gen_range(0.2..0.9),
                rng.gen_range(0.3..0.95),
                rng.gen_range(0.5..1.0),
                1.0,
            ],
            0,
        ));
    }
}

fn clustered_nodes(nodes: &mut Vec<NodeVisual>, count: usize, rng: &mut ChaCha8Rng) {
    if count == 0 {
        return;
    }
    let cluster_count = (count / 250).max(3).min(count);
    let spread = (count as f32).sqrt() * 8.0;
    let centers: Vec<_> = (0..cluster_count)
        .map(|_| {
            [
                rng.gen_range(-spread..spread),
                rng.gen_range(-spread..spread),
                rng.gen_range(-spread * 0.1..spread * 0.1),
            ]
        })
        .collect();
    for index in 0..count {
        let center = centers[index % cluster_count];
        let radius = rng.gen_range(5.0..40.0);
        let hue = (index % cluster_count) as f32 / cluster_count as f32;
        nodes.push(node(
            index,
            [
                center[0] + rng.gen_range(-radius..radius),
                center[1] + rng.gen_range(-radius..radius),
                center[2] + rng.gen_range(-radius * 0.3..radius * 0.3),
            ],
            rng.gen_range(1.2..3.0),
            [hue * 0.8 + 0.1, 1.0 - hue * 0.5, hue * 0.9 + 0.1, 1.0],
            (index % cluster_count) as u16,
        ));
    }
}

fn hub_nodes(
    nodes: &mut Vec<NodeVisual>,
    edges: &mut Vec<EdgeVisual>,
    count: usize,
    rng: &mut ChaCha8Rng,
) {
    if count == 0 {
        return;
    }
    let hub_count = (count / 500).max(1).min(count);
    for index in 0..hub_count {
        let angle = index as f32 / hub_count as f32 * std::f32::consts::TAU;
        nodes.push(node(
            index,
            [angle.cos() * 150.0, angle.sin() * 150.0, 0.0],
            8.0,
            [1.0, 0.3, 0.2, 1.0],
            1,
        ));
    }
    for index in hub_count..count {
        let hub_index = index % hub_count;
        let hub = nodes[hub_index];
        let distance = rng.gen_range(10.0..120.0);
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        nodes.push(node(
            index,
            [
                hub.position[0] + angle.cos() * distance,
                hub.position[1] + angle.sin() * distance,
                rng.gen_range(-10.0..10.0),
            ],
            rng.gen_range(1.0..2.5),
            [0.3, 0.7, 0.9, 1.0],
            0,
        ));
        edges.push(edge(edges.len(), hub.id, NodeId(index as u64 + 1)));
    }
}

fn chain_nodes(nodes: &mut Vec<NodeVisual>, edges: &mut Vec<EdgeVisual>, count: usize) {
    for index in 0..count {
        let phase = index as f32 * 0.05;
        nodes.push(node(
            index,
            [
                index as f32 * 8.0 - count as f32 * 4.0,
                (phase * 2.0).sin() * 50.0,
                (phase * 3.0).cos() * 20.0,
            ],
            2.0,
            [0.2, 0.8, 0.6, 1.0],
            0,
        ));
        if index > 0 {
            edges.push(edge(
                edges.len(),
                NodeId(index as u64),
                NodeId(index as u64 + 1),
            ));
        }
    }
}

fn community_nodes(nodes: &mut Vec<NodeVisual>, count: usize, rng: &mut ChaCha8Rng) {
    if count == 0 {
        return;
    }
    let communities = (count / 200).max(2).min(count);
    let spread = (count as f32).sqrt() * 6.0;
    for index in 0..count {
        let community = index % communities;
        let angle = community as f32 / communities as f32 * std::f32::consts::TAU;
        let hue = community as f32 / communities as f32;
        nodes.push(node(
            index,
            [
                angle.cos() * spread + rng.gen_range(-40.0..40.0),
                angle.sin() * spread + rng.gen_range(-40.0..40.0),
                rng.gen_range(-15.0..15.0),
            ],
            rng.gen_range(1.5..3.2),
            [hue, 0.7, 1.0 - hue, 1.0],
            community as u16,
        ));
    }
}

fn fill_edges(
    nodes: &[NodeVisual],
    edges: &mut Vec<EdgeVisual>,
    requested: usize,
    rng: &mut ChaCha8Rng,
) {
    if nodes.len() < 2 {
        return;
    }
    let maximum = nodes.len().saturating_mul(nodes.len() - 1);
    let target = requested.min(maximum);
    let mut pairs: HashSet<_> = edges
        .iter()
        .map(|edge| (edge.source, edge.target))
        .collect();
    while edges.len() < target {
        let source = nodes[rng.gen_range(0..nodes.len())].id;
        let target_node = nodes[rng.gen_range(0..nodes.len())].id;
        if source != target_node && pairs.insert((source, target_node)) {
            edges.push(edge(edges.len(), source, target_node));
        }
    }
}

fn node(index: usize, position: [f32; 3], radius: f32, color: [f32; 4], kind: u16) -> NodeVisual {
    NodeVisual {
        id: NodeId(index as u64 + 1),
        position,
        radius,
        color,
        kind,
        flags: 0,
    }
}

fn edge(index: usize, source: NodeId, target: NodeId) -> EdgeVisual {
    EdgeVisual {
        id: EdgeId(index as u64 + 1),
        source,
        target,
        width: 1.1,
        color: [0.42, 0.56, 0.78, 0.28],
        kind: 0,
        flags: 0,
    }
}
