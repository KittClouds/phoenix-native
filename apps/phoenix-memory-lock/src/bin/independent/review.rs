use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::HashSet;
use phoenix_lexical_qps::{
    JudgmentReasonV3, JudgmentSourceV3, PairwiseJudgmentDraftV3, PairwiseJudgmentV3,
    RelevanceLedgerV3, SplitGroupProvenanceV3,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DECISIONS_CONTRACT: &str = "phoenix.qps.relevance-review-decisions/v1";
const RECEIPT_CONTRACT: &str = "phoenix.qps.relevance-review-application/v1";

pub(crate) fn apply(
    ledger_path: &Path,
    decisions_path: &Path,
    output_path: &Path,
    receipt_path: &Path,
) -> Result<Publication> {
    for output in [output_path, receipt_path] {
        if output.exists() {
            bail!("refusing to overwrite {}", output.display());
        }
    }
    let mut ledger: RelevanceLedgerV3 = read_json(ledger_path, "source ledger")?;
    ledger.validate().map_err(anyhow::Error::msg)?;
    let decisions: ReviewDecisions = read_json(decisions_path, "review decisions")?;
    decisions.validate()?;

    let already_superseded = ledger
        .judgments
        .iter()
        .filter_map(|judgment| judgment.supersedes)
        .collect::<HashSet<_>>();
    let mut next_generation = ledger
        .judgments
        .iter()
        .map(|judgment| judgment.index_generation)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .context("ledger generation exhausted")?;
    let mut appended = 0_usize;
    let mut positive_confirmed = 0_usize;
    let mut preference_reversed = 0_usize;
    let mut reviewed_rows = Vec::with_capacity(decisions.decisions.len());
    for decision in &decisions.decisions {
        let source_index = ledger
            .judgments
            .iter()
            .position(|judgment| hex(judgment.identity.as_bytes()) == decision.judgment_identity)
            .with_context(|| {
                format!(
                    "review references unknown judgment {}",
                    decision.judgment_identity
                )
            })?;
        let source = ledger.judgments[source_index].clone();
        if source.source != JudgmentSourceV3::AutomaticallyMinedNegative
            || already_superseded.contains(&source.identity)
        {
            bail!(
                "review {} is not an active mined-negative candidate",
                decision.judgment_identity
            );
        }
        let reversed = decision.verdict == ReviewVerdict::NegativePreferred;
        let reviewed = PairwiseJudgmentV3::from_draft(reviewed_draft(
            &source,
            decision,
            next_generation,
            reversed,
        ));
        reviewed_rows.push(reviewed);
        next_generation = next_generation
            .checked_add(1)
            .context("ledger generation exhausted")?;
        appended += 1;
        positive_confirmed += usize::from(!reversed);
        preference_reversed += usize::from(reversed);
    }
    ledger
        .append_batch(reviewed_rows)
        .map_err(anyhow::Error::msg)?;
    ledger.validate().map_err(anyhow::Error::msg)?;
    write_json_atomic(output_path, &ledger)?;
    let receipt = ReviewApplicationReceipt {
        contract: RECEIPT_CONTRACT,
        schema_version: 1,
        source_ledger: file_identity(ledger_path)?,
        decisions: file_identity(decisions_path)?,
        output_ledger: file_identity(output_path)?,
        reviewer_identity: decisions.reviewer_identity,
        reviewed_at_unix_seconds: decisions.reviewed_at_unix_seconds,
        attestation: decisions.attestation,
        authorization_context: decisions.authorization_context,
        appended,
        positive_confirmed,
        preference_reversed,
        active_training_judgments: ledger.active_model_training_indices().len(),
        deterministic_round_trip: serde_json::from_slice::<RelevanceLedgerV3>(&fs::read(
            output_path,
        )?)? == ledger,
    };
    write_json_atomic(receipt_path, &receipt)?;
    Ok(Publication {
        contract: RECEIPT_CONTRACT,
        ledger: file_identity(output_path)?,
        receipt: file_identity(receipt_path)?,
        appended,
        active_training_judgments: receipt.active_training_judgments,
    })
}

fn reviewed_draft(
    source: &PairwiseJudgmentV3,
    decision: &ReviewDecision,
    generation: u64,
    reversed: bool,
) -> PairwiseJudgmentDraftV3 {
    let (positive_document_version, negative_document_version) = if reversed {
        (
            source.negative_document_version,
            source.positive_document_version,
        )
    } else {
        (
            source.positive_document_version,
            source.negative_document_version,
        )
    };
    let (positive_features, negative_features) = if reversed {
        (source.negative_features, source.positive_features)
    } else {
        (source.positive_features, source.negative_features)
    };
    let (positive_tier, negative_tier) = if reversed {
        (source.negative_tier, source.positive_tier)
    } else {
        (source.positive_tier, source.negative_tier)
    };
    let (positive_position, negative_position) = if reversed {
        (source.negative_position, source.positive_position)
    } else {
        (source.positive_position, source.negative_position)
    };
    PairwiseJudgmentDraftV3 {
        workspace_identity: source.workspace_identity,
        query_identity: source.query_identity,
        positive_document_version,
        negative_document_version,
        positive_features,
        negative_features,
        positive_tier,
        negative_tier,
        candidate_pool: source.candidate_pool.clone(),
        positive_position,
        negative_position,
        split_groups: if reversed {
            reversed_groups(source.split_groups)
        } else {
            source.split_groups
        },
        frozen_holdout: source.frozen_holdout,
        v2_model_identity: source.v2_model_identity,
        challenger_model_identity: source.challenger_model_identity,
        reason: decision.reason,
        source: decision.source,
        confidence: decision.confidence,
        weight: source_weight(decision.source),
        index_generation: generation,
        supersedes: Some(source.identity),
        contradicts: if reversed {
            vec![source.identity].into_boxed_slice()
        } else {
            Box::new([])
        },
    }
}

fn reversed_groups(groups: SplitGroupProvenanceV3) -> SplitGroupProvenanceV3 {
    SplitGroupProvenanceV3 {
        query_family_identity: groups.query_family_identity,
        positive_source_identity: groups.negative_source_identity,
        negative_source_identity: groups.positive_source_identity,
        positive_near_duplicate_cluster_identity: groups.negative_near_duplicate_cluster_identity,
        negative_near_duplicate_cluster_identity: groups.positive_near_duplicate_cluster_identity,
        entity_or_identifier_family_identity: groups.entity_or_identifier_family_identity,
        collection_cohort_identity: groups.collection_cohort_identity,
        collected_at_unix_seconds: groups.collected_at_unix_seconds,
    }
}

fn source_weight(source: JudgmentSourceV3) -> f32 {
    match source {
        JudgmentSourceV3::ExplicitUserCorrection => 2.0,
        JudgmentSourceV3::CuratedRegressionCase => 1.5,
        _ => unreachable!("validated review source"),
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct ReviewDecisions {
    pub(super) contract: String,
    pub(super) schema_version: u16,
    pub(super) reviewer_identity: String,
    pub(super) reviewed_at_unix_seconds: u64,
    pub(super) attestation: ReviewAttestation,
    pub(super) authorization_context: Option<String>,
    pub(super) decisions: Vec<ReviewDecision>,
}

impl ReviewDecisions {
    fn validate(&self) -> Result<()> {
        if self.contract != DECISIONS_CONTRACT
            || self.schema_version != 1
            || self.reviewer_identity.trim().is_empty()
            || self.reviewed_at_unix_seconds == 0
            || self.decisions.is_empty()
        {
            bail!("invalid or unattested review decisions");
        }
        if self.attestation == ReviewAttestation::AgentCuratedWithUserAuthorization
            && self
                .authorization_context
                .as_deref()
                .is_none_or(|context| context.trim().is_empty())
        {
            bail!("agent curation requires a user-authorization context");
        }
        let mut identities = HashSet::with_capacity(self.decisions.len());
        for decision in &self.decisions {
            if decision.judgment_identity.len() != 64
                || !identities.insert(decision.judgment_identity.as_str())
                || !matches!(
                    decision.source,
                    JudgmentSourceV3::ExplicitUserCorrection
                        | JudgmentSourceV3::CuratedRegressionCase
                )
                || !decision.confidence.is_finite()
                || !(0.5..=1.0).contains(&decision.confidence)
                || (self.attestation == ReviewAttestation::AgentCuratedWithUserAuthorization
                    && decision.source != JudgmentSourceV3::CuratedRegressionCase)
            {
                bail!("invalid review decision");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ReviewDecision {
    pub(super) judgment_identity: String,
    pub(super) verdict: ReviewVerdict,
    pub(super) reason: JudgmentReasonV3,
    pub(super) source: JudgmentSourceV3,
    pub(super) confidence: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewVerdict {
    PositivePreferred,
    NegativePreferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewAttestation {
    HumanReviewed,
    AgentCuratedWithUserAuthorization,
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    ledger: FileIdentity,
    receipt: FileIdentity,
    appended: usize,
    active_training_judgments: usize,
}

#[derive(Debug, Serialize)]
struct ReviewApplicationReceipt {
    contract: &'static str,
    schema_version: u16,
    source_ledger: FileIdentity,
    decisions: FileIdentity,
    output_ledger: FileIdentity,
    reviewer_identity: String,
    reviewed_at_unix_seconds: u64,
    attestation: ReviewAttestation,
    authorization_context: Option<String>,
    appended: usize,
    positive_confirmed: usize,
    preference_reversed: usize,
    active_training_judgments: usize,
    deterministic_round_trip: bool,
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

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
