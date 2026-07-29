use crate::{
    AcceptedEdgeId, CandidateEdgeId, ChunkId, DecisionId, DocumentId, EntityId, EvidenceId,
    MentionId, SentenceId, SpanId,
};
use bytemuck::{Pod, Zeroable};

pub const GRAPH_GENERATION_MAGIC: [u8; 8] = *b"PHXGG001";
pub const GRAPH_GENERATION_VERSION: u32 = 1;
pub const GRAPH_GENERATION_EXTENSION: &str = "phxgg";
pub const MAX_GENERATION_BYTES: u64 = 1 << 30;
pub const MAX_SECTION_COUNT: usize = 32;
pub const MAX_RECORDS_PER_SECTION: u64 = 16_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SectionKind {
    Document = 1,
    Chunks = 2,
    Sentences = 3,
    Spans = 4,
    Strings = 5,
    Entities = 6,
    Mentions = 7,
    Evidence = 8,
    AcceptedEdges = 9,
    CandidateEdges = 10,
    Adjudications = 11,
    Decisions = 12,
    Capabilities = 13,
    Identities = 14,
    StageReceipts = 15,
}

impl SectionKind {
    pub const ALL: [Self; 15] = [
        Self::Document,
        Self::Chunks,
        Self::Sentences,
        Self::Spans,
        Self::Strings,
        Self::Entities,
        Self::Mentions,
        Self::Evidence,
        Self::AcceptedEdges,
        Self::CandidateEdges,
        Self::Adjudications,
        Self::Decisions,
        Self::Capabilities,
        Self::Identities,
        Self::StageReceipts,
    ];

    pub fn from_raw(value: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as u16 == value)
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct GenerationHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub section_count: u32,
    pub flags: u32,
    pub total_len: u64,
    pub source_document_id_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub analysis_generation: u64,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SectionDescriptor {
    pub kind: u16,
    pub flags: u16,
    pub record_size: u32,
    pub offset: u64,
    pub length: u64,
    pub count: u64,
    pub hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct StringRef {
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DocumentRecord {
    pub id: u64,
    pub source_id: StringRef,
    pub source_len: u32,
    pub chunk_count: u32,
    pub sentence_count: u32,
    pub span_count: u32,
    pub entity_count: u32,
    pub mention_count: u32,
    pub accepted_edge_count: u32,
    pub candidate_edge_count: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ChunkRecord {
    pub id: u64,
    pub content_hash: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_start: u32,
    pub sentence_end: u32,
    pub paragraph_start: u32,
    pub paragraph_end: u32,
    pub chapter_index: u32,
    pub token_count: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SentenceRecord {
    pub id: u64,
    pub content_hash: u64,
    pub start: u32,
    pub end: u32,
    pub paragraph_index: u32,
    pub chapter_index: u32,
    pub token_count: u32,
    pub quality: u16,
    pub dialogue_hint: u16,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SpanRecord {
    pub id: u64,
    pub content_hash: u64,
    pub label: StringRef,
    pub start: u32,
    pub end: u32,
    pub parent_index: u32,
    pub child_start: u32,
    pub child_end: u32,
    pub token_count: u32,
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
    pub chunk_id: u64,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct AcceptedEdgeRecord {
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
pub struct CandidateEdgeRecord {
    pub candidate_id: [u8; 32],
    pub source_id: u64,
    pub target_id: u64,
    pub premise_start: u32,
    pub premise_end: u32,
    pub relation: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct AdjudicationRecord {
    pub candidate_id: [u8; 32],
    pub contradiction_bits: u32,
    pub entailment_bits: u32,
    pub neutral_bits: u32,
    pub label: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DecisionRecord {
    pub id: u64,
    pub candidate_id: [u8; 32],
    pub reason: StringRef,
    pub decided_at_revision: u64,
    pub status: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CapabilityRecord {
    pub name: StringRef,
    pub producer: StringRef,
    pub supported: u16,
    pub emitted: u16,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct IdentityRecord {
    pub name: StringRef,
    pub runtime: StringRef,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct StageReceiptRecord {
    pub name: StringRef,
    pub span_id: u64,
    pub parent_span_id: u64,
    pub elapsed_micros: u64,
    pub output_count: u64,
    pub flags: u32,
    pub reserved: u32,
}

impl DocumentRecord {
    pub const fn typed_id(&self) -> DocumentId {
        DocumentId(self.id)
    }
}

impl ChunkRecord {
    pub const fn typed_id(&self) -> ChunkId {
        ChunkId(self.id)
    }
}

impl SentenceRecord {
    pub const fn typed_id(&self) -> SentenceId {
        SentenceId(self.id)
    }
}

impl SpanRecord {
    pub const fn typed_id(&self) -> SpanId {
        SpanId(self.id)
    }
}

impl EntityRecord {
    pub const fn typed_id(&self) -> EntityId {
        EntityId(self.id)
    }
}

impl MentionRecord {
    pub const fn typed_id(&self) -> MentionId {
        MentionId(self.id)
    }

    pub const fn typed_entity_id(&self) -> EntityId {
        EntityId(self.entity_id)
    }

    pub const fn typed_evidence_id(&self) -> EvidenceId {
        EvidenceId(self.evidence_id)
    }

    pub const fn typed_chunk_id(&self) -> ChunkId {
        ChunkId(self.chunk_id)
    }
}

impl EvidenceRecord {
    pub const fn typed_id(&self) -> EvidenceId {
        EvidenceId(self.id)
    }
}

impl AcceptedEdgeRecord {
    pub const fn typed_id(&self) -> AcceptedEdgeId {
        AcceptedEdgeId(self.id)
    }
}

impl CandidateEdgeRecord {
    pub const fn typed_id(&self) -> CandidateEdgeId {
        CandidateEdgeId(self.candidate_id)
    }
}

impl AdjudicationRecord {
    pub const fn typed_candidate_id(&self) -> CandidateEdgeId {
        CandidateEdgeId(self.candidate_id)
    }
}

impl DecisionRecord {
    pub const fn typed_id(&self) -> DecisionId {
        DecisionId(self.id)
    }

    pub const fn typed_candidate_id(&self) -> CandidateEdgeId {
        CandidateEdgeId(self.candidate_id)
    }
}

pub fn expected_record_size(kind: SectionKind) -> u32 {
    match kind {
        SectionKind::Document => size::<DocumentRecord>(),
        SectionKind::Chunks => size::<ChunkRecord>(),
        SectionKind::Sentences => size::<SentenceRecord>(),
        SectionKind::Spans => size::<SpanRecord>(),
        SectionKind::Strings => 1,
        SectionKind::Entities => size::<EntityRecord>(),
        SectionKind::Mentions => size::<MentionRecord>(),
        SectionKind::Evidence => size::<EvidenceRecord>(),
        SectionKind::AcceptedEdges => size::<AcceptedEdgeRecord>(),
        SectionKind::CandidateEdges => size::<CandidateEdgeRecord>(),
        SectionKind::Adjudications => size::<AdjudicationRecord>(),
        SectionKind::Decisions => size::<DecisionRecord>(),
        SectionKind::Capabilities => size::<CapabilityRecord>(),
        SectionKind::Identities => size::<IdentityRecord>(),
        SectionKind::StageReceipts => size::<StageReceiptRecord>(),
    }
}

fn size<T>() -> u32 {
    std::mem::size_of::<T>() as u32
}
