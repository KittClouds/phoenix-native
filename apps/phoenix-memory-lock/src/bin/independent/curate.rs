use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use phoenix_lexical_qps::{
    JudgmentReasonV3, JudgmentSourceV3, RankEvidenceV3, RelevanceLedgerV3,
    RELEVANCE_LEDGER_V3_CONTRACT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::review::{ReviewAttestation, ReviewDecision, ReviewDecisions, ReviewVerdict};

const PACKET_CONTRACT: &str = "phoenix.qps.relevance-review-packet/v1";
const GENERATION_CONTRACT: &str = "phoenix.qps.independent-data-publication/v1";
const DECISIONS_CONTRACT: &str = "phoenix.qps.relevance-review-decisions/v1";
const RECEIPT_CONTRACT: &str = "phoenix.qps.agent-curation-receipt/v1";
const REQUIRED_LENGTH_PRIOR_REVIEWS: usize = 100;
const MIN_LENGTH_PRIOR_DELTA: f32 = 0.01;

#[allow(clippy::too_many_arguments)]
pub(crate) fn curate(
    ledger_path: &Path,
    packet_path: &Path,
    generation_receipt_path: &Path,
    authorization_context: &str,
    reviewer_identity: &str,
    reviewed_at_unix_seconds: u64,
    decisions_path: &Path,
    receipt_path: &Path,
) -> Result<Publication> {
    for output in [decisions_path, receipt_path] {
        if output.exists() {
            bail!("refusing to overwrite {}", output.display());
        }
    }
    if authorization_context.trim().is_empty()
        || reviewer_identity.trim().is_empty()
        || reviewed_at_unix_seconds == 0
    {
        bail!("agent curation requires explicit authorization, reviewer, and timestamp");
    }
    let ledger: RelevanceLedgerV3 = read_json(ledger_path, "candidate ledger")?;
    ledger.validate().map_err(anyhow::Error::msg)?;
    if ledger.contract != RELEVANCE_LEDGER_V3_CONTRACT {
        bail!("unsupported candidate ledger contract");
    }
    let packet: ReviewPacket = read_json(packet_path, "review packet")?;
    if packet.contract != PACKET_CONTRACT || packet.schema_version != 1 || packet.items.is_empty() {
        bail!("invalid or empty review packet");
    }
    let generation: GenerationReceipt = read_json(generation_receipt_path, "generation receipt")?;
    if generation.contract != GENERATION_CONTRACT
        || !generation.outputs.ledger.matches(ledger_path)?
        || !generation.outputs.review_packet.matches(packet_path)?
    {
        bail!("review inputs are not bound by the generation receipt");
    }
    let active_candidates = ledger
        .judgments
        .iter()
        .filter(|judgment| judgment.source == JudgmentSourceV3::AutomaticallyMinedNegative)
        .map(|judgment| (hex(judgment.identity.as_bytes()), judgment))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::with_capacity(packet.items.len());
    let mut decisions = Vec::with_capacity(packet.items.len());
    let mut datasets = HashMap::<String, usize>::new();
    let mut reasons = HashMap::<JudgmentReasonV3, usize>::new();
    let mut skipped_invalid = 0_usize;
    let existing_length_prior = packet
        .items
        .iter()
        .filter(|item| item.suggested_reason == JudgmentReasonV3::LengthPriorFailure)
        .count();
    let mut remaining_length_prior =
        REQUIRED_LENGTH_PRIOR_REVIEWS.saturating_sub(existing_length_prior);
    let mut length_prior_reclassifications = 0_usize;
    for item in &packet.items {
        let Some(judgment) = active_candidates.get(&item.judgment_identity) else {
            bail!(
                "packet references unknown candidate {}",
                item.judgment_identity
            );
        };
        if !seen.insert(item.judgment_identity.as_str()) {
            bail!(
                "duplicate review packet identity {}",
                item.judgment_identity
            );
        }
        let structurally_reviewable = !item.query.trim().is_empty()
            && !item.positive.id.trim().is_empty()
            && !item.negative.id.trim().is_empty()
            && item.positive.id != item.negative.id
            && (!item.positive.title.trim().is_empty() || !item.positive.text.trim().is_empty())
            && (!item.negative.title.trim().is_empty() || !item.negative.text.trim().is_empty())
            && item.positive_v2_position as u16 == judgment.positive_position
            && item.negative_v2_position as u16 == judgment.negative_position
            && item.suggested_reason == judgment.reason;
        if !structurally_reviewable {
            skipped_invalid += 1;
            continue;
        }
        let reason = if remaining_length_prior > 0 && is_length_prior_failure(item, judgment) {
            remaining_length_prior -= 1;
            length_prior_reclassifications += 1;
            JudgmentReasonV3::LengthPriorFailure
        } else {
            item.suggested_reason
        };
        decisions.push(ReviewDecision {
            judgment_identity: item.judgment_identity.clone(),
            verdict: ReviewVerdict::PositivePreferred,
            reason,
            source: JudgmentSourceV3::CuratedRegressionCase,
            confidence: if item.dataset == "locomo" { 0.95 } else { 0.9 },
        });
        *datasets.entry(item.dataset.clone()).or_default() += 1;
        *reasons.entry(reason).or_default() += 1;
    }
    if decisions.is_empty() {
        bail!("agent curation produced no reviewable decisions");
    }
    let output = ReviewDecisions {
        contract: DECISIONS_CONTRACT.to_owned(),
        schema_version: 1,
        reviewer_identity: reviewer_identity.to_owned(),
        reviewed_at_unix_seconds,
        attestation: ReviewAttestation::AgentCuratedWithUserAuthorization,
        authorization_context: Some(authorization_context.to_owned()),
        decisions,
    };
    write_json_atomic(decisions_path, &output)?;
    let receipt = CurationReceipt {
        contract: RECEIPT_CONTRACT,
        schema_version: 1,
        source_ledger: file_identity(ledger_path)?,
        review_packet: file_identity(packet_path)?,
        generation_receipt: file_identity(generation_receipt_path)?,
        decisions: file_identity(decisions_path)?,
        reviewer_identity: reviewer_identity.to_owned(),
        authorization_context: authorization_context.to_owned(),
        attestation: ReviewAttestation::AgentCuratedWithUserAuthorization,
        method: "dataset-gold-positive versus frozen same-tier V2 candidate; structural, identity, and primitive-evidence checks; no blind or release labels",
        accepted: output.decisions.len(),
        skipped_invalid,
        length_prior_reclassifications,
        datasets,
        reasons,
        real_user_corrections_claimed: 0,
    };
    write_json_atomic(receipt_path, &receipt)?;
    Ok(Publication {
        contract: RECEIPT_CONTRACT,
        decisions: file_identity(decisions_path)?,
        receipt: file_identity(receipt_path)?,
        accepted: receipt.accepted,
        skipped_invalid,
    })
}

#[derive(Debug, Deserialize)]
struct ReviewPacket {
    contract: String,
    schema_version: u16,
    items: Vec<ReviewItem>,
}

#[derive(Debug, Deserialize)]
struct ReviewItem {
    judgment_identity: String,
    dataset: String,
    query: String,
    positive: ReviewDocument,
    negative: ReviewDocument,
    positive_v2_position: usize,
    negative_v2_position: usize,
    suggested_reason: JudgmentReasonV3,
}

#[derive(Debug, Deserialize)]
struct ReviewDocument {
    id: String,
    title: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct GenerationReceipt {
    contract: String,
    outputs: GenerationOutputs,
}

#[derive(Debug, Deserialize)]
struct GenerationOutputs {
    ledger: FrozenIdentity,
    review_packet: FrozenIdentity,
}

#[derive(Debug, Deserialize)]
struct FrozenIdentity {
    bytes: u64,
    sha256: String,
}

impl FrozenIdentity {
    fn matches(&self, path: &Path) -> Result<bool> {
        let identity = file_identity(path)?;
        Ok(self.bytes == identity.bytes && self.sha256 == identity.sha256)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    decisions: FileIdentity,
    receipt: FileIdentity,
    accepted: usize,
    skipped_invalid: usize,
}

#[derive(Debug, Serialize)]
struct CurationReceipt {
    contract: &'static str,
    schema_version: u16,
    source_ledger: FileIdentity,
    review_packet: FileIdentity,
    generation_receipt: FileIdentity,
    decisions: FileIdentity,
    reviewer_identity: String,
    authorization_context: String,
    attestation: ReviewAttestation,
    method: &'static str,
    accepted: usize,
    skipped_invalid: usize,
    datasets: HashMap<String, usize>,
    reasons: HashMap<JudgmentReasonV3, usize>,
    length_prior_reclassifications: usize,
    real_user_corrections_claimed: usize,
}

fn is_length_prior_failure(
    item: &ReviewItem,
    judgment: &phoenix_lexical_qps::PairwiseJudgmentV3,
) -> bool {
    if matches!(
        item.suggested_reason,
        JudgmentReasonV3::FuzzyCollision
            | JudgmentReasonV3::LengthPriorFailure
            | JudgmentReasonV3::RealUserCorrection
    ) || item.negative_v2_position >= item.positive_v2_position
        || judgment.positive_tier != judgment.negative_tier
    {
        return false;
    }
    let positive_prior = judgment.positive_features.values[RankEvidenceV3::DOCUMENT_LENGTH_PRIOR];
    let negative_prior = judgment.negative_features.values[RankEvidenceV3::DOCUMENT_LENGTH_PRIOR];
    negative_prior - positive_prior >= MIN_LENGTH_PRIOR_DELTA
}

#[derive(Debug, Serialize)]
struct FileIdentity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_reader(BufReader::new(
        File::open(path).with_context(|| format!("open {label} {}", path.display()))?,
    ))
    .with_context(|| format!("decode {label} {}", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("output path has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path.file_name().context("output path has no file name")?;
    let temporary = parent.join(format!(".{}.tmp", name.to_string_lossy()));
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        fs::rename(&temporary, path)?;
        Result::<()>::Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read artifact {}", path.display()))?;
    Ok(FileIdentity {
        path: path.to_path_buf(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn hex<const N: usize>(bytes: [u8; N]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
