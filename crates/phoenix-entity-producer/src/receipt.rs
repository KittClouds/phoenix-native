use crate::build::stable_id;
use crate::types::EntityProducerInput;
use crate::EntityProducerError;
use phoenix_graph_generation_v2::{
    AuthorityClass, CacheState, CapabilityRecord, CapabilityState, EvidenceRecord,
    ModelIdentityRecord, ProducerProduct, PublicationReceiptRecord, PublicationStatus,
    StageReceiptRecord, StringRef, STAGE_FLAG_ALLOCATION_NOT_MEASURED,
    STAGE_FLAG_QUEUE_NOT_OBSERVED, STAGE_FLAG_TIMING_NOT_MEASURED,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_receipts(
    input: EntityProducerInput<'_>,
    strings: &mut Vec<u8>,
    capabilities: &mut Vec<CapabilityRecord>,
    model_identities: &mut Vec<ModelIdentityRecord>,
    stage_receipts: &mut Vec<StageReceiptRecord>,
    publication_receipts: &mut Vec<PublicationReceiptRecord>,
    entity_count: usize,
    mention_count: usize,
    identity_count: usize,
    evidence: &[EvidenceRecord],
) -> Result<(), EntityProducerError> {
    let producer = push_string(strings, "phoenix-entity-producer/v1")?;
    let runtime = push_string(strings, "rust-native")?;
    let empty = push_string(strings, "")?;
    let ner_name = push_string(strings, &input.ner.binding.dynamic_ner.model_id)?;
    let ner_runtime = push_string(strings, &input.ner.binding.dynamic_ner.runtime_id)?;
    let entity_stage = push_string(strings, "entities-and-evidence")?;
    let identity_stage = push_string(strings, "identity-candidates")?;
    let ner_model_index = u32::try_from(model_identities.len())
        .map_err(|_| EntityProducerError::RecordCountOverflow)?;

    model_identities.push(ModelIdentityRecord {
        name: ner_name,
        runtime: ner_runtime,
        artifact_uri: empty,
        artifact_hash: input.ner.binding.dynamic_ner.artifact_hash,
        config_hash: input.ner.binding.dynamic_ner.config_hash,
        flags: 0,
        reserved: 0,
    });
    model_identities.push(ModelIdentityRecord {
        name: producer,
        runtime,
        artifact_uri: empty,
        artifact_hash: input.ner.binding.producer_binary_hash,
        config_hash: input.structural.header().cohort_hash,
        flags: 0,
        reserved: 0,
    });
    capabilities.push(CapabilityRecord {
        product: ProducerProduct::CanonicalEntities as u16,
        authority: AuthorityClass::SourceAuthoritative as u16,
        state: CapabilityState::Produced as u16,
        flags_u16: 0,
        producer,
        output_count: entity_count as u64,
        reused_generation: 0,
        model_identity_index: ner_model_index,
        flags: 0,
    });
    capabilities.push(CapabilityRecord {
        product: ProducerProduct::Identity as u16,
        authority: AuthorityClass::SemanticCandidate as u16,
        state: CapabilityState::Produced as u16,
        flags_u16: 0,
        producer,
        output_count: identity_count as u64,
        reused_generation: 0,
        model_identity_index: ner_model_index,
        flags: 0,
    });

    let parent_span = stable_id(
        b"entity-stage-parent",
        &input.structural.header().content_hash,
        &[input.published_generation],
    );
    let unmeasured_flags = STAGE_FLAG_TIMING_NOT_MEASURED
        | STAGE_FLAG_ALLOCATION_NOT_MEASURED
        | STAGE_FLAG_QUEUE_NOT_OBSERVED;
    stage_receipts.push(StageReceiptRecord {
        name: entity_stage,
        span_id: parent_span,
        parent_span_id: 0,
        elapsed_micros: 0,
        output_count: (entity_count + mention_count + evidence.len()) as u64,
        allocated_bytes: 0,
        copied_bytes: input
            .structural
            .descriptor(phoenix_graph_generation_v2::PageKind::Strings)
            .length,
        queue_high_water: 0,
        cache_state: CacheState::Computed as u16,
        status: CapabilityState::Produced as u16,
        flags: unmeasured_flags,
    });
    stage_receipts.push(StageReceiptRecord {
        name: identity_stage,
        span_id: stable_id(
            b"identity-stage",
            &input.structural.header().content_hash,
            &[input.published_generation],
        ),
        parent_span_id: parent_span,
        elapsed_micros: 0,
        output_count: identity_count as u64,
        allocated_bytes: 0,
        copied_bytes: 0,
        queue_high_water: 0,
        cache_state: CacheState::Computed as u16,
        status: CapabilityState::Produced as u16,
        flags: unmeasured_flags,
    });
    publication_receipts.push(PublicationReceiptRecord {
        authority_hash: [0; 32],
        previous_generation_hash: input.structural.header().generation_hash,
        generation_id: input.published_generation,
        previous_generation_id: input.structural.header().published_generation,
        document_revision: input.structural.header().document_revision,
        registry_revision: input.structural.header().registry_revision,
        published_at_unix_millis: 0,
        status: PublicationStatus::Published as u16,
        flags_u16: 0,
        flags: 0,
    });
    Ok(())
}

fn push_string(strings: &mut Vec<u8>, value: &str) -> Result<StringRef, EntityProducerError> {
    let offset = strings.len() as u64;
    let length =
        u32::try_from(value.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
    strings.extend_from_slice(value.as_bytes());
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}
