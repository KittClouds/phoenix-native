use crate::RenderError;
use graph_model::{
    validate_edge, validate_node, EdgeId, EdgeVisual, GraphDiff, GraphRevision, GraphSnapshot,
    ModelError, NodeId, NodeVisual,
};
use hashbrown::{HashMap, HashSet};
use phoenix_scene_archive::{ManifoldPageSet, PositionRecord};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SceneChanges {
    pub dirty_node_slots: Vec<u32>,
    pub dirty_edge_slots: Vec<u32>,
    pub nodes_added: usize,
    pub nodes_updated: usize,
    pub nodes_removed: usize,
    pub edges_added: usize,
    pub edges_updated: usize,
    pub edges_removed: usize,
}

#[derive(Debug, Default)]
pub struct SceneState {
    revision: Option<GraphRevision>,
    node_slots: HashMap<NodeId, u32>,
    nodes: Vec<Option<NodeVisual>>,
    free_node_slots: Vec<u32>,
    edge_slots: HashMap<EdgeId, u32>,
    edges: Vec<Option<EdgeVisual>>,
    free_edge_slots: Vec<u32>,
}

impl SceneState {
    #[must_use]
    pub fn revision(&self) -> Option<GraphRevision> {
        self.revision
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.node_slots.len()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edge_slots.len()
    }

    #[must_use]
    pub fn node_capacity_slots(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn edge_capacity_slots(&self) -> usize {
        self.edges.len()
    }

    #[must_use]
    pub fn node_slot(&self, id: NodeId) -> Option<u32> {
        self.node_slots.get(&id).copied()
    }

    #[must_use]
    pub fn edge_slot(&self, id: EdgeId) -> Option<u32> {
        self.edge_slots.get(&id).copied()
    }

    #[must_use]
    pub fn node_at_slot(&self, slot: u32) -> Option<&NodeVisual> {
        self.nodes.get(slot as usize)?.as_ref()
    }

    #[must_use]
    pub fn edge_at_slot(&self, slot: u32) -> Option<&EdgeVisual> {
        self.edges.get(slot as usize)?.as_ref()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &NodeVisual> + Clone {
        self.nodes.iter().filter_map(Option::as_ref)
    }

    pub fn nodes_with_slots(&self) -> impl Iterator<Item = (usize, &NodeVisual)> + Clone {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(slot, node)| node.as_ref().map(|node| (slot, node)))
    }

    pub fn edges(&self) -> impl Iterator<Item = &EdgeVisual> {
        self.edges.iter().filter_map(Option::as_ref)
    }

    pub fn set_snapshot(&mut self, snapshot: &GraphSnapshot) -> Result<(), RenderError> {
        if let Some(current) = self.revision {
            if snapshot.revision <= current {
                return Err(ModelError::StaleRevision {
                    current,
                    incoming: snapshot.revision,
                }
                .into());
            }
        }
        snapshot.validate()?;

        let mut replacement = Self {
            revision: Some(snapshot.revision),
            node_slots: HashMap::with_capacity(snapshot.nodes.len()),
            nodes: Vec::with_capacity(snapshot.nodes.len()),
            free_node_slots: Vec::new(),
            edge_slots: HashMap::with_capacity(snapshot.edges.len()),
            edges: Vec::with_capacity(snapshot.edges.len()),
            free_edge_slots: Vec::new(),
        };

        for &node in &snapshot.nodes {
            let slot = checked_slot(replacement.nodes.len(), "node")?;
            replacement.node_slots.insert(node.id, slot);
            replacement.nodes.push(Some(node));
        }
        for &edge in &snapshot.edges {
            let slot = checked_slot(replacement.edges.len(), "edge")?;
            replacement.edge_slots.insert(edge.id, slot);
            replacement.edges.push(Some(edge));
        }
        *self = replacement;
        Ok(())
    }

    pub fn set_archive_pages(
        &mut self,
        revision: GraphRevision,
        pages: &ManifoldPageSet<'_>,
    ) -> Result<(), RenderError> {
        if let Some(current) = self.revision {
            if revision <= current {
                return Err(ModelError::StaleRevision {
                    current,
                    incoming: revision,
                }
                .into());
            }
        }
        if pages.identities.len() != pages.styles.len() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "node styles",
                expected: pages.identities.len(),
                actual: pages.styles.len(),
            });
        }
        if pages.identities.len() != pages.positions.len() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "positions",
                expected: pages.identities.len(),
                actual: pages.positions.len(),
            });
        }
        if pages.edges.len() != pages.topology.len() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "topology",
                expected: pages.edges.len(),
                actual: pages.topology.len(),
            });
        }

        let mut replacement = Self {
            revision: Some(revision),
            node_slots: HashMap::with_capacity(pages.identities.len()),
            nodes: Vec::with_capacity(pages.identities.len()),
            free_node_slots: Vec::new(),
            edge_slots: HashMap::with_capacity(pages.edges.len()),
            edges: Vec::with_capacity(pages.edges.len()),
            free_edge_slots: Vec::new(),
        };
        for ((identity, style), position) in pages
            .identities
            .iter()
            .zip(pages.styles)
            .zip(pages.positions)
        {
            let node = NodeVisual {
                id: NodeId(identity.id),
                position: position.position,
                radius: style.radius,
                color: style.color,
                kind: style.kind,
                flags: style.flags,
            };
            validate_node(&node)?;
            let slot = checked_slot(replacement.nodes.len(), "node")?;
            if replacement.node_slots.insert(node.id, slot).is_some() {
                return Err(ModelError::DuplicateNode(node.id).into());
            }
            replacement.nodes.push(Some(node));
        }
        for (record, topology) in pages.edges.iter().zip(pages.topology) {
            let edge = EdgeVisual {
                id: EdgeId(record.id),
                source: NodeId(topology.source_id),
                target: NodeId(topology.target_id),
                width: record.width,
                color: record.color,
                kind: record.kind,
                flags: record.flags,
            };
            validate_edge(&edge)?;
            if !replacement.node_slots.contains_key(&edge.source) {
                return Err(ModelError::MissingSourceNode {
                    edge_id: edge.id,
                    source_id: edge.source,
                }
                .into());
            }
            if !replacement.node_slots.contains_key(&edge.target) {
                return Err(ModelError::MissingTargetNode {
                    edge_id: edge.id,
                    target_id: edge.target,
                }
                .into());
            }
            let slot = checked_slot(replacement.edges.len(), "edge")?;
            if replacement.edge_slots.insert(edge.id, slot).is_some() {
                return Err(ModelError::DuplicateEdge(edge.id).into());
            }
            replacement.edges.push(Some(edge));
        }
        *self = replacement;
        Ok(())
    }

    pub fn update_packed_positions(
        &mut self,
        positions: &[PositionRecord],
    ) -> Result<(), RenderError> {
        if self.node_slots.len() != self.nodes.len() {
            return Err(RenderError::PackedSceneFragmented);
        }
        if positions.len() != self.nodes.len() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "positions",
                expected: self.nodes.len(),
                actual: positions.len(),
            });
        }
        for (slot, (node, position)) in self.nodes.iter_mut().zip(positions).enumerate() {
            if !position
                .position
                .iter()
                .all(|coordinate| coordinate.is_finite())
            {
                return Err(RenderError::InvalidPackedPosition { slot });
            }
            let node = node.as_mut().ok_or(RenderError::PackedSceneFragmented)?;
            node.position = position.position;
        }
        Ok(())
    }

    pub fn apply_diff(&mut self, diff: GraphDiff) -> Result<SceneChanges, RenderError> {
        self.validate_diff(&diff)?;
        let mut changes = SceneChanges {
            nodes_added: diff.added_nodes.len(),
            nodes_updated: diff.updated_nodes.len(),
            nodes_removed: diff.removed_nodes.len(),
            edges_added: diff.added_edges.len(),
            edges_updated: diff.updated_edges.len(),
            edges_removed: diff.removed_edges.len(),
            ..SceneChanges::default()
        };

        for id in diff.removed_edges {
            let slot = self
                .edge_slots
                .remove(&id)
                .ok_or(ModelError::EdgeNotFound(id))?;
            self.edges[slot as usize] = None;
            self.free_edge_slots.push(slot);
            changes.dirty_edge_slots.push(slot);
        }
        for id in diff.removed_nodes {
            let slot = self
                .node_slots
                .remove(&id)
                .ok_or(ModelError::NodeNotFound(id))?;
            self.nodes[slot as usize] = None;
            self.free_node_slots.push(slot);
            changes.dirty_node_slots.push(slot);
        }
        for node in diff.added_nodes {
            let slot = self.allocate_node_slot()?;
            self.node_slots.insert(node.id, slot);
            self.nodes[slot as usize] = Some(node);
            changes.dirty_node_slots.push(slot);
        }
        for node in diff.updated_nodes {
            let slot = self.node_slots[&node.id];
            self.nodes[slot as usize] = Some(node);
            changes.dirty_node_slots.push(slot);
        }
        for edge in diff.added_edges {
            let slot = self.allocate_edge_slot()?;
            self.edge_slots.insert(edge.id, slot);
            self.edges[slot as usize] = Some(edge);
            changes.dirty_edge_slots.push(slot);
        }
        for edge in diff.updated_edges {
            let slot = self.edge_slots[&edge.id];
            self.edges[slot as usize] = Some(edge);
            changes.dirty_edge_slots.push(slot);
        }

        changes.dirty_node_slots.sort_unstable();
        changes.dirty_node_slots.dedup();
        changes.dirty_edge_slots.sort_unstable();
        changes.dirty_edge_slots.dedup();
        self.revision = Some(diff.revision);
        Ok(changes)
    }

    fn validate_diff(&self, diff: &GraphDiff) -> Result<(), RenderError> {
        let current = self.revision.ok_or(RenderError::SceneUninitialized)?;
        if diff.revision <= current {
            return Err(ModelError::StaleRevision {
                current,
                incoming: diff.revision,
            }
            .into());
        }

        let mut node_lanes = HashSet::with_capacity(
            diff.added_nodes.len() + diff.updated_nodes.len() + diff.removed_nodes.len(),
        );
        for node in &diff.added_nodes {
            validate_node(node)?;
            insert_node_lane(&mut node_lanes, node.id)?;
            if self.node_slots.contains_key(&node.id) {
                return Err(ModelError::NodeAlreadyExists(node.id).into());
            }
        }
        for node in &diff.updated_nodes {
            validate_node(node)?;
            insert_node_lane(&mut node_lanes, node.id)?;
            if !self.node_slots.contains_key(&node.id) {
                return Err(ModelError::NodeNotFound(node.id).into());
            }
        }
        for &id in &diff.removed_nodes {
            insert_node_lane(&mut node_lanes, id)?;
            if !self.node_slots.contains_key(&id) {
                return Err(ModelError::NodeNotFound(id).into());
            }
        }

        let removed_nodes: HashSet<_> = diff.removed_nodes.iter().copied().collect();
        let added_nodes: HashSet<_> = diff.added_nodes.iter().map(|node| node.id).collect();
        let node_exists = |id: NodeId| {
            added_nodes.contains(&id)
                || (self.node_slots.contains_key(&id) && !removed_nodes.contains(&id))
        };

        let mut edge_lanes = HashSet::with_capacity(
            diff.added_edges.len() + diff.updated_edges.len() + diff.removed_edges.len(),
        );
        for edge in &diff.added_edges {
            validate_edge(edge)?;
            insert_edge_lane(&mut edge_lanes, edge.id)?;
            if self.edge_slots.contains_key(&edge.id) {
                return Err(ModelError::EdgeAlreadyExists(edge.id).into());
            }
            validate_predicted_endpoints(edge, &node_exists)?;
        }
        for edge in &diff.updated_edges {
            validate_edge(edge)?;
            insert_edge_lane(&mut edge_lanes, edge.id)?;
            if !self.edge_slots.contains_key(&edge.id) {
                return Err(ModelError::EdgeNotFound(edge.id).into());
            }
            validate_predicted_endpoints(edge, &node_exists)?;
        }
        for &id in &diff.removed_edges {
            insert_edge_lane(&mut edge_lanes, id)?;
            if !self.edge_slots.contains_key(&id) {
                return Err(ModelError::EdgeNotFound(id).into());
            }
        }

        if !removed_nodes.is_empty() {
            let removed_edges: HashSet<_> = diff.removed_edges.iter().copied().collect();
            let updated_edges: HashSet<_> = diff.updated_edges.iter().map(|edge| edge.id).collect();
            for edge in self.edges() {
                if removed_edges.contains(&edge.id) || updated_edges.contains(&edge.id) {
                    continue;
                }
                if removed_nodes.contains(&edge.source) {
                    return Err(ModelError::IncidentEdgeNotRemoved {
                        node_id: edge.source,
                        edge_id: edge.id,
                    }
                    .into());
                }
                if removed_nodes.contains(&edge.target) {
                    return Err(ModelError::IncidentEdgeNotRemoved {
                        node_id: edge.target,
                        edge_id: edge.id,
                    }
                    .into());
                }
            }
        }
        Ok(())
    }

    fn allocate_node_slot(&mut self) -> Result<u32, RenderError> {
        if let Some(slot) = self.free_node_slots.pop() {
            return Ok(slot);
        }
        let slot = checked_slot(self.nodes.len(), "node")?;
        self.nodes.push(None);
        Ok(slot)
    }

    fn allocate_edge_slot(&mut self) -> Result<u32, RenderError> {
        if let Some(slot) = self.free_edge_slots.pop() {
            return Ok(slot);
        }
        let slot = checked_slot(self.edges.len(), "edge")?;
        self.edges.push(None);
        Ok(slot)
    }
}

fn checked_slot(length: usize, resource: &'static str) -> Result<u32, RenderError> {
    u32::try_from(length).map_err(|_| RenderError::SlotLimit {
        resource,
        limit: u64::from(u32::MAX),
    })
}

fn insert_node_lane(lanes: &mut HashSet<NodeId>, id: NodeId) -> Result<(), ModelError> {
    if lanes.insert(id) {
        Ok(())
    } else {
        Err(ModelError::ConflictingNodeMutation(id))
    }
}

fn insert_edge_lane(lanes: &mut HashSet<EdgeId>, id: EdgeId) -> Result<(), ModelError> {
    if lanes.insert(id) {
        Ok(())
    } else {
        Err(ModelError::ConflictingEdgeMutation(id))
    }
}

fn validate_predicted_endpoints(
    edge: &EdgeVisual,
    exists: &impl Fn(NodeId) -> bool,
) -> Result<(), ModelError> {
    if !exists(edge.source) {
        return Err(ModelError::MissingSourceNode {
            edge_id: edge.id,
            source_id: edge.source,
        });
    }
    if !exists(edge.target) {
        return Err(ModelError::MissingTargetNode {
            edge_id: edge.id,
            target_id: edge.target,
        });
    }
    Ok(())
}
