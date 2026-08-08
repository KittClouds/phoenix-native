use crate::open::{ensure_strictly_increasing, validate_index_range};
use crate::{
    temporal_u64_subject_id, EvidenceRecordV3, MemoryContractError, SemanticCandidateRecordV3,
    TemporalBindingRoleV1, TemporalEnvelopeBindingRecordV1, TemporalEnvelopeRecordV1,
    TemporalPrecisionV1, TemporalSubjectKindV1, TurnRecord, VerifiedGraphGenerationV3,
    TEMPORAL_FLAG_ASSERTED_TIME, TEMPORAL_FLAG_EXPLICIT_TEXT, TEMPORAL_FLAG_OBSERVED_TIME,
    TEMPORAL_FLAG_OCCURRENCE_TIME, TEMPORAL_FLAG_SOURCE_TIME, TIMEZONE_OFFSET_UNKNOWN,
    TIME_UNKNOWN,
};
use hashbrown::HashSet;
use phoenix_graph_generation_v2::{EpisodeRecord, EventRecord};

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_temporal_envelopes(
    generation: &VerifiedGraphGenerationV3,
    envelopes: &[TemporalEnvelopeRecordV1],
    bindings: &[TemporalEnvelopeBindingRecordV1],
    candidates: &[SemanticCandidateRecordV3],
    events: &[EventRecord],
    episodes: &[EpisodeRecord],
    turns: &[TurnRecord],
    evidence: &[EvidenceRecordV3],
) -> Result<(), MemoryContractError> {
    ensure_strictly_increasing(envelopes.iter().map(|record| record.id))?;
    let evidence_ids = evidence
        .iter()
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    let candidate_subjects = candidates
        .iter()
        .map(|record| record.candidate_id)
        .collect::<HashSet<_>>();
    let event_subjects = events
        .iter()
        .map(|record| temporal_u64_subject_id(TemporalSubjectKindV1::Event, record.id))
        .collect::<HashSet<_>>();
    let episode_subjects = episodes
        .iter()
        .map(|record| temporal_u64_subject_id(TemporalSubjectKindV1::Episode, record.id))
        .collect::<HashSet<_>>();
    let turn_subjects = turns
        .iter()
        .map(|record| temporal_u64_subject_id(TemporalSubjectKindV1::Turn, record.id))
        .collect::<HashSet<_>>();

    let mut bound_count = 0_usize;
    for envelope in envelopes {
        validate_index_range(
            envelope.binding_start,
            envelope.binding_count,
            bindings.len(),
            "temporal envelope binding range is invalid",
        )?;
        let range = &bindings[envelope.binding_start as usize
            ..(envelope.binding_start + envelope.binding_count) as usize];
        validate_envelope_metadata(generation, envelope)?;
        for (ordinal, binding) in range.iter().enumerate() {
            validate_binding(
                envelope,
                binding,
                ordinal,
                &candidate_subjects,
                &event_subjects,
                &episode_subjects,
                &turn_subjects,
                &evidence_ids,
            )?;
        }
        bound_count = bound_count.saturating_add(range.len());
    }
    if bound_count != bindings.len() {
        return Err(MemoryContractError::InvalidSourceModel(
            "temporal envelope bindings contain orphan rows",
        ));
    }
    Ok(())
}

fn validate_envelope_metadata(
    generation: &VerifiedGraphGenerationV3,
    envelope: &TemporalEnvelopeRecordV1,
) -> Result<(), MemoryContractError> {
    let confidence = f32::from_bits(envelope.confidence_bits);
    let source_present = envelope.source_time_millis != TIME_UNKNOWN;
    let asserted_present = envelope.asserted_at_millis != TIME_UNKNOWN;
    let occurred_from_present = envelope.occurred_from_millis != TIME_UNKNOWN;
    let occurred_to_present = envelope.occurred_to_millis != TIME_UNKNOWN;
    let observed_present = envelope.observed_at_millis != TIME_UNKNOWN;
    let explicit_text = envelope.flags & TEMPORAL_FLAG_EXPLICIT_TEXT != 0;
    if envelope.id == [0; 32]
        || envelope.binding_count == 0
        || TemporalPrecisionV1::from_raw(envelope.precision).is_none()
        || !confidence.is_finite()
        || !(0.0..=1.0).contains(&confidence)
        || envelope.valid_time_from_millis > envelope.valid_time_to_millis
        || envelope.system_generation_from > envelope.system_generation_to
        || source_present != (envelope.flags & TEMPORAL_FLAG_SOURCE_TIME != 0)
        || asserted_present != (envelope.flags & TEMPORAL_FLAG_ASSERTED_TIME != 0)
        || observed_present != (envelope.flags & TEMPORAL_FLAG_OBSERVED_TIME != 0)
        || !observed_present
        || occurred_from_present != occurred_to_present
        || occurred_from_present != (envelope.flags & TEMPORAL_FLAG_OCCURRENCE_TIME != 0)
        || (occurred_from_present && envelope.occurred_from_millis > envelope.occurred_to_millis)
        || (envelope.timezone_offset_minutes != TIMEZONE_OFFSET_UNKNOWN
            && !(-1439..=1439).contains(&envelope.timezone_offset_minutes))
        || explicit_text
            == generation
                .resolve_string(envelope.original_text)?
                .is_empty()
    {
        return Err(MemoryContractError::InvalidSourceModel(
            "temporal envelope clocks or metadata are invalid",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_binding(
    envelope: &TemporalEnvelopeRecordV1,
    binding: &TemporalEnvelopeBindingRecordV1,
    ordinal: usize,
    candidate_subjects: &HashSet<[u8; 32]>,
    event_subjects: &HashSet<[u8; 32]>,
    episode_subjects: &HashSet<[u8; 32]>,
    turn_subjects: &HashSet<[u8; 32]>,
    evidence_ids: &HashSet<u64>,
) -> Result<(), MemoryContractError> {
    let subject_kind = TemporalSubjectKindV1::from_raw(binding.subject_kind).ok_or(
        MemoryContractError::InvalidSourceModel("temporal envelope subject kind is invalid"),
    )?;
    let subject_exists = match subject_kind {
        TemporalSubjectKindV1::SemanticCandidate => {
            candidate_subjects.contains(&binding.subject_id)
        }
        TemporalSubjectKindV1::Event => event_subjects.contains(&binding.subject_id),
        TemporalSubjectKindV1::Episode => episode_subjects.contains(&binding.subject_id),
        TemporalSubjectKindV1::Turn => turn_subjects.contains(&binding.subject_id),
    };
    if binding.envelope_id != envelope.id
        || binding.ordinal as usize != ordinal
        || binding.subject_id == [0; 32]
        || !subject_exists
        || !evidence_ids.contains(&binding.evidence_id)
        || TemporalBindingRoleV1::from_raw(binding.role).is_none()
    {
        return Err(MemoryContractError::InvalidSourceModel(
            "temporal envelope binding is invalid",
        ));
    }
    Ok(())
}
