use phoenix_analysis_contract::PhoenixNerArtifactV1;
use phoenix_graph_generation_v2::{CandidateId, EntityId, EvidenceId, MentionId};
use phoenix_scene_contract::EntityKind;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub struct UserTaggedEntityInput<'a> {
    pub stable_id: EntityId,
    pub label: &'a str,
    pub kind: EntityKind,
    pub custom_kind: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct UserTaggedMentionInput<'a> {
    pub source_entity_id: EntityId,
    pub start: u32,
    pub end: u32,
    pub surface: &'a str,
}

/// A coordinator-authored identity decision. No producer is allowed to infer
/// this mapping from labels or surface equality.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityMergeDecision {
    pub decision_id: u64,
    pub source_user_entity_id: EntityId,
    pub canonical_entity_id: EntityId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum IdentityCandidateKind {
    SameSurface = 1,
    Alias = 2,
    Coreference = 3,
}

#[derive(Clone, Copy, Debug)]
pub struct IdentityCandidateInput {
    pub candidate_id: CandidateId,
    pub left_entity_id: EntityId,
    pub right_entity_id: EntityId,
    pub left_mention_id: MentionId,
    pub right_mention_id: MentionId,
    pub kind: IdentityCandidateKind,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct EntityProducerInput<'a> {
    pub text: &'a str,
    pub structural: &'a phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
    pub ner: &'a PhoenixNerArtifactV1,
    pub user_entities: &'a [UserTaggedEntityInput<'a>],
    pub user_mentions: &'a [UserTaggedMentionInput<'a>],
    pub merge_decisions: &'a [IdentityMergeDecision],
    pub identity_candidates: &'a [IdentityCandidateInput],
    pub published_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityPublicationReceipt {
    pub path: PathBuf,
    pub previous_generation_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub entity_count: u32,
    pub mention_count: u32,
    pub evidence_count: u32,
    pub identity_candidate_count: u32,
    pub paint_span_count: u32,
    pub graph_evidence_hash: [u8; 32],
    pub paint_projection_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MentionAuthority {
    pub mention_id: MentionId,
    pub evidence_id: EvidenceId,
    pub entity_id: EntityId,
    pub chunk_id: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_index: u32,
    pub confidence_bits: u32,
    pub flags: u32,
    pub source_mask: u16,
    pub kind: u16,
}
