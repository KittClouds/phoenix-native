//! Deterministic visual contract receipts for the native scene product.
//!
//! This is deliberately separate from the archive format.  The archive keeps
//! the compact V1 pages readable; this receipt proves that the pages were
//! interpreted as the intended entity, structure, fact, discourse, and
//! relation lanes before the publication becomes resident.

use phoenix_scene_contract::{
    describe_edge, describe_node, VisualEdgeKind, VisualNodeKind, VisualNodeLane,
    VISUAL_GRAPH_CONTRACT_V3,
};
use phoenix_scene_publisher::{NativeScenePublication, ScenePublicationReceipt};
use thiserror::Error;

pub const VISUAL_NODE_KIND_COUNT: usize = 23;
pub const VISUAL_EDGE_KIND_COUNT: usize = 13;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VisualLaneCount {
    pub nodes: u64,
    pub edges: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisualContractReceiptV3 {
    pub contract: &'static str,
    pub scene_generation_id: u64,
    pub source_generation_hash: [u8; 32],
    pub archive_hash: [u8; 32],
    pub product_index_hash: [u8; 32],
    pub node_identity_hash: [u8; 32],
    pub edge_topology_hash: [u8; 32],
    pub node_role_hash: [u8; 32],
    pub edge_role_hash: [u8; 32],
    pub projection_hash: [u8; 32],
    pub node_count: u64,
    pub edge_count: u64,
    pub lane_counts: [VisualLaneCount; 4],
    pub node_kind_counts: [u64; VISUAL_NODE_KIND_COUNT],
    pub edge_kind_counts: [u64; VISUAL_EDGE_KIND_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisualContractDraftV3 {
    pub contract: &'static str,
    pub scene_generation_id: u64,
    pub node_identity_hash: [u8; 32],
    pub edge_topology_hash: [u8; 32],
    pub node_role_hash: [u8; 32],
    pub edge_role_hash: [u8; 32],
    pub projection_hash: [u8; 32],
    pub node_count: u64,
    pub edge_count: u64,
    pub lane_counts: [VisualLaneCount; 4],
    pub node_kind_counts: [u64; VISUAL_NODE_KIND_COUNT],
    pub edge_kind_counts: [u64; VISUAL_EDGE_KIND_COUNT],
}

impl VisualContractDraftV3 {
    pub fn from_publication(
        publication: &NativeScenePublication,
    ) -> Result<Self, VisualContractError> {
        publication
            .validate()
            .map_err(|error| VisualContractError::InvalidPublication(error.to_string()))?;
        let node_count = publication.identities.len() as u64;
        let edge_count = publication.edges.len() as u64;
        let mut lane_counts = [VisualLaneCount::default(); 4];
        let mut node_kind_counts = [0_u64; VISUAL_NODE_KIND_COUNT];
        let mut edge_kind_counts = [0_u64; VISUAL_EDGE_KIND_COUNT];
        let mut node_identity = blake3::Hasher::new();
        let mut node_roles = blake3::Hasher::new();
        let mut projection = blake3::Hasher::new();
        node_identity.update(b"phoenix-native-v3-node-identities");
        node_roles.update(b"phoenix-native-v3-node-roles");
        projection.update(b"phoenix-native-v3-projection");
        for (slot, identity) in publication.identities.iter().enumerate() {
            let style = publication
                .styles
                .get(slot)
                .ok_or(VisualContractError::InventoryMismatch("node style"))?;
            let product = publication
                .node_products
                .get(slot)
                .ok_or(VisualContractError::InventoryMismatch("node product"))?;
            let descriptor = describe_node(product.family_mask);
            let lane = lane_index(descriptor.lane);
            lane_counts[lane].nodes = lane_counts[lane].nodes.saturating_add(1);
            let kind = node_kind_index(descriptor.kind);
            node_kind_counts[kind] = node_kind_counts[kind].saturating_add(1);
            hash_u64(&mut node_identity, identity.id);
            hash_u64(&mut node_roles, identity.id);
            hash_u64(&mut node_roles, product.family_mask);
            hash_u64(&mut node_roles, product.scope_mask);
            hash_u32(&mut node_roles, product.review_mask);
            hash_u16(&mut node_roles, style.kind);
            hash_u16(&mut node_roles, style.flags);
            hash_u8(&mut node_roles, descriptor.lane as u8);
            hash_u8(&mut node_roles, descriptor.kind as u8);
            hash_u64(&mut node_roles, descriptor.detail_mask);
            hash_u64(&mut projection, identity.id);
            hash_f32(&mut projection, style.radius);
            for color in style.color {
                hash_f32(&mut projection, color);
            }
            for position_page in &publication.positions {
                let position = position_page
                    .get(slot)
                    .ok_or(VisualContractError::InventoryMismatch("node position"))?;
                for coordinate in position.position {
                    hash_f32(&mut projection, coordinate);
                }
            }
        }
        let mut edge_topology = blake3::Hasher::new();
        let mut edge_roles = blake3::Hasher::new();
        edge_topology.update(b"phoenix-native-v3-edge-topology");
        edge_roles.update(b"phoenix-native-v3-edge-roles");
        for (slot, edge) in publication.edges.iter().enumerate() {
            let topology = publication
                .topology
                .get(slot)
                .ok_or(VisualContractError::InventoryMismatch("edge topology"))?;
            let product = publication
                .edge_products
                .get(slot)
                .ok_or(VisualContractError::InventoryMismatch("edge product"))?;
            let descriptor = describe_edge(product.relation_mask);
            let lane = edge_lane_index(descriptor.kind);
            lane_counts[lane].edges = lane_counts[lane].edges.saturating_add(1);
            let kind = edge_kind_index(descriptor.kind);
            edge_kind_counts[kind] = edge_kind_counts[kind].saturating_add(1);
            hash_u64(&mut edge_topology, edge.id);
            hash_u64(&mut edge_topology, topology.source_id);
            hash_u64(&mut edge_topology, topology.target_id);
            hash_u64(&mut edge_roles, edge.id);
            hash_u64(&mut edge_roles, product.family_mask);
            hash_u64(&mut edge_roles, product.scope_mask);
            hash_u64(&mut edge_roles, product.relation_mask);
            hash_u32(&mut edge_roles, product.review_mask);
            hash_u16(&mut edge_roles, edge.kind);
            hash_u16(&mut edge_roles, edge.flags);
            hash_u8(&mut edge_roles, descriptor.kind as u8);
        }
        Ok(Self {
            contract: VISUAL_GRAPH_CONTRACT_V3,
            scene_generation_id: publication.generation_id,
            node_identity_hash: *node_identity.finalize().as_bytes(),
            edge_topology_hash: *edge_topology.finalize().as_bytes(),
            node_role_hash: *node_roles.finalize().as_bytes(),
            edge_role_hash: *edge_roles.finalize().as_bytes(),
            projection_hash: *projection.finalize().as_bytes(),
            node_count,
            edge_count,
            lane_counts,
            node_kind_counts,
            edge_kind_counts,
        })
    }

    pub fn bind_publication(
        self,
        publication: ScenePublicationReceipt,
        source_generation_hash: [u8; 32],
    ) -> Result<VisualContractReceiptV3, VisualContractError> {
        if self.scene_generation_id != publication.generation_id
            || self.node_count != publication.node_count
            || self.edge_count != publication.edge_count
        {
            return Err(VisualContractError::PublicationMismatch);
        }
        Ok(VisualContractReceiptV3 {
            contract: self.contract,
            scene_generation_id: self.scene_generation_id,
            source_generation_hash,
            archive_hash: publication.archive_cohort_hash,
            product_index_hash: publication.product_index_hash,
            node_identity_hash: self.node_identity_hash,
            edge_topology_hash: self.edge_topology_hash,
            node_role_hash: self.node_role_hash,
            edge_role_hash: self.edge_role_hash,
            projection_hash: self.projection_hash,
            node_count: self.node_count,
            edge_count: self.edge_count,
            lane_counts: self.lane_counts,
            node_kind_counts: self.node_kind_counts,
            edge_kind_counts: self.edge_kind_counts,
        })
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum VisualContractError {
    #[error("native visual publication has an invalid {0} inventory")]
    InventoryMismatch(&'static str),
    #[error("native visual publication is invalid: {0}")]
    InvalidPublication(String),
    #[error("native visual receipt does not match the publication")]
    PublicationMismatch,
}

fn lane_index(lane: VisualNodeLane) -> usize {
    match lane {
        VisualNodeLane::Entities => 0,
        VisualNodeLane::Structure => 1,
        VisualNodeLane::Facts => 2,
        VisualNodeLane::Discourse => 3,
    }
}

fn edge_lane_index(kind: VisualEdgeKind) -> usize {
    match kind {
        VisualEdgeKind::Structural => 1,
        VisualEdgeKind::Unknown => 3,
        _ => 2,
    }
}

fn node_kind_index(kind: VisualNodeKind) -> usize {
    match kind {
        VisualNodeKind::EntityCharacter => 0,
        VisualNodeKind::EntityLocation => 1,
        VisualNodeKind::EntityNetwork => 2,
        VisualNodeKind::EntityCreature => 3,
        VisualNodeKind::EntityNpc => 4,
        VisualNodeKind::EntityEvent => 5,
        VisualNodeKind::EntityConcept => 6,
        VisualNodeKind::EntityOther => 7,
        VisualNodeKind::Document => 8,
        VisualNodeKind::Episode => 9,
        VisualNodeKind::Chapter => 10,
        VisualNodeKind::Paragraph => 11,
        VisualNodeKind::Sentence => 12,
        VisualNodeKind::Chunk => 13,
        VisualNodeKind::Evidence => 14,
        VisualNodeKind::EventFact => 15,
        VisualNodeKind::RelationshipFact => 16,
        VisualNodeKind::TemporalFact => 17,
        VisualNodeKind::CausalFact => 18,
        VisualNodeKind::MemoryStateFact => 19,
        VisualNodeKind::IdentityDiscourse => 20,
        VisualNodeKind::ContextualDiscourse => 21,
        VisualNodeKind::Unknown => 22,
    }
}

fn edge_kind_index(kind: VisualEdgeKind) -> usize {
    match kind {
        VisualEdgeKind::Structural => 0,
        VisualEdgeKind::CoOccurrence => 1,
        VisualEdgeKind::Observation => 2,
        VisualEdgeKind::Communication => 3,
        VisualEdgeKind::Authority => 4,
        VisualEdgeKind::Relationship => 5,
        VisualEdgeKind::Identity => 6,
        VisualEdgeKind::Event => 7,
        VisualEdgeKind::Temporal => 8,
        VisualEdgeKind::Causal => 9,
        VisualEdgeKind::MemoryState => 10,
        VisualEdgeKind::Candidate => 11,
        VisualEdgeKind::Unknown => 12,
    }
}

fn hash_u8(hasher: &mut blake3::Hasher, value: u8) {
    hasher.update(&[value]);
}

fn hash_u16(hasher: &mut blake3::Hasher, value: u16) {
    hasher.update(&value.to_le_bytes());
}

fn hash_u32(hasher: &mut blake3::Hasher, value: u32) {
    hasher.update(&value.to_le_bytes());
}

fn hash_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn hash_f32(hasher: &mut blake3::Hasher, value: f32) {
    hasher.update(&value.to_bits().to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_archive::{NodeIdentityRecord, NodeStyleRecord, PositionRecord};
    use phoenix_scene_publisher::{SceneNodeProduct, ScenePublicationKind};
    use std::sync::Arc;

    fn publication() -> NativeScenePublication {
        let positions = std::array::from_fn(|_| {
            vec![PositionRecord {
                position: [1.0, 2.0, 3.0],
            }]
        });
        NativeScenePublication {
            generation_id: 7,
            kind: ScenePublicationKind::Full,
            registry_revision: 2,
            document_id: Some(9),
            identities: vec![NodeIdentityRecord { id: 1 }],
            styles: vec![NodeStyleRecord {
                color: [0.2, 0.4, 0.8, 1.0],
                radius: 1.0,
                kind: 0,
                flags: 0,
            }],
            topology: vec![],
            edges: vec![],
            positions,
            node_products: vec![SceneNodeProduct {
                node_id: 1,
                family_mask: phoenix_scene_contract::FamilyMask::ENTITIES.0
                    | phoenix_scene_contract::FamilyMask::CHARACTERS.0,
                scope_mask: 1,
                review_mask: 1,
                label: Arc::from("one"),
                inspector_ref: 0,
                provenance_ref: 0,
            }],
            edge_products: vec![],
            entity_mappings: vec![],
            references: vec![],
        }
    }

    #[test]
    fn deterministic_receipt_counts_entity_lane() {
        let draft = VisualContractDraftV3::from_publication(&publication()).unwrap();
        assert_eq!(draft.node_count, 1);
        assert_eq!(draft.lane_counts[0].nodes, 1);
        assert_eq!(draft.node_kind_counts[0], 1);
    }

    #[test]
    fn projection_digest_changes_when_position_changes() {
        let first = publication();
        let mut second = publication();
        second.positions[2][0].position[0] = 9.0;
        let first = VisualContractDraftV3::from_publication(&first).unwrap();
        let second = VisualContractDraftV3::from_publication(&second).unwrap();
        assert_eq!(first.node_identity_hash, second.node_identity_hash);
        assert_ne!(first.projection_hash, second.projection_hash);
    }
}
