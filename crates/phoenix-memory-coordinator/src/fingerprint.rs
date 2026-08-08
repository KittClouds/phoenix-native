use crate::{CommonProducts, CoordinatorState, DualFaceProducer};

pub(crate) fn state_hash<P: DualFaceProducer>(state: &CoordinatorState<P>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.dual-face-coordinator/state/v1\0");
    hasher.update(&state.namespace_hash);
    hasher.update(&state.config.registry_revision.to_le_bytes());

    let mut documents = state.documents.values().collect::<Vec<_>>();
    documents.sort_unstable_by_key(|stored| stored.request.lease.entry_id.0);
    for stored in documents {
        hasher.update(&stored.request.lease.entry_id.0.to_le_bytes());
        hasher.update(&stored.request.revision.0.to_le_bytes());
        hasher.update(&stored.request.hash.0);
        hash_products(&mut hasher, &stored.production.common);
        for chunk in &stored.production.structural.chunks {
            hasher.update(&chunk.start.to_le_bytes());
            hasher.update(&chunk.end.to_le_bytes());
            hasher.update(&chunk.content_hash.to_le_bytes());
        }
    }

    let mut conversations = state.conversations.values().collect::<Vec<_>>();
    conversations.sort_unstable_by(|left, right| left.external_id.cmp(&right.external_id));
    for conversation in conversations {
        hash_bytes(&mut hasher, &conversation.external_id);
        for stored in &conversation.turns {
            hasher.update(&stored.turn.ordinal.to_le_bytes());
            hasher.update(&(stored.turn.role as u16).to_le_bytes());
            hasher.update(&stored.turn.event_time_millis.to_le_bytes());
            hasher.update(blake3::hash(stored.turn.content.as_bytes()).as_bytes());
            hash_products(&mut hasher, &stored.production.common);
        }
    }

    for registration in state.config.registrations.iter() {
        hasher.update(&(registration.product as u16).to_le_bytes());
        hash_bytes(&mut hasher, registration.producer.as_bytes());
        hasher.update(&registration.producer_binary_hash);
        hasher.update(&registration.config_hash);
        hasher.update(
            &registration
                .model_identity_index
                .unwrap_or(crate::NO_MODEL_IDENTITY)
                .to_le_bytes(),
        );
        hasher.update(&(registration.support as u16).to_le_bytes());
    }
    for model in state.config.model_identities.iter() {
        hash_bytes(&mut hasher, model.name.as_bytes());
        hash_bytes(&mut hasher, model.runtime.as_bytes());
        hash_bytes(&mut hasher, model.artifact_uri.as_bytes());
        hasher.update(&model.artifact_hash);
        hasher.update(&model.config_hash);
        hasher.update(&(model.semantic_role as u16).to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn hash_products(hasher: &mut blake3::Hasher, products: &CommonProducts) {
    for entity in &products.entities {
        hasher.update(&entity.stable_id.to_le_bytes());
        hash_bytes(hasher, entity.label.as_bytes());
        if let Some(custom_kind) = &entity.custom_kind {
            hasher.update(&[1]);
            hash_bytes(hasher, custom_kind.as_bytes());
        } else {
            hasher.update(&[0]);
        }
        hasher.update(&entity.mention_count.to_le_bytes());
        hasher.update(&entity.kind.to_le_bytes());
        hasher.update(&entity.source_mask.to_le_bytes());
    }
    for mention in &products.mentions {
        hasher.update(&mention.stable_id.to_le_bytes());
        hasher.update(&mention.entity_id.to_le_bytes());
        hasher.update(&mention.evidence_id.to_le_bytes());
        hasher.update(&mention.start.to_le_bytes());
        hasher.update(&mention.end.to_le_bytes());
        hasher.update(&mention.confidence.to_bits().to_le_bytes());
        hasher.update(&mention.flags.to_le_bytes());
    }
    for binding in &products.canonical_bindings {
        hasher.update(&binding.source_entity_id.to_le_bytes());
        hasher.update(&binding.canonical_entity_id.to_le_bytes());
        hasher.update(&binding.decision_id.to_le_bytes());
        hasher.update(&binding.source_mask.to_le_bytes());
        hasher.update(&(binding.kind as u16).to_le_bytes());
    }
    for pack in &products.vocabulary_packs {
        hasher.update(&pack.id.to_le_bytes());
        hash_bytes(hasher, pack.name.as_bytes());
        hash_bytes(hasher, pack.version.as_bytes());
        hasher.update(&pack.schema_hash);
        hasher.update(&pack.producer_identity_hash);
        hasher.update(&(pack.kind as u16).to_le_bytes());
        hasher.update(&pack.flags.to_le_bytes());
    }
    for candidate in &products.candidates {
        hasher.update(&candidate.candidate_id);
        hasher.update(&(candidate.family as u16).to_le_bytes());
        hasher.update(&candidate.vocabulary_pack_id.to_le_bytes());
        hash_bytes(hasher, candidate.relation_kind.as_bytes());
        hash_bytes(hasher, candidate.value.as_bytes());
        hasher.update(&candidate.valid_time_from_millis.to_le_bytes());
        hasher.update(&candidate.valid_time_to_millis.to_le_bytes());
        hasher.update(&candidate.confidence.to_bits().to_le_bytes());
        hasher.update(
            &candidate
                .model_identity_index
                .unwrap_or(crate::NO_MODEL_IDENTITY)
                .to_le_bytes(),
        );
        hasher.update(&candidate.producer_identity_hash);
        hasher.update(&(candidate.status as u16).to_le_bytes());
        hasher.update(&candidate.flags.to_le_bytes());
        for endpoint in candidate.endpoints.iter() {
            hasher.update(&endpoint.endpoint_id.to_le_bytes());
            hasher.update(&(endpoint.role as u16).to_le_bytes());
            hasher.update(&endpoint.flags.to_le_bytes());
        }
        for evidence in candidate.evidence_ids.iter() {
            hasher.update(&evidence.to_le_bytes());
        }
    }
    for envelope in &products.temporal_envelopes {
        hasher.update(&envelope.id);
        hasher.update(&envelope.source_time_millis.to_le_bytes());
        hasher.update(&envelope.asserted_at_millis.to_le_bytes());
        hasher.update(&envelope.occurred_from_millis.to_le_bytes());
        hasher.update(&envelope.occurred_to_millis.to_le_bytes());
        hasher.update(&envelope.observed_at_millis.to_le_bytes());
        hasher.update(&envelope.valid_time_from_millis.to_le_bytes());
        hasher.update(&envelope.valid_time_to_millis.to_le_bytes());
        hash_bytes(hasher, envelope.original_text.as_bytes());
        hasher.update(&envelope.timezone_offset_minutes.to_le_bytes());
        hasher.update(&envelope.confidence.to_bits().to_le_bytes());
        hasher.update(&(envelope.precision as u16).to_le_bytes());
        hasher.update(&envelope.flags.to_le_bytes());
        for binding in envelope.bindings.iter() {
            hasher.update(&binding.subject_id);
            hasher.update(&binding.evidence_id.to_le_bytes());
            hasher.update(&(binding.subject_kind as u16).to_le_bytes());
            hasher.update(&(binding.role as u16).to_le_bytes());
            hasher.update(&binding.flags.to_le_bytes());
        }
    }
}

fn hash_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
