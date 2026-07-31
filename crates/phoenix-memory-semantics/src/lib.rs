//! Domain-general, proposal-only memory semantics.
//!
//! The core vocabulary is stable. Narrative, conversation, and document
//! meaning live in detachable packs. This crate cannot promote a candidate.

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
