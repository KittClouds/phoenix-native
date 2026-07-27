use crate::SceneState;
use std::collections::VecDeque;

const UNSET: u32 = u32::MAX;
const MAX_ROUTE_EDGES: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InteractionIndexStats {
    pub queue_capacity: usize,
    pub route_node_capacity: usize,
    pub route_edge_capacity: usize,
    pub route_nodes: usize,
    pub route_edges: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Adjacency {
    pub node_slot: u32,
    pub edge_slot: u32,
}

#[derive(Default)]
pub(crate) struct InteractionIndex {
    offsets: Vec<u32>,
    adjacency: Vec<Adjacency>,
    queue: VecDeque<u32>,
    visited: Vec<u32>,
    parents: Vec<u32>,
    parent_edges: Vec<u32>,
    visit_generation: u32,
    route_nodes: Vec<u32>,
    route_edges: Vec<u32>,
}

impl InteractionIndex {
    pub(crate) fn rebuild(&mut self, scene: &SceneState) {
        let node_count = scene.node_capacity_slots();
        self.offsets.clear();
        self.offsets.resize(node_count + 1, 0);
        for edge in scene.edges() {
            if let (Some(source), Some(target)) =
                (scene.node_slot(edge.source), scene.node_slot(edge.target))
            {
                self.offsets[source as usize + 1] += 1;
                self.offsets[target as usize + 1] += 1;
            }
        }
        for slot in 1..self.offsets.len() {
            self.offsets[slot] += self.offsets[slot - 1];
        }
        self.adjacency.clear();
        self.adjacency.resize(
            self.offsets.last().copied().unwrap_or_default() as usize,
            Adjacency {
                node_slot: 0,
                edge_slot: 0,
            },
        );
        let mut cursors = self.offsets[..node_count].to_vec();
        for edge_slot in 0..scene.edge_capacity_slots() {
            let Some(edge) = scene.edge_at_slot(edge_slot as u32) else {
                continue;
            };
            let (Some(source), Some(target)) =
                (scene.node_slot(edge.source), scene.node_slot(edge.target))
            else {
                continue;
            };
            insert_arc(
                &mut self.adjacency,
                &mut cursors,
                source,
                target,
                edge_slot as u32,
            );
            insert_arc(
                &mut self.adjacency,
                &mut cursors,
                target,
                source,
                edge_slot as u32,
            );
        }
        self.queue.clear();
        if self.queue.capacity() < node_count {
            self.queue.reserve(node_count - self.queue.capacity());
        }
        self.visited.resize(node_count, 0);
        self.parents.resize(node_count, UNSET);
        self.parent_edges.resize(node_count, UNSET);
        self.route_nodes.clear();
        self.route_edges.clear();
        if self.route_nodes.capacity() < MAX_ROUTE_EDGES + 1 {
            self.route_nodes
                .reserve_exact(MAX_ROUTE_EDGES + 1 - self.route_nodes.capacity());
        }
        if self.route_edges.capacity() < MAX_ROUTE_EDGES {
            self.route_edges
                .reserve_exact(MAX_ROUTE_EDGES - self.route_edges.capacity());
        }
    }

    pub(crate) fn neighbors(&self, node_slot: u32) -> &[Adjacency] {
        let slot = node_slot as usize;
        let Some((&start, &end)) = self.offsets.get(slot).zip(self.offsets.get(slot + 1)) else {
            return &[];
        };
        &self.adjacency[start as usize..end as usize]
    }

    pub(crate) fn compute_route(&mut self, source: u32, target: u32) {
        self.route_nodes.clear();
        self.route_edges.clear();
        if source == target
            || source as usize >= self.visited.len()
            || target as usize >= self.visited.len()
        {
            return;
        }
        self.visit_generation = self.visit_generation.wrapping_add(1).max(1);
        if self.visit_generation == 1 {
            self.visited.fill(0);
        }
        let generation = self.visit_generation;
        self.queue.clear();
        self.queue.push_back(source);
        self.visited[source as usize] = generation;
        self.parents[source as usize] = UNSET;
        let mut found = false;
        while let Some(node) = self.queue.pop_front() {
            if node == target {
                found = true;
                break;
            }
            let start = self.offsets[node as usize] as usize;
            let end = self.offsets[node as usize + 1] as usize;
            for arc in &self.adjacency[start..end] {
                let next = arc.node_slot as usize;
                if self.visited[next] == generation {
                    continue;
                }
                self.visited[next] = generation;
                self.parents[next] = node;
                self.parent_edges[next] = arc.edge_slot;
                self.queue.push_back(arc.node_slot);
            }
        }
        if !found {
            return;
        }
        let mut cursor = target;
        self.route_nodes.push(cursor);
        while cursor != source && self.route_edges.len() < MAX_ROUTE_EDGES {
            let parent = self.parents[cursor as usize];
            let edge = self.parent_edges[cursor as usize];
            if parent == UNSET || edge == UNSET {
                self.route_nodes.clear();
                self.route_edges.clear();
                return;
            }
            self.route_edges.push(edge);
            cursor = parent;
            self.route_nodes.push(cursor);
        }
        if cursor != source {
            self.route_nodes.clear();
            self.route_edges.clear();
        }
    }

    pub(crate) fn route_nodes(&self) -> &[u32] {
        &self.route_nodes
    }

    pub(crate) fn route_edges(&self) -> &[u32] {
        &self.route_edges
    }

    pub(crate) fn stats(&self) -> InteractionIndexStats {
        InteractionIndexStats {
            queue_capacity: self.queue.capacity(),
            route_node_capacity: self.route_nodes.capacity(),
            route_edge_capacity: self.route_edges.capacity(),
            route_nodes: self.route_nodes.len(),
            route_edges: self.route_edges.len(),
        }
    }
}

fn insert_arc(
    adjacency: &mut [Adjacency],
    cursors: &mut [u32],
    source: u32,
    target: u32,
    edge_slot: u32,
) {
    let cursor = &mut cursors[source as usize];
    adjacency[*cursor as usize] = Adjacency {
        node_slot: target,
        edge_slot,
    };
    *cursor += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_model::{EdgeId, EdgeVisual, GraphRevision, GraphSnapshot, NodeId, NodeVisual};

    #[test]
    fn bounded_route_uses_stable_slots() -> Result<(), crate::RenderError> {
        let nodes = (1..=4)
            .map(|id| NodeVisual {
                id: NodeId(id),
                position: [id as f32, 0.0, 0.0],
                radius: 1.0,
                color: [1.0; 4],
                kind: 1,
                flags: 0,
            })
            .collect();
        let edges = [(1, 2), (2, 3), (3, 4)]
            .into_iter()
            .enumerate()
            .map(|(slot, (source, target))| EdgeVisual {
                id: EdgeId(slot as u64 + 1),
                source: NodeId(source),
                target: NodeId(target),
                width: 1.0,
                color: [1.0; 4],
                kind: 1,
                flags: 0,
            })
            .collect();
        let mut scene = SceneState::default();
        scene.set_snapshot(&GraphSnapshot {
            revision: GraphRevision(1),
            nodes,
            edges,
        })?;
        let mut index = InteractionIndex::default();
        index.rebuild(&scene);
        index.compute_route(0, 3);
        assert_eq!(index.route_edges().len(), 3);
        assert_eq!(index.route_nodes().len(), 4);
        Ok(())
    }
}
