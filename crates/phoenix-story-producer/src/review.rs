use crate::{
    derive_episode_candidate_id, derive_episode_membership_candidate_id, derive_event_candidate_id,
    relationship_code, story_candidate_origin, EpisodeMember, EpisodeMembershipInput,
    StoryProducerError,
};
use bytemuck::{bytes_of, Pod};
use hashbrown::HashMap;
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateId, CausalCandidateRecord, EpisodeId,
    EpisodeMemberKind, EpisodeMembershipRecord, EpisodeRecord, EventId, EventRecord,
    EvidenceRecord, MemoryStateCandidateRecord, PageKind, TemporalCandidateRecord,
    TypedRelationshipCandidateRecord, VerifiedGraphGenerationV2,
};
use phoenix_semantic_lens::{
    CoreSemanticClass, EndpointKind, LensNeutralReviewBinding, SemanticEndpointRef,
};
use phoenix_semantic_review::{
    ReviewAuthority, ReviewCandidate, ReviewCandidateLocation, ReviewCatalog, ReviewPage,
};

const ENDPOINT_SOURCE_SHIFT: u32 = 8;
const ENDPOINT_TARGET_SHIFT: u32 = 12;

pub fn story_review_catalog(
    generation: &VerifiedGraphGenerationV2,
) -> Result<ReviewCatalog, StoryProducerError> {
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence)?;
    let evidence_by_id = evidence
        .iter()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();
    let bindings: &[CandidateEvidenceBindingRecord] =
        generation.typed_page(PageKind::CandidateEvidenceBindings)?;
    let authority = ReviewAuthority {
        source_generation_hash: generation.header().generation_hash,
        document_hash: generation.header().content_hash,
        native_document_id: generation.header().native_document_id,
        document_revision: generation.header().document_revision,
        registry_revision: generation.header().registry_revision,
        producer_generation: generation.header().producer_generation,
    };
    let mut output = Vec::new();

    let relationships: &[TypedRelationshipCandidateRecord] =
        generation.typed_page(PageKind::TypedRelationshipCandidates)?;
    for (index, row) in relationships.iter().enumerate() {
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            row.candidate_id,
            row,
            story_candidate_origin(row.candidate_id, CoreSemanticClass::Relation, row.relation)
                .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            SemanticEndpointRef::new(EndpointKind::Entity, row.source_entity_id),
            SemanticEndpointRef::new(EndpointKind::Entity, row.target_entity_id),
            ReviewCandidateLocation::new(ReviewPage::TypedRelationship, checked(index)?),
        )?;
        debug_assert_eq!(
            output.last().map(|item| item.binding.origin.semantic_code),
            Some(relationship_code(crate::RelationshipKind::try_from(
                row.relation
            )?))
        );
    }

    let events: &[EventRecord] = generation.typed_page(PageKind::Events)?;
    for (index, row) in events.iter().enumerate() {
        let candidate_id =
            derive_event_candidate_id(&generation.header().content_hash, EventId(row.id));
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            candidate_id,
            row,
            story_candidate_origin(candidate_id, CoreSemanticClass::Occurrence, row.kind)
                .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            SemanticEndpointRef::new(EndpointKind::Occurrence, row.id),
            SemanticEndpointRef::default(),
            ReviewCandidateLocation::new(ReviewPage::Event, checked(index)?),
        )?;
    }

    let episodes: &[EpisodeRecord] = generation.typed_page(PageKind::Episodes)?;
    let memberships: &[EpisodeMembershipRecord] =
        generation.typed_page(PageKind::EpisodeMemberships)?;
    for (index, row) in episodes.iter().enumerate() {
        let candidate_id =
            derive_episode_candidate_id(&generation.header().content_hash, EpisodeId(row.id));
        let first = memberships
            .get(row.membership_start as usize)
            .ok_or(StoryProducerError::InvalidEpisodeMembership)?;
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            candidate_id,
            row,
            story_candidate_origin(candidate_id, CoreSemanticClass::Grouping, row.family)
                .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            SemanticEndpointRef::new(EndpointKind::Grouping, row.id),
            member_endpoint(first)?,
            ReviewCandidateLocation::new(ReviewPage::Episode, checked(index)?),
        )?;
    }
    for (index, row) in memberships.iter().enumerate() {
        let binding_rows = binding_slice(bindings, row.evidence_start, row.evidence_count)?;
        let evidence_ids = binding_rows
            .iter()
            .map(|binding| phoenix_graph_generation_v2::EvidenceId(binding.evidence_id))
            .collect::<Vec<_>>();
        let member = match EpisodeMemberKind::from_raw(row.member_kind) {
            Some(EpisodeMemberKind::Chunk) => {
                EpisodeMember::Chunk(phoenix_graph_generation_v2::ChunkId(row.member_id))
            }
            Some(EpisodeMemberKind::Event) => EpisodeMember::Event(EventId(row.member_id)),
            None => return Err(StoryProducerError::InvalidEpisodeMembership),
        };
        let input = EpisodeMembershipInput {
            member,
            evidence_ids: &evidence_ids,
            confidence: f32::from_bits(row.confidence_bits),
        };
        let candidate_id = derive_episode_membership_candidate_id(
            &generation.header().content_hash,
            EpisodeId(row.episode_id),
            &input,
        );
        let parent = episodes
            .iter()
            .find(|episode| episode.id == row.episode_id)
            .ok_or(StoryProducerError::InvalidEpisodeMembership)?;
        push_candidate(
            &mut output,
            generation,
            binding_rows,
            &evidence_by_id,
            candidate_id,
            row,
            story_candidate_origin(candidate_id, CoreSemanticClass::Grouping, parent.family)
                .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            SemanticEndpointRef::new(EndpointKind::Grouping, row.episode_id),
            member_endpoint(row)?,
            ReviewCandidateLocation::new(ReviewPage::EpisodeMembership, checked(index)?),
        )?;
    }

    let temporal: &[TemporalCandidateRecord] =
        generation.typed_page(PageKind::TemporalCandidates)?;
    for (index, row) in temporal.iter().enumerate() {
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            row.candidate_id,
            row,
            story_candidate_origin(
                row.candidate_id,
                CoreSemanticClass::TemporalConstraint,
                row.relation,
            )
            .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            encoded_endpoint(row.flags, ENDPOINT_SOURCE_SHIFT, row.source_id)?,
            encoded_endpoint(row.flags, ENDPOINT_TARGET_SHIFT, row.target_id)?,
            ReviewCandidateLocation::new(ReviewPage::Temporal, checked(index)?),
        )?;
    }

    let causal: &[CausalCandidateRecord] = generation.typed_page(PageKind::CausalCandidates)?;
    for (index, row) in causal.iter().enumerate() {
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            row.candidate_id,
            row,
            story_candidate_origin(row.candidate_id, CoreSemanticClass::Influence, row.relation)
                .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            encoded_endpoint(row.flags, ENDPOINT_SOURCE_SHIFT, row.cause_id)?,
            encoded_endpoint(row.flags, ENDPOINT_TARGET_SHIFT, row.effect_id)?,
            ReviewCandidateLocation::new(ReviewPage::Causal, checked(index)?),
        )?;
    }

    let memory: &[MemoryStateCandidateRecord] =
        generation.typed_page(PageKind::MemoryStateCandidates)?;
    for (index, row) in memory.iter().enumerate() {
        push_candidate(
            &mut output,
            generation,
            binding_slice(bindings, row.evidence_start, row.evidence_count)?,
            &evidence_by_id,
            row.candidate_id,
            row,
            story_candidate_origin(
                row.candidate_id,
                CoreSemanticClass::AttributedState,
                row.kind,
            )
            .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?,
            SemanticEndpointRef::new(EndpointKind::Entity, row.subject_id),
            encoded_endpoint(row.flags, ENDPOINT_TARGET_SHIFT, row.context_id)?,
            ReviewCandidateLocation::new(ReviewPage::MemoryState, checked(index)?),
        )?;
    }

    Ok(ReviewCatalog::new(authority, output)?)
}

#[allow(clippy::too_many_arguments)]
fn push_candidate<T: Pod>(
    output: &mut Vec<ReviewCandidate>,
    generation: &VerifiedGraphGenerationV2,
    candidate_bindings: &[CandidateEvidenceBindingRecord],
    evidence_by_id: &HashMap<u64, &EvidenceRecord>,
    candidate_id: CandidateId,
    row: &T,
    origin: phoenix_semantic_lens::CandidateOrigin,
    source: SemanticEndpointRef,
    target: SemanticEndpointRef,
    location: ReviewCandidateLocation,
) -> Result<(), StoryProducerError> {
    if candidate_bindings.is_empty() {
        return Err(StoryProducerError::InvalidEvidenceBinding);
    }
    let mut evidence_hash = blake3::Hasher::new();
    evidence_hash.update(b"phoenix-review-evidence/v1");
    for binding in candidate_bindings {
        if binding.candidate_id != candidate_id {
            return Err(StoryProducerError::InvalidEvidenceBinding);
        }
        evidence_hash.update(bytes_of(binding));
        let evidence = evidence_by_id
            .get(&binding.evidence_id)
            .ok_or(StoryProducerError::InvalidEvidenceBinding)?;
        evidence_hash.update(bytes_of(*evidence));
    }
    output.push(ReviewCandidate {
        binding: LensNeutralReviewBinding {
            origin,
            origin_alignment_padding: 0,
            source,
            target,
            document_hash: generation.header().content_hash,
            candidate_hash: *blake3::hash(bytes_of(row)).as_bytes(),
            evidence_hash: *evidence_hash.finalize().as_bytes(),
            producer_generation: generation.header().producer_generation,
            registry_revision: generation.header().registry_revision,
            flags: 0,
            reserved: 0,
        },
        location,
    });
    Ok(())
}

fn binding_slice(
    bindings: &[CandidateEvidenceBindingRecord],
    start: u32,
    count: u32,
) -> Result<&[CandidateEvidenceBindingRecord], StoryProducerError> {
    let start = start as usize;
    let end = start
        .checked_add(count as usize)
        .ok_or(StoryProducerError::InvalidEvidenceBinding)?;
    bindings
        .get(start..end)
        .ok_or(StoryProducerError::InvalidEvidenceBinding)
}

fn member_endpoint(
    row: &EpisodeMembershipRecord,
) -> Result<SemanticEndpointRef, StoryProducerError> {
    match EpisodeMemberKind::from_raw(row.member_kind) {
        Some(EpisodeMemberKind::Chunk) => {
            Ok(SemanticEndpointRef::new(EndpointKind::Chunk, row.member_id))
        }
        Some(EpisodeMemberKind::Event) => Ok(SemanticEndpointRef::new(
            EndpointKind::Occurrence,
            row.member_id,
        )),
        None => Err(StoryProducerError::InvalidEpisodeMembership),
    }
}

fn encoded_endpoint(
    flags: u32,
    shift: u32,
    id: u64,
) -> Result<SemanticEndpointRef, StoryProducerError> {
    let kind = match (flags >> shift) & 0x0f {
        1 => EndpointKind::Entity,
        2 => EndpointKind::Occurrence,
        3 => EndpointKind::Grouping,
        4 => EndpointKind::Chunk,
        _ => return Err(StoryProducerError::UnknownReference),
    };
    Ok(SemanticEndpointRef::new(kind, id))
}

fn checked(index: usize) -> Result<u32, StoryProducerError> {
    u32::try_from(index).map_err(|_| StoryProducerError::RecordCountOverflow)
}

impl TryFrom<u16> for crate::RelationshipKind {
    type Error = StoryProducerError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::CommunicatesWith),
            2 => Ok(Self::Supports),
            3 => Ok(Self::Opposes),
            4 => Ok(Self::Owns),
            5 => Ok(Self::LocatedIn),
            6 => Ok(Self::ParticipatesIn),
            7 => Ok(Self::Knows),
            _ => Err(StoryProducerError::InvalidCandidateIdentity),
        }
    }
}
