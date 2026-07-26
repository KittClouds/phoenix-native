use crate::{EdgeId, EdgeVisual, GraphRevision, NodeId, NodeVisual};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GraphDiff {
    pub revision: GraphRevision,
    pub added_nodes: Vec<NodeVisual>,
    pub updated_nodes: Vec<NodeVisual>,
    pub removed_nodes: Vec<NodeId>,
    pub added_edges: Vec<EdgeVisual>,
    pub updated_edges: Vec<EdgeVisual>,
    pub removed_edges: Vec<EdgeId>,
}

impl GraphDiff {
    #[must_use]
    pub fn new(revision: GraphRevision) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.updated_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.updated_edges.is_empty()
            && self.removed_edges.is_empty()
    }
}
