use crate::{
    describe_edge, describe_node, FamilyMask, Manifold, RelationFamily, RelationMask, ReviewMask,
    SceneContractError, VisualEdgeKind, VisualNodeKind, VisualNodeLane,
};
use hashbrown::HashSet;
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, ManifoldPageSet, NodeIdentityRecord, PageKey, PageKind,
    PhoenixSceneArchiveV1, RelationMaskRecord, TopologyRecord,
};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use thiserror::Error;

pub const TOPOLOGY_INVENTORY_CONTRACT: &str = "phoenix.native.topology-inventory/v1";

/// A compact census of the one canonical topology consumed by every manifold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopologyInventoryCensus {
    node_kind_counts: [u32; 256],
    relation_counts: [u32; RelationFamily::ALL.len()],
    pub node_count: usize,
    pub edge_count: usize,
    pub manifold_count: u8,
}

impl TopologyInventoryCensus {
    #[must_use]
    pub const fn node_kind_count(&self, kind: VisualNodeKind) -> u32 {
        self.node_kind_counts[kind as usize]
    }

    #[must_use]
    pub const fn relation_count(&self, family: RelationFamily) -> u32 {
        self.relation_counts[family as usize]
    }

    #[must_use]
    pub const fn fact_node_count(&self) -> u32 {
        self.node_kind_count(VisualNodeKind::EventFact)
            + self.node_kind_count(VisualNodeKind::RelationshipFact)
            + self.node_kind_count(VisualNodeKind::TemporalFact)
            + self.node_kind_count(VisualNodeKind::CausalFact)
            + self.node_kind_count(VisualNodeKind::MemoryStateFact)
    }
}

/// Validates the shared topology and product semantics before any manifold is
/// allowed to project it. This runs at scene installation, never in a frame loop.
pub(crate) fn validate_topology_inventory(
    archive: &PhoenixSceneArchiveV1,
    index: &PhoenixSceneProductIndexV1,
) -> Result<TopologyInventoryCensus, SceneContractError> {
    let pages = archive.open_manifold(ArchiveManifold::Hybrid)?;
    let mut census = validate_shared_inventory(pages, index)?;
    validate_relation_page(archive, index)?;

    for manifold in Manifold::ALL {
        let archive_manifold = manifold.into();
        let projected = archive.open_manifold(archive_manifold)?;
        validate_positions(manifold, projected)?;
        let path_kind = super::preferred_path_kind(manifold);
        let key = PageKey::manifold(path_kind, archive_manifold);
        if !archive.has_page(key) {
            return Err(SceneContractError::MissingTopologyProjection { manifold });
        }
        let paths = archive.paths(path_kind, archive_manifold)?;
        super::validate_topology_projection(paths, census.edge_count)?;
        census.manifold_count = census.manifold_count.saturating_add(1);
    }
    Ok(census)
}

/// Validates the manifold-independent graph skeleton before publication.
/// Publishers and resident readers call this same function, so dangling or
/// duplicate topology cannot be hidden by a projection.
pub fn validate_topology_endpoints(
    identities: &[NodeIdentityRecord],
    topology: &[TopologyRecord],
    edges: &[EdgeRecord],
) -> Result<(), TopologyInventoryError> {
    validated_node_ids(identities, topology, edges).map(|_| ())
}

fn validate_shared_inventory(
    pages: ManifoldPageSet<'_>,
    index: &PhoenixSceneProductIndexV1,
) -> Result<TopologyInventoryCensus, TopologyInventoryError> {
    let node_ids = validated_node_ids(pages.identities, pages.topology, pages.edges)?;
    let mut node_kind_counts = [0_u32; 256];
    for (slot, product) in index.nodes().iter().enumerate() {
        validate_review_mask("node", slot, product.review_mask)?;
        let descriptor = validate_node_mask(slot, product.node_id, product.family_mask)?;
        node_kind_counts[descriptor.kind as usize] =
            node_kind_counts[descriptor.kind as usize].saturating_add(1);
    }

    let mut relation_counts = [0_u32; RelationFamily::ALL.len()];
    for (slot, (topology, product)) in pages.topology.iter().zip(index.edges()).enumerate() {
        debug_assert!(node_ids.contains(&topology.source_id));
        debug_assert!(node_ids.contains(&topology.target_id));
        validate_review_mask("edge", slot, product.review_mask)?;
        let relation = validate_relation_mask(slot, product.edge_id, product.relation_mask)?;
        relation_counts[relation as usize] = relation_counts[relation as usize].saturating_add(1);
    }

    Ok(TopologyInventoryCensus {
        node_kind_counts,
        relation_counts,
        node_count: pages.identities.len(),
        edge_count: pages.edges.len(),
        manifold_count: 0,
    })
}

fn validated_node_ids(
    identities: &[NodeIdentityRecord],
    topology: &[TopologyRecord],
    edges: &[EdgeRecord],
) -> Result<HashSet<u64>, TopologyInventoryError> {
    if topology.len() != edges.len() {
        return Err(TopologyInventoryError::EdgeInventoryMismatch {
            topology: topology.len(),
            edges: edges.len(),
        });
    }
    let mut node_ids = HashSet::with_capacity(identities.len());
    for (slot, identity) in identities.iter().enumerate() {
        if !node_ids.insert(identity.id) {
            return Err(TopologyInventoryError::DuplicateNodeIdentity {
                slot,
                node_id: identity.id,
            });
        }
    }
    let mut edge_ids = HashSet::with_capacity(edges.len());
    for (slot, (topology, edge)) in topology.iter().zip(edges).enumerate() {
        if !edge_ids.insert(edge.id) {
            return Err(TopologyInventoryError::DuplicateEdgeIdentity {
                slot,
                edge_id: edge.id,
            });
        }
        for (endpoint, node_id) in [
            ("source", topology.source_id),
            ("target", topology.target_id),
        ] {
            if !node_ids.contains(&node_id) {
                return Err(TopologyInventoryError::MissingEndpoint {
                    edge_slot: slot,
                    edge_id: edge.id,
                    endpoint,
                    node_id,
                });
            }
        }
    }
    Ok(node_ids)
}

fn validate_node_mask(
    slot: usize,
    node_id: u64,
    family_mask: u64,
) -> Result<crate::VisualNodeDescriptor, TopologyInventoryError> {
    let allowed = FamilyMask::ALL.0 | FamilyMask::ENTITY_LANES.0 | FamilyMask::TOPOLOGY_LANES.0;
    let unknown_bits = family_mask & !allowed;
    if unknown_bits != 0 {
        return Err(TopologyInventoryError::UnknownNodeFamilyBits {
            slot,
            node_id,
            unknown_bits,
        });
    }
    let topology_bits = family_mask & FamilyMask::TOPOLOGY_LANES.0;
    let entity_bits = family_mask & FamilyMask::ENTITY_LANES.0;
    if topology_bits.count_ones() > 1 || (topology_bits == 0 && entity_bits.count_ones() > 1) {
        return Err(TopologyInventoryError::AmbiguousNodeKind {
            slot,
            node_id,
            family_mask,
        });
    }
    let descriptor = describe_node(family_mask);
    if descriptor.kind == VisualNodeKind::Unknown
        || (topology_bits == 0 && entity_bits.count_ones() != 1)
        || (topology_bits != 0 && topology_bits.count_ones() != 1)
    {
        return Err(TopologyInventoryError::UnknownNodeKind {
            slot,
            node_id,
            family_mask,
        });
    }
    let broad_lane = match descriptor.lane {
        VisualNodeLane::Entities => FamilyMask::ENTITIES,
        VisualNodeLane::Structure => FamilyMask::STRUCTURE,
        VisualNodeLane::Facts => FamilyMask::FACTS,
        VisualNodeLane::Discourse => FamilyMask::DISCOURSE,
    };
    if family_mask & broad_lane.0 == 0 {
        return Err(TopologyInventoryError::NodeLaneMismatch {
            slot,
            node_id,
            kind: descriptor.kind,
            family_mask,
        });
    }
    Ok(descriptor)
}

fn validate_relation_mask(
    slot: usize,
    edge_id: u64,
    relation_mask: u64,
) -> Result<RelationFamily, TopologyInventoryError> {
    if relation_mask == 0
        || relation_mask & !RelationMask::ALL.0 != 0
        || !relation_mask.is_power_of_two()
        || describe_edge(relation_mask).kind == VisualEdgeKind::Unknown
    {
        return Err(TopologyInventoryError::InvalidEdgeRelation {
            slot,
            edge_id,
            relation_mask,
        });
    }
    Ok(RelationFamily::ALL[relation_mask.trailing_zeros() as usize])
}

fn validate_review_mask(
    resource: &'static str,
    slot: usize,
    review_mask: u32,
) -> Result<(), TopologyInventoryError> {
    if review_mask == 0 || review_mask & !ReviewMask::ALL.0 != 0 || !review_mask.is_power_of_two() {
        return Err(TopologyInventoryError::InvalidReviewMask {
            resource,
            slot,
            review_mask,
        });
    }
    Ok(())
}

fn validate_relation_page(
    archive: &PhoenixSceneArchiveV1,
    index: &PhoenixSceneProductIndexV1,
) -> Result<(), SceneContractError> {
    let records =
        archive.typed_page::<RelationMaskRecord>(PageKey::shared(PageKind::RelationMasks))?;
    if records.len() != index.edges().len() {
        return Err(TopologyInventoryError::RelationPageInventoryMismatch {
            expected: index.edges().len(),
            actual: records.len(),
        }
        .into());
    }
    for (slot, (record, product)) in records.iter().zip(index.edges()).enumerate() {
        let expected_kind = product.relation_mask.trailing_zeros() as u16;
        if record.visible_mask != product.relation_mask || record.relation_kind != expected_kind {
            return Err(TopologyInventoryError::RelationPageMismatch {
                slot,
                product_mask: product.relation_mask,
                page_mask: record.visible_mask,
                product_kind: expected_kind,
                page_kind: record.relation_kind,
            }
            .into());
        }
    }
    Ok(())
}

fn validate_positions(
    manifold: Manifold,
    pages: ManifoldPageSet<'_>,
) -> Result<(), TopologyInventoryError> {
    for (slot, (identity, position)) in pages.identities.iter().zip(pages.positions).enumerate() {
        if position.position.iter().any(|value| !value.is_finite()) {
            return Err(TopologyInventoryError::NonFinitePosition {
                manifold,
                slot,
                node_id: identity.id,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TopologyInventoryError {
    #[error("topology contains {topology} endpoint pairs for {edges} edge records")]
    EdgeInventoryMismatch { topology: usize, edges: usize },
    #[error("node slot {slot} duplicates stable node identity {node_id}")]
    DuplicateNodeIdentity { slot: usize, node_id: u64 },
    #[error("edge slot {slot} duplicates stable edge identity {edge_id}")]
    DuplicateEdgeIdentity { slot: usize, edge_id: u64 },
    #[error("node slot {slot} ({node_id}) carries unknown family bits {unknown_bits:#x}")]
    UnknownNodeFamilyBits {
        slot: usize,
        node_id: u64,
        unknown_bits: u64,
    },
    #[error("node slot {slot} ({node_id}) has ambiguous family mask {family_mask:#x}")]
    AmbiguousNodeKind {
        slot: usize,
        node_id: u64,
        family_mask: u64,
    },
    #[error("node slot {slot} ({node_id}) has no typed node kind in mask {family_mask:#x}")]
    UnknownNodeKind {
        slot: usize,
        node_id: u64,
        family_mask: u64,
    },
    #[error(
        "node slot {slot} ({node_id}) kind {kind:?} disagrees with broad lane in {family_mask:#x}"
    )]
    NodeLaneMismatch {
        slot: usize,
        node_id: u64,
        kind: VisualNodeKind,
        family_mask: u64,
    },
    #[error("{resource} slot {slot} has invalid review mask {review_mask:#x}")]
    InvalidReviewMask {
        resource: &'static str,
        slot: usize,
        review_mask: u32,
    },
    #[error("edge slot {slot} ({edge_id}) has invalid relation mask {relation_mask:#x}")]
    InvalidEdgeRelation {
        slot: usize,
        edge_id: u64,
        relation_mask: u64,
    },
    #[error("edge slot {edge_slot} ({edge_id}) references missing {endpoint} node {node_id}")]
    MissingEndpoint {
        edge_slot: usize,
        edge_id: u64,
        endpoint: &'static str,
        node_id: u64,
    },
    #[error("relation page has {actual} records; product topology has {expected}")]
    RelationPageInventoryMismatch { expected: usize, actual: usize },
    #[error(
        "relation page slot {slot} differs from product topology: mask {page_mask:#x}/{product_mask:#x}, kind {page_kind}/{product_kind}"
    )]
    RelationPageMismatch {
        slot: usize,
        product_mask: u64,
        page_mask: u64,
        product_kind: u16,
        page_kind: u16,
    },
    #[error("{manifold:?} node slot {slot} ({node_id}) has a non-finite position")]
    NonFinitePosition {
        manifold: Manifold,
        slot: usize,
        node_id: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(id: u64) -> EdgeRecord {
        EdgeRecord {
            id,
            color: [0.0; 4],
            width: 1.0,
            kind: 0,
            flags: 0,
        }
    }

    #[test]
    fn endpoint_contract_rejects_dangling_and_duplicate_topology() {
        let identities = [NodeIdentityRecord { id: 1 }, NodeIdentityRecord { id: 2 }];
        let valid = [TopologyRecord {
            source_id: 1,
            target_id: 2,
        }];
        assert_eq!(
            validate_topology_endpoints(&identities, &valid, &[edge(3)]),
            Ok(())
        );

        let dangling = [TopologyRecord {
            source_id: 1,
            target_id: 9,
        }];
        assert!(matches!(
            validate_topology_endpoints(&identities, &dangling, &[edge(3)]),
            Err(TopologyInventoryError::MissingEndpoint {
                edge_slot: 0,
                endpoint: "target",
                node_id: 9,
                ..
            })
        ));
        assert!(matches!(
            validate_topology_endpoints(
                &[NodeIdentityRecord { id: 1 }, NodeIdentityRecord { id: 1 }],
                &[],
                &[]
            ),
            Err(TopologyInventoryError::DuplicateNodeIdentity {
                slot: 1,
                node_id: 1
            })
        ));
    }

    #[test]
    fn topology_kind_is_singular_while_entity_facets_may_combine() {
        let relationship = FamilyMask::FACTS.0
            | FamilyMask::RELATIONSHIP_FACTS.0
            | FamilyMask::CHARACTERS.0
            | FamilyMask::LOCATIONS.0;
        assert_eq!(
            validate_node_mask(0, 7, relationship).map(|descriptor| descriptor.kind),
            Ok(VisualNodeKind::RelationshipFact)
        );

        let ambiguous_entity =
            FamilyMask::ENTITIES.0 | FamilyMask::CHARACTERS.0 | FamilyMask::LOCATIONS.0;
        assert!(matches!(
            validate_node_mask(0, 7, ambiguous_entity),
            Err(TopologyInventoryError::AmbiguousNodeKind { .. })
        ));
        let ambiguous_fact =
            FamilyMask::FACTS.0 | FamilyMask::EVENT_FACTS.0 | FamilyMask::CAUSAL_FACTS.0;
        assert!(matches!(
            validate_node_mask(0, 7, ambiguous_fact),
            Err(TopologyInventoryError::AmbiguousNodeKind { .. })
        ));
    }

    #[test]
    fn every_edge_has_exactly_one_named_relation() {
        assert_eq!(
            validate_relation_mask(0, 5, RelationFamily::Event.mask().0),
            Ok(RelationFamily::Event)
        );
        assert!(matches!(
            validate_relation_mask(
                0,
                5,
                RelationFamily::Event.mask().0 | RelationFamily::Causal.mask().0
            ),
            Err(TopologyInventoryError::InvalidEdgeRelation { .. })
        ));
    }
}
