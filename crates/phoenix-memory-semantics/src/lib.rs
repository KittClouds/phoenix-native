//! Domain-general, proposal-only memory semantics.
//!
//! The core vocabulary is stable. Narrative, conversation, and document
//! meaning live in detachable packs. This crate cannot promote a candidate.

mod adjudication;
mod candidate;
mod consolidation;
mod lens;
mod pack;

pub use candidate::{CandidateBuilder, SemanticError};
pub use consolidation::{
    ConsolidationEngine, ConsolidationObservation, ConsolidationProposal, ConsolidationReport,
};
pub use lens::{
    ConversationRelation, CoreRelation, DocumentRelation, LensSet, NarrativeRelation,
    VocabularyRelation,
};
pub use pack::{
    conversation_pack, core_pack, document_pack, narrative_pack, PackDescriptor,
    CONVERSATION_PACK_NAME, CORE_PACK_NAME, DOCUMENT_PACK_NAME, NARRATIVE_PACK_NAME,
};

#[cfg(test)]
mod tests;
pub use adjudication::{
    AdjudicationError, DeterministicAdjudicatorV1, MemoryActionV1, MemoryEventV1, NliRelationV1,
    PolicyProposalV1, PolicyReasonV1, ScopeRelationV1, SemanticAdjudicationInputV1,
    SourceAuthorityV1, TemporalRelationV1, CUE_CURRENT_STATE, CUE_EXPLICIT_CORRECTION,
    CUE_FUTURE_INTENTION, CUE_NEGATED_PROPOSITION, CUE_PREVIOUS_STATE, CUE_SCOPE_QUALIFIER,
    CUE_TEMPORAL_QUALIFIER, CUE_UNCERTAINTY,
};
