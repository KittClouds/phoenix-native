mod build;
mod error;
mod format;
mod ids;
mod open;

pub use build::{
    write_graph_generation_new, AcceptedEdgeInput, CanonicalEntityInput, DurableDecisionInput,
    GraphGenerationInput, GraphGenerationWriteReceipt, ProducerCapabilityInput,
};
pub use error::GraphGenerationError;
pub use format::{
    AcceptedEdgeRecord, AdjudicationRecord, CandidateEdgeRecord, CapabilityRecord, ChunkRecord,
    DecisionRecord, DocumentRecord, EntityRecord, EvidenceRecord, GenerationHeader, IdentityRecord,
    MentionRecord, SectionDescriptor, SectionKind, SentenceRecord, SpanRecord, StageReceiptRecord,
    StringRef, GRAPH_GENERATION_EXTENSION, GRAPH_GENERATION_VERSION,
};
pub use ids::{
    AcceptedEdgeId, CandidateEdgeId, ChunkId, DecisionId, DocumentId, EntityId, EvidenceId,
    MentionId, SentenceId, SpanId,
};
pub use open::VerifiedGraphGeneration;

pub const GRAPH_GENERATION_CONTRACT: &str = "phoenix.graph-generation/v1";
pub const DECISION_STATUS_ACCEPTED: u16 = 1;
pub const DECISION_STATUS_REJECTED: u16 = 2;
pub const DECISION_STATUS_DEFERRED: u16 = 3;
pub const DECISION_FLAG_DURABLE_RECEIPT: u16 = 1;
pub const ACCEPTED_EDGE_FLAG_PROMOTED: u16 = 1 << 15;

pub fn promoted_edge_id(candidate_id: [u8; 32]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation.promoted-edge/v1\0");
    hasher.update(&candidate_id);
    let mut raw = [0; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

#[cfg(test)]
mod tests;
