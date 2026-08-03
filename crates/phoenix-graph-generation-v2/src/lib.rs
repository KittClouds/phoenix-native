mod authority;
mod error;
mod format;
mod ids;
mod open;
mod records;
mod topology;
mod validate;
mod write;

pub use authority::{
    AuthorityClass, CacheState, CandidateStatus, CanonicalBindingKind, CapabilityState,
    DecisionAction, EpisodeMemberKind, EvidenceRole, ProducerProduct, PublicationStatus,
    SemanticFamily, STAGE_FLAG_ALLOCATION_NOT_MEASURED, STAGE_FLAG_QUEUE_NOT_OBSERVED,
    STAGE_FLAG_TIMING_NOT_MEASURED,
};
pub use error::GraphGenerationV2Error;
pub use format::{
    expected_authority, expected_record_alignment, expected_record_size, expected_schema_hash,
    GenerationHeader, PageDescriptor, PageKind, GRAPH_GENERATION_V2_CONTRACT,
    GRAPH_GENERATION_V2_EXTENSION, GRAPH_GENERATION_V2_MAGIC, GRAPH_GENERATION_V2_VERSION,
    HEADER_FLAG_COMPLETE, MAX_GENERATION_BYTES, MAX_PAGE_COUNT, MAX_RECORDS_PER_PAGE,
    PAGE_ALIGNMENT, PAGE_FLAG_REQUIRED,
};
pub use ids::{
    CandidateId, CausalId, ChapterId, ChunkId, DecisionId, DocumentId, EntityId, EpisodeId,
    EventId, EvidenceId, GenerationId, MemoryStateId, MentionId, ParagraphId, ReceiptId,
    SentenceId, SpanId, StructuralEdgeId, TemporalId,
};
pub use open::VerifiedGraphGenerationV2;
pub use records::{
    CandidateEvidenceBindingRecord, CanonicalEntityBindingRecord, CapabilityRecord,
    CausalCandidateRecord, ChapterRecord, ChunkRecord, ContextualEvidenceRecord, DecisionRecord,
    DocumentRecord, EntityRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord,
    EvidenceRecord, IdentityCandidateRecord, MemoryStateCandidateRecord, MentionRecord,
    ModelIdentityRecord, NliAdjudicationRecord, ParagraphRecord, PublicationReceiptRecord,
    SentenceRecord, SpanRecord, StageReceiptRecord, StringRef, StructuralEdgeRecord,
    TemporalCandidateRecord, TypedRelationshipCandidateRecord,
};
pub use topology::{
    CausalRelationKind, SemanticEndpointKind, StructuralRelationKind, TemporalRelationKind,
    TopologyNodeKind, TopologyValidationError, VerifiedTopologyV2, ENDPOINT_SOURCE_SHIFT,
    ENDPOINT_TARGET_SHIFT,
};
pub use validate::{
    align_up, compute_generation_hash, validate_directory, validate_header, validate_page_payload,
};
pub use write::{write_generation_new, GenerationPages, GenerationWriteAuthority};

#[cfg(test)]
mod tests;
