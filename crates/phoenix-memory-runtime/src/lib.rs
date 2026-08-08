//! Append-only policy decisions, deterministic current-memory projection, and
//! bounded recursive traversal for verified Phoenix V3 memory generations.

mod catalog;
mod contract;
mod error;
mod ledger;
mod projection;
mod working_set;

pub use catalog::{CandidateBindingV1, MemoryCatalogV1};
pub use contract::{
    DecisionDispositionV1, PolicyDecisionCommandV1, PolicyDecisionOutcomeV1,
    PolicyDecisionReceiptHeaderV1, VerifiedPolicyDecisionReceiptV1, MAX_POLICY_REASON_BYTES,
    POLICY_DECISION_EXTENSION,
};
pub use error::MemoryRuntimeError;
pub use ledger::PolicyDecisionLedgerV1;
pub use projection::{
    CurrentMemoryProjectionHeaderV1, CurrentMemoryProjectionV1, CurrentMemoryRecordV1,
    CurrentMemoryStateV1, ProjectionReceiptV1, VerifiedCurrentMemoryProjectionV1,
    CURRENT_MEMORY_PROJECTION_EXTENSION,
};
pub use working_set::{
    RecursiveQueryV1, RecursiveScratchV1, RecursiveTraversalReceiptV1, WorkingEdgeKindV1,
    WorkingEdgeRecordV1, WorkingNodeId, WorkingNodeKeyV1, WorkingNodeKindV1, WorkingNodeRecordV1,
    WorkingSetGraphV1, MAX_RECURSIVE_DEPTH, MAX_RECURSIVE_EDGES, MAX_RECURSIVE_NODES,
    MAX_WORKING_SET_BUILD_EDGES, MAX_WORKING_SET_BUILD_NODES,
};

#[cfg(test)]
mod tests;
