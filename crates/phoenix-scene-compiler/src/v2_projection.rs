use crate::{layout, NativeSceneCompilerError};
use hashbrown::{HashMap, HashSet};
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{CapsRole, HighlightPalette};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use phoenix_scene_publisher::{
    NativeScenePublication, SceneEdgeProduct, SceneNodeProduct, ScenePublicationKind,
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
    pub color: [f32; 4],
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
    pub color: [f32; 4],
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

    pub(crate) fn push_edge(&mut self, edge: EdgeDraft) -> Result<(), NativeSceneCompilerError> {
        if self.edges.len() >= MAX_SCENE_EDGES {
            return Err(NativeSceneCompilerError::EdgeLimit(MAX_SCENE_EDGES));
        }
        if edge.id == 0 || !self.edge_ids.insert(edge.id) {
            return Err(NativeSceneCompilerError::IdentityCollision {
                resource: "V2 scene edge",
            });
        }
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
        _palette: HighlightPalette,
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

        let node_count = self.nodes.len();
        let mut identities = Vec::with_capacity(node_count);
        let mut styles = Vec::with_capacity(node_count);
        let mut products = Vec::with_capacity(node_count);
        let mut positions: [Vec<PositionRecord>; 5] =
            std::array::from_fn(|_| Vec::with_capacity(node_count));
        let mut caps_nodes = Vec::with_capacity(node_count);
        for (ordinal, node) in self.nodes.into_iter().enumerate() {
            identities.push(NodeIdentityRecord { id: node.id });
            styles.push(NodeStyleRecord {
                color: node.color,
                radius: (node.base_radius + (degrees[ordinal] as f32 + 1.0).ln() * 0.11)
                    .min(node.base_radius * 1.9),
                kind: node.kind,
                flags: node.flags,
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
                node.family_mask.trailing_zeros().min(u32::from(u16::MAX)) as u16,
                degrees[ordinal],
            )) {
                page.push(position);
            }
        }
        assign_sibling_ranks(&mut caps_nodes)?;
        positions[phoenix_scene_archive::ArchiveManifold::Caps as usize] =
            layout::compile_caps_positions(&caps_nodes)?;

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
                color: edge.color,
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
            node_products: products,
            edge_products,
            entity_mappings: self.entity_mappings,
            references: self.references,
        })
    }
}

fn assign_sibling_ranks(nodes: &mut [layout::CapsNode]) -> Result<(), NativeSceneCompilerError> {
    let mut counts = HashMap::<(Option<u32>, CapsRole), u32>::new();
    for node in nodes.iter() {
        let count = counts.entry((node.parent_slot, node.role)).or_default();
        *count = count
            .checked_add(1)
            .ok_or(NativeSceneCompilerError::RangeOverflow(
                "V2 CAPS sibling count",
            ))?;
    }
    let mut ranks = HashMap::<(Option<u32>, CapsRole), u32>::with_capacity(counts.len());
    for node in nodes {
        let key = (node.parent_slot, node.role);
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
