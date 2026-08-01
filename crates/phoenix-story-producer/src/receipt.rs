use crate::types::{ProducerRegistration, StoryProducerInput};
use crate::StoryProducerError;
use phoenix_graph_generation_v2::{
    AuthorityClass, CacheState, CapabilityRecord, CapabilityState, ModelIdentityRecord,
    ProducerProduct, PublicationReceiptRecord, PublicationStatus, StageReceiptRecord, StringRef,
    STAGE_FLAG_ALLOCATION_NOT_MEASURED, STAGE_FLAG_QUEUE_NOT_OBSERVED,
    STAGE_FLAG_TIMING_NOT_MEASURED,
};

const STORY_PRODUCTS: [ProducerProduct; 6] = [
    ProducerProduct::Relationships,
    ProducerProduct::Events,
    ProducerProduct::Episodes,
    ProducerProduct::Temporal,
    ProducerProduct::Causal,
    ProducerProduct::MemoryState,
];

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_story_receipts(
    input: &StoryProducerInput<'_>,
    strings: &mut Vec<u8>,
    capabilities: &mut Vec<CapabilityRecord>,
    model_identities: &mut Vec<ModelIdentityRecord>,
    stage_receipts: &mut Vec<StageReceiptRecord>,
    publication_receipts: &mut Vec<PublicationReceiptRecord>,
    counts: [usize; 6],
    contextual_count: usize,
    model_ranked_count: usize,
) -> Result<u16, StoryProducerError> {
    let producer_name = push_string(strings, "phoenix-story-producer/v1")?;
    let runtime = push_string(strings, "rust-native")?;
    let empty = push_string(strings, "")?;
    let producer_model_index = checked_u32(model_identities.len())?;
    model_identities.push(ModelIdentityRecord {
        name: producer_name,
        runtime,
        artifact_uri: empty,
        artifact_hash: input.producer_binary_hash,
        config_hash: input.source.header().cohort_hash,
        flags: 0,
        reserved: 0,
    });

    let registrations = registration_views(input, strings)?;
    let parent_span = stable_id(
        b"story-candidate-parent",
        &input.source.header().content_hash,
        input.published_generation,
    );
    let unmeasured = STAGE_FLAG_TIMING_NOT_MEASURED
        | STAGE_FLAG_ALLOCATION_NOT_MEASURED
        | STAGE_FLAG_QUEUE_NOT_OBSERVED;
    let parent_name = push_string(strings, "story-candidates")?;
    stage_receipts.push(StageReceiptRecord {
        name: parent_name,
        span_id: parent_span,
        parent_span_id: 0,
        elapsed_micros: 0,
        output_count: counts.iter().sum::<usize>() as u64,
        allocated_bytes: 0,
        copied_bytes: input
            .source
            .descriptor(phoenix_graph_generation_v2::PageKind::Strings)
            .length,
        queue_high_water: 0,
        cache_state: CacheState::Computed as u16,
        status: CapabilityState::Produced as u16,
        flags: unmeasured,
    });

    let mut unsupported_mask = 0_u16;
    for (index, ((product, count), registration)) in STORY_PRODUCTS
        .into_iter()
        .zip(counts)
        .zip(registrations)
        .enumerate()
    {
        let state = if registration.supported {
            CapabilityState::Produced
        } else {
            unsupported_mask |= 1 << index;
            CapabilityState::Unsupported
        };
        capabilities.push(CapabilityRecord {
            product: product as u16,
            authority: AuthorityClass::SemanticCandidate as u16,
            state: state as u16,
            flags_u16: 0,
            producer: registration.producer,
            output_count: count as u64,
            reused_generation: 0,
            model_identity_index: producer_model_index,
            flags: 0,
        });
        stage_receipts.push(StageReceiptRecord {
            name: registration.producer,
            span_id: stable_id(
                &(product as u16).to_le_bytes(),
                &input.source.header().content_hash,
                input.published_generation,
            ),
            parent_span_id: parent_span,
            elapsed_micros: 0,
            output_count: count as u64,
            allocated_bytes: 0,
            copied_bytes: 0,
            queue_high_water: 0,
            cache_state: CacheState::Computed as u16,
            status: state as u16,
            flags: unmeasured,
        });
    }

    let contextual_producer = push_string(strings, "phoenix-contextual-evidence/v1")?;
    capabilities.push(CapabilityRecord {
        product: ProducerProduct::ContextualEvidence as u16,
        authority: AuthorityClass::ContextualEvidenceOnly as u16,
        state: CapabilityState::Produced as u16,
        flags_u16: 0,
        producer: contextual_producer,
        output_count: contextual_count as u64,
        reused_generation: 0,
        model_identity_index: producer_model_index,
        flags: 0,
    });

    if let Some(batch) = input.model_ranking {
        let name = push_string(strings, batch.model.name)?;
        let runtime = push_string(strings, batch.model.runtime)?;
        let artifact_uri = push_string(strings, batch.model.artifact_uri)?;
        model_identities.push(ModelIdentityRecord {
            name,
            runtime,
            artifact_uri,
            artifact_hash: batch.model.artifact_hash,
            config_hash: batch.model.config_hash,
            flags: 0,
            reserved: 0,
        });
        let ranking_stage = push_string(strings, "story-candidate-model-ranking")?;
        stage_receipts.push(StageReceiptRecord {
            name: ranking_stage,
            span_id: stable_id(
                b"story-model-ranking",
                &input.source.header().content_hash,
                input.published_generation,
            ),
            parent_span_id: parent_span,
            elapsed_micros: 0,
            output_count: model_ranked_count as u64,
            allocated_bytes: 0,
            copied_bytes: 0,
            queue_high_water: 0,
            cache_state: CacheState::Computed as u16,
            status: CapabilityState::Produced as u16,
            flags: unmeasured,
        });
    }

    publication_receipts.push(PublicationReceiptRecord {
        authority_hash: [0; 32],
        previous_generation_hash: input.source.header().generation_hash,
        generation_id: input.published_generation,
        previous_generation_id: input.source.header().published_generation,
        document_revision: input.source.header().document_revision,
        registry_revision: input.source.header().registry_revision,
        published_at_unix_millis: 0,
        status: PublicationStatus::Published as u16,
        flags_u16: 0,
        flags: 0,
    });
    Ok(unsupported_mask)
}

#[derive(Clone, Copy)]
struct RegistrationView {
    producer: StringRef,
    supported: bool,
}

fn registration_views(
    input: &StoryProducerInput<'_>,
    strings: &mut Vec<u8>,
) -> Result<[RegistrationView; 6], StoryProducerError> {
    let registrations = [
        view(&input.registrations.relationships),
        view(&input.registrations.events),
        view(&input.registrations.episodes),
        view(&input.registrations.temporal),
        view(&input.registrations.causal),
        view(&input.registrations.memory_state),
    ];
    let mut output = [RegistrationView {
        producer: StringRef {
            offset: 0,
            length: 0,
            reserved: 0,
        },
        supported: false,
    }; 6];
    for (target, (producer, supported)) in output.iter_mut().zip(registrations) {
        *target = RegistrationView {
            producer: push_string(strings, producer)?,
            supported,
        };
    }
    Ok(output)
}

fn view<'a, T>(registration: &'a ProducerRegistration<'a, T>) -> (&'a str, bool) {
    (registration.producer_id(), registration.is_supported())
}

fn stable_id(domain: &[u8], content_hash: &[u8; 32], generation: u64) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-producer/v1/receipt\0");
    hasher.update(domain);
    hasher.update(content_hash);
    hasher.update(&generation.to_le_bytes());
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(bytes).max(1)
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
