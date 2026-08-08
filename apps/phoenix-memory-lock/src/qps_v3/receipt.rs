use serde::Serialize;

use phoenix_lexical_qps::{
    CandidateSelection, RankEvidenceV3, RelevanceTier, SearchHit, SearchReceipt,
};

pub const BASELINE_CONTRACT: &str = "phoenix.memory.qps-v3-baseline/v1";
pub const BASELINE_PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-baseline-publication/v1";

#[derive(Debug, Serialize)]
pub struct BaselineReceiptV3 {
    pub contract: &'static str,
    pub architecture: &'static str,
    pub v2_binary: FileIdentity,
    pub v2_configuration: FrozenV2Configuration,
    pub v2_configuration_sha256: String,
    pub artifacts: BaselineArtifacts,
    pub mixed_suite: CohortBaseline,
    pub longmemeval_release: CohortBaseline,
    pub gates: BaselineGates,
    pub phase_1_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct BaselinePublication {
    pub contract: &'static str,
    pub output: FileIdentity,
    pub mixed_metrics: QualityMetrics,
    pub longmemeval_metrics: QualityMetrics,
    pub gates: BaselineGates,
    pub phase_1_verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileIdentity {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct BaselineArtifacts {
    pub freeze_manifest: FileIdentity,
    pub mixed_suite: FileIdentity,
    pub source_corpus: FileIdentity,
    pub workload: FileIdentity,
    pub gold: FileIdentity,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrozenV2Configuration {
    pub engine: &'static str,
    pub top_k: usize,
    pub k1: f32,
    pub coverage_floor: f32,
    pub coverage_exponent: f32,
    pub proximity_weight: f32,
    pub order_weight: f32,
    pub phrase_weight: f32,
    pub segment_weight: f32,
    pub proximity_decay_tokens: f32,
    pub minimum_candidate_pool: usize,
    pub candidate_pool_multiplier: usize,
    pub maximum_candidate_pool: usize,
    pub dense_simd_threshold: f32,
    pub maximum_query_groups: usize,
    pub maximum_expansions_per_group: usize,
    pub learned_ranker_enabled: bool,
    pub learned_ranker_identity: String,
    pub mixed_fields: Vec<FrozenFieldConfiguration>,
    pub longmemeval_fields: Vec<FrozenFieldConfiguration>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrozenFieldConfiguration {
    pub name: &'static str,
    pub weight: f32,
    pub length_normalization: f32,
    pub exact_match_bonus: f32,
}

#[derive(Debug, Serialize)]
pub struct CohortBaseline {
    pub cohort: &'static str,
    pub queries: Vec<QueryBaseline>,
    pub metrics: QualityMetrics,
    pub aggregate_latency: LatencyReceipt,
    pub warm_allocation_growths: u64,
    pub deterministic_ranking_failures: u64,
    pub maximum_candidate_pool: usize,
}

#[derive(Debug, Serialize)]
pub struct QueryBaseline {
    pub query_identity: String,
    pub query_shape: String,
    pub query_sha256: String,
    pub candidate_pool: Vec<CandidateEvidenceV2>,
    pub v2_final_order: Vec<String>,
    pub oracle_locations: Vec<OracleLocation>,
    pub latency: LatencyReceipt,
    pub execution: QueryExecutionReceipt,
}

#[derive(Debug, Serialize)]
pub struct CandidateEvidenceV2 {
    pub document_identity: String,
    pub document_version_sha256: String,
    pub v2_order: u16,
    pub v2_score_bits: u32,
    pub lexical_score_bits: u32,
    pub coverage_bits: u32,
    pub proximity_bits: u32,
    pub order_bits: u32,
    pub phrase_bits: u32,
    pub segment_bits: u32,
    pub exact_field_bits: u32,
    pub legacy_rank_feature_bits: [u32; 12],
    pub rank_evidence_v3: RankEvidenceV3,
    pub relevance_tier: RelevanceTier,
}

impl CandidateEvidenceV2 {
    pub fn from_hit(
        hit: &SearchHit,
        document_identity: String,
        document_version_sha256: String,
        v2_order: usize,
    ) -> Self {
        Self {
            document_identity,
            document_version_sha256,
            v2_order: u16::try_from(v2_order).unwrap_or(u16::MAX),
            v2_score_bits: hit.score.to_bits(),
            lexical_score_bits: hit.lexical_score.to_bits(),
            coverage_bits: hit.coverage.to_bits(),
            proximity_bits: hit.proximity.to_bits(),
            order_bits: hit.order.to_bits(),
            phrase_bits: hit.phrase.to_bits(),
            segment_bits: hit.segment.to_bits(),
            exact_field_bits: hit.exact_field.to_bits(),
            legacy_rank_feature_bits: hit.rank_features.0.map(f32::to_bits),
            rank_evidence_v3: hit.rank_evidence_v3,
            relevance_tier: hit.relevance_tier,
        }
    }

    pub fn is_finite(&self) -> bool {
        [
            self.v2_score_bits,
            self.lexical_score_bits,
            self.coverage_bits,
            self.proximity_bits,
            self.order_bits,
            self.phrase_bits,
            self.segment_bits,
            self.exact_field_bits,
        ]
        .into_iter()
        .chain(self.legacy_rank_feature_bits)
        .all(|bits| f32::from_bits(bits).is_finite())
            && self.rank_evidence_v3.is_valid()
    }
}

#[derive(Debug, Serialize)]
pub struct OracleLocation {
    pub document_identity: String,
    pub candidate_pool_rank: Option<u16>,
    pub top_10_rank: Option<u16>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct QualityMetrics {
    pub answerable_queries: usize,
    pub hit_at_10: f64,
    pub mean_reciprocal_rank: f64,
    pub top_1_accuracy: f64,
    pub no_result_queries: usize,
    pub no_result_accuracy: f64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct LatencyReceipt {
    pub samples: usize,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub p99_nanos: u64,
    pub maximum_nanos: u64,
}

impl LatencyReceipt {
    pub fn from_samples(samples: &mut [u64]) -> Self {
        samples.sort_unstable();
        Self {
            samples: samples.len(),
            p50_nanos: percentile(samples, 50),
            p95_nanos: percentile(samples, 95),
            p99_nanos: percentile(samples, 99),
            maximum_nanos: samples.last().copied().unwrap_or(0),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QueryExecutionReceipt {
    pub query_groups: u16,
    pub posting_candidates: u32,
    pub covered_candidates: u32,
    pub reranked_candidates: u32,
    pub posting_rows_visited: u32,
    pub position_values_visited: u32,
    pub selection: &'static str,
    pub warm_allocation_growths: u64,
}

impl QueryExecutionReceipt {
    pub fn from_search(receipt: SearchReceipt, warm_allocation_growths: u64) -> Self {
        Self {
            query_groups: receipt.query_groups,
            posting_candidates: receipt.candidates,
            covered_candidates: receipt.covered_candidates,
            reranked_candidates: receipt.reranked_candidates,
            posting_rows_visited: receipt.posting_rows_visited,
            position_values_visited: receipt.position_values_visited,
            selection: match receipt.selection {
                CandidateSelection::SparseTouched => "sparse_touched",
                CandidateSelection::DenseSimd => "dense_simd",
                CandidateSelection::Exhaustive => "exhaustive",
            },
            warm_allocation_growths,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct BaselineGates {
    pub artifact_hashes_match_frozen_inputs: bool,
    pub v2_ranker_disabled: bool,
    pub candidate_cap_is_160: bool,
    pub every_frozen_query_recorded: bool,
    pub every_reranked_candidate_recorded: bool,
    pub all_evidence_finite: bool,
    pub zero_warm_allocation_growth: bool,
    pub deterministic_ranking_failures_are_zero: bool,
    pub mixed_hit_at_10_is_1: bool,
    pub mixed_mrr_is_1: bool,
    pub longmemeval_hit_at_10_is_098: bool,
    pub longmemeval_mrr_is_0891005: bool,
}

impl BaselineGates {
    pub fn all_pass(self) -> bool {
        self.artifact_hashes_match_frozen_inputs
            && self.v2_ranker_disabled
            && self.candidate_cap_is_160
            && self.every_frozen_query_recorded
            && self.every_reranked_candidate_recorded
            && self.all_evidence_finite
            && self.zero_warm_allocation_growth
            && self.deterministic_ranking_failures_are_zero
            && self.mixed_hit_at_10_is_1
            && self.mixed_mrr_is_1
            && self.longmemeval_hit_at_10_is_098
            && self.longmemeval_mrr_is_0891005
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    samples
        .get((samples.len().saturating_sub(1) * percentile) / 100)
        .copied()
        .unwrap_or(0)
}
