use crate::{ScenePublicationError, ScenePublicationKind};
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SceneNodeProduct {
    pub node_id: u64,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub review_mask: u32,
    pub label: Arc<str>,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneEdgeProduct {
    pub edge_id: u64,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub relation_mask: u64,
    pub review_mask: u32,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
}

#[derive(Debug)]
pub struct NativeScenePublication {
    pub generation_id: u64,
    pub kind: ScenePublicationKind,
    pub registry_revision: u64,
    pub document_id: Option<u64>,
    pub identities: Vec<NodeIdentityRecord>,
    pub styles: Vec<NodeStyleRecord>,
    pub topology: Vec<TopologyRecord>,
    pub edges: Vec<EdgeRecord>,
    pub positions: [Vec<PositionRecord>; 5],
    pub node_products: Vec<SceneNodeProduct>,
    pub edge_products: Vec<SceneEdgeProduct>,
    pub entity_mappings: Vec<EntityNodeMappingRecord>,
    pub references: Vec<ProductReferenceRecord>,
}

impl NativeScenePublication {
    pub fn validate(&self) -> Result<(), ScenePublicationError> {
        if self.generation_id == 0 {
            return Err(ScenePublicationError::ZeroGeneration);
        }
        let node_count = self.identities.len();
        let edge_count = self.edges.len();
        if self.styles.len() != node_count
            || self.node_products.len() != node_count
            || self
                .positions
                .iter()
                .any(|positions| positions.len() != node_count)
        {
            return Err(ScenePublicationError::InventoryMismatch("node pages"));
        }
        if self.topology.len() != edge_count || self.edge_products.len() != edge_count {
            return Err(ScenePublicationError::InventoryMismatch("edge pages"));
        }
        for (slot, (identity, product)) in
            self.identities.iter().zip(&self.node_products).enumerate()
        {
            if identity.id == 0 || identity.id != product.node_id {
                return Err(ScenePublicationError::IdentityMismatch {
                    resource: "node",
                    slot,
                });
            }
        }
        for (slot, (edge, product)) in self.edges.iter().zip(&self.edge_products).enumerate() {
            if edge.id == 0 || edge.id != product.edge_id {
                return Err(ScenePublicationError::IdentityMismatch {
                    resource: "edge",
                    slot,
                });
            }
        }
        if self.kind == ScenePublicationKind::RegistryOnly {
            self.validate_registry_only()?;
        }
        Ok(())
    }

    fn validate_registry_only(&self) -> Result<(), ScenePublicationError> {
        if !self.topology.is_empty() || !self.edges.is_empty() || !self.edge_products.is_empty() {
            return Err(ScenePublicationError::RegistryContainsTopology);
        }
        if self.entity_mappings.len() != self.identities.len() {
            return Err(ScenePublicationError::RegistryMappingMismatch);
        }
        for ((identity, product), mapping) in self
            .identities
            .iter()
            .zip(&self.node_products)
            .zip(&self.entity_mappings)
        {
            if mapping.entity_id == 0
                || mapping.node_id != identity.id
                || product.node_id != identity.id
            {
                return Err(ScenePublicationError::RegistryMappingMismatch);
            }
        }
        Ok(())
    }
}
