use crate::ids::{
    derive_episode_candidate_id, derive_episode_membership_candidate_id, derive_event_candidate_id,
};
use crate::receipt::append_story_receipts;
use crate::types::{EpisodeMember, ProducerRegistration, StoryProducerInput};
use crate::validate::{
    validate_input, SourceIndex, CANDIDATE_FLAG_MODEL_RANKED, ENDPOINT_SOURCE_SHIFT,
    ENDPOINT_TARGET_SHIFT,
};
use crate::StoryProducerError;
use bytemuck::cast_slice;
use phoenix_graph_generation_v2::{
    AuthorityClass, CandidateEvidenceBindingRecord, CandidateId, CandidateStatus, CapabilityRecord,
    CausalCandidateRecord, ContextualEvidenceRecord, EpisodeMemberKind, EpisodeMembershipRecord,
    EpisodeRecord, EventRecord, EvidenceId, EvidenceRole, MemoryStateCandidateRecord,
    ModelIdentityRecord, PublicationReceiptRecord, SemanticFamily, StageReceiptRecord, StringRef,
    TemporalCandidateRecord, TypedRelationshipCandidateRecord,
};

pub(crate) struct BuiltStoryGeneration {
    pub strings: Vec<u8>,
    pub typed_relationships: Vec<TypedRelationshipCandidateRecord>,
    pub events: Vec<EventRecord>,
    pub episodes: Vec<EpisodeRecord>,
    pub episode_memberships: Vec<EpisodeMembershipRecord>,
    pub temporal: Vec<TemporalCandidateRecord>,
    pub causal: Vec<CausalCandidateRecord>,
    pub memory_state: Vec<MemoryStateCandidateRecord>,
    pub contextual_evidence: Vec<ContextualEvidenceRecord>,
    pub candidate_evidence_bindings: Vec<CandidateEvidenceBindingRecord>,
    pub capabilities: Vec<CapabilityRecord>,
    pub model_identities: Vec<ModelIdentityRecord>,
    pub stage_receipts: Vec<StageReceiptRecord>,
    pub publication_receipts: Vec<PublicationReceiptRecord>,
    pub candidate_authority_hash: [u8; 32],
    pub story_binding_start: usize,
    pub model_ranked_count: usize,
    pub unsupported_mask: u16,
}

pub(crate) fn build_story_pages(
    input: &StoryProducerInput<'_>,
) -> Result<BuiltStoryGeneration, StoryProducerError> {
    let index = validate_input(input)?;
    let mut strings = input
        .source
        .page_bytes(phoenix_graph_generation_v2::PageKind::Strings)
        .to_vec();
    let mut bindings: Vec<CandidateEvidenceBindingRecord> = typed_copy(
        input,
        phoenix_graph_generation_v2::PageKind::CandidateEvidenceBindings,
    )?;
    let story_binding_start = bindings.len();

    let typed_relationships = pack_relationships(input, &index, &mut bindings)?;
    let events = pack_events(input, &index, &mut strings, &mut bindings)?;
    let (episodes, episode_memberships) =
        pack_episodes(input, &index, &mut strings, &mut bindings)?;
    let temporal = pack_temporal(input, &index, &mut bindings)?;
    let causal = pack_causal(input, &index, &mut bindings)?;
    let memory_state = pack_memory(input, &index, &mut strings, &mut bindings)?;
    let contextual_evidence = input.contextual_evidence.to_vec();

    let mut capabilities = typed_copy(input, phoenix_graph_generation_v2::PageKind::Capabilities)?;
    let mut model_identities = typed_copy(
        input,
        phoenix_graph_generation_v2::PageKind::ModelIdentities,
    )?;
    let mut stage_receipts =
        typed_copy(input, phoenix_graph_generation_v2::PageKind::StageReceipts)?;
    let mut publication_receipts = typed_copy(
        input,
        phoenix_graph_generation_v2::PageKind::PublicationReceipts,
    )?;
    let model_ranked_count = index.ranking.len();
    let unsupported_mask = append_story_receipts(
        input,
        &mut strings,
        &mut capabilities,
        &mut model_identities,
        &mut stage_receipts,
        &mut publication_receipts,
        [
            typed_relationships.len(),
            events.len(),
            episodes.len(),
            temporal.len(),
            causal.len(),
            memory_state.len(),
        ],
        contextual_evidence.len(),
        model_ranked_count,
    )?;

    let candidate_authority_hash = candidate_authority_hash(
        &typed_relationships,
        &events,
        &episodes,
        &episode_memberships,
        &temporal,
        &causal,
        &memory_state,
        &bindings[story_binding_start..],
    );
    publication_receipts
        .last_mut()
        .ok_or(StoryProducerError::RecordCountOverflow)?
        .authority_hash = candidate_authority_hash;

    Ok(BuiltStoryGeneration {
        strings,
        typed_relationships,
        events,
        episodes,
        episode_memberships,
        temporal,
        causal,
        memory_state,
        contextual_evidence,
        candidate_evidence_bindings: bindings,
        capabilities,
        model_identities,
        stage_receipts,
        publication_receipts,
        candidate_authority_hash,
        story_binding_start,
        model_ranked_count,
        unsupported_mask,
    })
}

fn pack_relationships(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<Vec<TypedRelationshipCandidateRecord>, StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.relationships
    else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(rules.len());
    for rule in rules {
        let start = binding_start(bindings)?;
        push_binding(
            bindings,
            rule.candidate_id,
            rule.source_evidence_id,
            EvidenceRole::Source,
            0,
        )?;
        push_binding(
            bindings,
            rule.candidate_id,
            rule.target_evidence_id,
            EvidenceRole::Target,
            1,
        )?;
        for evidence in rule.additional_evidence_ids {
            push_binding(
                bindings,
                rule.candidate_id,
                *evidence,
                EvidenceRole::Premise,
                bindings.len() - start as usize,
            )?;
        }
        let (confidence_bits, flags) = ranked(index, rule.candidate_id, rule.confidence);
        records.push(TypedRelationshipCandidateRecord {
            candidate_id: rule.candidate_id,
            source_entity_id: rule.source_entity_id.0,
            target_entity_id: rule.target_entity_id.0,
            evidence_start: start,
            evidence_count: binding_count(bindings, start)?,
            premise_start: 0,
            premise_end: 0,
            relation: rule.relation as u16,
            family: SemanticFamily::Relationship as u16,
            status: CandidateStatus::Proposed as u16,
            flags_u16: 0,
            confidence_bits,
            flags,
        });
    }
    Ok(records)
}

fn pack_events(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    strings: &mut Vec<u8>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<Vec<EventRecord>, StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.events else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(rules.len());
    for rule in rules {
        let candidate =
            derive_event_candidate_id(&input.source.header().content_hash, rule.event_id);
        let start = push_bindings(bindings, candidate, rule.evidence_ids, EvidenceRole::Source)?;
        let (confidence_bits, flags) = ranked(index, candidate, rule.confidence);
        records.push(EventRecord {
            id: rule.event_id.0,
            label: push_string(strings, rule.label)?,
            evidence_start: start,
            evidence_count: binding_count(bindings, start)?,
            start: rule.label_start,
            end: rule.label_end,
            kind: rule.kind as u16,
            status: CandidateStatus::Proposed as u16,
            confidence_bits,
            flags,
            reserved: 0,
        });
    }
    Ok(records)
}

fn pack_episodes(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    strings: &mut Vec<u8>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<(Vec<EpisodeRecord>, Vec<EpisodeMembershipRecord>), StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.episodes else {
        return Ok((Vec::new(), Vec::new()));
    };
    let membership_capacity = rules.iter().map(|rule| rule.memberships.len()).sum();
    let mut episodes = Vec::with_capacity(rules.len());
    let mut memberships = Vec::with_capacity(membership_capacity);
    for rule in rules {
        let candidate =
            derive_episode_candidate_id(&input.source.header().content_hash, rule.episode_id);
        let evidence_start =
            push_bindings(bindings, candidate, rule.evidence_ids, EvidenceRole::Source)?;
        let evidence_count = binding_count(bindings, evidence_start)?;
        let membership_start = checked_u32(memberships.len())?;
        for membership in rule.memberships {
            let membership_candidate = derive_episode_membership_candidate_id(
                &input.source.header().content_hash,
                rule.episode_id,
                membership,
            );
            let start = push_bindings(
                bindings,
                membership_candidate,
                membership.evidence_ids,
                EvidenceRole::Membership,
            )?;
            let (member_id, member_kind) = match membership.member {
                EpisodeMember::Chunk(id) => (id.0, EpisodeMemberKind::Chunk),
                EpisodeMember::Event(id) => (id.0, EpisodeMemberKind::Event),
            };
            memberships.push(EpisodeMembershipRecord {
                episode_id: rule.episode_id.0,
                member_id,
                evidence_start: start,
                evidence_count: binding_count(bindings, start)?,
                member_kind: member_kind as u16,
                status: CandidateStatus::Proposed as u16,
                confidence_bits: membership.confidence.to_bits(),
                flags: 0,
                reserved: 0,
            });
        }
        let (confidence_bits, flags) = ranked(index, candidate, rule.confidence);
        episodes.push(EpisodeRecord {
            id: rule.episode_id.0,
            label: push_string(strings, rule.label)?,
            evidence_start,
            evidence_count,
            membership_start,
            membership_count: checked_u32(memberships.len())? - membership_start,
            ordinal: rule.ordinal,
            status: CandidateStatus::Proposed as u16,
            family: rule.family as u16,
            confidence_bits,
            flags,
        });
    }
    Ok((episodes, memberships))
}

fn pack_temporal(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<Vec<TemporalCandidateRecord>, StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.temporal else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(rules.len());
    for rule in rules {
        let start = push_bindings(
            bindings,
            rule.candidate_id,
            rule.evidence_ids,
            EvidenceRole::Premise,
        )?;
        let (confidence_bits, model_flags) = ranked(index, rule.candidate_id, rule.confidence);
        records.push(TemporalCandidateRecord {
            candidate_id: rule.candidate_id,
            source_id: rule.source.raw(),
            target_id: rule.target.raw(),
            evidence_start: start,
            evidence_count: binding_count(bindings, start)?,
            relation: rule.relation as u16,
            status: CandidateStatus::Proposed as u16,
            confidence_bits,
            flags: endpoint_flags(rule.source, rule.target) | model_flags,
            reserved: 0,
        });
    }
    Ok(records)
}

fn pack_causal(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<Vec<CausalCandidateRecord>, StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.causal else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(rules.len());
    for rule in rules {
        let start = push_bindings(
            bindings,
            rule.candidate_id,
            rule.evidence_ids,
            EvidenceRole::Cause,
        )?;
        let (confidence_bits, model_flags) = ranked(index, rule.candidate_id, rule.confidence);
        records.push(CausalCandidateRecord {
            candidate_id: rule.candidate_id,
            cause_id: rule.cause.raw(),
            effect_id: rule.effect.raw(),
            evidence_start: start,
            evidence_count: binding_count(bindings, start)?,
            relation: rule.relation as u16,
            status: CandidateStatus::Proposed as u16,
            confidence_bits,
            flags: endpoint_flags(rule.cause, rule.effect) | model_flags,
            reserved: 0,
        });
    }
    Ok(records)
}

fn pack_memory(
    input: &StoryProducerInput<'_>,
    index: &SourceIndex<'_>,
    strings: &mut Vec<u8>,
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
) -> Result<Vec<MemoryStateCandidateRecord>, StoryProducerError> {
    let ProducerRegistration::Deterministic { rules, .. } = input.registrations.memory_state else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(rules.len());
    for rule in rules {
        let start = push_bindings(
            bindings,
            rule.candidate_id,
            rule.evidence_ids,
            EvidenceRole::State,
        )?;
        let (confidence_bits, model_flags) = ranked(index, rule.candidate_id, rule.confidence);
        records.push(MemoryStateCandidateRecord {
            candidate_id: rule.candidate_id,
            subject_id: rule.subject_entity_id.0,
            context_id: rule.context.raw(),
            key: push_string(strings, rule.key)?,
            value: push_string(strings, rule.value)?,
            evidence_start: start,
            evidence_count: binding_count(bindings, start)?,
            status: CandidateStatus::Proposed as u16,
            kind: rule.kind as u16,
            confidence_bits,
            flags: (rule.context.tag() << ENDPOINT_TARGET_SHIFT) | model_flags,
            reserved: 0,
        });
    }
    Ok(records)
}

fn ranked(index: &SourceIndex<'_>, candidate: CandidateId, fallback: f32) -> (u32, u32) {
    index.ranking.get(&candidate).map_or_else(
        || (fallback.to_bits(), 0),
        |score| (score.to_bits(), CANDIDATE_FLAG_MODEL_RANKED),
    )
}

fn endpoint_flags(source: crate::SemanticEndpoint, target: crate::SemanticEndpoint) -> u32 {
    (source.tag() << ENDPOINT_SOURCE_SHIFT) | (target.tag() << ENDPOINT_TARGET_SHIFT)
}

fn push_bindings(
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
    candidate: CandidateId,
    evidence: &[EvidenceId],
    role: EvidenceRole,
) -> Result<u32, StoryProducerError> {
    let start = binding_start(bindings)?;
    for (ordinal, evidence) in evidence.iter().enumerate() {
        push_binding(bindings, candidate, *evidence, role, ordinal)?;
    }
    Ok(start)
}

fn push_binding(
    bindings: &mut Vec<CandidateEvidenceBindingRecord>,
    candidate: CandidateId,
    evidence: EvidenceId,
    role: EvidenceRole,
    ordinal: usize,
) -> Result<(), StoryProducerError> {
    bindings.push(CandidateEvidenceBindingRecord {
        candidate_id: candidate,
        evidence_id: evidence.0,
        ordinal: checked_u32(ordinal)?,
        role: role as u16,
        flags: 0,
    });
    Ok(())
}

fn binding_start(bindings: &[CandidateEvidenceBindingRecord]) -> Result<u32, StoryProducerError> {
    checked_u32(bindings.len())
}

fn binding_count(
    bindings: &[CandidateEvidenceBindingRecord],
    start: u32,
) -> Result<u32, StoryProducerError> {
    checked_u32(bindings.len())?
        .checked_sub(start)
        .ok_or(StoryProducerError::RecordCountOverflow)
}

fn push_string(strings: &mut Vec<u8>, value: &str) -> Result<StringRef, StoryProducerError> {
    let offset = strings.len() as u64;
    let length = checked_u32(value.len())?;
    strings.extend_from_slice(value.as_bytes());
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}

fn checked_u32(value: usize) -> Result<u32, StoryProducerError> {
    u32::try_from(value).map_err(|_| StoryProducerError::RecordCountOverflow)
}

fn typed_copy<T: bytemuck::Pod + Copy>(
    input: &StoryProducerInput<'_>,
    kind: phoenix_graph_generation_v2::PageKind,
) -> Result<Vec<T>, StoryProducerError> {
    Ok(input.source.typed_page::<T>(kind)?.to_vec())
}

#[allow(clippy::too_many_arguments)]
fn candidate_authority_hash(
    relationships: &[TypedRelationshipCandidateRecord],
    events: &[EventRecord],
    episodes: &[EpisodeRecord],
    memberships: &[EpisodeMembershipRecord],
    temporal: &[TemporalCandidateRecord],
    causal: &[CausalCandidateRecord],
    memory: &[MemoryStateCandidateRecord],
    bindings: &[CandidateEvidenceBindingRecord],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-producer/v1/candidate-authority\0");
    hasher.update(&[AuthorityClass::SemanticCandidate as u8]);
    hasher.update(cast_slice(relationships));
    hasher.update(cast_slice(events));
    hasher.update(cast_slice(episodes));
    hasher.update(cast_slice(memberships));
    hasher.update(cast_slice(temporal));
    hasher.update(cast_slice(causal));
    hasher.update(cast_slice(memory));
    hasher.update(cast_slice(bindings));
    *hasher.finalize().as_bytes()
}
