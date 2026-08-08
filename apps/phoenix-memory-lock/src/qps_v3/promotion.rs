use std::ffi::OsStr;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;

use phoenix_lexical_qps::{LinearRankerV3, RankEvidenceV3, RelevanceTier};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
};

use super::train::LinearModelArtifactV3;
use super::*;

const SHADOW_CONTRACT: &str = "phoenix.memory.qps-v3-shadow/v1";
const RECONCILIATION_CONTRACT: &str = "phoenix.qps.v3-shadow-reconciliation/v1";
const ACTIVE_CONTRACT: &str = "phoenix.memory.qps-v3-active-model/v1";
const QUALITY_CONTRACT: &str = "phoenix.memory.qps-v3-quality-qualification/v1";
const PERFORMANCE_CONTRACT: &str = "phoenix.memory.qps-v3-performance-qualification/v1";

pub(crate) fn shadow(
    model_path: &Path,
    phase_3_path: &Path,
    output_path: &Path,
) -> Result<ShadowPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite shadow receipt {}",
            output_path.display()
        );
    }
    let model = read_model(model_path)?;
    let phase_3: FrozenPhase3 = read_json(phase_3_path, "Phase 3 receipt")?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("shadow evaluation requires the verified frozen Phase 3 candidate pools");
    }
    let mut disagreements = Vec::new();
    evaluate_cohort(
        "frozen_mixed_suite_v1",
        &phase_3.mixed_suite,
        &model.model_parameters,
        &model.model_identity,
        &mut disagreements,
    )?;
    evaluate_cohort(
        "frozen_longmemeval_release_cohort",
        &phase_3.longmemeval_release,
        &model.model_parameters,
        &model.model_identity,
        &mut disagreements,
    )?;
    disagreements.sort_unstable_by(|left, right| {
        right
            .top_1_changed
            .cmp(&left.top_1_changed)
            .then_with(|| right.maximum_rank_delta.cmp(&left.maximum_rank_delta))
            .then_with(|| right.confidence_gap.total_cmp(&left.confidence_gap))
            .then_with(|| left.disagreement_identity.cmp(&right.disagreement_identity))
    });
    let total_queries =
        phase_3.mixed_suite.queries.len() + phase_3.longmemeval_release.queries.len();
    let receipt = ShadowReceipt {
        contract: SHADOW_CONTRACT,
        model_artifact: file_identity(model_path)?,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        model_identity: model.model_identity,
        returned_engine: "V2",
        returned_context_changed: false,
        total_queries,
        disagreement_count: disagreements.len(),
        disagreements,
        gates: ShadowGates {
            same_frozen_candidate_pool: true,
            v2_context_remained_authoritative: true,
            every_disagreement_has_stable_identity: true,
            high_impact_order_is_deterministic: true,
        },
        shadow_verified: true,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(ShadowPublication {
        contract: SHADOW_CONTRACT,
        output: file_identity(output_path)?,
        model_identity: model.model_identity,
        total_queries,
        disagreement_count: receipt.disagreement_count,
        returned_context_changed: false,
        shadow_verified: true,
    })
}

pub(crate) fn promote(
    model_path: &Path,
    phase_8_path: &Path,
    phase_9_path: &Path,
    shadow_path: &Path,
    reconciliation_path: &Path,
    active_pointer_path: &Path,
    receipt_path: &Path,
) -> Result<PromotionPublication> {
    if receipt_path.exists() {
        bail!(
            "refusing to overwrite promotion receipt {}",
            receipt_path.display()
        );
    }
    let model = read_model(model_path)?;
    let phase_8: QualificationInput = read_json(phase_8_path, "Phase 8 receipt")?;
    let phase_9: QualificationInput = read_json(phase_9_path, "Phase 9 receipt")?;
    validate_qualification(&phase_8, QUALITY_CONTRACT, "Phase 8", &model.model_identity)?;
    validate_qualification(
        &phase_9,
        PERFORMANCE_CONTRACT,
        "Phase 9",
        &model.model_identity,
    )?;
    let shadow: ShadowInput = read_json(shadow_path, "shadow receipt")?;
    if shadow.contract != SHADOW_CONTRACT
        || !shadow.shadow_verified
        || shadow.returned_context_changed
        || shadow.model_identity != model.model_identity
    {
        bail!("promotion requires a verified same-model shadow receipt");
    }
    let reconciliation: ReconciliationInput =
        read_json(reconciliation_path, "shadow reconciliation")?;
    validate_reconciliation(&shadow, &reconciliation, shadow_path, &model.model_identity)?;

    let previous = read_previous_pointer(active_pointer_path);
    let pointer = ActiveModelPointer {
        contract: ACTIVE_CONTRACT.to_owned(),
        engine: "v3".to_owned(),
        model_artifact: file_identity(model_path)?.into(),
        model_identity: model.model_identity,
        rollback_model_identity: model.rollback_model_identity,
        phase_8_receipt: file_identity(phase_8_path)?.into(),
        phase_9_receipt: file_identity(phase_9_path)?.into(),
        shadow_receipt: file_identity(shadow_path)?.into(),
        reconciliation: file_identity(reconciliation_path)?.into(),
        prior_active_model_identity: previous,
        promotion_receipt_path: receipt_path.display().to_string(),
        phase_10_verified: true,
    };
    publish_pointer_atomic(active_pointer_path, &pointer)?;
    let receipt = PromotionReceipt {
        contract: "phoenix.memory.qps-v3-promotion/v1",
        producer_binary: current_binary_identity()?,
        active_pointer: file_identity(active_pointer_path)?,
        model_identity: model.model_identity,
        rollback_model_identity: model.rollback_model_identity,
        disagreements_reviewed: shadow.disagreements.len(),
        verified_failures_recorded_in_ledger: reconciliation
            .reviews
            .iter()
            .filter(|review| review.disposition == ReviewDisposition::V2Better)
            .count(),
        prior_active_model_identity: pointer.prior_active_model_identity,
        phase_10_verified: true,
    };
    write_json_atomic(receipt_path, &receipt)?;
    Ok(PromotionPublication {
        contract: receipt.contract,
        output: file_identity(receipt_path)?,
        active_pointer: file_identity(active_pointer_path)?,
        model_identity: model.model_identity,
        phase_10_verified: true,
    })
}

fn evaluate_cohort(
    cohort_name: &'static str,
    cohort: &FrozenCohort,
    model: &LinearRankerV3,
    model_identity: &[u8; 32],
    disagreements: &mut Vec<ShadowDisagreement>,
) -> Result<()> {
    for query in &cohort.queries {
        let mut v2 = query.candidate_pool.iter().collect::<Vec<_>>();
        v2.sort_unstable_by_key(|candidate| candidate.v2_order);
        let mut v3 = query
            .candidate_pool
            .iter()
            .map(|candidate| {
                model
                    .score(candidate.rank_evidence_v3)
                    .map(|score| (candidate, score))
                    .context("invalid candidate evidence in shadow cohort")
            })
            .collect::<Result<Vec<_>>>()?;
        v3.sort_unstable_by(|(left, left_score), (right, right_score)| {
            left.relevance_tier
                .cmp(&right.relevance_tier)
                .then_with(|| right_score.total_cmp(left_score))
                .then_with(|| left.document_identity.cmp(&right.document_identity))
        });
        let v2_top = v2
            .iter()
            .take(10)
            .map(|candidate| candidate.document_identity.clone())
            .collect::<Vec<_>>();
        let v3_top = v3
            .iter()
            .take(10)
            .map(|(candidate, _)| candidate.document_identity.clone())
            .collect::<Vec<_>>();
        if v2_top == v3_top {
            continue;
        }
        let top_1_changed = v2_top.first() != v3_top.first();
        let maximum_rank_delta = maximum_rank_delta(&v2_top, &v3_top);
        let confidence_gap = if v3.len() >= 2 {
            (v3[0].1 - v3[1].1).abs()
        } else {
            0.0
        };
        let disagreement_identity = disagreement_identity(
            model_identity,
            cohort_name,
            &query.query_identity,
            &v2_top,
            &v3_top,
        );
        disagreements.push(ShadowDisagreement {
            disagreement_identity,
            cohort: cohort_name,
            query_identity: query.query_identity.clone(),
            candidate_count: query.candidate_pool.len(),
            v2_top_10: v2_top,
            v3_top_10: v3_top,
            top_1_changed,
            maximum_rank_delta,
            confidence_gap,
            review_state: "pending_explicit_review",
        });
    }
    Ok(())
}

fn maximum_rank_delta(v2: &[String], v3: &[String]) -> usize {
    v2.iter()
        .enumerate()
        .map(|(v2_rank, identity)| {
            let v3_rank = v3
                .iter()
                .position(|candidate| candidate == identity)
                .unwrap_or(10);
            v2_rank.abs_diff(v3_rank)
        })
        .max()
        .unwrap_or(0)
}

fn disagreement_identity(
    model_identity: &[u8; 32],
    cohort: &str,
    query: &str,
    v2: &[String],
    v3: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model_identity);
    for value in std::iter::once(cohort)
        .chain(std::iter::once(query))
        .chain(v2.iter().map(String::as_str))
        .chain(v3.iter().map(String::as_str))
    {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    hex(hasher.finalize().into())
}

fn validate_qualification(
    receipt: &QualificationInput,
    expected_contract: &str,
    label: &str,
    model_identity: &[u8; 32],
) -> Result<()> {
    let verified = match expected_contract {
        QUALITY_CONTRACT => receipt.phase_8_verified,
        PERFORMANCE_CONTRACT => receipt.phase_9_verified,
        _ => false,
    };
    if receipt.contract != expected_contract
        || !verified
        || &receipt.model_identity != model_identity
    {
        bail!("promotion requires a verified same-model {label} receipt");
    }
    Ok(())
}

fn validate_reconciliation(
    shadow: &ShadowInput,
    reconciliation: &ReconciliationInput,
    shadow_path: &Path,
    model_identity: &[u8; 32],
) -> Result<()> {
    let shadow_identity = file_identity(shadow_path)?;
    if reconciliation.contract != RECONCILIATION_CONTRACT
        || reconciliation.model_identity != *model_identity
        || reconciliation.shadow_receipt_sha256 != shadow_identity.sha256
        || reconciliation.reviews.len() != shadow.disagreements.len()
    {
        bail!("shadow reconciliation does not bind the complete same-model disagreement set");
    }
    let expected = shadow
        .disagreements
        .iter()
        .map(|item| item.disagreement_identity.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(reconciliation.reviews.len());
    for review in &reconciliation.reviews {
        if review.reviewer_identity.trim().is_empty()
            || !expected.contains(review.disagreement_identity.as_str())
            || !seen.insert(review.disagreement_identity.as_str())
            || (review.disposition == ReviewDisposition::V2Better
                && review.ledger_judgment_identities.is_empty())
        {
            bail!("invalid, duplicate, or unrecorded shadow review");
        }
    }
    Ok(())
}

fn read_model(path: &Path) -> Result<LinearModelArtifactV3> {
    let artifact: LinearModelArtifactV3 = read_json(path, "Phase 7 model artifact")?;
    if !artifact.validate_challenger() {
        bail!("invalid Phase 7 challenger artifact");
    }
    Ok(artifact)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("decode {label} {}", path.display()))
}

fn read_previous_pointer(path: &Path) -> Option<[u8; 32]> {
    serde_json::from_slice::<ActiveModelPointer>(&fs::read(path).ok()?)
        .ok()
        .filter(|pointer| pointer.contract == ACTIVE_CONTRACT && pointer.phase_10_verified)
        .map(|pointer| pointer.model_identity)
}

fn publish_pointer_atomic(path: &Path, pointer: &ActiveModelPointer) -> Result<()> {
    let parent = path.parent().context("active pointer has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("active pointer filename is not UTF-8")?;
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, pointer)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        if path.exists() {
            replace_file(path, &temporary)
        } else {
            move_new_file(path, &temporary)
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(destination: &Path, replacement: &Path) -> Result<()> {
    let destination_wide = wide_null(destination.as_os_str());
    let replacement_wide = wide_null(replacement.as_os_str());
    unsafe {
        ReplaceFileW(
            PCWSTR(destination_wide.as_ptr()),
            PCWSTR(replacement_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .with_context(|| {
        format!(
            "atomically replace active pointer {}",
            destination.display()
        )
    })
}

fn move_new_file(destination: &Path, replacement: &Path) -> Result<()> {
    let destination_wide = wide_null(destination.as_os_str());
    let replacement_wide = wide_null(replacement.as_os_str());
    unsafe {
        MoveFileExW(
            PCWSTR(replacement_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .with_context(|| {
        format!(
            "atomically publish active pointer {}",
            destination.display()
        )
    })
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[derive(Debug, Serialize)]
struct ShadowReceipt {
    contract: &'static str,
    model_artifact: FileIdentity,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    model_identity: [u8; 32],
    returned_engine: &'static str,
    returned_context_changed: bool,
    total_queries: usize,
    disagreement_count: usize,
    disagreements: Vec<ShadowDisagreement>,
    gates: ShadowGates,
    shadow_verified: bool,
}

#[derive(Debug, Serialize)]
struct ShadowDisagreement {
    disagreement_identity: String,
    cohort: &'static str,
    query_identity: String,
    candidate_count: usize,
    v2_top_10: Vec<String>,
    v3_top_10: Vec<String>,
    top_1_changed: bool,
    maximum_rank_delta: usize,
    confidence_gap: f32,
    review_state: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ShadowGates {
    same_frozen_candidate_pool: bool,
    v2_context_remained_authoritative: bool,
    every_disagreement_has_stable_identity: bool,
    high_impact_order_is_deterministic: bool,
}

#[derive(Debug, Serialize)]
pub struct ShadowPublication {
    contract: &'static str,
    output: FileIdentity,
    model_identity: [u8; 32],
    total_queries: usize,
    disagreement_count: usize,
    returned_context_changed: bool,
    shadow_verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActiveModelPointer {
    contract: String,
    engine: String,
    model_artifact: FileIdentityInput,
    model_identity: [u8; 32],
    rollback_model_identity: [u8; 32],
    phase_8_receipt: FileIdentityInput,
    phase_9_receipt: FileIdentityInput,
    shadow_receipt: FileIdentityInput,
    reconciliation: FileIdentityInput,
    prior_active_model_identity: Option<[u8; 32]>,
    promotion_receipt_path: String,
    phase_10_verified: bool,
}

impl From<FileIdentity> for FileIdentityInput {
    fn from(value: FileIdentity) -> Self {
        Self {
            path: value.path,
            bytes: value.bytes,
            sha256: value.sha256,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FileIdentityInput {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct PromotionReceipt {
    contract: &'static str,
    producer_binary: FileIdentity,
    active_pointer: FileIdentity,
    model_identity: [u8; 32],
    rollback_model_identity: [u8; 32],
    disagreements_reviewed: usize,
    verified_failures_recorded_in_ledger: usize,
    prior_active_model_identity: Option<[u8; 32]>,
    phase_10_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct PromotionPublication {
    contract: &'static str,
    output: FileIdentity,
    active_pointer: FileIdentity,
    model_identity: [u8; 32],
    phase_10_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    contract: String,
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    candidate_pool: Vec<FrozenCandidate>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    v2_order: usize,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[derive(Debug, Deserialize)]
struct QualificationInput {
    contract: String,
    model_identity: [u8; 32],
    #[serde(default)]
    phase_8_verified: bool,
    #[serde(default)]
    phase_9_verified: bool,
}

#[derive(Debug, Deserialize)]
struct ShadowInput {
    contract: String,
    model_identity: [u8; 32],
    returned_context_changed: bool,
    disagreements: Vec<ShadowDisagreementInput>,
    shadow_verified: bool,
}

#[derive(Debug, Deserialize)]
struct ShadowDisagreementInput {
    disagreement_identity: String,
}

#[derive(Debug, Deserialize)]
struct ReconciliationInput {
    contract: String,
    shadow_receipt_sha256: String,
    model_identity: [u8; 32],
    reviews: Vec<ShadowReview>,
}

#[derive(Debug, Deserialize)]
struct ShadowReview {
    disagreement_identity: String,
    disposition: ReviewDisposition,
    reviewer_identity: String,
    #[serde(default)]
    ledger_judgment_identities: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewDisposition {
    V3Better,
    V2Better,
    Equivalent,
    InvalidComparison,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_delta_accounts_for_top_ten_exits() {
        let v2 = vec!["a".to_owned(), "b".to_owned()];
        let v3 = vec!["b".to_owned(), "c".to_owned()];
        assert_eq!(maximum_rank_delta(&v2, &v3), 10);
    }

    #[test]
    fn disagreement_identity_is_deterministic_and_model_bound() {
        let v2 = vec!["a".to_owned()];
        let v3 = vec!["b".to_owned()];
        let first = disagreement_identity(&[1; 32], "cohort", "query", &v2, &v3);
        assert_eq!(
            first,
            disagreement_identity(&[1; 32], "cohort", "query", &v2, &v3)
        );
        assert_ne!(
            first,
            disagreement_identity(&[2; 32], "cohort", "query", &v2, &v3)
        );
    }
}
