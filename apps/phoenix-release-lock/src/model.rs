use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT: &str = "phoenix.native.exact-parity-release-lock/v1";

#[derive(Debug, Deserialize)]
pub struct FrozenCohort {
    pub contract: String,
    pub cohort_id: String,
    pub captured_at: String,
    pub document: FrozenDocument,
    pub angular_runtime: FrozenRuntime,
    pub native_runtime: FrozenNativeRuntime,
    pub model_runtime: FrozenModelRuntime,
    pub manifolds: Vec<FrozenManifold>,
}

#[derive(Debug, Deserialize)]
pub struct FrozenDocument {
    pub note_id: String,
    pub title: String,
    pub version: u64,
    pub markdown_utf16_chars: u64,
    pub markdown_utf8_bytes: u64,
    pub markdown_sha256: String,
    pub plain_text_utf16_chars: u64,
    pub plain_text_utf8_bytes: u64,
    pub plain_text_sha256: String,
    pub footer_words: u64,
    pub footer_chars_without_line_breaks: u64,
}

#[derive(Debug, Deserialize)]
pub struct FrozenRuntime {
    pub binary_sha256: String,
    pub repo_head: String,
    pub dirty_status_sha256: String,
    pub page_url: String,
    pub packet_schema: String,
}

#[derive(Debug, Deserialize)]
pub struct FrozenModelRuntime {
    pub status: String,
    pub identities: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FrozenNativeRuntime {
    pub binary_sha256: String,
    pub proof_contract: String,
}

#[derive(Debug, Deserialize)]
pub struct FrozenManifold {
    pub name: String,
    pub layout_mode: String,
    pub generation_id: String,
    pub authority_receipt: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub packet_hash: String,
    pub pages: BTreeMap<String, FrozenPage>,
}

#[derive(Debug, Deserialize)]
pub struct FrozenPage {
    pub elements: u64,
    pub bytes: u64,
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct ReleaseReceipt {
    pub contract: &'static str,
    pub cohort_id: String,
    pub result: ReleaseResult,
    pub frozen_document: DocumentReceipt,
    pub frozen_angular: AngularReceipt,
    pub native: NativeReceipt,
    pub gates: Vec<GateReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseResult {
    Pass,
    Stop,
}

#[derive(Debug, Serialize)]
pub struct DocumentReceipt {
    pub note_id: String,
    pub title: String,
    pub version: u64,
    pub markdown_utf16_chars: u64,
    pub markdown_utf8_bytes: u64,
    pub markdown_sha256: String,
    pub plain_text_utf16_chars: u64,
    pub plain_text_utf8_bytes: u64,
    pub plain_text_sha256: String,
    pub footer_words: u64,
    pub footer_chars_without_line_breaks: u64,
}

#[derive(Debug, Serialize)]
pub struct AngularReceipt {
    pub captured_at: String,
    pub binary_sha256: String,
    pub repo_head: String,
    pub dirty_status_sha256: String,
    pub page_url: String,
    pub packet_schema: String,
    pub model_status: String,
    pub model_identities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NativeReceipt {
    pub binary_sha256: String,
    pub proof_contract: String,
    pub fallback_count: u64,
    pub archive_generation: u64,
    pub archive_cohort_hash_blake3: String,
    pub archive_pages: u32,
    pub archive_bytes: u64,
    pub product_index_hash_blake3: String,
    pub product_index_bytes: u64,
    pub node_count: usize,
    pub edge_count: usize,
    pub mapping_count: u32,
    pub label_bytes: u32,
    pub manifolds: Vec<NativeManifoldReceipt>,
}

#[derive(Debug, Serialize)]
pub struct NativeManifoldReceipt {
    pub name: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub node_identity_hash: String,
    pub edge_identity_hash: String,
    pub topology_hash: String,
    pub positions_hash: String,
    pub node_colors_hash: String,
    pub edge_colors_hash: String,
}

#[derive(Debug, Serialize)]
pub struct GateReceipt {
    pub id: &'static str,
    pub passed: bool,
    pub detail: String,
}
