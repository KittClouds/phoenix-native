use crate::ids::{
    derive_causal_candidate_id, derive_episode_candidate_id, derive_episode_id,
    derive_event_candidate_id, derive_event_id, derive_memory_candidate_id,
    derive_relationship_candidate_id, derive_temporal_candidate_id,
};
use crate::types::{EpisodeMember, ProducerRegistration, SemanticEndpoint, StoryProducerInput};
use crate::StoryProducerError;
use hashbrown::{HashMap, HashSet};
use phoenix_graph_generation_v2::{
    CandidateId, ChunkId, EntityId, EpisodeId, EventId, EvidenceId, EvidenceRecord, PageKind,
};

pub(crate) const CANDIDATE_FLAG_MODEL_RANKED: u32 = 1 << 0;
pub(crate) const ENDPOINT_SOURCE_SHIFT: u32 = 8;
pub(crate) const ENDPOINT_TARGET_SHIFT: u32 = 12;

pub(crate) struct SourceIndex<'a> {
    pub evidence: HashMap<EvidenceId, &'a EvidenceRecord>,
    pub entities: HashSet<EntityId>,
    pub chunks: HashSet<ChunkId>,
    pub events: HashSet<EventId>,
    pub episodes: HashSet<EpisodeId>,
    pub binding_keys: HashSet<CandidateId>,
    pub candidate_keys: HashSet<CandidateId>,
    pub ranking: HashMap<CandidateId, f32>,
}

pub(crate) fn validate_input<'a>(
    input: &'a StoryProducerInput<'a>,
) -> Result<SourceIndex<'a>, StoryProducerError> {
    validate_authority(input)?;
    validate_empty_target_pages(input.source)?;
    validate_registrations(input)?;

    let evidence_records: &[EvidenceRecord] = input.source.typed_page(PageKind::Evidence)?;
    let entity_records: &[phoenix_graph_generation_v2::EntityRecord] =
        input.source.typed_page(PageKind::Entities)?;
    let chunk_records: &[phoenix_graph_generation_v2::ChunkRecord] =
        input.source.typed_page(PageKind::Chunks)?;
    let evidence = unique_map(
        evidence_records,
        |record| EvidenceId(record.id),
        StoryProducerError::InvalidEvidenceBinding,
    )?;
    let entities = unique_set(
        entity_records.iter().map(|record| EntityId(record.id)),
        StoryProducerError::UnknownReference,
    )?;
    let chunks = unique_set(
        chunk_records.iter().map(|record| ChunkId(record.id)),
        StoryProducerError::UnknownReference,
    )?;
    let source_bindings: &[phoenix_graph_generation_v2::CandidateEvidenceBindingRecord] = input
        .source
        .typed_page(PageKind::CandidateEvidenceBindings)?;
    let mut index = SourceIndex {
        evidence,
        entities,
        chunks,
        events: HashSet::new(),
        episodes: HashSet::new(),
        binding_keys: source_bindings
            .iter()
            .map(|binding| binding.candidate_id)
            .collect(),
        candidate_keys: HashSet::new(),
        ranking: HashMap::new(),
    };

    validate_events(input, &mut index)?;
    validate_episodes(input, &mut index)?;
    validate_relationships(input, &mut index)?;
    validate_temporal(input, &mut index)?;
    validate_causal(input, &mut index)?;
    validate_memory(input, &mut index)?;
    validate_model_ranking(input, &mut index)?;
    Ok(index)
}

fn validate_authority(input: &StoryProducerInput<'_>) -> Result<(), StoryProducerError> {
    let header = input.source.header();
    if input.published_generation <= header.published_generation
        || input.text.len() > u32::MAX as usize
        || blake3::hash(input.text.as_bytes()).as_bytes() != &header.content_hash
    {
        return Err(StoryProducerError::AuthorityMismatch);
    }
    let documents: &[phoenix_graph_generation_v2::DocumentRecord] =
        input.source.typed_page(PageKind::Documents)?;
    if documents.len() != 1 || documents[0].source_len as usize != input.text.len() {
        return Err(StoryProducerError::SourceBindingMismatch);
    }
    Ok(())
}

fn validate_empty_target_pages(
    source: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
) -> Result<(), StoryProducerError> {
    if [
        PageKind::TypedRelationshipCandidates,
        PageKind::Events,
        PageKind::Episodes,
        PageKind::EpisodeMemberships,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
    ]
    .into_iter()
    .any(|kind| source.descriptor(kind).count != 0)
    {
        return Err(StoryProducerError::StoryPagesAlreadyPopulated);
    }
    Ok(())
}

fn validate_registrations(input: &StoryProducerInput<'_>) -> Result<(), StoryProducerError> {
    let ids = [
        input.registrations.relationships.producer_id(),
        input.registrations.events.producer_id(),
        input.registrations.episodes.producer_id(),
        input.registrations.temporal.producer_id(),
        input.registrations.causal.producer_id(),
        input.registrations.memory_state.producer_id(),
    ];
    if ids.iter().any(|id| id.trim().is_empty()) {
        return Err(StoryProducerError::InvalidProducerRegistration);
    }
    Ok(())
}

fn validate_events(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } = input.registrations.events
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        validate_source_label(
            input.text,
            rule.label,
            rule.label_start,
            rule.label_end,
            rule.evidence_ids,
            &index.evidence,
        )?;
        validate_evidence_ids(rule.evidence_ids, &index.evidence)?;
        if rule.event_id != derive_event_id(&input.source.header().content_hash, producer_id, rule)
            || !index.events.insert(rule.event_id)
        {
            return Err(StoryProducerError::InvalidCandidateIdentity);
        }
        insert_primary_candidate(
            index,
            derive_event_candidate_id(&input.source.header().content_hash, rule.event_id),
        )?;
    }
    Ok(())
}

fn validate_episodes(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } = input.registrations.episodes
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        validate_source_label(
            input.text,
            rule.label,
            rule.label_start,
            rule.label_end,
            rule.evidence_ids,
            &index.evidence,
        )?;
        validate_evidence_ids(rule.evidence_ids, &index.evidence)?;
        if rule.memberships.is_empty()
            || rule.episode_id
                != derive_episode_id(&input.source.header().content_hash, producer_id, rule)
            || !index.episodes.insert(rule.episode_id)
        {
            return Err(StoryProducerError::InvalidEpisodeMembership);
        }
        let mut members = HashSet::with_capacity(rule.memberships.len());
        for membership in rule.memberships {
            validate_confidence(membership.confidence)?;
            validate_evidence_ids(membership.evidence_ids, &index.evidence)?;
            let exists = match membership.member {
                EpisodeMember::Chunk(id) => index.chunks.contains(&id),
                EpisodeMember::Event(id) => index.events.contains(&id),
            };
            if !exists || !members.insert(membership.member) {
                return Err(StoryProducerError::InvalidEpisodeMembership);
            }
        }
        insert_primary_candidate(
            index,
            derive_episode_candidate_id(&input.source.header().content_hash, rule.episode_id),
        )?;
        for membership in rule.memberships {
            let candidate = crate::derive_episode_membership_candidate_id(
                &input.source.header().content_hash,
                rule.episode_id,
                membership,
            );
            if candidate.is_zero() || !index.binding_keys.insert(candidate) {
                return Err(StoryProducerError::InvalidCandidateIdentity);
            }
        }
    }
    Ok(())
}

fn validate_relationships(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } =
        input.registrations.relationships
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        if rule.source_entity_id == rule.target_entity_id
            || !index.entities.contains(&rule.source_entity_id)
            || !index.entities.contains(&rule.target_entity_id)
        {
            return Err(StoryProducerError::UnknownReference);
        }
        let source = evidence(index, rule.source_evidence_id)?;
        let target = evidence(index, rule.target_evidence_id)?;
        if source.entity_id != rule.source_entity_id.0
            || target.entity_id != rule.target_entity_id.0
            || rule.source_evidence_id == rule.target_evidence_id
        {
            return Err(StoryProducerError::InvalidEvidenceBinding);
        }
        if !rule.additional_evidence_ids.is_empty() {
            validate_evidence_ids(rule.additional_evidence_ids, &index.evidence)?;
        }
        if rule
            .additional_evidence_ids
            .iter()
            .any(|id| *id == rule.source_evidence_id || *id == rule.target_evidence_id)
        {
            return Err(StoryProducerError::InvalidEvidenceBinding);
        }
        if rule.candidate_id
            != derive_relationship_candidate_id(
                &input.source.header().content_hash,
                producer_id,
                rule,
            )
        {
            return Err(StoryProducerError::InvalidCandidateIdentity);
        }
        insert_primary_candidate(index, rule.candidate_id)?;
    }
    Ok(())
}

fn validate_temporal(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } = input.registrations.temporal
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        validate_endpoint(rule.source, index)?;
        validate_endpoint(rule.target, index)?;
        validate_evidence_ids(rule.evidence_ids, &index.evidence)?;
        if rule.source == rule.target
            || rule.candidate_id
                != derive_temporal_candidate_id(
                    &input.source.header().content_hash,
                    producer_id,
                    rule,
                )
        {
            return Err(StoryProducerError::InvalidCandidateIdentity);
        }
        insert_primary_candidate(index, rule.candidate_id)?;
    }
    Ok(())
}

fn validate_causal(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } = input.registrations.causal
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        validate_endpoint(rule.cause, index)?;
        validate_endpoint(rule.effect, index)?;
        validate_evidence_ids(rule.evidence_ids, &index.evidence)?;
        if rule.cause == rule.effect
            || rule.candidate_id
                != derive_causal_candidate_id(
                    &input.source.header().content_hash,
                    producer_id,
                    rule,
                )
        {
            return Err(StoryProducerError::InvalidCandidateIdentity);
        }
        insert_primary_candidate(index, rule.candidate_id)?;
    }
    Ok(())
}

fn validate_memory(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let ProducerRegistration::Deterministic { producer_id, rules } =
        input.registrations.memory_state
    else {
        return Ok(());
    };
    for rule in rules {
        validate_confidence(rule.confidence)?;
        validate_endpoint(SemanticEndpoint::Entity(rule.subject_entity_id), index)?;
        validate_endpoint(rule.context, index)?;
        validate_evidence_ids(rule.evidence_ids, &index.evidence)?;
        if rule.key.trim().is_empty()
            || rule.value.trim().is_empty()
            || !rule.evidence_ids.iter().any(|id| {
                evidence(index, *id).is_ok_and(|row| row.entity_id == rule.subject_entity_id.0)
            })
            || rule.candidate_id
                != derive_memory_candidate_id(
                    &input.source.header().content_hash,
                    producer_id,
                    rule,
                )
        {
            return Err(StoryProducerError::InvalidEvidenceBinding);
        }
        insert_primary_candidate(index, rule.candidate_id)?;
    }
    Ok(())
}

fn validate_model_ranking(
    input: &StoryProducerInput<'_>,
    index: &mut SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let Some(batch) = input.model_ranking else {
        return Ok(());
    };
    if batch.model.name.trim().is_empty() || batch.model.runtime.trim().is_empty() {
        return Err(StoryProducerError::InvalidModelRanking);
    }
    for score in batch.scores {
        validate_confidence(score.confidence)
            .map_err(|_| StoryProducerError::InvalidModelRanking)?;
        if !index.candidate_keys.contains(&score.candidate_id)
            || index
                .ranking
                .insert(score.candidate_id, score.confidence)
                .is_some()
        {
            return Err(StoryProducerError::InvalidModelRanking);
        }
    }
    Ok(())
}

fn validate_source_label(
    text: &str,
    label: &str,
    start: u32,
    end: u32,
    evidence_ids: &[EvidenceId],
    evidence: &HashMap<EvidenceId, &EvidenceRecord>,
) -> Result<(), StoryProducerError> {
    let range = start as usize..end as usize;
    let exact = text.get(range).is_some_and(|source| source == label);
    let overlaps = evidence_ids.iter().any(|id| {
        evidence
            .get(id)
            .is_some_and(|row| row.start < end && row.end > start)
    });
    if label.trim().is_empty() || !exact || !overlaps {
        return Err(StoryProducerError::InvalidSourceLabel);
    }
    Ok(())
}

fn validate_evidence_ids(
    ids: &[EvidenceId],
    evidence: &HashMap<EvidenceId, &EvidenceRecord>,
) -> Result<(), StoryProducerError> {
    if ids.is_empty()
        || ids.windows(2).any(|pair| pair[0] >= pair[1])
        || ids.iter().any(|id| !evidence.contains_key(id))
    {
        return Err(StoryProducerError::InvalidEvidenceBinding);
    }
    Ok(())
}

fn validate_endpoint(
    endpoint: SemanticEndpoint,
    index: &SourceIndex<'_>,
) -> Result<(), StoryProducerError> {
    let exists = match endpoint {
        SemanticEndpoint::Entity(id) => index.entities.contains(&id),
        SemanticEndpoint::Event(id) => index.events.contains(&id),
        SemanticEndpoint::Episode(id) => index.episodes.contains(&id),
        SemanticEndpoint::Chunk(id) => index.chunks.contains(&id),
    };
    exists
        .then_some(())
        .ok_or(StoryProducerError::UnknownReference)
}

fn evidence<'a>(
    index: &'a SourceIndex<'a>,
    id: EvidenceId,
) -> Result<&'a EvidenceRecord, StoryProducerError> {
    index
        .evidence
        .get(&id)
        .copied()
        .ok_or(StoryProducerError::UnknownReference)
}

fn insert_primary_candidate(
    index: &mut SourceIndex<'_>,
    candidate: CandidateId,
) -> Result<(), StoryProducerError> {
    if candidate.is_zero()
        || !index.binding_keys.insert(candidate)
        || !index.candidate_keys.insert(candidate)
    {
        return Err(StoryProducerError::InvalidCandidateIdentity);
    }
    Ok(())
}

fn validate_confidence(confidence: f32) -> Result<(), StoryProducerError> {
    if confidence.is_finite() && (0.0..=1.0).contains(&confidence) {
        Ok(())
    } else {
        Err(StoryProducerError::InvalidEvidenceBinding)
    }
}

fn unique_set<T: Eq + std::hash::Hash>(
    values: impl IntoIterator<Item = T>,
    error: StoryProducerError,
) -> Result<HashSet<T>, StoryProducerError> {
    let mut set = HashSet::new();
    for value in values {
        if !set.insert(value) {
            return Err(error);
        }
    }
    Ok(set)
}

fn unique_map<T, K: Eq + std::hash::Hash>(
    values: &[T],
    key: impl Fn(&T) -> K,
    error: StoryProducerError,
) -> Result<HashMap<K, &T>, StoryProducerError> {
    let mut map = HashMap::with_capacity(values.len());
    for value in values {
        if map.insert(key(value), value).is_some() {
            return Err(error);
        }
    }
    Ok(map)
}
