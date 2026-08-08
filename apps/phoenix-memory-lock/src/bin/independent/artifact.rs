use std::path::PathBuf;

use phoenix_lexical_qps::{JudgmentReasonV3, JudgmentSourceV3, RankEvidenceV3, RelevanceTier};
use serde::Serialize;

use super::build::DatasetAudit;

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    pub(super) contract: &'static str,
    pub(super) ledger: FileIdentity,
    pub(super) graded_suite: FileIdentity,
    pub(super) review_packet: FileIdentity,
    pub(super) receipt: FileIdentity,
    pub(super) ledger_judgments: usize,
    pub(super) graded_queries: usize,
    pub(super) authoritative_promotion_evidence: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct GenerationReceipt {
    pub(super) contract: &'static str,
    pub(super) schema_version: u16,
    pub(super) producer_binary: FileIdentity,
    pub(super) inputs: InputReceipt,
    pub(super) outputs: OutputReceipt,
    pub(super) policy: GenerationPolicy,
    pub(super) constitutional_holdouts: usize,
    pub(super) training: Vec<DatasetAudit>,
    pub(super) graded: Vec<DatasetAudit>,
    pub(super) ledger_judgments: usize,
    pub(super) graded_queries: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct InputReceipt {
    pub(super) phase_3: FileIdentity,
    pub(super) locomo: FileIdentity,
    pub(super) scifact_corpus: FileIdentity,
    pub(super) scifact_queries: FileIdentity,
    pub(super) scifact_qrels: FileIdentity,
    pub(super) nfcorpus_corpus: FileIdentity,
    pub(super) nfcorpus_queries: FileIdentity,
    pub(super) nfcorpus_qrels: FileIdentity,
    pub(super) workspace_key_recorded: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct OutputReceipt {
    pub(super) ledger: FileIdentity,
    pub(super) graded_suite: FileIdentity,
    pub(super) review_packet: FileIdentity,
}

#[derive(Debug, Serialize)]
pub(super) struct GenerationPolicy {
    pub(super) training_sources: [&'static str; 2],
    pub(super) graded_sources: [&'static str; 2],
    pub(super) negative_policy: &'static str,
    pub(super) judgment_source: JudgmentSourceV3,
    pub(super) authoritative_promotion_evidence: bool,
    pub(super) real_user_corrections_inferred: bool,
    pub(super) release_feature_vectors_consumed_for_training: bool,
    pub(super) cross_source_near_duplicates_excluded: bool,
    pub(super) candidate_cap: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct FileIdentity {
    pub(super) path: PathBuf,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

#[derive(Debug, Serialize)]
pub(super) struct GradedEvaluationSuite {
    pub(super) contract: &'static str,
    pub(super) schema_version: u16,
    pub(super) queries: Vec<GradedQuery>,
}

#[derive(Debug, Serialize)]
pub(super) struct GradedQuery {
    pub(super) query_identity: String,
    pub(super) candidates: Vec<GradedCandidate>,
}

#[derive(Debug, Serialize)]
pub(super) struct GradedCandidate {
    pub(super) document_identity: String,
    pub(super) v2_order: usize,
    pub(super) rank_evidence_v3: RankEvidenceV3,
    pub(super) relevance_tier: RelevanceTier,
    pub(super) grade: u8,
}

#[derive(Debug, Serialize)]
pub(super) struct ReviewPacket {
    pub(super) contract: &'static str,
    pub(super) schema_version: u16,
    pub(super) instructions: &'static str,
    pub(super) negative_label_warning: &'static str,
    pub(super) items: Vec<ReviewItem>,
}

#[derive(Debug, Serialize)]
pub(super) struct ReviewItem {
    pub(super) judgment_identity: String,
    pub(super) dataset: &'static str,
    pub(super) query_id: String,
    pub(super) query: String,
    pub(super) positive: ReviewDocument,
    pub(super) negative: ReviewDocument,
    pub(super) positive_v2_position: usize,
    pub(super) negative_v2_position: usize,
    pub(super) suggested_reason: JudgmentReasonV3,
}

#[derive(Debug, Serialize)]
pub(super) struct ReviewDocument {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) text: String,
}
