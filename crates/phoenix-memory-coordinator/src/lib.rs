//! Bounded, dual-face ingestion for Phoenix documents and conversations.
//!
//! The coordinator owns mutable construction state on one worker. Recall is a
//! read-only view of committed turns; document and turn ingestion both publish
//! through the same immutable V3 generation boundary.

mod assemble;
mod command;
mod coordinator;
mod error;
mod fingerprint;
mod product;
mod publication;
mod retrieval;
mod shadow;
mod state;

pub use command::{
    CommittedTurn, ContextCandidateItem, ContextEvidenceExcerpt, ContextItem, ContextPacket,
    ContextTemporalEnvelopeV1, ConversationKey, IngestDocumentRevision, IngestTurn,
    IngestionOrigin, LexicalRecallPathId, LexicalRecallReceipt, LexicalRecallStatus, MemoryScope,
    MemorySourceLocator, PendingTurn, RecallTurn, LEXICAL_RECALL_PATH,
};
pub use coordinator::{
    CoordinatorConfig, CoordinatorMetrics, CoordinatorTicket, DualFaceIngestionCoordinator,
    LexicalRecallConfig, DEFAULT_COMMAND_CAPACITY, DEFAULT_CONTEXT_BYTES, DEFAULT_CONTEXT_ITEMS,
    MAX_COMMAND_CAPACITY, MAX_CONTEXT_BYTES, MAX_CONTEXT_ITEMS,
};
pub use error::CoordinatorError;
pub use phoenix_memory_contract::{ProducerProductV3, SemanticCandidateFamilyV3};
pub use product::{
    authoritative_product, CancellationProbe, CandidateEndpointDraft, CanonicalBindingDraft,
    CommonProducts, DocumentProduction, DualFaceProducer, EntityDraft, MentionDraft,
    ModelIdentityInputV3, ProducerRegistrationV3, RegistrationSupport, SemanticCandidateDraft,
    TemporalEnvelopeBindingDraftV1, TemporalEnvelopeDraftV1, TurnProduction, VocabularyPackDraft,
    MAX_CANDIDATES_PER_SOURCE, MAX_ENDPOINTS_PER_CANDIDATE, MAX_ENTITIES_PER_SOURCE,
    MAX_EVIDENCE_PER_CANDIDATE, MAX_MENTIONS_PER_SOURCE, MAX_TEMPORAL_BINDINGS_PER_ENVELOPE,
    MAX_TEMPORAL_ENVELOPES_PER_SOURCE, MAX_VOCABULARY_PACKS, NO_MODEL_IDENTITY,
};
pub use publication::GenerationPublication;
pub use shadow::{
    QpsShadowConfig, QpsShadowPathId, QpsShadowQueryShape, QpsShadowReceipt, QpsShadowStatus,
};

pub(crate) use publication::publish_or_reuse;
pub(crate) use state::{CoordinatorState, StoredConversation, StoredDocument, StoredTurn};

#[cfg(test)]
mod tests;
