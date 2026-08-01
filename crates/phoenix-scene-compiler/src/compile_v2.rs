use crate::v2_projection::{projection_id, EdgeDraft, NodeDraft, ProjectionBuilder};
use crate::{NativeSceneCompilerError, VerifiedStructuralSource};
use hashbrown::HashMap;
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateId, CandidateStatus, ChunkRecord, DecisionAction,
    DecisionRecord, EntityRecord, EpisodeMemberKind, EpisodeMembershipRecord, EpisodeRecord,
    EventRecord, EvidenceRecord, IdentityCandidateRecord, MemoryStateCandidateRecord, PageKind,
    PublicationReceiptRecord, StructuralEdgeRecord, TemporalCandidateRecord,
    TypedRelationshipCandidateRecord, VerifiedGraphGenerationV2,
};
use phoenix_scene_contract::{
    CapsRole, EntityFamily, EntityKind, FamilyMask, HighlightPalette, RelationFamily, ReviewMask,
    ScopeMask, CAUSAL_MIDPOINT_NODE_KIND, CHUNK_NODE_KIND, CONTEXTUAL_MIDPOINT_NODE_KIND,
    DOCUMENT_NODE_KIND, EPISODE_NODE_KIND, EVENT_NODE_KIND, EVIDENCE_NODE_KIND,
    IDENTITY_MIDPOINT_NODE_KIND, MEMORY_STATE_NODE_KIND, RELATIONSHIP_FACT_NODE_KIND,
    TEMPORAL_MIDPOINT_NODE_KIND,
};
use phoenix_scene_product_index::ProductReferenceRecord;
use phoenix_scene_publisher::NativeScenePublication;
use phoenix_semantic_review::{ReviewCandidate, ReviewCatalog, ReviewPage};
use std::sync::Arc;
use std::time::Instant;

const DOCUMENT_COLOR: [f32; 4] = [0.24, 0.55, 0.95, 0.92];
const CHUNK_COLOR: [f32; 4] = [0.94, 0.28, 0.52, 0.82];
const EVIDENCE_EDGE_COLOR: [f32; 4] = [0.22, 0.73, 0.78, 0.34];
const EPISODE_COLOR: [f32; 4] = [0.72, 0.32, 0.94, 0.82];
const EVENT_COLOR: [f32; 4] = [0.98, 0.43, 0.15, 0.84];
const SCOPE: u64 = ScopeMask::NOTE.0 | ScopeMask::NARRATIVE.0;

pub struct NativeSceneCompilerV2Input<'a> {
    pub scene_generation_id: u64,
    pub generation: &'a VerifiedGraphGenerationV2,
    pub review_catalog: &'a ReviewCatalog,
    pub palette: HighlightPalette,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSceneCompileReceiptV2 {
    pub scene_generation_id: u64,
    pub source_generation_hash: [u8; 32],
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub registry_revision: u64,
    pub chunk_count: u64,
    pub verified_mentions: u64,
    pub node_count: u64,
    pub edge_count: u64,
    pub accepted_semantic_edges: u64,
    pub candidate_overlay_edges: u64,
    pub identity_candidate_count: u64,
    pub relationship_candidate_count: u64,
    pub event_candidate_count: u64,
    pub episode_count: u64,
    pub temporal_candidate_count: u64,
    pub causal_candidate_count: u64,
    pub memory_state_candidate_count: u64,
    pub contextual_evidence_count: u64,
    pub compile_micros: u64,
}

pub struct CompiledNativeSceneV2 {
    pub publication: NativeScenePublication,
    pub receipt: NativeSceneCompileReceiptV2,
}

#[derive(Clone, Copy)]
struct ProjectedStatus {
    candidate_id: CandidateId,
    mask: u32,
    accepted: bool,
}

struct ReviewAuthority<'a> {
    catalog: &'a ReviewCatalog,
    by_location: HashMap<(u16, u32), &'a ReviewCandidate>,
    decisions: HashMap<CandidateId, &'a DecisionRecord>,
}

impl<'a> ReviewAuthority<'a> {
    fn open(
        generation: &'a VerifiedGraphGenerationV2,
        catalog: &'a ReviewCatalog,
    ) -> Result<Self, NativeSceneCompilerError> {
        let header = generation.header();
        let authority = catalog.authority();
        let receipts: &[PublicationReceiptRecord] =
            typed(generation, PageKind::PublicationReceipts)?;
        let source_matches = authority.source_generation_hash == header.generation_hash
            || receipts.iter().any(|receipt| {
                receipt.previous_generation_hash == authority.source_generation_hash
            });
        if !source_matches
            || authority.document_hash != header.content_hash
            || authority.native_document_id != header.native_document_id
            || authority.document_revision != header.document_revision
            || authority.registry_revision != header.registry_revision
            || authority.producer_generation != header.producer_generation
        {
            return Err(NativeSceneCompilerError::V2AuthorityMismatch);
        }

        let mut by_location = HashMap::with_capacity(catalog.candidates().len());
        for candidate in catalog.candidates() {
            let key = (candidate.location.page, candidate.location.row_index);
            if by_location.insert(key, candidate).is_some() {
                return Err(NativeSceneCompilerError::IdentityCollision {
                    resource: "V2 review location",
                });
            }
        }
        let decision_rows: &[DecisionRecord] = typed(generation, PageKind::Decisions)?;
        let mut decisions = HashMap::with_capacity(decision_rows.len());
        for decision in decision_rows {
            if decisions.insert(decision.candidate_id, decision).is_some() {
                return Err(NativeSceneCompilerError::IdentityCollision {
                    resource: "V2 decision candidate",
                });
            }
        }
        Ok(Self {
            catalog,
            by_location,
            decisions,
        })
    }

    fn direct(
        &self,
        candidate_id: CandidateId,
        status: u16,
        page: ReviewPage,
        row: usize,
    ) -> Result<ProjectedStatus, NativeSceneCompilerError> {
        self.status(candidate_id, status, page, row)
    }

    fn located(
        &self,
        status: u16,
        page: ReviewPage,
        row: usize,
    ) -> Result<ProjectedStatus, NativeSceneCompilerError> {
        let row_index = u32::try_from(row)
            .map_err(|_| NativeSceneCompilerError::RangeOverflow("V2 review row"))?;
        let candidate = self.by_location.get(&(page as u16, row_index)).ok_or(
            NativeSceneCompilerError::V2MissingReviewBinding {
                page: page as u16,
                row: row_index,
            },
        )?;
        self.status(candidate.binding.origin.candidate_id, status, page, row)
    }

    fn status(
        &self,
        candidate_id: CandidateId,
        raw: u16,
        page: ReviewPage,
        row: usize,
    ) -> Result<ProjectedStatus, NativeSceneCompilerError> {
        let status = CandidateStatus::from_raw(raw)
            .ok_or(NativeSceneCompilerError::V2CandidateStatus(raw))?;
        let row_index = u32::try_from(row)
            .map_err(|_| NativeSceneCompilerError::RangeOverflow("V2 review row"))?;
        let candidate = self.catalog.get(candidate_id).ok_or(
            NativeSceneCompilerError::V2MissingReviewBinding {
                page: page as u16,
                row: row_index,
            },
        )?;
        if candidate.location.page != page as u16 || candidate.location.row_index != row_index {
            return Err(NativeSceneCompilerError::V2MissingReviewBinding {
                page: page as u16,
                row: row_index,
            });
        }
        if status == CandidateStatus::Proposed {
            return Ok(ProjectedStatus {
                candidate_id,
                mask: ReviewMask::PROPOSED.0,
                accepted: false,
            });
        }
        if status == CandidateStatus::Superseded {
            return Ok(ProjectedStatus {
                candidate_id,
                mask: ReviewMask::SUPERSEDED.0,
                accepted: false,
            });
        }
        let decision = self.decisions.get(&candidate_id).ok_or(
            NativeSceneCompilerError::V2DecisionReceiptMismatch(candidate_id),
        )?;
        let expected_action = match status {
            CandidateStatus::Accepted => DecisionAction::Accept,
            CandidateStatus::Rejected => DecisionAction::Reject,
            CandidateStatus::Deferred => DecisionAction::Defer,
            CandidateStatus::Proposed | CandidateStatus::Superseded => unreachable!(),
        };
        if decision.evidence_hash != candidate.binding.evidence_hash
            || decision.decided_at_revision != self.catalog.authority().document_revision
            || decision.registry_revision != self.catalog.authority().registry_revision
            || decision.producer_generation != self.catalog.authority().producer_generation
            || DecisionAction::from_raw(decision.action) != Some(expected_action)
            || CandidateStatus::from_raw(decision.status) != Some(status)
        {
            return Err(NativeSceneCompilerError::V2DecisionReceiptMismatch(
                candidate_id,
            ));
        }
        Ok(ProjectedStatus {
            candidate_id,
            mask: match status {
                CandidateStatus::Accepted => ReviewMask::ACCEPTED.0,
                CandidateStatus::Rejected => ReviewMask::REJECTED.0,
                CandidateStatus::Deferred => ReviewMask::DEFERRED.0,
                CandidateStatus::Proposed | CandidateStatus::Superseded => unreachable!(),
            },
            accepted: status == CandidateStatus::Accepted,
        })
    }
}

pub fn compile_graph_generation_v2(
    input: NativeSceneCompilerV2Input<'_>,
) -> Result<CompiledNativeSceneV2, NativeSceneCompilerError> {
    let started = Instant::now();
    input
        .palette
        .validate()
        .map_err(|_| NativeSceneCompilerError::InvalidPalette)?;
    let structural = VerifiedStructuralSource::open(input.generation)?;
    let review = ReviewAuthority::open(input.generation, input.review_catalog)?;
    let entities: &[EntityRecord] = typed(input.generation, PageKind::Entities)?;
    let evidence: &[EvidenceRecord] = typed(input.generation, PageKind::Evidence)?;
    let candidate_evidence: &[CandidateEvidenceBindingRecord] =
        typed(input.generation, PageKind::CandidateEvidenceBindings)?;
    let events: &[EventRecord] = typed(input.generation, PageKind::Events)?;
    let episodes: &[EpisodeRecord] = typed(input.generation, PageKind::Episodes)?;
    let memberships: &[EpisodeMembershipRecord] =
        typed(input.generation, PageKind::EpisodeMemberships)?;

    let event_statuses = events
        .iter()
        .enumerate()
        .map(|(row, record)| review.located(record.status, ReviewPage::Event, row))
        .collect::<Result<Vec<_>, _>>()?;
    let episode_statuses = episodes
        .iter()
        .enumerate()
        .map(|(row, record)| review.located(record.status, ReviewPage::Episode, row))
        .collect::<Result<Vec<_>, _>>()?;
    let membership_statuses = memberships
        .iter()
        .enumerate()
        .map(|(row, record)| review.located(record.status, ReviewPage::EpisodeMembership, row))
        .collect::<Result<Vec<_>, _>>()?;
    let accepted_memberships = accepted_membership_parents(memberships, &membership_statuses)?;

    // Chapters, paragraphs, and sentences remain exact source authority pages.
    // They are not graph products. The canvas projects the semantic read model:
    // document, dynamic chunks, evidence, entities, and real semantic records.
    let node_capacity = 1
        + structural.chunks().len()
        + entities.len()
        + evidence.len()
        + events.len()
        + episodes.len()
        + semantic_midpoint_count(input.generation)?;
    let candidate_edge_capacity = candidate_edge_count(input.generation)?;
    let edge_capacity = structural
        .structural_edges()
        .len()
        .saturating_add(structural.chunks().len())
        .saturating_add(evidence.len().saturating_mul(2))
        .saturating_add(candidate_edge_capacity)
        .saturating_add(semantic_midpoint_count(input.generation)?);
    let document = structural.document();
    let mut builder = ProjectionBuilder::with_capacity(
        input.scene_generation_id,
        input.generation.header().registry_revision,
        input.generation.header().native_document_id,
        node_capacity,
        edge_capacity,
    )?;

    add_structural_nodes(
        &mut builder,
        &structural,
        input.generation,
        &accepted_memberships,
    )?;
    add_entity_nodes(&mut builder, input.generation, entities, input.palette)?;
    add_evidence_nodes(
        &mut builder,
        input.generation,
        evidence,
        entities,
        input.palette,
    )?;
    add_episode_nodes(
        &mut builder,
        input.generation,
        episodes,
        &episode_statuses,
        document.id,
    )?;
    add_event_nodes(
        &mut builder,
        input.generation,
        events,
        &event_statuses,
        EventEvidence {
            records: evidence,
            bindings: candidate_evidence,
        },
        &accepted_memberships,
        document.id,
    )?;
    add_chunk_membership_edges(
        &mut builder,
        document.id,
        structural.chunks(),
        structural.structural_edges(),
    )?;
    add_source_edges(&mut builder, structural.structural_edges())?;
    add_evidence_projection_edges(&mut builder, evidence)?;

    let mut accepted_semantic_edges = 0_u64;
    let mut candidate_overlay_edges = 0_u64;
    add_candidate_edges(
        &mut builder,
        input.generation,
        &review,
        document.id,
        events,
        &event_statuses,
        episodes,
        &episode_statuses,
        memberships,
        &membership_statuses,
        evidence,
        candidate_evidence,
        &mut accepted_semantic_edges,
        &mut candidate_overlay_edges,
    )?;

    let publication = builder.finish(input.palette)?;
    let receipt = NativeSceneCompileReceiptV2 {
        scene_generation_id: input.scene_generation_id,
        source_generation_hash: input.generation.header().generation_hash,
        document_id: input.generation.header().native_document_id,
        document_revision: input.generation.header().document_revision,
        content_hash: input.generation.header().content_hash,
        registry_revision: input.generation.header().registry_revision,
        chunk_count: structural.chunks().len() as u64,
        verified_mentions: evidence.len() as u64,
        node_count: publication.identities.len() as u64,
        edge_count: publication.edges.len() as u64,
        accepted_semantic_edges,
        candidate_overlay_edges,
        identity_candidate_count: input
            .generation
            .descriptor(PageKind::IdentityCandidates)
            .count,
        relationship_candidate_count: input
            .generation
            .descriptor(PageKind::TypedRelationshipCandidates)
            .count,
        event_candidate_count: events.len() as u64,
        episode_count: episodes.len() as u64,
        temporal_candidate_count: input
            .generation
            .descriptor(PageKind::TemporalCandidates)
            .count,
        causal_candidate_count: input
            .generation
            .descriptor(PageKind::CausalCandidates)
            .count,
        memory_state_candidate_count: input
            .generation
            .descriptor(PageKind::MemoryStateCandidates)
            .count,
        contextual_evidence_count: input
            .generation
            .descriptor(PageKind::ContextualEvidence)
            .count,
        compile_micros: started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
    };
    Ok(CompiledNativeSceneV2 {
        publication,
        receipt,
    })
}

pub fn semantic_candidate_edge_id(candidate_id: CandidateId) -> u64 {
    projection_id(
        b"semantic-candidate",
        &[&candidate_id.0, &0_u64.to_le_bytes()],
    )
}

fn add_structural_nodes(
    builder: &mut ProjectionBuilder,
    source: &VerifiedStructuralSource<'_>,
    generation: &VerifiedGraphGenerationV2,
    memberships: &HashMap<u64, u64>,
) -> Result<(), NativeSceneCompilerError> {
    let document = source.document();
    builder.push_node(NodeDraft {
        id: document.id,
        label: text(generation, document.source_id)?,
        kind: DOCUMENT_NODE_KIND,
        family_mask: FamilyMask::STRUCTURE.0,
        scope_mask: SCOPE,
        review_mask: ReviewMask::ACCEPTED.0,
        color: DOCUMENT_COLOR,
        base_radius: 1.25,
        flags: 0,
        caps_role: CapsRole::Document,
        caps_parent: None,
        inspector_ref: 0,
        provenance_ref: 0,
    })?;
    builder.push_reference(ProductReferenceRecord {
        stable_ref: document.id,
        source_offset: 0,
        source_len: document.source_len,
        kind: 1,
        flags: 0,
    });
    for chunk in source.chunks() {
        let parent = memberships.get(&chunk.id).copied();
        push_structure(
            builder,
            chunk.id,
            Arc::from(format!("Chunk {}", chunk.id)),
            CHUNK_NODE_KIND,
            CapsRole::Chunk,
            parent.or(Some(document.id)),
        )?;
    }
    Ok(())
}

fn push_structure(
    builder: &mut ProjectionBuilder,
    id: u64,
    label: Arc<str>,
    kind: u16,
    caps_role: CapsRole,
    parent: Option<u64>,
) -> Result<(), NativeSceneCompilerError> {
    builder.push_node(NodeDraft {
        id,
        label,
        kind,
        family_mask: FamilyMask::STRUCTURE.0,
        scope_mask: SCOPE,
        review_mask: ReviewMask::ACCEPTED.0,
        color: CHUNK_COLOR,
        base_radius: 0.74,
        flags: 0,
        caps_role,
        caps_parent: parent,
        inspector_ref: 0,
        provenance_ref: 0,
    })
}

fn add_entity_nodes(
    builder: &mut ProjectionBuilder,
    generation: &VerifiedGraphGenerationV2,
    entities: &[EntityRecord],
    palette: HighlightPalette,
) -> Result<(), NativeSceneCompilerError> {
    for entity in entities {
        let kind = entity_kind(entity.kind)?;
        let family = kind.family();
        builder.push_node(NodeDraft {
            id: entity.id,
            label: text(generation, entity.label)?,
            kind: entity.kind,
            family_mask: entity_family_mask(family),
            scope_mask: SCOPE,
            review_mask: ReviewMask::ACCEPTED.0,
            color: palette.for_family(family).primary,
            base_radius: 0.63,
            flags: entity.flags as u16,
            caps_role: CapsRole::Entity,
            caps_parent: None,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
        builder.push_entity_mapping(entity.id);
    }
    Ok(())
}

fn add_evidence_nodes(
    builder: &mut ProjectionBuilder,
    generation: &VerifiedGraphGenerationV2,
    evidence: &[EvidenceRecord],
    entities: &[EntityRecord],
    palette: HighlightPalette,
) -> Result<(), NativeSceneCompilerError> {
    let families: HashMap<u64, EntityFamily> = entities
        .iter()
        .map(|entity| Ok((entity.id, entity_kind(entity.kind)?.family())))
        .collect::<Result<_, NativeSceneCompilerError>>()?;
    for record in evidence {
        let family = families
            .get(&record.entity_id)
            .copied()
            .unwrap_or(EntityFamily::Other);
        builder.push_node(NodeDraft {
            id: record.id,
            label: Arc::from("Evidence"),
            kind: EVIDENCE_NODE_KIND,
            family_mask: FamilyMask::STRUCTURE.0,
            scope_mask: SCOPE,
            review_mask: ReviewMask::ACCEPTED.0,
            color: palette.for_family(family).secondary,
            base_radius: 0.38,
            flags: record.flags,
            caps_role: CapsRole::Evidence,
            caps_parent: Some(record.chunk_id),
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
        builder.push_reference(ProductReferenceRecord {
            stable_ref: record.id,
            source_offset: u64::from(record.start),
            source_len: record.end.saturating_sub(record.start),
            kind: 2,
            flags: record.flags,
        });
    }
    let _ = generation;
    Ok(())
}

fn add_episode_nodes(
    builder: &mut ProjectionBuilder,
    generation: &VerifiedGraphGenerationV2,
    episodes: &[EpisodeRecord],
    statuses: &[ProjectedStatus],
    document_id: u64,
) -> Result<(), NativeSceneCompilerError> {
    for (episode, status) in episodes.iter().zip(statuses) {
        builder.push_node(NodeDraft {
            id: episode.id,
            label: text(generation, episode.label)?,
            kind: EPISODE_NODE_KIND,
            family_mask: FamilyMask::STRUCTURE.0,
            scope_mask: SCOPE,
            review_mask: status.mask,
            color: color_for_status(EPISODE_COLOR, *status),
            base_radius: 0.96,
            flags: episode.flags as u16,
            caps_role: CapsRole::Episode,
            caps_parent: Some(document_id),
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    Ok(())
}

fn add_event_nodes(
    builder: &mut ProjectionBuilder,
    generation: &VerifiedGraphGenerationV2,
    events: &[EventRecord],
    statuses: &[ProjectedStatus],
    evidence: EventEvidence<'_>,
    memberships: &HashMap<u64, u64>,
    document_id: u64,
) -> Result<(), NativeSceneCompilerError> {
    for (event, status) in events.iter().zip(statuses) {
        let evidence_parent = first_candidate_evidence_id(
            evidence.bindings,
            event.evidence_start,
            event.evidence_count,
        )?
        .filter(|evidence_id| {
            evidence
                .records
                .iter()
                .any(|record| record.id == *evidence_id)
        });
        builder.push_node(NodeDraft {
            id: event.id,
            label: text(generation, event.label)?,
            kind: EVENT_NODE_KIND,
            family_mask: FamilyMask::FACTS.0,
            scope_mask: SCOPE,
            review_mask: status.mask,
            color: color_for_status(EVENT_COLOR, *status),
            base_radius: 0.78,
            flags: event.flags as u16,
            caps_role: CapsRole::Event,
            caps_parent: memberships
                .get(&event.id)
                .copied()
                .or(evidence_parent)
                .or(Some(document_id)),
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct EventEvidence<'a> {
    records: &'a [EvidenceRecord],
    bindings: &'a [CandidateEvidenceBindingRecord],
}

fn add_source_edges(
    builder: &mut ProjectionBuilder,
    edges: &[StructuralEdgeRecord],
) -> Result<(), NativeSceneCompilerError> {
    for edge in edges {
        if !builder.contains_node(edge.source_id) || !builder.contains_node(edge.target_id) {
            continue;
        }
        builder.push_edge(EdgeDraft {
            id: edge.id,
            source: edge.source_id,
            target: edge.target_id,
            family_mask: FamilyMask::STRUCTURE.0,
            scope_mask: SCOPE,
            relation_mask: RelationFamily::Structural.mask().0,
            review_mask: ReviewMask::ACCEPTED.0,
            color: [0.28, 0.58, 0.84, 0.34],
            width: f32::from_bits(edge.weight_bits).max(0.3),
            kind: edge.relation,
            flags: edge.flags,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    Ok(())
}

fn add_chunk_membership_edges(
    builder: &mut ProjectionBuilder,
    document_id: u64,
    chunks: &[ChunkRecord],
    source_edges: &[StructuralEdgeRecord],
) -> Result<(), NativeSceneCompilerError> {
    for chunk in chunks {
        if source_edges
            .iter()
            .any(|edge| edge.source_id == document_id && edge.target_id == chunk.id)
        {
            continue;
        }
        let document_bytes = document_id.to_le_bytes();
        let chunk_bytes = chunk.id.to_le_bytes();
        builder.push_edge(EdgeDraft {
            id: projection_id(
                b"document-chunk-membership",
                &[&document_bytes, &chunk_bytes],
            ),
            source: document_id,
            target: chunk.id,
            family_mask: FamilyMask::STRUCTURE.0,
            scope_mask: SCOPE,
            relation_mask: RelationFamily::Structural.mask().0,
            review_mask: ReviewMask::ACCEPTED.0,
            color: [0.28, 0.58, 0.84, 0.34],
            width: 0.42,
            kind: 0,
            flags: 0,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    Ok(())
}

fn add_evidence_projection_edges(
    builder: &mut ProjectionBuilder,
    evidence: &[EvidenceRecord],
) -> Result<(), NativeSceneCompilerError> {
    for record in evidence {
        let evidence_id = record.id.to_le_bytes();
        builder.push_edge(EdgeDraft {
            id: projection_id(b"chunk-evidence", &[&evidence_id]),
            source: record.chunk_id,
            target: record.id,
            family_mask: FamilyMask::STRUCTURE.0,
            scope_mask: SCOPE,
            relation_mask: RelationFamily::Observation.mask().0,
            review_mask: ReviewMask::ACCEPTED.0,
            color: EVIDENCE_EDGE_COLOR,
            width: 0.32,
            kind: 0,
            flags: 0,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
        builder.push_edge(EdgeDraft {
            id: projection_id(b"evidence-entity", &[&evidence_id]),
            source: record.id,
            target: record.entity_id,
            family_mask: FamilyMask::STRUCTURE.0 | FamilyMask::ENTITIES.0,
            scope_mask: SCOPE,
            relation_mask: RelationFamily::Observation.mask().0,
            review_mask: ReviewMask::ACCEPTED.0,
            color: [0.28, 0.84, 0.65, 0.42],
            width: 0.34,
            kind: 0,
            flags: 0,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_candidate_edges(
    builder: &mut ProjectionBuilder,
    generation: &VerifiedGraphGenerationV2,
    review: &ReviewAuthority<'_>,
    document_id: u64,
    events: &[EventRecord],
    event_statuses: &[ProjectedStatus],
    episodes: &[EpisodeRecord],
    episode_statuses: &[ProjectedStatus],
    memberships: &[EpisodeMembershipRecord],
    membership_statuses: &[ProjectedStatus],
    evidence: &[EvidenceRecord],
    candidate_evidence: &[CandidateEvidenceBindingRecord],
    accepted_count: &mut u64,
    overlay_count: &mut u64,
) -> Result<(), NativeSceneCompilerError> {
    for ((event, status), row) in events.iter().zip(event_statuses).zip(0_u64..) {
        let source = first_candidate_evidence_id(
            candidate_evidence,
            event.evidence_start,
            event.evidence_count,
        )?
        .filter(|evidence_id| evidence.iter().any(|record| record.id == *evidence_id))
        .unwrap_or(document_id);
        push_candidate_with_domain(
            builder,
            *status,
            source,
            event.id,
            FamilyMask::FACTS.0,
            RelationFamily::Event,
            event.kind,
            f32::from_bits(event.confidence_bits),
            b"event-candidate",
            row,
            accepted_count,
            overlay_count,
        )?;
    }
    for ((episode, status), row) in episodes.iter().zip(episode_statuses).zip(0_u64..) {
        push_candidate_with_domain(
            builder,
            *status,
            document_id,
            episode.id,
            FamilyMask::STRUCTURE.0,
            RelationFamily::Structural,
            episode.family,
            f32::from_bits(episode.confidence_bits),
            b"episode-candidate",
            row,
            accepted_count,
            overlay_count,
        )?;
    }
    let relationships: &[TypedRelationshipCandidateRecord] =
        typed(generation, PageKind::TypedRelationshipCandidates)?;
    for (row, record) in relationships.iter().enumerate() {
        let status = review.direct(
            record.candidate_id,
            record.status,
            ReviewPage::TypedRelationship,
            row,
        )?;
        push_candidate_through_node(
            builder,
            status,
            record.source_entity_id,
            record.target_entity_id,
            FamilyMask::FACTS.0,
            relation_for(record.family),
            record.relation,
            f32::from_bits(record.confidence_bits),
            "Relationship",
            RELATIONSHIP_FACT_NODE_KIND,
            CapsRole::Fact,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    let identities: &[IdentityCandidateRecord] = typed(generation, PageKind::IdentityCandidates)?;
    for (row, record) in identities.iter().enumerate() {
        let status = review.direct(
            record.candidate_id,
            record.status,
            ReviewPage::Identity,
            row,
        )?;
        push_candidate_through_node(
            builder,
            status,
            record.left_entity_id,
            record.right_entity_id,
            FamilyMask::DISCOURSE.0,
            RelationFamily::Identity,
            record.kind,
            f32::from_bits(record.confidence_bits),
            "Identity proposal",
            IDENTITY_MIDPOINT_NODE_KIND,
            CapsRole::Fact,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    for (record, status) in memberships.iter().zip(membership_statuses) {
        push_candidate(
            builder,
            *status,
            record.episode_id,
            record.member_id,
            FamilyMask::STRUCTURE.0,
            RelationFamily::Structural,
            record.member_kind,
            f32::from_bits(record.confidence_bits),
            accepted_count,
            overlay_count,
        )?;
    }
    let temporal: &[TemporalCandidateRecord] = typed(generation, PageKind::TemporalCandidates)?;
    for (row, record) in temporal.iter().enumerate() {
        let status = review.direct(
            record.candidate_id,
            record.status,
            ReviewPage::Temporal,
            row,
        )?;
        push_candidate_through_node(
            builder,
            status,
            record.source_id,
            record.target_id,
            FamilyMask::FACTS.0,
            RelationFamily::Temporal,
            record.relation,
            f32::from_bits(record.confidence_bits),
            "Temporal relation",
            TEMPORAL_MIDPOINT_NODE_KIND,
            CapsRole::Fact,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    let causal: &[phoenix_graph_generation_v2::CausalCandidateRecord] =
        typed(generation, PageKind::CausalCandidates)?;
    for (row, record) in causal.iter().enumerate() {
        let status = review.direct(record.candidate_id, record.status, ReviewPage::Causal, row)?;
        push_candidate_through_node(
            builder,
            status,
            record.cause_id,
            record.effect_id,
            FamilyMask::FACTS.0,
            RelationFamily::Causal,
            record.relation,
            f32::from_bits(record.confidence_bits),
            "Causal relation",
            CAUSAL_MIDPOINT_NODE_KIND,
            CapsRole::Fact,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    let memory: &[MemoryStateCandidateRecord] = typed(generation, PageKind::MemoryStateCandidates)?;
    for (row, record) in memory.iter().enumerate() {
        let status = review.direct(
            record.candidate_id,
            record.status,
            ReviewPage::MemoryState,
            row,
        )?;
        push_candidate_through_node(
            builder,
            status,
            record.subject_id,
            record.context_id,
            FamilyMask::FACTS.0,
            RelationFamily::MemoryState,
            record.kind,
            f32::from_bits(record.confidence_bits),
            "Memory state",
            MEMORY_STATE_NODE_KIND,
            CapsRole::Memory,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    let contextual: &[phoenix_graph_generation_v2::ContextualEvidenceRecord] =
        typed(generation, PageKind::ContextualEvidence)?;
    for record in contextual {
        let source = record.source_entity_id.to_le_bytes();
        let target = record.target_entity_id.to_le_bytes();
        let source_mention = record.source_mention_id.to_le_bytes();
        let target_mention = record.target_mention_id.to_le_bytes();
        let chunk = record.chunk_id.to_le_bytes();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix.native.contextual-candidate/v2\0");
        hasher.update(&source);
        hasher.update(&target);
        hasher.update(&source_mention);
        hasher.update(&target_mention);
        hasher.update(&chunk);
        let candidate_id = CandidateId(*hasher.finalize().as_bytes());
        push_candidate_through_node(
            builder,
            ProjectedStatus {
                candidate_id,
                mask: ReviewMask::PROPOSED.0,
                accepted: false,
            },
            record.source_entity_id,
            record.target_entity_id,
            FamilyMask::DISCOURSE.0,
            RelationFamily::CoOccurrence,
            0,
            f32::from_bits(record.weight_bits),
            "Context evidence",
            CONTEXTUAL_MIDPOINT_NODE_KIND,
            CapsRole::Fact,
            document_id,
            accepted_count,
            overlay_count,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_candidate(
    builder: &mut ProjectionBuilder,
    status: ProjectedStatus,
    source: u64,
    target: u64,
    family_mask: u64,
    relation: RelationFamily,
    kind: u16,
    confidence: f32,
    accepted_count: &mut u64,
    overlay_count: &mut u64,
) -> Result<(), NativeSceneCompilerError> {
    push_candidate_with_domain(
        builder,
        status,
        source,
        target,
        family_mask,
        relation,
        kind,
        confidence,
        b"semantic-candidate",
        0,
        accepted_count,
        overlay_count,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_candidate_through_node(
    builder: &mut ProjectionBuilder,
    status: ProjectedStatus,
    source: u64,
    target: u64,
    family_mask: u64,
    relation: RelationFamily,
    kind: u16,
    confidence: f32,
    label: &'static str,
    node_kind: u16,
    caps_role: CapsRole,
    document_id: u64,
    accepted_count: &mut u64,
    overlay_count: &mut u64,
) -> Result<(), NativeSceneCompilerError> {
    let candidate = status.candidate_id.0;
    let node_id = projection_id(b"semantic-midpoint-node/v2", &[&candidate]);
    let color = color_for_status(relation_color(relation), status);
    builder.push_node(NodeDraft {
        id: node_id,
        label: Arc::from(label),
        kind: node_kind,
        family_mask,
        scope_mask: SCOPE,
        review_mask: status.mask,
        color,
        base_radius: if caps_role == CapsRole::Memory {
            0.58
        } else {
            0.52
        },
        flags: 0,
        caps_role,
        caps_parent: Some(document_id),
        inspector_ref: 0,
        provenance_ref: 0,
    })?;
    for (ordinal, (edge_source, edge_target)) in [(source, node_id), (node_id, target)]
        .into_iter()
        .enumerate()
    {
        builder.push_edge(EdgeDraft {
            id: projection_id(
                b"semantic-midpoint-edge/v2",
                &[&candidate, &(ordinal as u64).to_le_bytes()],
            ),
            source: edge_source,
            target: edge_target,
            family_mask,
            scope_mask: SCOPE,
            relation_mask: relation.mask().0,
            review_mask: status.mask,
            color,
            width: confidence.clamp(0.22, 1.0) * 0.62,
            kind,
            flags: 0,
            inspector_ref: 0,
            provenance_ref: 0,
        })?;
    }
    if status.accepted {
        *accepted_count = accepted_count.saturating_add(1);
    } else if status.mask == ReviewMask::PROPOSED.0 {
        *overlay_count = overlay_count.saturating_add(1);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_candidate_with_domain(
    builder: &mut ProjectionBuilder,
    status: ProjectedStatus,
    source: u64,
    target: u64,
    family_mask: u64,
    relation: RelationFamily,
    kind: u16,
    confidence: f32,
    domain: &[u8],
    ordinal: u64,
    accepted_count: &mut u64,
    overlay_count: &mut u64,
) -> Result<(), NativeSceneCompilerError> {
    let candidate = status.candidate_id.0;
    let ordinal = ordinal.to_le_bytes();
    builder.push_edge(EdgeDraft {
        id: projection_id(domain, &[&candidate, &ordinal]),
        source,
        target,
        family_mask,
        scope_mask: SCOPE,
        relation_mask: relation.mask().0,
        review_mask: status.mask,
        color: color_for_status(relation_color(relation), status),
        width: confidence.clamp(0.22, 1.0) * 0.62,
        kind,
        flags: 0,
        inspector_ref: 0,
        provenance_ref: 0,
    })?;
    if status.accepted {
        *accepted_count = accepted_count.saturating_add(1);
    } else if status.mask == ReviewMask::PROPOSED.0 {
        *overlay_count = overlay_count.saturating_add(1);
    }
    Ok(())
}

fn accepted_membership_parents(
    memberships: &[EpisodeMembershipRecord],
    statuses: &[ProjectedStatus],
) -> Result<HashMap<u64, u64>, NativeSceneCompilerError> {
    let mut parents = HashMap::new();
    for (membership, status) in memberships.iter().zip(statuses) {
        if EpisodeMemberKind::from_raw(membership.member_kind).is_none() {
            return Err(NativeSceneCompilerError::V2CandidateStatus(
                membership.member_kind,
            ));
        }
        if status.accepted
            && parents
                .insert(membership.member_id, membership.episode_id)
                .is_some()
        {
            return Err(NativeSceneCompilerError::IdentityCollision {
                resource: "accepted V2 episode membership",
            });
        }
    }
    Ok(parents)
}

fn first_candidate_evidence_id(
    bindings: &[CandidateEvidenceBindingRecord],
    start: u32,
    count: u32,
) -> Result<Option<u64>, NativeSceneCompilerError> {
    if count == 0 {
        return Ok(None);
    }
    let start = start as usize;
    let end = start
        .checked_add(count as usize)
        .ok_or(NativeSceneCompilerError::RangeOverflow(
            "V2 candidate evidence range",
        ))?;
    bindings
        .get(start..end)
        .and_then(|rows| rows.first())
        .map(|binding| Some(binding.evidence_id))
        .ok_or(NativeSceneCompilerError::V2InvalidPage(
            PageKind::CandidateEvidenceBindings,
        ))
}

fn candidate_edge_count(
    generation: &VerifiedGraphGenerationV2,
) -> Result<usize, NativeSceneCompilerError> {
    let pages = [
        PageKind::TypedRelationshipCandidates,
        PageKind::IdentityCandidates,
        PageKind::Events,
        PageKind::Episodes,
        PageKind::EpisodeMemberships,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
        PageKind::ContextualEvidence,
    ];
    pages.into_iter().try_fold(0_usize, |sum, page| {
        sum.checked_add(generation.descriptor(page).count as usize)
            .ok_or(NativeSceneCompilerError::RangeOverflow(
                "V2 candidate edge count",
            ))
    })
}

fn semantic_midpoint_count(
    generation: &VerifiedGraphGenerationV2,
) -> Result<usize, NativeSceneCompilerError> {
    [
        PageKind::TypedRelationshipCandidates,
        PageKind::IdentityCandidates,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
        PageKind::ContextualEvidence,
    ]
    .into_iter()
    .try_fold(0_usize, |sum, page| {
        sum.checked_add(generation.descriptor(page).count as usize)
            .ok_or(NativeSceneCompilerError::RangeOverflow(
                "V2 semantic midpoint count",
            ))
    })
}

fn color_for_status(mut color: [f32; 4], status: ProjectedStatus) -> [f32; 4] {
    color[3] *= match status.mask {
        value if value == ReviewMask::ACCEPTED.0 => 1.0,
        value if value == ReviewMask::REJECTED.0 => 0.18,
        value if value == ReviewMask::DEFERRED.0 => 0.32,
        value if value == ReviewMask::SUPERSEDED.0 => 0.12,
        _ => 0.52,
    };
    color
}

const fn relation_color(relation: RelationFamily) -> [f32; 4] {
    match relation {
        RelationFamily::CoOccurrence => [0.42, 0.58, 0.56, 0.38],
        RelationFamily::Observation => [0.24, 0.76, 0.86, 0.52],
        RelationFamily::Communication => [0.32, 0.58, 0.96, 0.56],
        RelationFamily::Causal => [0.94, 0.28, 0.34, 0.62],
        RelationFamily::Temporal => [0.94, 0.76, 0.16, 0.58],
        RelationFamily::Structural => [0.34, 0.62, 0.88, 0.48],
        RelationFamily::Identity => [0.61, 0.38, 0.96, 0.58],
        RelationFamily::Relationship => [0.94, 0.32, 0.62, 0.58],
        RelationFamily::Event => [0.98, 0.43, 0.15, 0.58],
        RelationFamily::MemoryState => [0.28, 0.82, 0.52, 0.56],
    }
}

fn relation_for(family: u16) -> RelationFamily {
    match phoenix_graph_generation_v2::SemanticFamily::from_raw(family) {
        Some(phoenix_graph_generation_v2::SemanticFamily::Relationship)
        | Some(phoenix_graph_generation_v2::SemanticFamily::GenericRelated) => {
            RelationFamily::Relationship
        }
        Some(phoenix_graph_generation_v2::SemanticFamily::Event)
        | Some(phoenix_graph_generation_v2::SemanticFamily::Episode) => RelationFamily::Event,
        Some(phoenix_graph_generation_v2::SemanticFamily::Causal) => RelationFamily::Causal,
        Some(phoenix_graph_generation_v2::SemanticFamily::Temporal) => RelationFamily::Temporal,
        Some(phoenix_graph_generation_v2::SemanticFamily::MemoryState) => {
            RelationFamily::MemoryState
        }
        Some(phoenix_graph_generation_v2::SemanticFamily::ContextualCoOccurrence) => {
            RelationFamily::CoOccurrence
        }
        Some(phoenix_graph_generation_v2::SemanticFamily::Identity)
        | Some(phoenix_graph_generation_v2::SemanticFamily::Alias)
        | Some(phoenix_graph_generation_v2::SemanticFamily::Coreference) => {
            RelationFamily::Identity
        }
        None => RelationFamily::Observation,
    }
}

fn entity_kind(raw: u16) -> Result<EntityKind, NativeSceneCompilerError> {
    EntityKind::TOOLBAR
        .into_iter()
        .find(|kind| *kind as u16 == raw)
        .ok_or(NativeSceneCompilerError::V2EntityKind(raw))
}

fn entity_family_mask(family: EntityFamily) -> u64 {
    match family {
        EntityFamily::Character => 1 << 0,
        EntityFamily::Location => 1 << 1,
        EntityFamily::Organization => 1 << 2,
        EntityFamily::Item => 1 << 3,
        EntityFamily::Concept => 1 << 4,
        EntityFamily::Event => 1 << 5,
        EntityFamily::Structure => 1 << 6,
        EntityFamily::Other => 1 << 7,
    }
}

fn text(
    generation: &VerifiedGraphGenerationV2,
    reference: phoenix_graph_generation_v2::StringRef,
) -> Result<Arc<str>, NativeSceneCompilerError> {
    generation
        .resolve_string(reference)
        .map(Arc::from)
        .map_err(|_| NativeSceneCompilerError::V2StringReference)
}

fn typed<T: bytemuck::Pod>(
    generation: &VerifiedGraphGenerationV2,
    page: PageKind,
) -> Result<&[T], NativeSceneCompilerError> {
    generation
        .typed_page(page)
        .map_err(|_| NativeSceneCompilerError::V2InvalidPage(page))
}
