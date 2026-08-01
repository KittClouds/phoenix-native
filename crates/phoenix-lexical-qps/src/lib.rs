//! Bounded positional BM25F/QPS retrieval for Phoenix Native.
//!
//! Construction produces compact immutable arrays. Serving reuses caller-owned
//! scratch and output buffers; V2.01 selects dense candidate sets with SIMD and
//! exact linear-time selection rather than maintaining a heap.

mod builder;
mod index;
mod ranker;
mod score;
mod selection;
mod tokenize;
mod types;

pub use builder::QpsBuilder;
pub use index::{IndexStats, QpsIndex, SearchScratch};
pub use ranker::{
    HardNegativeJudgment, HardNegativeLedgerV1, HardNegativeReason, LinearRankerV1,
    RankFeatureVector, RankerTrainingConfig, RankerTrainingReceipt, RANK_FEATURE_COUNT,
};
pub use types::{
    CandidateSelection, DocumentId, DocumentInput, Expansion, FieldConfig, QpsConfig, QpsError,
    QueryGroup, SearchHit, SearchReceipt, SearchStageNanos, MAXIMUM_QUERY_GROUPS,
};
