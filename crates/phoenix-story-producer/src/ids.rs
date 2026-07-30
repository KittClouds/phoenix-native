use crate::{
    CausalCandidateInput, EpisodeCandidateInput, EpisodeMembershipInput, EventCandidateInput,
    MemoryStateCandidateInput, RelationshipCandidateInput, SemanticEndpoint,
    TemporalCandidateInput,
};
use phoenix_graph_generation_v2::{CandidateId, EpisodeId, EventId, EvidenceId};

pub fn derive_relationship_candidate_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &RelationshipCandidateInput<'_>,
) -> CandidateId {
    let mut hasher = candidate_hasher(b"relationship", content_hash, producer_id);
    update_u64(&mut hasher, input.source_entity_id.0);
    update_u64(&mut hasher, input.target_entity_id.0);
    update_u16(&mut hasher, input.relation as u16);
    update_evidence(&mut hasher, input.source_evidence_id);
    update_evidence(&mut hasher, input.target_evidence_id);
    for evidence in input.additional_evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_event_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &EventCandidateInput<'_>,
) -> EventId {
    let mut hasher = candidate_hasher(b"event-id", content_hash, producer_id);
    update_u32(&mut hasher, input.label_start);
    update_u32(&mut hasher, input.label_end);
    update_u16(&mut hasher, input.kind as u16);
    update_bytes(&mut hasher, input.label.as_bytes());
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    EventId(nonzero_u64(hasher.finalize().as_bytes()))
}

pub fn derive_event_candidate_id(content_hash: &[u8; 32], event_id: EventId) -> CandidateId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-candidate/v1/event\0");
    hasher.update(content_hash);
    update_u64(&mut hasher, event_id.0);
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_episode_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &EpisodeCandidateInput<'_>,
) -> EpisodeId {
    let mut hasher = candidate_hasher(b"episode-id", content_hash, producer_id);
    update_u32(&mut hasher, input.label_start);
    update_u32(&mut hasher, input.label_end);
    update_u32(&mut hasher, input.ordinal);
    update_u16(&mut hasher, input.family as u16);
    update_bytes(&mut hasher, input.label.as_bytes());
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    for membership in input.memberships {
        update_u64(&mut hasher, membership.member.raw());
    }
    EpisodeId(nonzero_u64(hasher.finalize().as_bytes()))
}

pub fn derive_episode_candidate_id(content_hash: &[u8; 32], episode_id: EpisodeId) -> CandidateId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-candidate/v1/episode\0");
    hasher.update(content_hash);
    update_u64(&mut hasher, episode_id.0);
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_episode_membership_candidate_id(
    content_hash: &[u8; 32],
    episode_id: EpisodeId,
    input: &EpisodeMembershipInput<'_>,
) -> CandidateId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-candidate/v1/episode-membership\0");
    hasher.update(content_hash);
    update_u64(&mut hasher, episode_id.0);
    update_u64(&mut hasher, input.member.raw());
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_temporal_candidate_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &TemporalCandidateInput<'_>,
) -> CandidateId {
    let mut hasher = candidate_hasher(b"temporal", content_hash, producer_id);
    update_endpoint(&mut hasher, input.source);
    update_endpoint(&mut hasher, input.target);
    update_u16(&mut hasher, input.relation as u16);
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_causal_candidate_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &CausalCandidateInput<'_>,
) -> CandidateId {
    let mut hasher = candidate_hasher(b"causal", content_hash, producer_id);
    update_endpoint(&mut hasher, input.cause);
    update_endpoint(&mut hasher, input.effect);
    update_u16(&mut hasher, input.relation as u16);
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    CandidateId(*hasher.finalize().as_bytes())
}

pub fn derive_memory_candidate_id(
    content_hash: &[u8; 32],
    producer_id: &str,
    input: &MemoryStateCandidateInput<'_>,
) -> CandidateId {
    let mut hasher = candidate_hasher(b"memory-state", content_hash, producer_id);
    update_u64(&mut hasher, input.subject_entity_id.0);
    update_endpoint(&mut hasher, input.context);
    update_u16(&mut hasher, input.kind as u16);
    update_bytes(&mut hasher, input.key.as_bytes());
    update_bytes(&mut hasher, input.value.as_bytes());
    for evidence in input.evidence_ids {
        update_evidence(&mut hasher, *evidence);
    }
    CandidateId(*hasher.finalize().as_bytes())
}

fn candidate_hasher(domain: &[u8], content_hash: &[u8; 32], producer_id: &str) -> blake3::Hasher {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.story-candidate/v1\0");
    update_bytes(&mut hasher, domain);
    hasher.update(content_hash);
    update_bytes(&mut hasher, producer_id.as_bytes());
    hasher
}

fn update_endpoint(hasher: &mut blake3::Hasher, endpoint: SemanticEndpoint) {
    update_u32(hasher, endpoint.tag());
    update_u64(hasher, endpoint.raw());
}

fn update_evidence(hasher: &mut blake3::Hasher, evidence: EvidenceId) {
    update_u64(hasher, evidence.0);
}

fn update_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    update_u64(hasher, bytes.len() as u64);
    hasher.update(bytes);
}

fn update_u16(hasher: &mut blake3::Hasher, value: u16) {
    hasher.update(&value.to_le_bytes());
}

fn update_u32(hasher: &mut blake3::Hasher, value: u32) {
    hasher.update(&value.to_le_bytes());
}

fn update_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn nonzero_u64(hash: &[u8; 32]) -> u64 {
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(bytes).max(1)
}
