use crate::{
    layout,
    scene_build::{
        ensure_node_id_available, entity_caps_role, evidence_color, evidence_surface,
        push_structural_edge, push_structure_node, rank_evidence, stable_evidence_id,
        stable_structural_id, EvidenceSpec, StructuralEdgeBuffers, StructureNodeSpec,
    },
    NativeSceneCompilerError,
};
use hashbrown::{HashMap, HashSet};
use memchr::memchr;
use phoenix_analysis_contract::{NliCandidateKind, PhoenixNliArtifactV1};
use phoenix_graph_generation::{
    promoted_edge_id, VerifiedGraphGeneration, ACCEPTED_EDGE_FLAG_PROMOTED,
    DECISION_STATUS_ACCEPTED, DECISION_STATUS_REJECTED,
};
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, NodeIdentityRecord, NodeStyleRecord, TopologyRecord,
};
use phoenix_scene_contract::{
    AnchorCandidate, CapsRole, EntityFamily, FamilyMask, HighlightPalette, RelationFamily,
    ReviewMask, ScopeMask, VerifiedDocumentAnchors, CHUNK_NODE_KIND, DOCUMENT_NODE_KIND,
    EVIDENCE_NODE_KIND,
};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use phoenix_scene_publisher::{
    NativeScenePublication, SceneEdgeProduct, SceneNodeProduct, ScenePublicationKind,
};
use phoenix_workspace::{DocumentLease, EntityRegistry, EntitySourceMask};
use smallvec::SmallVec;
use std::sync::Arc;
use std::time::Instant;

const DOCUMENT_REFERENCE_KIND: u16 = 1;
const MAX_CHUNK_ENTITIES: usize = 64;
const MAX_EDGES: usize = 1_000_000;
#[derive(Clone, Copy, Debug)]
pub struct NativeSceneCompilerInput<'a> {
    pub generation_id: u64,
    pub registry_revision: u64,
    pub document: &'a DocumentLease,
    pub registry: &'a EntityRegistry,
    /// Production supplies the document-bound analysis/manual authority here.
    /// `None` is retained only for compiler fixtures that construct mentions
    /// directly in an isolated `EntityRegistry`.
    pub verified_anchors: Option<&'a VerifiedDocumentAnchors>,
    /// Candidate-only NLI authority. These relations are emitted as proposed
    /// product edges and never become accepted graph truth in this compiler.
    pub nli: Option<&'a PhoenixNliArtifactV1>,
    pub palette: HighlightPalette,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSceneCompileReceipt {
    pub generation_id: u64,
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub registry_revision: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub verified_mentions: u32,
    pub chunk_count: u32,
    pub compile_micros: u64,
}

#[derive(Debug)]
pub struct CompiledNativeScene {
    pub publication: NativeScenePublication,
    pub anchors: Vec<AnchorCandidate>,
    pub receipt: NativeSceneCompileReceipt,
}

pub fn compile_active_document(
    input: NativeSceneCompilerInput<'_>,
) -> Result<CompiledNativeScene, NativeSceneCompilerError> {
    compile_document(input, None)
}

pub fn compile_graph_generation(
    input: NativeSceneCompilerInput<'_>,
    generation: &VerifiedGraphGeneration,
) -> Result<CompiledNativeScene, NativeSceneCompilerError> {
    generation
        .verify_binding(
            input.document.entry_id.0,
            input.document.revision.0,
            input.document.content_hash.0,
            input.registry_revision,
        )
        .map_err(|_| NativeSceneCompilerError::GraphGenerationAuthorityMismatch)?;
    let candidate_authority_matches = match input.nli {
        Some(nli) => generation
            .candidate_edges()
            .iter()
            .zip(&nli.nli_candidates)
            .all(|(edge, candidate)| {
                edge.candidate_id == candidate.candidate_id
                    && edge.source_id == candidate.left_entity_id
                    && edge.target_id == candidate.right_entity_id
                    && edge.premise_start == candidate.premise_start
                    && edge.premise_end == candidate.premise_end
            }),
        None => generation.candidate_edges().is_empty(),
    };
    let registry_entity_ids = input
        .registry
        .entities()
        .iter()
        .map(|entity| entity.id)
        .collect::<HashSet<_>>();
    if generation.candidate_edges().len() != input.nli.map_or(0, |nli| nli.nli_candidates.len())
        || !candidate_authority_matches
        || generation
            .entities()
            .iter()
            .any(|entity| !registry_entity_ids.contains(&entity.id))
    {
        return Err(NativeSceneCompilerError::GraphGenerationAuthorityMismatch);
    }
    compile_document(input, Some(generation))
}

fn compile_document(
    input: NativeSceneCompilerInput<'_>,
    generation: Option<&VerifiedGraphGeneration>,
) -> Result<CompiledNativeScene, NativeSceneCompilerError> {
    let started = Instant::now();
    validate_input(input)?;
    let mut entities = input.registry.entities().iter().collect::<Vec<_>>();
    entities.sort_unstable_by_key(|entity| entity.id);
    let mut entity_slots = HashMap::with_capacity(entities.len());
    for (slot, entity) in entities.iter().enumerate() {
        let slot = u32::try_from(slot)
            .map_err(|_| NativeSceneCompilerError::RangeOverflow("node slot"))?;
        if entity_slots.insert(entity.id, slot).is_some() {
            return Err(NativeSceneCompilerError::IdentityCollision { resource: "node" });
        }
    }

    let boundaries = generation.map_or_else(
        || paragraph_boundaries(&input.document.content),
        |generation| Ok(generation.chunks().iter().map(|chunk| chunk.end).collect()),
    )?;
    let mut chunk_entities = HashMap::<u32, SmallVec<[u64; 8]>>::new();
    let mut generation_chunk_ids = Vec::new();
    let mut generation_mentions = HashMap::new();
    if let Some(generation) = generation {
        generation_chunk_ids.reserve(generation.chunks().len());
        let chunk_ordinals = generation
            .chunks()
            .iter()
            .enumerate()
            .map(|(ordinal, chunk)| {
                let ordinal = u32::try_from(ordinal)
                    .map_err(|_| NativeSceneCompilerError::RangeOverflow("chunk ordinal"))?;
                chunk_entities.insert(ordinal, SmallVec::new());
                generation_chunk_ids.push(chunk.id);
                Ok((chunk.id, ordinal))
            })
            .collect::<Result<HashMap<_, _>, NativeSceneCompilerError>>()?;
        for mention in generation.mentions() {
            let chunk = *chunk_ordinals
                .get(&mention.chunk_id)
                .ok_or(NativeSceneCompilerError::GraphGenerationAuthorityMismatch)?;
            if generation_mentions
                .insert(
                    (mention.entity_id, mention.start, mention.end),
                    (mention.evidence_id, chunk),
                )
                .is_some()
            {
                return Err(NativeSceneCompilerError::GraphGenerationAuthorityMismatch);
            }
        }
    }
    let mut anchors = Vec::new();
    let mut evidence_specs = Vec::new();
    let mut mentioned_entities = HashSet::new();
    if let Some(verified) = input.verified_anchors {
        for mention in verified.anchors() {
            let slot = *entity_slots
                .get(&mention.node_id)
                .ok_or(NativeSceneCompilerError::IdentityCollision { resource: "entity" })?;
            let entity = entities[slot as usize];
            if mention.family != entity.kind.family() {
                return Err(NativeSceneCompilerError::AnchorAuthorityMismatch);
            }
            let surface = verified_surface(input.document, mention.start, mention.end)?;
            anchors.push(AnchorCandidate {
                start: mention.start,
                end: mention.end,
                node_id: mention.node_id,
                entity_slot: slot,
                family: mention.family,
                surface: surface.to_owned(),
            });
            mentioned_entities.insert(mention.node_id);
            let (evidence_id, chunk) = if generation.is_some() {
                generation_mentions
                    .get(&(mention.node_id, mention.start, mention.end))
                    .copied()
                    .ok_or(NativeSceneCompilerError::GraphGenerationAuthorityMismatch)?
            } else {
                (
                    stable_evidence_id(
                        input.document.entry_id.0,
                        mention.node_id,
                        mention.start,
                        mention.end,
                    ),
                    u32::try_from(boundaries.partition_point(|end| *end <= mention.start))
                        .map_err(|_| NativeSceneCompilerError::RangeOverflow("chunk ordinal"))?,
                )
            };
            evidence_specs.push(EvidenceSpec {
                id: evidence_id,
                entity_id: mention.node_id,
                chunk,
                start: mention.start,
                end: mention.end,
                sibling_rank: 0,
                sibling_count: 0,
            });
            chunk_entities
                .entry(chunk)
                .or_default()
                .push(mention.node_id);
        }
    } else {
        for (mention, entity) in input.registry.active_mentions_for(input.document) {
            verify_mention(input.document, mention.start, mention.end, &mention.surface)?;
            let slot = *entity_slots
                .get(&entity.id)
                .ok_or(NativeSceneCompilerError::IdentityCollision { resource: "entity" })?;
            anchors.push(AnchorCandidate {
                start: mention.start,
                end: mention.end,
                node_id: entity.id,
                entity_slot: slot,
                family: entity.kind.family(),
                surface: mention.surface.clone(),
            });
            mentioned_entities.insert(entity.id);
            let chunk = u32::try_from(boundaries.partition_point(|end| *end <= mention.start))
                .map_err(|_| NativeSceneCompilerError::RangeOverflow("chunk ordinal"))?;
            evidence_specs.push(EvidenceSpec {
                id: stable_evidence_id(
                    input.document.entry_id.0,
                    entity.id,
                    mention.start,
                    mention.end,
                ),
                entity_id: entity.id,
                chunk,
                start: mention.start,
                end: mention.end,
                sibling_rank: 0,
                sibling_count: 0,
            });
            chunk_entities.entry(chunk).or_default().push(entity.id);
        }
    }
    if anchors.is_empty() {
        return Err(NativeSceneCompilerError::NoVerifiedMentions);
    }
    anchors.sort_unstable_by_key(|anchor| (anchor.start, anchor.end, anchor.node_id));
    evidence_specs.sort_unstable_by_key(|evidence| {
        (
            evidence.chunk,
            evidence.start,
            evidence.end,
            evidence.entity_id,
        )
    });
    rank_evidence(&mut evidence_specs)?;

    let edge_weights = build_edge_weights(&mut chunk_entities)?;
    let mut edge_pairs = edge_weights.into_iter().collect::<Vec<_>>();
    edge_pairs.sort_unstable_by_key(|((source, target), _)| (*source, *target));
    let mut degrees = vec![0_u32; entities.len()];
    let proposed_capacity = input.nli.map_or(0, |nli| nli.nli_candidates.len());
    let edge_capacity = edge_pairs
        .len()
        .checked_add(proposed_capacity)
        .ok_or(NativeSceneCompilerError::EdgeLimit(MAX_EDGES))?;
    if edge_capacity > MAX_EDGES {
        return Err(NativeSceneCompilerError::EdgeLimit(MAX_EDGES));
    }
    let mut topology = Vec::with_capacity(edge_capacity);
    let mut edges = Vec::with_capacity(edge_capacity);
    let mut edge_products = Vec::with_capacity(edge_capacity);
    let mut edge_ids = HashSet::with_capacity(edge_capacity);
    let promoted_edges = generation
        .map(promoted_edges_by_candidate)
        .transpose()?
        .unwrap_or_default();
    let review_decisions = generation
        .map(|generation| {
            generation
                .decisions()
                .iter()
                .map(|decision| (decision.candidate_id, decision.status))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    for ((source, target), weight) in edge_pairs {
        let source_slot = entity_slots[&source] as usize;
        let target_slot = entity_slots[&target] as usize;
        degrees[source_slot] = degrees[source_slot].saturating_add(1);
        degrees[target_slot] = degrees[target_slot].saturating_add(1);
        let edge_id = stable_edge_id(input.document.entry_id.0, source, target);
        if !edge_ids.insert(edge_id) {
            return Err(NativeSceneCompilerError::IdentityCollision { resource: "edge" });
        }
        let source_color = input
            .palette
            .for_family(entities[source_slot].kind.family())
            .primary;
        let target_color = input
            .palette
            .for_family(entities[target_slot].kind.family())
            .primary;
        topology.push(TopologyRecord {
            source_id: source,
            target_id: target,
        });
        edges.push(EdgeRecord {
            id: edge_id,
            color: blend(source_color, target_color),
            width: 0.8 + (weight as f32 + 1.0).ln() * 0.55,
            kind: 1,
            flags: 0,
        });
        edge_products.push(SceneEdgeProduct {
            edge_id,
            family_mask: family_mask(entities[source_slot].kind.family())
                | family_mask(entities[target_slot].kind.family()),
            scope_mask: ScopeMask::NOTE.0,
            relation_mask: RelationFamily::CoOccurrence.mask().0,
            review_mask: ReviewMask::PROPOSED.0,
            inspector_ref: 0,
            provenance_ref: 0,
        });
    }
    if let Some(nli) = input.nli {
        for candidate in &nli.nli_candidates {
            let source_slot = *entity_slots
                .get(&candidate.left_entity_id)
                .ok_or(NativeSceneCompilerError::NliAuthorityMismatch)?
                as usize;
            let target_slot = *entity_slots
                .get(&candidate.right_entity_id)
                .ok_or(NativeSceneCompilerError::NliAuthorityMismatch)?
                as usize;
            degrees[source_slot] = degrees[source_slot].saturating_add(1);
            degrees[target_slot] = degrees[target_slot].saturating_add(1);
            let promoted = promoted_edges.get(&candidate.candidate_id).copied();
            let edge_id = promoted.map_or_else(
                || proposed_nli_edge_id(input.document.entry_id.0, &candidate.candidate_id),
                |edge| edge.id,
            );
            if !edge_ids.insert(edge_id) {
                return Err(NativeSceneCompilerError::IdentityCollision {
                    resource: "NLI edge",
                });
            }
            let source_color = input
                .palette
                .for_family(entities[source_slot].kind.family())
                .primary;
            let target_color = input
                .palette
                .for_family(entities[target_slot].kind.family())
                .primary;
            topology.push(TopologyRecord {
                source_id: candidate.left_entity_id,
                target_id: candidate.right_entity_id,
            });
            edges.push(EdgeRecord {
                id: edge_id,
                color: if promoted.is_some() {
                    blend(source_color, target_color)
                } else {
                    proposed_blend(source_color, target_color)
                },
                width: promoted.map_or(0.52, |edge| {
                    f32::from_bits(edge.weight_bits).clamp(0.56, 1.2)
                }),
                kind: nli_edge_kind(candidate.kind),
                flags: 0,
            });
            edge_products.push(SceneEdgeProduct {
                edge_id,
                family_mask: family_mask(entities[source_slot].kind.family())
                    | family_mask(entities[target_slot].kind.family())
                    | FamilyMask::DISCOURSE.0,
                scope_mask: ScopeMask::NOTE.0,
                relation_mask: RelationFamily::Observation.mask().0,
                review_mask: match review_decisions.get(&candidate.candidate_id).copied() {
                    Some(DECISION_STATUS_ACCEPTED) => ReviewMask::ACCEPTED.0,
                    Some(DECISION_STATUS_REJECTED) => ReviewMask::REJECTED.0,
                    _ => ReviewMask::PROPOSED.0,
                },
                inspector_ref: 0,
                provenance_ref: 0,
            });
        }
    }

    let mut structural_chunks = chunk_entities.into_iter().collect::<Vec<_>>();
    structural_chunks.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    let entity_count = entities.len();
    let total_node_count = entity_count
        .checked_add(structural_chunks.len())
        .and_then(|count| count.checked_add(evidence_specs.len()))
        .and_then(|count| count.checked_add(1))
        .ok_or(NativeSceneCompilerError::RangeOverflow(
            "structural node count",
        ))?;
    let mut identities = Vec::with_capacity(total_node_count);
    let mut styles = Vec::with_capacity(total_node_count);
    let mut node_products = Vec::with_capacity(total_node_count);
    let mut entity_mappings = Vec::with_capacity(entity_count);
    let mut positions = std::array::from_fn(|_| Vec::with_capacity(total_node_count));
    let mut caps_nodes = Vec::with_capacity(total_node_count);
    let mut caps_memberships = HashMap::with_capacity(mentioned_entities.len());
    let document_slot = u32::try_from(entity_count)
        .map_err(|_| NativeSceneCompilerError::RangeOverflow("CAPS document slot"))?;
    let evidence_base = entity_count
        .checked_add(1)
        .and_then(|slot| slot.checked_add(structural_chunks.len()))
        .ok_or(NativeSceneCompilerError::RangeOverflow(
            "CAPS evidence base",
        ))?;
    for (evidence_index, evidence) in evidence_specs.iter().enumerate() {
        let parent_slot = evidence_base.checked_add(evidence_index).ok_or(
            NativeSceneCompilerError::RangeOverflow("CAPS evidence parent"),
        )?;
        caps_memberships
            .entry(evidence.entity_id)
            .and_modify(|membership: &mut CapsMembership| {
                membership.membership_count = membership.membership_count.saturating_add(1);
            })
            .or_insert(CapsMembership {
                parent_slot: u32::try_from(parent_slot)
                    .map_err(|_| NativeSceneCompilerError::RangeOverflow("CAPS parent slot"))?,
                membership_count: 1,
            });
    }
    let unmentioned_count = entities
        .iter()
        .filter(|entity| !mentioned_entities.contains(&entity.id))
        .count()
        .max(1);
    let mut unmentioned_rank = 0_u32;
    for (ordinal, entity) in entities.into_iter().enumerate() {
        let family = entity.kind.family();
        let mentioned = mentioned_entities.contains(&entity.id);
        identities.push(NodeIdentityRecord { id: entity.id });
        styles.push(NodeStyleRecord {
            color: input.palette.for_family(family).primary,
            radius: (0.38 + (degrees[ordinal] as f32 + 1.0).ln() * 0.16).min(0.9),
            kind: entity.kind as u16,
            flags: source_flags(entity.sources),
        });
        node_products.push(SceneNodeProduct {
            node_id: entity.id,
            family_mask: family_mask(family),
            scope_mask: if mentioned {
                ScopeMask::NOTE.0
            } else {
                ScopeMask::REGISTRY.0
            },
            review_mask: ReviewMask::ACCEPTED.0,
            label: Arc::from(entity.label.as_str()),
            inspector_ref: 0,
            provenance_ref: 0,
        });
        entity_mappings.push(EntityNodeMappingRecord {
            entity_id: entity.id,
            node_id: entity.id,
        });
        let (parent_slot, sibling_rank, sibling_count, membership_count) =
            if let Some(membership) = caps_memberships.get(&entity.id).copied() {
                (membership.parent_slot, 0, 1, membership.membership_count)
            } else {
                let rank = unmentioned_rank;
                unmentioned_rank = unmentioned_rank.saturating_add(1);
                (
                    document_slot,
                    rank,
                    u32::try_from(unmentioned_count).map_err(|_| {
                        NativeSceneCompilerError::RangeOverflow("CAPS unmentioned count")
                    })?,
                    1,
                )
            };
        caps_nodes.push(layout::CapsNode {
            stable_id: entity.id,
            role: entity_caps_role(entity.kind),
            parent_slot: Some(parent_slot),
            sibling_rank,
            sibling_count,
            membership_count,
        });
        for (page, position) in positions.iter_mut().zip(layout::positions(
            entity.id,
            ordinal,
            total_node_count,
            family_slot(family),
            degrees[ordinal],
        )) {
            page.push(position);
        }
    }
    let structure_palette = input.palette.for_family(EntityFamily::Structure);
    let document_id = generation.map_or_else(
        || stable_structural_id(input.document.entry_id.0, b"document", 0),
        |generation| generation.document().id,
    );
    ensure_node_id_available(&identities, document_id)?;
    push_structure_node(
        &mut identities,
        &mut styles,
        &mut node_products,
        &mut positions,
        &mut caps_nodes,
        StructureNodeSpec {
            id: document_id,
            label: "Document",
            kind: DOCUMENT_NODE_KIND,
            color: structure_palette.primary,
            radius: 1.4,
            total_node_count,
            degree: structural_chunks.len() as u32,
            caps: layout::CapsNode {
                stable_id: document_id,
                role: CapsRole::Document,
                parent_slot: None,
                sibling_rank: 0,
                sibling_count: 1,
                membership_count: 1,
            },
        },
    );
    let mut structural_edges = StructuralEdgeBuffers {
        topology: &mut topology,
        edges: &mut edges,
        products: &mut edge_products,
        ids: &mut edge_ids,
    };
    let mut chunk_slots = HashMap::with_capacity(structural_chunks.len());
    for (chunk_index, (chunk_ordinal, _)) in structural_chunks.iter().enumerate() {
        let node_id = generation_chunk_ids
            .get(*chunk_ordinal as usize)
            .copied()
            .unwrap_or_else(|| {
                stable_structural_id(
                    input.document.entry_id.0,
                    b"chunk",
                    u64::from(*chunk_ordinal),
                )
            });
        ensure_node_id_available(&identities, node_id)?;
        let chunk_slot = entity_count
            .checked_add(1)
            .and_then(|slot| slot.checked_add(chunk_index))
            .ok_or(NativeSceneCompilerError::RangeOverflow("CAPS chunk slot"))?;
        chunk_slots.insert(
            *chunk_ordinal,
            u32::try_from(chunk_slot)
                .map_err(|_| NativeSceneCompilerError::RangeOverflow("CAPS chunk slot"))?,
        );
        push_structure_node(
            &mut identities,
            &mut styles,
            &mut node_products,
            &mut positions,
            &mut caps_nodes,
            StructureNodeSpec {
                id: node_id,
                label: &format!("Chunk {}", chunk_ordinal.saturating_add(1)),
                kind: CHUNK_NODE_KIND,
                color: structure_palette.secondary,
                radius: 0.8,
                total_node_count,
                degree: chunk_index as u32,
                caps: layout::CapsNode {
                    stable_id: node_id,
                    role: CapsRole::Chunk,
                    parent_slot: Some(document_slot),
                    sibling_rank: u32::try_from(chunk_index)
                        .map_err(|_| NativeSceneCompilerError::RangeOverflow("CAPS chunk rank"))?,
                    sibling_count: u32::try_from(structural_chunks.len())
                        .map_err(|_| NativeSceneCompilerError::RangeOverflow("CAPS chunk count"))?,
                    membership_count: 1,
                },
            },
        );
        push_structural_edge(
            &mut structural_edges,
            input.document.entry_id.0,
            document_id,
            node_id,
            structure_palette.primary,
            structure_palette.secondary,
        )?;
    }
    for evidence in &evidence_specs {
        ensure_node_id_available(&identities, evidence.id)?;
        let parent_slot = chunk_slots[&evidence.chunk];
        let entity_slot = entity_slots[&evidence.entity_id] as usize;
        let entity_color = styles[entity_slot].color;
        let label = evidence_surface(input.document, *evidence)?;
        push_structure_node(
            &mut identities,
            &mut styles,
            &mut node_products,
            &mut positions,
            &mut caps_nodes,
            StructureNodeSpec {
                id: evidence.id,
                label,
                kind: EVIDENCE_NODE_KIND,
                color: evidence_color(entity_color),
                radius: 0.46,
                total_node_count,
                degree: 2,
                caps: layout::CapsNode {
                    stable_id: evidence.id,
                    role: CapsRole::Evidence,
                    parent_slot: Some(parent_slot),
                    sibling_rank: evidence.sibling_rank,
                    sibling_count: evidence.sibling_count,
                    membership_count: 1,
                },
            },
        );
        let chunk_id = identities[parent_slot as usize].id;
        push_structural_edge(
            &mut structural_edges,
            input.document.entry_id.0,
            chunk_id,
            evidence.id,
            structure_palette.secondary,
            entity_color,
        )?;
        push_structural_edge(
            &mut structural_edges,
            input.document.entry_id.0,
            evidence.id,
            evidence.entity_id,
            entity_color,
            entity_color,
        )?;
    }
    positions[ArchiveManifold::Caps as usize] = layout::compile_caps_positions(&caps_nodes)?;

    let node_count = u32::try_from(identities.len())
        .map_err(|_| NativeSceneCompilerError::RangeOverflow("node count"))?;
    let edge_count = u32::try_from(edges.len())
        .map_err(|_| NativeSceneCompilerError::RangeOverflow("edge count"))?;
    let verified_mentions = u32::try_from(anchors.len())
        .map_err(|_| NativeSceneCompilerError::RangeOverflow("mention count"))?;
    let chunk_count = u32::try_from(structural_chunks.len())
        .map_err(|_| NativeSceneCompilerError::RangeOverflow("chunk count"))?;
    let source_len = u32::try_from(input.document.content.len()).map_err(|_| {
        NativeSceneCompilerError::DocumentTooLarge {
            actual: input.document.content.len(),
            maximum: u32::MAX as usize,
        }
    })?;
    let receipt = NativeSceneCompileReceipt {
        generation_id: input.generation_id,
        document_id: input.document.entry_id.0,
        document_revision: input.document.revision.0,
        content_hash: input.document.content_hash.0,
        registry_revision: input.registry_revision,
        node_count,
        edge_count,
        verified_mentions,
        chunk_count,
        compile_micros: started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
    };
    Ok(CompiledNativeScene {
        publication: NativeScenePublication {
            generation_id: input.generation_id,
            kind: ScenePublicationKind::Full,
            registry_revision: input.registry_revision,
            document_id: Some(input.document.entry_id.0),
            identities,
            styles,
            topology,
            edges,
            positions,
            node_products,
            edge_products,
            entity_mappings,
            references: vec![ProductReferenceRecord {
                stable_ref: input.document.entry_id.0,
                source_offset: 0,
                source_len,
                kind: DOCUMENT_REFERENCE_KIND,
                flags: 0,
            }],
        },
        anchors,
        receipt,
    })
}

fn promoted_edges_by_candidate(
    generation: &VerifiedGraphGeneration,
) -> Result<
    HashMap<[u8; 32], &phoenix_graph_generation::AcceptedEdgeRecord>,
    NativeSceneCompilerError,
> {
    let accepted = generation
        .decisions()
        .iter()
        .filter(|decision| decision.status == DECISION_STATUS_ACCEPTED)
        .map(|decision| decision.candidate_id)
        .collect::<HashSet<_>>();
    let promoted_by_id = generation
        .accepted_edges()
        .iter()
        .filter(|edge| edge.flags & ACCEPTED_EDGE_FLAG_PROMOTED != 0)
        .map(|edge| (edge.id, edge))
        .collect::<HashMap<_, _>>();
    let mut promoted = HashMap::with_capacity(accepted.len());
    for candidate in generation.candidate_edges() {
        if !accepted.contains(&candidate.candidate_id) {
            continue;
        }
        let edge = promoted_by_id
            .get(&promoted_edge_id(candidate.candidate_id))
            .copied()
            .ok_or(NativeSceneCompilerError::GraphGenerationAuthorityMismatch)?;
        promoted.insert(candidate.candidate_id, edge);
    }
    if promoted.len() != accepted.len() {
        return Err(NativeSceneCompilerError::GraphGenerationAuthorityMismatch);
    }
    Ok(promoted)
}

fn validate_input(input: NativeSceneCompilerInput<'_>) -> Result<(), NativeSceneCompilerError> {
    if input.generation_id == 0 {
        return Err(NativeSceneCompilerError::ZeroGeneration);
    }
    if input.registry_revision != input.registry.revision() {
        return Err(NativeSceneCompilerError::RegistryRevisionMismatch {
            provided: input.registry_revision,
            canonical: input.registry.revision(),
        });
    }
    if input.verified_anchors.is_some_and(|anchors| {
        anchors.document().0 != input.document.entry_id.0
            || anchors.document_revision() != input.document.revision.0
            || anchors.content_hash() != input.document.content_hash.0
    }) {
        return Err(NativeSceneCompilerError::AnchorAuthorityMismatch);
    }
    if input.nli.is_some_and(|nli| {
        nli.binding.native_document_id != input.document.entry_id.0
            || nli.binding.document_revision != input.document.revision.0
            || nli.binding.content_hash != input.document.content_hash.0
            || nli.binding.target_registry_revision != input.registry_revision
            || nli.validate().is_err()
    }) {
        return Err(NativeSceneCompilerError::NliAuthorityMismatch);
    }
    input
        .palette
        .validate()
        .map_err(|_| NativeSceneCompilerError::InvalidPalette)
}

fn verified_surface(
    document: &DocumentLease,
    start: u32,
    end: u32,
) -> Result<&str, NativeSceneCompilerError> {
    let start_usize = usize::try_from(start)
        .map_err(|_| NativeSceneCompilerError::StaleMention { start, end })?;
    let end_usize =
        usize::try_from(end).map_err(|_| NativeSceneCompilerError::StaleMention { start, end })?;
    if start_usize >= end_usize
        || end_usize > document.content.len()
        || !document.content.is_char_boundary(start_usize)
        || !document.content.is_char_boundary(end_usize)
    {
        return Err(NativeSceneCompilerError::StaleMention { start, end });
    }
    Ok(&document.content[start_usize..end_usize])
}

fn verify_mention(
    document: &DocumentLease,
    start: u32,
    end: u32,
    surface: &str,
) -> Result<(), NativeSceneCompilerError> {
    let start_usize = usize::try_from(start)
        .map_err(|_| NativeSceneCompilerError::StaleMention { start, end })?;
    let end_usize =
        usize::try_from(end).map_err(|_| NativeSceneCompilerError::StaleMention { start, end })?;
    if start_usize >= end_usize
        || end_usize > document.content.len()
        || !document.content.is_char_boundary(start_usize)
        || !document.content.is_char_boundary(end_usize)
        || &document.content[start_usize..end_usize] != surface
    {
        return Err(NativeSceneCompilerError::StaleMention { start, end });
    }
    Ok(())
}

fn paragraph_boundaries(content: &str) -> Result<Vec<u32>, NativeSceneCompilerError> {
    let bytes = content.as_bytes();
    let mut boundaries = Vec::new();
    let mut cursor = 0usize;
    let mut line_start = 0usize;
    while let Some(relative) = memchr(b'\n', &bytes[cursor..]) {
        let newline = cursor + relative;
        let line = &bytes[line_start..newline];
        if line.iter().all(u8::is_ascii_whitespace) {
            boundaries.push(
                u32::try_from(newline + 1)
                    .map_err(|_| NativeSceneCompilerError::RangeOverflow("chunk boundary"))?,
            );
        }
        cursor = newline + 1;
        line_start = cursor;
    }
    Ok(boundaries)
}

fn build_edge_weights(
    chunks: &mut HashMap<u32, SmallVec<[u64; 8]>>,
) -> Result<HashMap<(u64, u64), u32>, NativeSceneCompilerError> {
    let mut weights = HashMap::<(u64, u64), u32>::new();
    for entities in chunks.values_mut() {
        entities.sort_unstable();
        entities.dedup();
        if entities.len() > MAX_CHUNK_ENTITIES {
            return Err(NativeSceneCompilerError::DenseChunk {
                actual: entities.len(),
                maximum: MAX_CHUNK_ENTITIES,
            });
        }
        for source in 0..entities.len() {
            for target in (source + 1)..entities.len() {
                if weights.len() >= MAX_EDGES
                    && !weights.contains_key(&(entities[source], entities[target]))
                {
                    return Err(NativeSceneCompilerError::EdgeLimit(MAX_EDGES));
                }
                let weight = weights
                    .entry((entities[source], entities[target]))
                    .or_insert(0);
                *weight = weight.saturating_add(1);
            }
        }
    }
    Ok(weights)
}

fn stable_edge_id(document_id: u64, source: u64, target: u64) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.native.cooccurrence-edge/v1\0");
    hasher.update(&document_id.to_le_bytes());
    hasher.update(&source.to_le_bytes());
    hasher.update(&target.to_le_bytes());
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

/// Stable scene edge identity for an unpromoted NLI candidate.
///
/// Review overlays use this exact identity to update the product record in
/// place without rebuilding topology. Promoted candidates use
/// `phoenix_graph_generation::promoted_edge_id` instead.
#[must_use]
pub fn proposed_nli_edge_id(document_id: u64, candidate_id: &[u8; 32]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.native.proposed-nli-edge/v1\0");
    hasher.update(&document_id.to_le_bytes());
    hasher.update(candidate_id);
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

#[derive(Clone, Copy)]
struct CapsMembership {
    parent_slot: u32,
    membership_count: u16,
}

fn blend(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        (left[0] + right[0]) * 0.5,
        (left[1] + right[1]) * 0.5,
        (left[2] + right[2]) * 0.5,
        0.28,
    ]
}

fn proposed_blend(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        (left[0] + right[0]) * 0.5,
        (left[1] + right[1]) * 0.5,
        (left[2] + right[2]) * 0.5,
        0.24,
    ]
}

const fn nli_edge_kind(kind: NliCandidateKind) -> u16 {
    match kind {
        NliCandidateKind::SameSurface => 10,
        NliCandidateKind::Alias => 11,
        NliCandidateKind::Coreference => 12,
        NliCandidateKind::Related => 13,
    }
}

fn source_flags(source: EntitySourceMask) -> u16 {
    u16::from(source.ner) | (u16::from(source.user_tagged) << 1)
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
