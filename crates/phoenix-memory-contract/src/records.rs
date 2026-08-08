use bytemuck::{Pod, Zeroable};
use phoenix_graph_generation_v2::StringRef;

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SourceRecord {
    pub id: u64,
    pub namespace_id: u64,
    pub external_identity_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub kind: u16,
    pub flags: u16,
    pub reserved_u32: u32,
    pub reserved: [u64; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DocumentRevisionRecord {
    pub source_id: u64,
    pub document_id: u64,
    pub revision: u64,
    pub path: StringRef,
    pub content: StringRef,
    pub content_hash: [u8; 32],
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub system_generation_from: u64,
    pub system_generation_to: u64,
    pub chunk_start: u32,
    pub chunk_count: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ConversationRecord {
    pub source_id: u64,
    pub conversation_id: u64,
    pub started_at_millis: i64,
    pub ended_at_millis: i64,
    pub turn_start: u32,
    pub turn_count: u32,
    pub flags: u32,
    pub reserved: u32,
    pub content_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TurnRecord {
    pub id: u64,
    pub source_id: u64,
    pub conversation_id: u64,
    pub reply_to_turn_id: u64,
    pub actor_entity_id: u64,
    pub content: StringRef,
    pub content_hash: [u8; 32],
    pub event_time_millis: i64,
    pub ordinal: u32,
    pub model_identity_index: u32,
    pub role: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ContentUnitRecord {
    pub id: u64,
    pub source_id: u64,
    pub owner_id: u64,
    pub parent_id: u64,
    pub content_hash: [u8; 32],
    pub start: u32,
    pub end: u32,
    pub ordinal: u32,
    pub token_count: u32,
    pub kind: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct MentionRecordV3 {
    pub id: u64,
    pub source_id: u64,
    pub entity_id: u64,
    pub evidence_id: u64,
    pub content_unit_id: u64,
    pub start: u32,
    pub end: u32,
    pub confidence_bits: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EvidenceRecordV3 {
    pub id: u64,
    pub source_id: u64,
    pub entity_id: u64,
    pub mention_id: u64,
    pub content_unit_id: u64,
    pub start: u32,
    pub end: u32,
    pub role: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ValidityIntervalRecord {
    pub subject_id: [u8; 32],
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub system_generation_from: u64,
    pub system_generation_to: u64,
    pub subject_kind: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SupersessionRecord {
    pub subject_id: [u8; 32],
    pub replacement_id: [u8; 32],
    pub evidence_id: u64,
    pub decision_id: u64,
    pub system_generation: u64,
    pub status: u16,
    pub flags_u16: u16,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SemanticCandidateRecordV3 {
    pub candidate_id: [u8; 32],
    pub source_id: u64,
    pub vocabulary_pack_id: u64,
    pub relation_kind: StringRef,
    pub value: StringRef,
    pub endpoint_start: u32,
    pub endpoint_count: u32,
    pub evidence_start: u32,
    pub evidence_count: u32,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub system_generation_from: u64,
    pub system_generation_to: u64,
    pub confidence_bits: u32,
    pub model_identity_index: u32,
    pub producer_identity_hash: [u8; 32],
    pub family: u16,
    pub status: u16,
    pub flags: u32,
    pub reserved: [u32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CandidateEndpointBindingRecordV3 {
    pub candidate_id: [u8; 32],
    pub endpoint_id: u64,
    pub ordinal: u32,
    pub role: u16,
    pub flags: u16,
}

/// Candidate-only temporal normalization. Source timestamps remain authoritative
/// on their source records; this envelope preserves every distinct clock rather
/// than collapsing them into one ambiguous timestamp.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TemporalEnvelopeRecordV1 {
    pub id: [u8; 32],
    pub source_time_millis: i64,
    pub asserted_at_millis: i64,
    pub occurred_from_millis: i64,
    pub occurred_to_millis: i64,
    pub observed_at_millis: i64,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub system_generation_from: u64,
    pub system_generation_to: u64,
    pub original_text: StringRef,
    pub binding_start: u32,
    pub binding_count: u32,
    pub timezone_offset_minutes: i32,
    pub confidence_bits: u32,
    pub precision: u16,
    pub reserved_u16: u16,
    pub flags: u32,
    pub reserved: [u32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TemporalEnvelopeBindingRecordV1 {
    pub envelope_id: [u8; 32],
    pub subject_id: [u8; 32],
    pub evidence_id: u64,
    pub ordinal: u32,
    pub subject_kind: u16,
    pub role: u16,
    pub flags: u32,
    pub reserved: u32,
}

const _: [(); 152] = [(); core::mem::size_of::<TemporalEnvelopeRecordV1>()];
const _: [(); 88] = [(); core::mem::size_of::<TemporalEnvelopeBindingRecordV1>()];

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct VocabularyPackRecordV3 {
    pub id: u64,
    pub name: StringRef,
    pub version: StringRef,
    pub schema_hash: [u8; 32],
    pub producer_identity_hash: [u8; 32],
    pub kind: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ProducerCapabilityRecordV3 {
    pub producer: StringRef,
    pub producer_binary_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub output_count: u64,
    pub reused_generation: u64,
    pub model_identity_index: u32,
    pub product: u16,
    pub state: u16,
    pub flags_u16: u16,
    pub padding_u16: u16,
    pub flags: u32,
    pub reserved: [u32; 2],
}
