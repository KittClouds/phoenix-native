use crate::{layout, NativeSceneCompilerError};
use hashbrown::{HashMap, HashSet};
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{
    describe_node, with_visual_role, CapsRole, FamilyMask, GraphPalette, HighlightPalette,
    VisualNodeLane, VisualRole,
};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use phoenix_scene_publisher::{
    NativeScenePublication, SceneCapsGuide, SceneEdgeProduct, SceneNodeProduct,
    ScenePublicationKind,
};
use std::sync::Arc;

const MAX_SCENE_EDGES: usize = 1_000_000;

pub(crate) struct NodeDraft {
    pub id: u64,
    pub label: Arc<str>,
    pub kind: u16,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub review_mask: u32,
    pub base_radius: f32,
    pub flags: u16,
    pub caps_role: CapsRole,
    pub caps_parent: Option<u64>,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
}

pub(crate) struct EdgeDraft {
    pub id: u64,
    pub source: u64,
    pub target: u64,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub relation_mask: u64,
    pub review_mask: u32,
    pub width: f32,
    pub kind: u16,
    pub flags: u16,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
}

pub(crate) struct ProjectionBuilder {
    generation_id: u64,
    registry_revision: u64,
    document_id: u64,
    nodes: Vec<NodeDraft>,
    node_ids: HashSet<u64>,
    edges: Vec<EdgeDraft>,
    edge_ids: HashSet<u64>,
    entity_mappings: Vec<EntityNodeMappingRecord>,
    references: Vec<ProductReferenceRecord>,
}

impl ProjectionBuilder {
    pub(crate) fn with_capacity(
        generation_id: u64,
        registry_revision: u64,
        document_id: u64,
        node_capacity: usize,
        edge_capacity: usize,
    ) -> Result<Self, NativeSceneCompilerError> {
        if generation_id == 0 {
            return Err(NativeSceneCompilerError::ZeroGeneration);
        }
        if edge_capacity > MAX_SCENE_EDGES {
            return Err(NativeSceneCompilerError::EdgeLimit(MAX_SCENE_EDGES));
        }
        Ok(Self {
            generation_id,
            registry_revision,
            document_id,
            nodes: Vec::with_capacity(node_capacity),
            node_ids: HashSet::with_capacity(node_capacity),
            edges: Vec::with_capacity(edge_capacity),
            edge_ids: HashSet::with_capacity(edge_capacity),
            entity_mappings: Vec::new(),
            references: Vec::new(),
        })
    }

    pub(crate) fn push_node(&mut self, node: NodeDraft) -> Result<(), NativeSceneCompilerError> {
        if node.id == 0 || !self.node_ids.insert(node.id) {
            return Err(NativeSceneCompilerError::IdentityCollision {
                resource: "V2 scene node",
            });
        }
        self.nodes.push(node);
        Ok(())
    }

    pub(crate) fn contains_node(&self, node_id: u64) -> bool {
        self.node_ids.contains(&node_id)
    }

    pub(crate) fn push_edge(&mut self, edge: EdgeDraft) -> Result<(), NativeSceneCompilerError> {
        if self.edges.len() >= MAX_SCENE_EDGES {
            return Err(NativeSceneCompilerError::EdgeLimit(MAX_SCENE_EDGES));
        }
        if edge.id == 0 || !self.edge_ids.insert(edge.id) {
            return Err(NativeSceneCompilerError::IdentityCollision {
                resource: "V2 scene edge",
            });
        }
        // The producer-supplied mask is the edge's primary semantic identity.
        // Endpoint families are renderer context and must not be folded into
        // this mask, otherwise one relation is counted and filtered as several
        // unrelated products.
        self.edges.push(edge);
        Ok(())
    }

    pub(crate) fn push_entity_mapping(&mut self, entity_id: u64) {
        self.entity_mappings.push(EntityNodeMappingRecord {
            entity_id,
            node_id: entity_id,
        });
    }

    pub(crate) fn push_reference(&mut self, reference: ProductReferenceRecord) {
        self.references.push(reference);
    }

    pub(crate) fn finish(
        self,
        palette: HighlightPalette,
    ) -> Result<NativeScenePublication, NativeSceneCompilerError> {
        let node_slots = self
            .nodes
            .iter()
            .enumerate()
            .map(|(slot, node)| {
                let slot = u32::try_from(slot)
                    .map_err(|_| NativeSceneCompilerError::RangeOverflow("V2 node slot"))?;
                Ok((node.id, slot))
            })
            .collect::<Result<HashMap<_, _>, NativeSceneCompilerError>>()?;
        for edge in &self.edges {
            if !node_slots.contains_key(&edge.source) {
                return Err(NativeSceneCompilerError::V2MissingEndpoint(edge.source));
            }
            if !node_slots.contains_key(&edge.target) {
                return Err(NativeSceneCompilerError::V2MissingEndpoint(edge.target));
            }
        }

        let mut degrees = vec![0_u32; self.nodes.len()];
        for edge in &self.edges {
            let source = node_slots[&edge.source] as usize;
            let target = node_slots[&edge.target] as usize;
            degrees[source] = degrees[source].saturating_add(1);
            degrees[target] = degrees[target].saturating_add(1);
        }

        // Promote only the statistically dense tail to a visual hub.  The
        // threshold is computed from the complete degree distribution rather
        // than insertion order, so a regeneration with the same topology
        // produces identical role metadata.
        let hub_threshold = hub_degree_threshold(&degrees);

        let node_count = self.nodes.len();
        let mut identities = Vec::with_capacity(node_count);
        let mut styles = Vec::with_capacity(node_count);
        let mut products = Vec::with_capacity(node_count);
        let mut positions: [Vec<PositionRecord>; 6] =
            std::array::from_fn(|_| Vec::with_capacity(node_count));
        let mut caps_nodes = Vec::with_capacity(node_count);
        for (ordinal, node) in self.nodes.into_iter().enumerate() {
            let descriptor = describe_node(node.family_mask);
            let role = visual_role_for(node.caps_role, degrees[ordinal], hub_threshold);
            identities.push(NodeIdentityRecord { id: node.id });
            let color_key = GraphPalette::node_key(descriptor, node.kind).ok_or(
                NativeSceneCompilerError::PaletteKeyMissing {
                    resource: "node",
                    id: node.id,
                },
            )?;
            styles.push(NodeStyleRecord {
                color: palette.graph.color(color_key),
                radius: (node.base_radius + (degrees[ordinal] as f32 + 1.0).ln() * 0.11)
                    .min(node.base_radius * 1.9),
                kind: node.kind,
                flags: with_visual_role(node.flags, role),
            });
            products.push(SceneNodeProduct {
                node_id: node.id,
                family_mask: node.family_mask,
                scope_mask: node.scope_mask,
                review_mask: node.review_mask,
                label: node.label,
                inspector_ref: node.inspector_ref,
                provenance_ref: node.provenance_ref,
            });
            caps_nodes.push(layout::CapsNode {
                stable_id: node.id,
                role: node.caps_role,
                semantic_kind: descriptor.kind,
                parent_slot: node
                    .caps_parent
                    .map(|parent| {
                        node_slots
                            .get(&parent)
                            .copied()
                            .ok_or(NativeSceneCompilerError::V2MissingEndpoint(parent))
                    })
                    .transpose()?,
                sibling_rank: 0,
                sibling_count: 1,
                membership_count: 1,
            });
            for (page, position) in positions.iter_mut().zip(layout::positions(
                node.id,
                ordinal,
                node_count,
                layout_family_slot(node.family_mask),
                degrees[ordinal],
            )) {
                page.push(position);
            }
        }
        assign_sibling_ranks(&mut caps_nodes)?;
        let mut hybrid_nodes = Vec::with_capacity(node_count);
        for ((caps, product), degree) in caps_nodes.iter().zip(&products).zip(&degrees) {
            hybrid_nodes.push(layout::HybridNode {
                stable_id: caps.stable_id,
                lane: hybrid_lane(product.family_mask),
                role: caps.role,
                parent_slot: caps.parent_slot,
                sibling_rank: caps.sibling_rank,
                sibling_count: caps.sibling_count,
                degree: *degree,
            });
        }
        positions[phoenix_scene_archive::ArchiveManifold::Hybrid as usize] =
            layout::compile_hybrid_positions(&hybrid_nodes)?;
        let caps_layout = layout::compile_caps_layout(&caps_nodes)?;
        positions[phoenix_scene_archive::ArchiveManifold::Caps as usize] = caps_layout.positions;
        let caps_guides = caps_layout
            .guides
            .into_iter()
            .map(|guide| SceneCapsGuide {
                stable_id: guide.stable_id,
                center: guide.center,
                aperture: guide.aperture,
                radius: guide.radius,
                role: guide.role,
                weight: guide.weight,
            })
            .collect();
        let hopf_nodes = caps_nodes
            .iter()
            .zip(&products)
            .zip(&degrees)
            .map(|((caps, product), degree)| layout::HopfNode {
                stable_id: caps.stable_id,
                semantic_slot: describe_node(product.family_mask).kind as u16,
                role: caps.role,
                parent_slot: caps.parent_slot,
                sibling_rank: caps.sibling_rank,
                sibling_count: caps.sibling_count,
                degree: *degree,
            })
            .collect::<Vec<_>>();
        positions[phoenix_scene_archive::ArchiveManifold::Hopf as usize] =
            layout::compile_hopf_positions(&hopf_nodes)?;

        let edge_count = self.edges.len();
        let mut topology = Vec::with_capacity(edge_count);
        let mut edges = Vec::with_capacity(edge_count);
        let mut edge_products = Vec::with_capacity(edge_count);
        for edge in self.edges {
            topology.push(TopologyRecord {
                source_id: edge.source,
                target_id: edge.target,
            });
            edges.push(EdgeRecord {
                id: edge.id,
                color: palette.graph.color(
                    GraphPalette::edge_key(edge.relation_mask, edge.review_mask).ok_or(
                        NativeSceneCompilerError::PaletteKeyMissing {
                            resource: "edge",
                            id: edge.id,
                        },
                    )?,
                ),
                width: edge.width,
                kind: edge.kind,
                flags: edge.flags,
            });
            edge_products.push(SceneEdgeProduct {
                edge_id: edge.id,
                family_mask: edge.family_mask,
                scope_mask: edge.scope_mask,
                relation_mask: edge.relation_mask,
                review_mask: edge.review_mask,
                inspector_ref: edge.inspector_ref,
                provenance_ref: edge.provenance_ref,
            });
        }

        Ok(NativeScenePublication {
            generation_id: self.generation_id,
            kind: ScenePublicationKind::Full,
            registry_revision: self.registry_revision,
            document_id: Some(self.document_id),
            identities,
            styles,
            topology,
            edges,
            positions,
            caps_guides,
            node_products: products,
            edge_products,
            entity_mappings: self.entity_mappings,
            references: self.references,
        })
    }
}

fn hybrid_lane(family_mask: u64) -> phoenix_hybrid_space::HybridLane {
    match describe_node(family_mask).lane {
        VisualNodeLane::Structure => phoenix_hybrid_space::HybridLane::Structure,
        VisualNodeLane::Facts => phoenix_hybrid_space::HybridLane::Facts,
        VisualNodeLane::Discourse => phoenix_hybrid_space::HybridLane::Discourse,
        VisualNodeLane::Entities => phoenix_hybrid_space::HybridLane::Entities,
    }
}

fn hub_degree_threshold(degrees: &[u32]) -> u32 {
    if degrees.is_empty() {
        return u32::MAX;
    }
    let mut ordered = degrees.to_vec();
    ordered.sort_unstable();
    // Use a percentile over sample *positions*, not a sample count.  This
    // keeps a five-node fixture from selecting its maximum as the 95th
    // percentile and makes the threshold stable as the graph grows.
    let index = ordered.len().saturating_sub(1).saturating_mul(19) / 20;
    ordered[index].max(8)
}

fn visual_role_for(caps_role: CapsRole, degree: u32, hub_threshold: u32) -> VisualRole {
    let base = match caps_role {
        CapsRole::Document | CapsRole::Episode => VisualRole::Root,
        CapsRole::Chapter
        | CapsRole::Paragraph
        | CapsRole::Sentence
        | CapsRole::Entity
        | CapsRole::Chunk
        | CapsRole::Evidence
        | CapsRole::Event
        | CapsRole::Fact
        | CapsRole::Discourse
        | CapsRole::Memory => VisualRole::Anchor,
    };
    if base != VisualRole::Root && degree >= hub_threshold {
        VisualRole::Hub
    } else {
        base
    }
}

/// Select an entity-kind lane before the broad product lane.  A fact node can
/// carry both `FACTS` and `CHARACTERS`, for example; using `trailing_zeros`
/// directly would always place it in the generic facts band and erase the
/// entity granularity from Hopf/Transit geometry.
fn layout_family_slot(mask: u64) -> u16 {
    let entity_lanes = [
        FamilyMask::CHARACTERS.0,
        FamilyMask::LOCATIONS.0,
        FamilyMask::NETWORKS.0,
        FamilyMask::CREATURES.0,
        FamilyMask::NPCS.0,
        FamilyMask::EVENTS.0,
        FamilyMask::CONCEPTS.0,
        FamilyMask::OTHER_ENTITIES.0,
    ];
    if let Some(slot) = entity_lanes.iter().position(|lane| mask & lane != 0) {
        return slot as u16;
    }
    mask.trailing_zeros().min(u32::from(u16::MAX)) as u16
}

fn assign_sibling_ranks(nodes: &mut [layout::CapsNode]) -> Result<(), NativeSceneCompilerError> {
    let mut counts = HashMap::<Option<u32>, u32>::new();
    for node in nodes.iter() {
        let count = counts.entry(node.parent_slot).or_default();
        *count = count
            .checked_add(1)
            .ok_or(NativeSceneCompilerError::RangeOverflow(
                "V2 CAPS sibling count",
            ))?;
    }
    let mut ranks = HashMap::<Option<u32>, u32>::with_capacity(counts.len());
    for node in nodes {
        let key = node.parent_slot;
        node.sibling_count = counts[&key];
        let rank = ranks.entry(key).or_default();
        node.sibling_rank = *rank;
        *rank = rank
            .checked_add(1)
            .ok_or(NativeSceneCompilerError::RangeOverflow(
                "V2 CAPS sibling rank",
            ))?;
    }
    Ok(())
}

pub(crate) fn projection_id(domain: &[u8], parts: &[&[u8]]) -> u64 {
    let mut hash = blake3::Hasher::new();
    hash.update(b"phoenix.native.scene-projection/v2\0");
    hash.update(domain);
    for part in parts {
        hash.update(part);
    }
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hash.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_contract::VisualNodeKind;

    #[test]
    fn product_lanes_keep_entity_granularity_for_structure_and_facts() {
        assert_eq!(
            layout_family_slot(FamilyMask::STRUCTURE.0 | FamilyMask::CHARACTERS.0),
            0
        );
        assert_eq!(
            layout_family_slot(FamilyMask::FACTS.0 | FamilyMask::LOCATIONS.0),
            1
        );
        assert_eq!(
            layout_family_slot(FamilyMask::DISCOURSE.0 | FamilyMask::NPCS.0),
            4
        );
    }

    #[test]
    fn generic_product_lanes_keep_their_legacy_slots() {
        assert_eq!(layout_family_slot(FamilyMask::STRUCTURE.0), 8);
        assert_eq!(layout_family_slot(FamilyMask::FACTS.0), 9);
        assert_eq!(layout_family_slot(FamilyMask::DISCOURSE.0), 10);
    }

    #[test]
    fn hopf_semantics_preserve_fact_subtypes_over_entity_tags() {
        let event = describe_node(
            FamilyMask::FACTS.0 | FamilyMask::EVENT_FACTS.0 | FamilyMask::CHARACTERS.0,
        );
        let temporal = describe_node(
            FamilyMask::FACTS.0 | FamilyMask::TEMPORAL_FACTS.0 | FamilyMask::LOCATIONS.0,
        );
        let causal = describe_node(FamilyMask::FACTS.0 | FamilyMask::CAUSAL_FACTS.0);
        let memory = describe_node(FamilyMask::FACTS.0 | FamilyMask::MEMORY_STATE_FACTS.0);
        assert_eq!(event.kind as u16, 40);
        assert_eq!(temporal.kind as u16, 42);
        assert_eq!(causal.kind as u16, 43);
        assert_eq!(memory.kind as u16, 44);
    }

    #[test]
    fn visual_hub_threshold_is_deterministic_and_bounded() {
        assert_eq!(hub_degree_threshold(&[]), u32::MAX);
        assert_eq!(hub_degree_threshold(&[0, 1, 2, 3]), 8);
        assert_eq!(hub_degree_threshold(&[1, 2, 8, 64, 128]), 64);
    }

    #[test]
    fn roots_remain_roots_even_when_they_are_dense() {
        assert_eq!(
            visual_role_for(CapsRole::Document, 10_000, 1),
            VisualRole::Root
        );
        assert_eq!(
            visual_role_for(CapsRole::Entity, 10_000, 1),
            VisualRole::Hub
        );
        assert_eq!(
            visual_role_for(CapsRole::Evidence, 1, 8),
            VisualRole::Anchor
        );
    }

    #[test]
    fn caps_siblings_share_one_parent_chart_across_roles() {
        let mut nodes = [
            layout::CapsNode {
                stable_id: 1,
                role: CapsRole::Document,
                semantic_kind: VisualNodeKind::Document,
                parent_slot: None,
                sibling_rank: 0,
                sibling_count: 1,
                membership_count: 1,
            },
            layout::CapsNode {
                stable_id: 2,
                role: CapsRole::Chapter,
                semantic_kind: VisualNodeKind::Chapter,
                parent_slot: Some(0),
                sibling_rank: 0,
                sibling_count: 1,
                membership_count: 1,
            },
            layout::CapsNode {
                stable_id: 3,
                role: CapsRole::Episode,
                semantic_kind: VisualNodeKind::Episode,
                parent_slot: Some(0),
                sibling_rank: 0,
                sibling_count: 1,
                membership_count: 1,
            },
        ];
        assign_sibling_ranks(&mut nodes).expect("CAPS sibling ranks");
        assert_eq!((nodes[1].sibling_rank, nodes[1].sibling_count), (0, 2));
        assert_eq!((nodes[2].sibling_rank, nodes[2].sibling_count), (1, 2));
    }

    #[test]
    fn compiler_preserves_relation_identity_without_endpoint_detail_inheritance() {
        let mut builder = ProjectionBuilder::with_capacity(1, 1, 1, 0, 1).expect("builder");
        let causal = FamilyMask::FACTS.0 | FamilyMask::CAUSAL_FACTS.0;
        builder
            .push_edge(EdgeDraft {
                id: 1,
                source: 10,
                target: 11,
                family_mask: causal,
                scope_mask: u64::MAX,
                relation_mask: 1 << 3,
                review_mask: u32::MAX,
                width: 1.0,
                kind: 0,
                flags: 0,
                inspector_ref: 0,
                provenance_ref: 0,
            })
            .expect("edge");
        assert_eq!(builder.edges[0].family_mask, causal);
        assert_eq!(builder.edges[0].family_mask & FamilyMask::EVENT_FACTS.0, 0);
        assert_eq!(builder.edges[0].family_mask & FamilyMask::ENTITY_LANES.0, 0);
    }
}
