use crate::{layout, NativeSceneCompilerError};
use hashbrown::{HashMap, HashSet};
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{
    CapsRole, EntityFamily, EntityKind, RelationFamily, ReviewMask, ScopeMask,
};
use phoenix_scene_publisher::{SceneEdgeProduct, SceneNodeProduct};
use phoenix_workspace::DocumentLease;
use std::sync::Arc;

const MAX_EDGES: usize = 1_000_000;

pub(crate) struct StructureNodeSpec<'a> {
    pub id: u64,
    pub label: &'a str,
    pub kind: u16,
    pub color: [f32; 4],
    pub radius: f32,
    pub total_node_count: usize,
    pub degree: u32,
    pub caps: layout::CapsNode,
}

pub(crate) struct StructuralEdgeBuffers<'a> {
    pub topology: &'a mut Vec<TopologyRecord>,
    pub edges: &'a mut Vec<EdgeRecord>,
    pub products: &'a mut Vec<SceneEdgeProduct>,
    pub ids: &'a mut HashSet<u64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EvidenceSpec {
    pub id: u64,
    pub entity_id: u64,
    pub chunk: u32,
    pub start: u32,
    pub end: u32,
    pub sibling_rank: u32,
    pub sibling_count: u32,
}

pub(crate) fn push_structure_node(
    identities: &mut Vec<NodeIdentityRecord>,
    styles: &mut Vec<NodeStyleRecord>,
    products: &mut Vec<SceneNodeProduct>,
    positions: &mut [Vec<PositionRecord>; 5],
    caps_nodes: &mut Vec<layout::CapsNode>,
    spec: StructureNodeSpec<'_>,
) {
    let ordinal = identities.len();
    identities.push(NodeIdentityRecord { id: spec.id });
    styles.push(NodeStyleRecord {
        color: spec.color,
        radius: spec.radius,
        kind: spec.kind,
        flags: 0,
    });
    products.push(SceneNodeProduct {
        node_id: spec.id,
        family_mask: family_mask(EntityFamily::Structure),
        scope_mask: ScopeMask::NOTE.0,
        review_mask: ReviewMask::ACCEPTED.0,
        label: Arc::from(spec.label),
        inspector_ref: 0,
        provenance_ref: 0,
    });
    caps_nodes.push(spec.caps);
    for (page, position) in positions.iter_mut().zip(layout::positions(
        spec.id,
        ordinal,
        spec.total_node_count,
        family_slot(EntityFamily::Structure),
        spec.degree,
    )) {
        page.push(position);
    }
}

pub(crate) fn push_structural_edge(
    buffers: &mut StructuralEdgeBuffers<'_>,
    document_id: u64,
    source: u64,
    target: u64,
    source_color: [f32; 4],
    target_color: [f32; 4],
) -> Result<(), NativeSceneCompilerError> {
    if buffers.edges.len() >= MAX_EDGES {
        return Err(NativeSceneCompilerError::EdgeLimit(MAX_EDGES));
    }
    let edge_id = stable_structural_edge_id(document_id, source, target);
    if !buffers.ids.insert(edge_id) {
        return Err(NativeSceneCompilerError::IdentityCollision {
            resource: "structural edge",
        });
    }
    buffers.topology.push(TopologyRecord {
        source_id: source,
        target_id: target,
    });
    let mut color = blend(source_color, target_color);
    color[3] = 0.22;
    buffers.edges.push(EdgeRecord {
        id: edge_id,
        color,
        width: 0.92,
        kind: RelationFamily::Structural as u16,
        flags: 0,
    });
    buffers.products.push(SceneEdgeProduct {
        edge_id,
        family_mask: family_mask(EntityFamily::Structure),
        scope_mask: ScopeMask::NOTE.0,
        relation_mask: RelationFamily::Structural.mask().0,
        review_mask: ReviewMask::ACCEPTED.0,
        inspector_ref: 0,
        provenance_ref: 0,
    });
    Ok(())
}

pub(crate) fn stable_structural_id(document_id: u64, kind: &[u8], ordinal: u64) -> u64 {
    stable_id(
        b"phoenix.native.structural-node/v1\0",
        &[&document_id.to_le_bytes(), kind, &ordinal.to_le_bytes()],
    )
}

pub(crate) fn stable_evidence_id(document_id: u64, entity_id: u64, start: u32, end: u32) -> u64 {
    stable_id(
        b"phoenix.native.evidence-node/v1\0",
        &[
            &document_id.to_le_bytes(),
            &entity_id.to_le_bytes(),
            &start.to_le_bytes(),
            &end.to_le_bytes(),
        ],
    )
}

pub(crate) fn ensure_node_id_available(
    identities: &[NodeIdentityRecord],
    node_id: u64,
) -> Result<(), NativeSceneCompilerError> {
    if identities.iter().any(|identity| identity.id == node_id) {
        return Err(NativeSceneCompilerError::IdentityCollision {
            resource: "structural node",
        });
    }
    Ok(())
}

pub(crate) fn rank_evidence(
    evidence_specs: &mut [EvidenceSpec],
) -> Result<(), NativeSceneCompilerError> {
    let mut counts = HashMap::<u32, u32>::new();
    for evidence in evidence_specs.iter() {
        let count = counts.entry(evidence.chunk).or_default();
        *count = count
            .checked_add(1)
            .ok_or(NativeSceneCompilerError::RangeOverflow("evidence count"))?;
    }
    let mut ranks = HashMap::<u32, u32>::with_capacity(counts.len());
    for evidence in evidence_specs {
        evidence.sibling_count = counts[&evidence.chunk];
        evidence.sibling_rank = *ranks.entry(evidence.chunk).or_default();
        ranks.insert(
            evidence.chunk,
            evidence
                .sibling_rank
                .checked_add(1)
                .ok_or(NativeSceneCompilerError::RangeOverflow(
                    "evidence sibling rank",
                ))?,
        );
    }
    Ok(())
}

pub(crate) fn evidence_surface(
    document: &DocumentLease,
    evidence: EvidenceSpec,
) -> Result<&str, NativeSceneCompilerError> {
    let start = evidence.start as usize;
    let end = evidence.end as usize;
    document
        .content
        .get(start..end)
        .ok_or(NativeSceneCompilerError::StaleMention {
            start: evidence.start,
            end: evidence.end,
        })
}

pub(crate) const fn entity_caps_role(kind: EntityKind) -> CapsRole {
    match kind {
        EntityKind::Event => CapsRole::Event,
        _ => CapsRole::Entity,
    }
}

pub(crate) fn evidence_color(mut color: [f32; 4]) -> [f32; 4] {
    color[0] *= 0.78;
    color[1] *= 0.78;
    color[2] *= 0.78;
    color[3] = 0.84;
    color
}

fn stable_structural_edge_id(document_id: u64, source: u64, target: u64) -> u64 {
    stable_id(
        b"phoenix.native.structural-edge/v1\0",
        &[
            &document_id.to_le_bytes(),
            &source.to_le_bytes(),
            &target.to_le_bytes(),
        ],
    )
}

fn stable_id(domain: &[u8], parts: &[&[u8]]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(part);
    }
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

fn blend(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        (left[0] + right[0]) * 0.5,
        (left[1] + right[1]) * 0.5,
        (left[2] + right[2]) * 0.5,
        0.28,
    ]
}

const fn family_mask(family: EntityFamily) -> u64 {
    1_u64 << family_slot(family)
}

const fn family_slot(family: EntityFamily) -> u16 {
    match family {
        EntityFamily::Character => 0,
        EntityFamily::Location => 1,
        EntityFamily::Organization => 2,
        EntityFamily::Item => 3,
        EntityFamily::Concept => 4,
        EntityFamily::Event => 5,
        EntityFamily::Structure => 6,
        EntityFamily::Other => 7,
    }
}
