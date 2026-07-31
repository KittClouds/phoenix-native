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
        hasher.update(&conversation.external_id);
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
        hasher.update(registration.producer.as_bytes());
        hasher.update(&registration.producer_binary_hash);
        hasher.update(&registration.config_hash);
    }
    *hasher.finalize().as_bytes()
}

fn hash_products(hasher: &mut blake3::Hasher, products: &CommonProducts) {
    for entity in &products.entities {
        hasher.update(&entity.stable_id.to_le_bytes());
        hasher.update(entity.label.as_bytes());
        hasher.update(&entity.mention_count.to_le_bytes());
    }
    for mention in &products.mentions {
        hasher.update(&mention.stable_id.to_le_bytes());
        hasher.update(&mention.entity_id.to_le_bytes());
        hasher.update(&mention.start.to_le_bytes());
        hasher.update(&mention.end.to_le_bytes());
    }
    for pack in &products.vocabulary_packs {
        hasher.update(&pack.id.to_le_bytes());
        hasher.update(pack.name.as_bytes());
        hasher.update(pack.version.as_bytes());
        hasher.update(&pack.schema_hash);
        hasher.update(&pack.producer_identity_hash);
    }
    for candidate in &products.candidates {
        hasher.update(&candidate.candidate_id);
        hasher.update(&(candidate.family as u16).to_le_bytes());
        hasher.update(&candidate.vocabulary_pack_id.to_le_bytes());
        hasher.update(candidate.relation_kind.as_bytes());
        hasher.update(candidate.value.as_bytes());
        hasher.update(&candidate.producer_identity_hash);
        for endpoint in candidate.endpoints.iter() {
            hasher.update(&endpoint.endpoint_id.to_le_bytes());
            hasher.update(&(endpoint.role as u16).to_le_bytes());
        }
        for evidence in candidate.evidence_ids.iter() {
            hasher.update(&evidence.to_le_bytes());
        }
    }
}
