//! Immutable mixed-source graph authority for Phoenix documents and
//! conversations.
//!
//! V3 is an isolated successor contract. It does not register itself as the
//! production authority and does not modify or translate V2 artifacts.

mod authority;
mod builder;
mod error;
mod format;
mod ids;
mod nli;
mod open;
mod records;
mod temporal;
mod validate;
mod write;

pub use authority::{
    AuthoritySubjectKind, CandidateEndpointRoleV3, ContentUnitKind, ModelSemanticRoleV3,
    ParticipantRole, ProducerProductV3, ProducerStateV3, SemanticCandidateFamilyV3, SourceKind,
    TemporalBindingRoleV1, TemporalPrecisionV1, TemporalSubjectKindV1, VocabularyPackKindV3,
    SOURCE_FLAG_COMPLETE, TEMPORAL_FLAG_ASSERTED_TIME, TEMPORAL_FLAG_EXPLICIT_TEXT,
    TEMPORAL_FLAG_NORMALIZED, TEMPORAL_FLAG_OBSERVED_TIME, TEMPORAL_FLAG_OCCURRENCE_TIME,
    TEMPORAL_FLAG_SOURCE_TIME, TEMPORAL_FLAG_UNCERTAIN, TIMEZONE_OFFSET_UNKNOWN, TIME_UNBOUNDED,
    TIME_UNKNOWN,
};
pub use builder::{
    deterministic_id, ConversationInput, DocumentChunkInput, DocumentInput, MixedSourceBuilder,
    PreparedMixedSource, TurnInput,
};
pub use error::MemoryContractError;
pub use format::{
    expected_authority, expected_record_alignment, expected_record_size, expected_schema_hash,
    GenerationHeaderV3, PageDescriptorV3, PageKindV3, GRAPH_GENERATION_V3_CONTRACT,
    GRAPH_GENERATION_V3_EXTENSION, GRAPH_GENERATION_V3_MAGIC, GRAPH_GENERATION_V3_VERSION,
    HEADER_FLAG_COMPLETE, MAX_GENERATION_BYTES, MAX_PAGE_COUNT, MAX_RECORDS_PER_PAGE,
    PAGE_ALIGNMENT, PAGE_COUNT_V3, PAGE_FLAG_REQUIRED,
};
pub use ids::{
    temporal_candidate_subject_id, temporal_u64_subject_id, ContentUnitId, ConversationId,
    NamespaceId, SourceId, TurnId,
};
pub use open::{OpenExpectation, VerifiedGraphGenerationV3};
pub use phoenix_graph_generation_v2::{
    AuthorityClass, CandidateEvidenceBindingRecord, CandidateId, CandidateStatus,
    CanonicalBindingKind, CanonicalEntityBindingRecord, CapabilityRecord, CausalCandidateRecord,
    ChapterRecord, ChunkRecord, ContextualEvidenceRecord, DecisionRecord, EntityRecord,
    EpisodeMembershipRecord, EpisodeRecord, EventRecord, EvidenceRole, IdentityCandidateRecord,
    MemoryStateCandidateRecord, ModelIdentityRecord, NliAdjudicationRecord, ParagraphRecord,
    PublicationReceiptRecord, PublicationStatus, SentenceRecord, SpanRecord, StageReceiptRecord,
    StringRef, StructuralEdgeRecord, TemporalCandidateRecord, TypedRelationshipCandidateRecord,
};
pub use records::{
    CandidateEndpointBindingRecordV3, ContentUnitRecord, ConversationRecord,
    DocumentRevisionRecord, EvidenceRecordV3, MentionRecordV3, ProducerCapabilityRecordV3,
    SemanticCandidateRecordV3, SourceRecord, SupersessionRecord, TemporalEnvelopeBindingRecordV1,
    TemporalEnvelopeRecordV1, TurnRecord, ValidityIntervalRecord, VocabularyPackRecordV3,
};
pub use validate::{align_up, compute_generation_hash, compute_source_set_hash};
pub use write::{write_generation_new, GenerationPagesV3, GenerationWriteAuthorityV3};

#[cfg(test)]
mod tests;
