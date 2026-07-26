use crate::{EdgeId, EdgeVisual, GraphRevision, GraphSnapshot, NodeId, NodeVisual};
use hashbrown::HashSet;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ModelError {
    #[error("duplicate node ID: {0}")]
    DuplicateNode(NodeId),
    #[error("duplicate edge ID: {0}")]
    DuplicateEdge(EdgeId),
    #[error("edge {edge_id} references missing source node {source_id}")]
    MissingSourceNode { edge_id: EdgeId, source_id: NodeId },
    #[error("edge {edge_id} references missing target node {target_id}")]
    MissingTargetNode { edge_id: EdgeId, target_id: NodeId },
    #[error("node {0} contains non-finite position, radius, or color data")]
    InvalidNodeVisual(NodeId),
    #[error("node {0} has a negative radius")]
    NegativeNodeRadius(NodeId),
    #[error("edge {0} contains non-finite width or color data")]
    InvalidEdgeVisual(EdgeId),
    #[error("edge {0} has a negative width")]
    NegativeEdgeWidth(EdgeId),
    #[error("stale revision: incoming {incoming} is not greater than current {current}")]
    StaleRevision {
        current: GraphRevision,
        incoming: GraphRevision,
    },
    #[error("node {0} already exists")]
    NodeAlreadyExists(NodeId),
    #[error("edge {0} already exists")]
    EdgeAlreadyExists(EdgeId),
    #[error("node {0} does not exist")]
    NodeNotFound(NodeId),
    #[error("edge {0} does not exist")]
    EdgeNotFound(EdgeId),
    #[error("node {0} appears in more than one mutation lane")]
    ConflictingNodeMutation(NodeId),
    #[error("edge {0} appears in more than one mutation lane")]
    ConflictingEdgeMutation(EdgeId),
    #[error("removing node {node_id} would leave edge {edge_id} dangling")]
    IncidentEdgeNotRemoved { node_id: NodeId, edge_id: EdgeId },
}

#[derive(Debug)]
pub struct GraphInventory {
    pub node_ids: HashSet<NodeId>,
    pub edge_ids: HashSet<EdgeId>,
}

impl GraphInventory {
    pub fn from_snapshot(snapshot: &GraphSnapshot) -> Result<Self, ModelError> {
        let mut node_ids = HashSet::with_capacity(snapshot.nodes.len());
        for node in &snapshot.nodes {
            validate_node(node)?;
            if !node_ids.insert(node.id) {
                return Err(ModelError::DuplicateNode(node.id));
            }
        }

        let mut edge_ids = HashSet::with_capacity(snapshot.edges.len());
        for edge in &snapshot.edges {
            validate_edge(edge)?;
            if !edge_ids.insert(edge.id) {
                return Err(ModelError::DuplicateEdge(edge.id));
            }
            validate_endpoints(edge, &node_ids)?;
        }
        Ok(Self { node_ids, edge_ids })
    }
}

pub fn validate_node(node: &NodeVisual) -> Result<(), ModelError> {
    if !node
        .position
        .iter()
        .chain(node.color.iter())
        .all(|v| v.is_finite())
        || !node.radius.is_finite()
    {
        return Err(ModelError::InvalidNodeVisual(node.id));
    }
    if node.radius < 0.0 {
        return Err(ModelError::NegativeNodeRadius(node.id));
    }
    Ok(())
}

pub fn validate_edge(edge: &EdgeVisual) -> Result<(), ModelError> {
    if !edge.width.is_finite() || !edge.color.iter().all(|v| v.is_finite()) {
        return Err(ModelError::InvalidEdgeVisual(edge.id));
    }
    if edge.width < 0.0 {
        return Err(ModelError::NegativeEdgeWidth(edge.id));
    }
    Ok(())
}

pub fn validate_endpoints(edge: &EdgeVisual, node_ids: &HashSet<NodeId>) -> Result<(), ModelError> {
    if !node_ids.contains(&edge.source) {
        return Err(ModelError::MissingSourceNode {
            edge_id: edge.id,
            source_id: edge.source,
        });
    }
    if !node_ids.contains(&edge.target) {
        return Err(ModelError::MissingTargetNode {
            edge_id: edge.id,
            target_id: edge.target,
        });
    }
    Ok(())
}
