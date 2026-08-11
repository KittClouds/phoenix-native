use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DECISIONS_CONTRACT: &str = "phoenix.qps.relevance-review-decisions/v1";
const RECEIPT_CONTRACT: &str = "phoenix.qps.audited-decision-finalization/v1";
const FINAL_REVIEWER: &str = "phoenix-qps-v3-independent-agent-audit-v1";
const AUTHORIZATION: &str = "User explicitly authorized continued grouped semantic curation through the QPS V3 promotion corpus and canonical Phase 6-8 chain.";

pub(crate) fn finalize(
    cut_a_path: &Path,
    cut_b_path: &Path,
    audit_a_path: &Path,
    audit_b_path: &Path,
    audit_c_path: &Path,
    output_path: &Path,
    receipt_path: &Path,
) -> Result<Publication> {
    refuse_overwrite(output_path)?;
    refuse_overwrite(receipt_path)?;

    let cuts = [
        read_json::<DecisionCut>(cut_a_path, "decision cut A")?,
        read_json::<DecisionCut>(cut_b_path, "decision cut B")?,
    ];
    for cut in &cuts {
        validate_cut(cut)?;
    }
    let source_files = [file_identity(cut_a_path)?, file_identity(cut_b_path)?];
    let audit_paths = [audit_a_path, audit_b_path, audit_c_path];
    let mut audit_files = Vec::with_capacity(audit_paths.len());
    let mut reviewers = BTreeSet::new();
    let mut slices = Vec::with_capacity(6);
    for path in audit_paths {
        let fragment = read_json::<AuditFragment>(path, "exhaustive review audit fragment")?;
        if fragment.reviewer_identity.trim().is_empty()
            || !reviewers.insert(fragment.reviewer_identity)
            || fragment.slices.is_empty()
        {
            bail!("invalid or duplicate exhaustive audit fragment");
        }
        slices.extend(fragment.slices);
        audit_files.push(file_identity(path)?);
    }
    slices.sort_unstable_by_key(|slice| slice.start_index);

    let decisions = cuts
        .into_iter()
        .flat_map(|cut| cut.decisions)
        .collect::<Vec<_>>();
    validate_unique_decisions(&decisions)?;
    let rejected = validate_slices(&slices, &decisions)?;
    let admitted = decisions
        .into_iter()
        .filter(|decision| !rejected.contains(&decision.judgment_identity))
        .collect::<Vec<_>>();
    let reviewed_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();

    let output = DecisionCut {
        contract: DECISIONS_CONTRACT.to_owned(),
        schema_version: 1,
        reviewer_identity: FINAL_REVIEWER.to_owned(),
        reviewed_at_unix_seconds,
        attestation: "agent_curated_with_user_authorization".to_owned(),
        authorization_context: Some(AUTHORIZATION.to_owned()),
        decisions: admitted,
    };
    write_json_create_only(output_path, &output)?;
    let output_file = file_identity(output_path)?;
    let receipt = FinalizationReceipt {
        contract: RECEIPT_CONTRACT,
        schema_version: 1,
        sources: source_files,
        audits: audit_files,
        reviewed_decisions: output.decisions.len() + rejected.len(),
        admitted_decisions: output.decisions.len(),
        rejected_decisions: rejected.len(),
        exhaustive_coverage_verified: true,
        output: output_file.clone(),
    };
    write_json_create_only(receipt_path, &receipt)?;
    Ok(Publication {
        contract: RECEIPT_CONTRACT,
        output: output_file,
        reviewed_decisions: receipt.reviewed_decisions,
        admitted_decisions: receipt.admitted_decisions,
        rejected_decisions: receipt.rejected_decisions,
        exhaustive_coverage_verified: true,
    })
}

fn validate_cut(cut: &DecisionCut) -> Result<()> {
    if cut.contract != DECISIONS_CONTRACT
        || cut.schema_version != 1
        || cut.reviewer_identity.trim().is_empty()
        || cut.attestation != "agent_curated_with_user_authorization"
        || cut
            .authorization_context
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        || cut.decisions.is_empty()
    {
        bail!("invalid source decision cut");
    }
    Ok(())
}

fn validate_unique_decisions(decisions: &[ReviewDecision]) -> Result<()> {
    let mut identities = BTreeSet::new();
    for decision in decisions {
        if !is_sha256(&decision.judgment_identity)
            || !identities.insert(decision.judgment_identity.as_str())
            || !matches!(
                decision.verdict.as_str(),
                "positive_preferred" | "negative_preferred"
            )
            || decision.reason.trim().is_empty()
            || decision.source != "curated_regression_case"
            || !(0.5..=1.0).contains(&decision.confidence)
            || !decision.confidence.is_finite()
        {
            bail!("invalid or duplicate source decision");
        }
    }
    Ok(())
}

fn validate_slices(
    slices: &[AuditSlice],
    decisions: &[ReviewDecision],
) -> Result<BTreeSet<String>> {
    if slices.is_empty() {
        bail!("exhaustive audit contains no slices");
    }
    let mut next_index = 0usize;
    let mut rejected = BTreeSet::new();
    let mut admitted_total = 0usize;
    for slice in slices {
        let span = slice
            .end_index_inclusive
            .checked_sub(slice.start_index)
            .and_then(|value| value.checked_add(1))
            .context("invalid audit slice range")?;
        if slice.start_index != next_index
            || slice.reviewed_count != span
            || slice.admitted_count + slice.rejected_count != span
            || slice.rejected_count != slice.rejected.len()
        {
            bail!("incomplete or inconsistent audit slice");
        }
        for rejection in &slice.rejected {
            if !(slice.start_index..=slice.end_index_inclusive).contains(&rejection.index)
                || decisions.get(rejection.index).is_none_or(|decision| {
                    decision.judgment_identity != rejection.judgment_identity
                })
                || rejection.reason.trim().is_empty()
                || !rejected.insert(rejection.judgment_identity.clone())
            {
                bail!("invalid, duplicate, or misplaced audit rejection");
            }
        }
        admitted_total += slice.admitted_count;
        next_index = slice.end_index_inclusive + 1;
    }
    if next_index != decisions.len() || admitted_total + rejected.len() != decisions.len() {
        bail!("audit slices do not exhaustively cover source decisions");
    }
    Ok(rejected)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_reader(BufReader::new(
        File::open(path).with_context(|| format!("open {label} {}", path.display()))?,
    ))
    .with_context(|| format!("decode {label} {}", path.display()))
}

fn refuse_overwrite(path: &Path) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    Ok(())
}

fn write_json_create_only(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    let temporary = temporary_path(path);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("create temporary output {}", temporary.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value)
        .with_context(|| format!("encode output {}", path.display()))?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    fs::rename(&temporary, path)
        .with_context(|| format!("publish create-only output {}", path.display()))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(FileIdentity {
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        sha256: hex(&Sha256::digest(&bytes)),
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Debug, Deserialize, Serialize)]
struct DecisionCut {
    contract: String,
    schema_version: u32,
    reviewer_identity: String,
    reviewed_at_unix_seconds: u64,
    attestation: String,
    authorization_context: Option<String>,
    decisions: Vec<ReviewDecision>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReviewDecision {
    judgment_identity: String,
    verdict: String,
    reason: String,
    source: String,
    confidence: f64,
}

#[derive(Debug, Deserialize)]
struct AuditFragment {
    reviewer_identity: String,
    slices: Vec<AuditSlice>,
}

#[derive(Debug, Deserialize)]
struct AuditSlice {
    start_index: usize,
    end_index_inclusive: usize,
    reviewed_count: usize,
    admitted_count: usize,
    rejected_count: usize,
    rejected: Vec<AuditRejection>,
}

#[derive(Debug, Deserialize)]
struct AuditRejection {
    index: usize,
    judgment_identity: String,
    reason: String,
}

#[derive(Clone, Debug, Serialize)]
struct FileIdentity {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct FinalizationReceipt {
    contract: &'static str,
    schema_version: u32,
    sources: [FileIdentity; 2],
    audits: Vec<FileIdentity>,
    reviewed_decisions: usize,
    admitted_decisions: usize,
    rejected_decisions: usize,
    exhaustive_coverage_verified: bool,
    output: FileIdentity,
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    output: FileIdentity,
    reviewed_decisions: usize,
    admitted_decisions: usize,
    rejected_decisions: usize,
    exhaustive_coverage_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(index: usize) -> ReviewDecision {
        ReviewDecision {
            judgment_identity: format!("{index:064x}"),
            verdict: "positive_preferred".to_owned(),
            reason: "phrase_order_failure".to_owned(),
            source: "curated_regression_case".to_owned(),
            confidence: 1.0,
        }
    }

    fn rejection(index: usize) -> AuditRejection {
        AuditRejection {
            index,
            judgment_identity: format!("{index:064x}"),
            reason: "unsupported atomic answer component".to_owned(),
        }
    }

    #[test]
    fn slices_require_exhaustive_contiguous_coverage() {
        let decisions = (0..6).map(decision).collect::<Vec<_>>();
        let slices = vec![
            AuditSlice {
                start_index: 0,
                end_index_inclusive: 2,
                reviewed_count: 3,
                admitted_count: 2,
                rejected_count: 1,
                rejected: vec![rejection(1)],
            },
            AuditSlice {
                start_index: 3,
                end_index_inclusive: 5,
                reviewed_count: 3,
                admitted_count: 2,
                rejected_count: 1,
                rejected: vec![rejection(4)],
            },
        ];
        let rejected = validate_slices(&slices, &decisions).expect("valid exhaustive slices");
        assert_eq!(rejected.len(), 2);
        assert!(rejected.contains(&format!("{:064x}", 1)));
        assert!(rejected.contains(&format!("{:064x}", 4)));
    }

    #[test]
    fn slices_reject_gaps_and_misplaced_ids() {
        let decisions = (0..4).map(decision).collect::<Vec<_>>();
        let gap = vec![AuditSlice {
            start_index: 1,
            end_index_inclusive: 3,
            reviewed_count: 3,
            admitted_count: 3,
            rejected_count: 0,
            rejected: Vec::new(),
        }];
        assert!(validate_slices(&gap, &decisions).is_err());

        let misplaced = vec![AuditSlice {
            start_index: 0,
            end_index_inclusive: 3,
            reviewed_count: 4,
            admitted_count: 3,
            rejected_count: 1,
            rejected: vec![AuditRejection {
                index: 2,
                judgment_identity: format!("{:064x}", 3),
                reason: "wrong index".to_owned(),
            }],
        }];
        assert!(validate_slices(&misplaced, &decisions).is_err());
    }
}
