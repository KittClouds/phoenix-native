use crate::{EdgeId, GraphInventory, GraphRevision, ModelError, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct NodeVisual {
    pub id: NodeId,
    pub position: [f32; 3],
    pub radius: f32,
    pub color: [f32; 4],
    pub kind: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct EdgeVisual {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub width: f32,
    pub color: [f32; 4],
    pub kind: u16,
    pub flags: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GraphSnapshot {
    pub revision: GraphRevision,
    pub nodes: Vec<NodeVisual>,
    pub edges: Vec<EdgeVisual>,
}

impl GraphSnapshot {
    #[must_use]
    pub fn new(revision: GraphRevision, nodes: Vec<NodeVisual>, edges: Vec<EdgeVisual>) -> Self {
        Self {
            revision,
            nodes,
            edges,
        }
    }

    pub fn validate(&self) -> Result<GraphInventory, ModelError> {
        GraphInventory::from_snapshot(self)
    }
}
