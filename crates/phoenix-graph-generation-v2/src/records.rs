use crate::CandidateId;
use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct StringRef {
    pub offset: u64,
    pub length: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DocumentRecord {
    pub id: u64,
    pub source_id: StringRef,
    pub source_len: u32,
    pub chapter_count: u32,
    pub paragraph_count: u32,
    pub sentence_count: u32,
    pub chunk_count: u32,
    pub span_count: u32,
    pub entity_count: u32,
    pub mention_count: u32,
    pub evidence_count: u32,
    pub structural_edge_count: u32,
    pub flags: u32,
    pub reserved: [u32; 3],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ChapterRecord {
    pub id: u64,
    pub document_id: u64,
    pub title: StringRef,
    pub start: u32,
    pub end: u32,
    pub paragraph_start: u32,
    pub paragraph_end: u32,
    pub ordinal: u32,
    pub flags: u32,
    pub reserved: [u32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ParagraphRecord {
    pub id: u64,
    pub document_id: u64,
    pub chapter_id: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_start: u32,
    pub sentence_end: u32,
    pub ordinal: u32,
    pub flags: u32,
    pub reserved: [u32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SentenceRecord {
    pub id: u64,
    pub document_id: u64,
    pub paragraph_id: u64,
    pub content_hash: u64,
    pub start: u32,
    pub end: u32,
    pub ordinal: u32,
    pub token_count: u32,
    pub quality: u16,
    pub dialogue_hint: u16,
    pub flags: u16,
    pub reserved_u16: u16,
    pub reserved: u64,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ChunkRecord {
    pub id: u64,
    pub document_id: u64,
    pub content_hash: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_start: u32,
    pub sentence_end: u32,
    pub paragraph_start: u32,
    pub paragraph_end: u32,
    pub chapter_index: u32,
    pub token_count: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SpanRecord {
    pub id: u64,
    pub document_id: u64,
    pub parent_id: u64,
    pub content_hash: u64,
    pub label: StringRef,
    pub start: u32,
    pub end: u32,
    pub child_start: u32,
    pub child_end: u32,
    pub token_count: u32,
    pub flags: u32,
    pub kind: u16,
    pub dialogue_hint: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EntityRecord {
    pub id: u64,
    pub label: StringRef,
    pub custom_kind: StringRef,
    pub mention_count: u32,
    pub kind: u16,
    pub source_mask: u16,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CanonicalEntityBindingRecord {
    pub source_entity_id: u64,
    pub canonical_entity_id: u64,
    pub decision_id: u64,
    pub source_mask: u16,
    pub kind: u16,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct MentionRecord {
    pub id: u64,
    pub entity_id: u64,
    pub evidence_id: u64,
    pub chunk_id: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_index: u32,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EvidenceRecord {
    pub id: u64,
    pub entity_id: u64,
    pub mention_id: u64,
    pub chunk_id: u64,
    pub start: u32,
    pub end: u32,
    pub role: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CandidateEvidenceBindingRecord {
    pub candidate_id: CandidateId,
    pub evidence_id: u64,
    pub ordinal: u32,
    pub role: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct StructuralEdgeRecord {
    pub id: u64,
    pub source_id: u64,
    pub target_id: u64,
    pub evidence_id: u64,
    pub weight_bits: u32,
    pub relation: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TypedRelationshipCandidateRecord {
    pub candidate_id: CandidateId,
    pub source_entity_id: u64,
    pub target_entity_id: u64,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub premise_start: u32,
    pub premise_end: u32,
    pub relation: u16,
    pub family: u16,
    pub status: u16,
    pub flags_u16: u16,
    pub confidence_bits: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct IdentityCandidateRecord {
    pub candidate_id: CandidateId,
    pub left_entity_id: u64,
    pub right_entity_id: u64,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub confidence_bits: u32,
    pub flags: u32,
    pub kind: u16,
    pub status: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EventRecord {
    pub id: u64,
    pub label: StringRef,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub start: u32,
    pub end: u32,
    pub kind: u16,
    pub status: u16,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EpisodeRecord {
    pub id: u64,
    pub label: StringRef,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub membership_start: u32,
    pub membership_count: u32,
    pub ordinal: u32,
    pub status: u16,
    pub family: u16,
    pub confidence_bits: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EpisodeMembershipRecord {
    pub episode_id: u64,
    pub member_id: u64,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub member_kind: u16,
    pub status: u16,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TemporalCandidateRecord {
    pub candidate_id: CandidateId,
    pub source_id: u64,
    pub target_id: u64,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub relation: u16,
    pub status: u16,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CausalCandidateRecord {
    pub candidate_id: CandidateId,
    pub cause_id: u64,
    pub effect_id: u64,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub relation: u16,
    pub status: u16,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct MemoryStateCandidateRecord {
    pub candidate_id: CandidateId,
    pub subject_id: u64,
    pub context_id: u64,
    pub key: StringRef,
    pub value: StringRef,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub status: u16,
    pub kind: u16,
    pub confidence_bits: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ContextualEvidenceRecord {
    pub source_entity_id: u64,
    pub target_entity_id: u64,
    pub source_mention_id: u64,
    pub target_mention_id: u64,
    pub chunk_id: u64,
    pub weight_bits: u32,
    pub byte_distance: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct NliAdjudicationRecord {
    pub candidate_id: CandidateId,
    pub contradiction_bits: u32,
    pub entailment_bits: u32,
    pub neutral_bits: u32,
    pub label: u16,
    pub status: u16,
    pub model_index: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DecisionRecord {
    pub id: u64,
    pub candidate_id: CandidateId,
    pub reason: StringRef,
    pub evidence_hash: [u8; 32],
    pub decided_at_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub action: u16,
    pub status: u16,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CapabilityRecord {
    pub product: u16,
    pub authority: u16,
    pub state: u16,
    pub flags_u16: u16,
    pub producer: StringRef,
    pub output_count: u64,
    pub reused_generation: u64,
    pub model_identity_index: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ModelIdentityRecord {
    pub name: StringRef,
    pub runtime: StringRef,
    pub artifact_uri: StringRef,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct StageReceiptRecord {
    pub name: StringRef,
    pub span_id: u64,
    pub parent_span_id: u64,
    pub elapsed_micros: u64,
    pub output_count: u64,
    pub allocated_bytes: u64,
    pub copied_bytes: u64,
    pub queue_high_water: u64,
    pub cache_state: u16,
    pub status: u16,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct PublicationReceiptRecord {
    /// Hash of the authority inputs and accepted page set. The enclosing
    /// generation hash cannot be embedded here without a circular hash.
    pub authority_hash: [u8; 32],
    pub previous_generation_hash: [u8; 32],
    pub generation_id: u64,
    pub previous_generation_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub published_at_unix_millis: u64,
    pub status: u16,
    pub flags_u16: u16,
    pub flags: u32,
}
