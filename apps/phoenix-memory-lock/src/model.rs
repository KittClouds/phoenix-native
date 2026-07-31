use serde::{Deserialize, Serialize};

pub const FREEZE_CONTRACT: &str = "phoenix.memory.longmemeval-freeze/v1";
pub const WORKLOAD_CONTRACT: &str = "phoenix.memory.longmemeval-workload/v1";
pub const GOLD_CONTRACT: &str = "phoenix.memory.longmemeval-gold/v1";
pub const RETRIEVAL_CONTRACT: &str = "phoenix.memory.longmemeval-retrieval/v1";

pub const WORKLOAD_MAGIC: [u8; 8] = *b"PHXLMW01";
pub const GOLD_MAGIC: [u8; 8] = *b"PHXLMG01";
pub const RETRIEVAL_MAGIC: [u8; 8] = *b"PHXLMR01";

#[derive(Clone, Debug, Deserialize)]
pub struct FreezeManifest {
    pub contract: String,
    pub freeze_id: String,
    pub behavioral_reference: BehavioralReference,
    pub benchmark: BenchmarkFreeze,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BehavioralReference {
    pub name: String,
    pub repository: String,
    pub commit: String,
    pub workspace_version: String,
    pub license: String,
    pub runtime_dependency: bool,
    pub source_files: Vec<SourceFileLock>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BenchmarkFreeze {
    pub name: String,
    pub repository: String,
    pub commit: String,
    pub dataset_repository: String,
    pub dataset_revision: String,
    pub license: String,
    pub baseline_variant: String,
    pub datasets: Vec<DatasetLock>,
    pub source_files: Vec<SourceFileLock>,
    pub prompt_profiles: Vec<PromptProfile>,
    pub model_profiles: Vec<ModelProfile>,
    pub gold_firewall: GoldFirewall,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SourceFileLock {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DatasetLock {
    pub variant: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PromptProfile {
    pub id: String,
    pub source_path: String,
    pub source_sha256: String,
    pub symbol: String,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelProfile {
    pub role: String,
    pub provider: String,
    pub model: String,
    pub weights_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GoldFirewall {
    pub workload_magic: String,
    pub gold_magic: String,
    pub retrieval_magic: String,
    pub forbidden_workload_fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceBinding {
    pub freeze_id: String,
    pub variant: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkloadArtifact {
    pub contract: String,
    pub source: SourceBinding,
    pub cases: Vec<WorkloadCase>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkloadCase {
    pub question_id: String,
    pub question_type: String,
    pub question: String,
    pub question_date: String,
    pub sessions: Vec<HistorySession>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistorySession {
    pub stable_id: String,
    pub date: String,
    pub turns: Vec<HistoryTurn>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryTurn {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoldArtifact {
    pub contract: String,
    pub source: SourceBinding,
    pub cases: Vec<GoldCase>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoldCase {
    pub question_id: String,
    pub question_type: String,
    pub answer: String,
    pub answer_session_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrievalArtifact {
    pub contract: String,
    pub source: SourceBinding,
    pub engine: String,
    pub top_k: u32,
    pub cases: Vec<RetrievalCase>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrievalCase {
    pub question_id: String,
    pub ranked_sessions: Vec<RankedSession>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankedSession {
    pub stable_id: String,
    pub score_bits: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvaluationReceipt {
    pub contract: &'static str,
    pub freeze_id: String,
    pub source_sha256: String,
    pub engine: String,
    pub cases: usize,
    pub answerable_cases: usize,
    pub hit_at_k: f64,
    pub mean_reciprocal_rank: f64,
}
