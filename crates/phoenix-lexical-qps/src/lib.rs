//! Bounded positional BM25F/QPS retrieval for Phoenix Native.
//!
//! Construction produces compact immutable arrays. Serving reuses caller-owned
//! scratch and output buffers; V2.01 selects dense candidate sets with SIMD and
//! exact linear-time selection rather than maintaining a heap.

mod builder;
mod index;
mod ledger_v3;
mod rank_evidence;
mod ranker;
mod ranker_v3;
mod score;
mod selection;
mod split_v3;
mod tokenize;
mod types;

pub use builder::QpsBuilder;
pub use index::{
    rerank_v3_generated_candidates_in_place, rerank_v3_generated_top_k_in_place,
    rerank_v3_in_place, IndexStats, QpsIndex, SearchScratch,
};
pub use ledger_v3::{
    FrozenHoldoutV3, JudgmentAuthorityV3, JudgmentIdentity, JudgmentReasonV3, JudgmentSourceV3,
    KeyedIdentity, LedgerAuditV3, PairwiseJudgmentDraftV3, PairwiseJudgmentV3, RelevanceLedgerV3,
    SplitGroupProvenanceV3, WorkspaceIdentityKey, RELEVANCE_LEDGER_V3_CONTRACT,
    RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
};
pub use rank_evidence::{
    RankEvidenceV3, RelevanceTier, RANK_EVIDENCE_V3_FEATURE_COUNT, RANK_EVIDENCE_V3_FEATURE_NAMES,
    RANK_EVIDENCE_V3_FIELD_SLOTS, RANK_EVIDENCE_V3_SCHEMA_VERSION,
};
pub use ranker::{
    HardNegativeJudgment, HardNegativeLedgerV1, HardNegativeReason, LinearRankerV1,
    RankFeatureVector, RankerTrainingConfig, RankerTrainingReceipt, RANK_FEATURE_COUNT,
};
pub use ranker_v3::{
    leakage_split_identity_v3, rank_evidence_schema_identity_v3, train_linear_ranker_v3,
    FeatureNormalizationV3, LinearRankerV3, LinearTrainingConfigV3, LinearTrainingReceiptV3,
    LINEAR_RANKER_V3_VERSION, RANK_EVIDENCE_V3_SCHEMA_IDENTITY,
};
pub use split_v3::{
    JudgmentSplitV3, LeakageSplitAuditV3, LeakageSplitV3, PrimarySplitV3,
    LEAKAGE_SPLIT_V3_CONTRACT, LEAKAGE_SPLIT_V3_SCHEMA_VERSION, SPLIT_RATIO_TOLERANCE_BPS,
};
pub use types::{
    CandidateSelection, DocumentId, DocumentInput, Expansion, FieldConfig, QpsConfig, QpsError,
    QueryGroup, SearchHit, SearchReceipt, SearchStageNanos, MAXIMUM_QUERY_GROUPS,
};
