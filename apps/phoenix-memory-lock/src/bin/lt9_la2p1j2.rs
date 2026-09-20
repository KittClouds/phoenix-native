//! LT9-LA2-P1J2: frozen within-family purity decision.
//!
//! This binary consumes the sealed P1J receipt only. It does not replay the
//! corpus, train a model, inspect retrieval quality, or alter learner state.
//! The rule family is deliberately small and descriptive: it asks whether the
//! two actionable invalid joins can be rejected using context evidence already
//! present in the frozen fourteen-join artifact.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1j2/v1";
const MAX_VALID_LOSS: usize = 2;

#[derive(Clone, Debug, Deserialize)]
struct FeatureDiff {
    class: String,
    candidate: String,
    witness_kind: String,
    nomination_runner_up: String,
    witness_runner_up: String,
    marker_mask_hamming: u32,
    side_mask_hamming: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct P1jReceipt {
    schema: String,
    feature_diffs: Vec<FeatureDiff>,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum RuleKind {
    SideDistanceAtMost2,
    MarkerDistanceAtMost1,
    MarkerAtMost1AndSideAtMost2,
    MarkerAtMost2AndSideAtMost2,
    RunnerUpAgrees,
    RunnerUpAgreesAndSideAtMost2,
    MarkerAtMost1SideAtMost2RunnerUpAgrees,
}

impl RuleKind {
    fn name(self) -> &'static str {
        match self {
            Self::SideDistanceAtMost2 => "side_mask_hamming <= 2",
            Self::MarkerDistanceAtMost1 => "marker_mask_hamming <= 1",
            Self::MarkerAtMost1AndSideAtMost2 => {
                "marker_mask_hamming <= 1 && side_mask_hamming <= 2"
            }
            Self::MarkerAtMost2AndSideAtMost2 => {
                "marker_mask_hamming <= 2 && side_mask_hamming <= 2"
            }
            Self::RunnerUpAgrees => "nomination_runner_up == witness_runner_up",
            Self::RunnerUpAgreesAndSideAtMost2 => "runner_up_agrees && side_mask_hamming <= 2",
            Self::MarkerAtMost1SideAtMost2RunnerUpAgrees => {
                "marker_mask_hamming <= 1 && side_mask_hamming <= 2 && runner_up_agrees"
            }
        }
    }

    fn terms(self) -> u8 {
        match self {
            Self::SideDistanceAtMost2 | Self::MarkerDistanceAtMost1 | Self::RunnerUpAgrees => 1,
            Self::MarkerAtMost1AndSideAtMost2
            | Self::MarkerAtMost2AndSideAtMost2
            | Self::RunnerUpAgreesAndSideAtMost2 => 2,
            Self::MarkerAtMost1SideAtMost2RunnerUpAgrees => 3,
        }
    }

    fn accepts(self, item: &FeatureDiff) -> bool {
        let runner_up_agrees = item.nomination_runner_up == item.witness_runner_up;
        match self {
            Self::SideDistanceAtMost2 => item.side_mask_hamming <= 2,
            Self::MarkerDistanceAtMost1 => item.marker_mask_hamming <= 1,
            Self::MarkerAtMost1AndSideAtMost2 => {
                item.marker_mask_hamming <= 1 && item.side_mask_hamming <= 2
            }
            Self::MarkerAtMost2AndSideAtMost2 => {
                item.marker_mask_hamming <= 2 && item.side_mask_hamming <= 2
            }
            Self::RunnerUpAgrees => runner_up_agrees,
            Self::RunnerUpAgreesAndSideAtMost2 => runner_up_agrees && item.side_mask_hamming <= 2,
            Self::MarkerAtMost1SideAtMost2RunnerUpAgrees => {
                runner_up_agrees && item.marker_mask_hamming <= 1 && item.side_mask_hamming <= 2
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RuleResult {
    rule: &'static str,
    terms: u8,
    valid_actionable_total: usize,
    valid_actionable_retained: usize,
    valid_actionable_loss: usize,
    invalid_actionable_total: usize,
    invalid_actionable_retained: usize,
    invalid_actionable_removed: usize,
    separates_all_invalid: bool,
    qualifies_under_frozen_gate: bool,
    rejected_invalid_ids: Vec<String>,
    rejected_valid_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Decision {
    status: &'static str,
    selected_rule: Option<&'static str>,
    selected_rule_terms: Option<u8>,
    selected_valid_loss: Option<usize>,
    selected_invalid_removed: Option<usize>,
    frozen_gate: &'static str,
    basis: &'static str,
    next_step: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    p1j_receipt_sha256: String,
    p1j_schema: String,
    frozen_join_count: usize,
    valid_actionable_count: usize,
    invalid_actionable_count: usize,
    invalid_abstain_count: usize,
    diagnostic_rules: Vec<RuleResult>,
    decision: Decision,
    conclusion: &'static str,
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_valid_actionable(item: &FeatureDiff) -> bool {
    item.class == "VALID_ACTIONABLE" && item.witness_kind != "ABSTAIN"
}

fn is_invalid_actionable(item: &FeatureDiff) -> bool {
    item.class == "INVALID" && item.witness_kind != "ABSTAIN"
}

fn evaluate(rule: RuleKind, items: &[FeatureDiff]) -> RuleResult {
    let valid: Vec<&FeatureDiff> = items
        .iter()
        .filter(|item| is_valid_actionable(item))
        .collect();
    let invalid: Vec<&FeatureDiff> = items
        .iter()
        .filter(|item| is_invalid_actionable(item))
        .collect();
    let rejected_valid_ids = valid
        .iter()
        .filter(|item| !rule.accepts(item))
        .map(|item| item.candidate.clone())
        .collect::<Vec<_>>();
    let rejected_invalid_ids = invalid
        .iter()
        .filter(|item| !rule.accepts(item))
        .map(|item| item.candidate.clone())
        .collect::<Vec<_>>();
    let invalid_removed = rejected_invalid_ids.len();
    RuleResult {
        rule: rule.name(),
        terms: rule.terms(),
        valid_actionable_total: valid.len(),
        valid_actionable_retained: valid.len() - rejected_valid_ids.len(),
        valid_actionable_loss: rejected_valid_ids.len(),
        invalid_actionable_total: invalid.len(),
        invalid_actionable_retained: invalid.len() - invalid_removed,
        invalid_actionable_removed: invalid_removed,
        separates_all_invalid: invalid_removed == invalid.len(),
        qualifies_under_frozen_gate: invalid_removed == invalid.len()
            && rejected_valid_ids.len() <= MAX_VALID_LOSS,
        rejected_invalid_ids,
        rejected_valid_ids,
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let p1j_path = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1j\\lt9-la2p1j-receipt.json".to_owned(),
        Clone::clone,
    );
    let output = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1j2\\lt9-la2p1j2-receipt.json".to_owned(),
        Clone::clone,
    );
    let bytes = fs::read(&p1j_path).with_context(|| format!("read {p1j_path}"))?;
    let p1j_hash = hex_digest(Sha256::digest(&bytes));
    let p1j: P1jReceipt = serde_json::from_slice(&bytes).context("decode P1J receipt")?;
    let same_join_count = p1j.feature_diffs.len();
    let valid_actionable_count = p1j
        .feature_diffs
        .iter()
        .filter(|item| is_valid_actionable(item))
        .count();
    let invalid_actionable_count = p1j
        .feature_diffs
        .iter()
        .filter(|item| is_invalid_actionable(item))
        .count();
    let invalid_abstain_count = p1j
        .feature_diffs
        .iter()
        .filter(|item| item.class == "INVALID" && item.witness_kind == "ABSTAIN")
        .count();
    let rules = [
        RuleKind::SideDistanceAtMost2,
        RuleKind::MarkerDistanceAtMost1,
        RuleKind::MarkerAtMost1AndSideAtMost2,
        RuleKind::MarkerAtMost2AndSideAtMost2,
        RuleKind::RunnerUpAgrees,
        RuleKind::RunnerUpAgreesAndSideAtMost2,
        RuleKind::MarkerAtMost1SideAtMost2RunnerUpAgrees,
    ];
    let diagnostic_rules = rules
        .iter()
        .copied()
        .map(|rule| evaluate(rule, &p1j.feature_diffs))
        .collect::<Vec<_>>();
    let selected = diagnostic_rules
        .iter()
        .filter(|result| result.qualifies_under_frozen_gate)
        .min_by_key(|result| (result.valid_actionable_loss, result.terms, result.rule));
    let decision = selected.map_or(Decision {
        status: "NO_PURITY_GUARD_AUTHORIZED",
        selected_rule: None,
        selected_rule_terms: None,
        selected_valid_loss: None,
        selected_invalid_removed: None,
        frozen_gate: "all actionable invalid joins must be rejected and valid actionable loss must be <= 2 of 10",
        basis: "no predeclared context-evidence rule met the frozen local purity gate",
        next_step: "open a phenotype-feature diagnostic; do not add a compatibility gate",
    }, |result| Decision {
        status: "PURITY_GUARD_EXPERIMENT_AUTHORIZED",
        selected_rule: Some(result.rule),
        selected_rule_terms: Some(result.terms),
        selected_valid_loss: Some(result.valid_actionable_loss),
        selected_invalid_removed: Some(result.invalid_actionable_removed),
        frozen_gate: "all actionable invalid joins must be rejected and valid actionable loss must be <= 2 of 10",
        basis: "a predeclared local context-evidence rule separates the actionable invalid joins on the frozen P1J artifact",
        next_step: "run a separately sealed narrow purity-guard experiment; do not promote this diagnostic to natural learning or serving",
    });
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-P1J2 frozen P1J fourteen-join diagnostic; no corpus replay, training, or downstream retrieval tuning",
        hypothesis: "the two actionable invalid joins are separable from valid actionable joins using existing within-family context evidence",
        p1j_receipt_sha256: p1j_hash,
        p1j_schema: p1j.schema,
        frozen_join_count: same_join_count,
        valid_actionable_count,
        invalid_actionable_count,
        invalid_abstain_count,
        diagnostic_rules,
        decision,
        conclusion: "P1J2_COMPLETE: the frozen local purity decision is diagnostic only; any authorized guard requires a separately sealed experiment",
    };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("schema={SCHEMA} joins={} valid_actionable={} invalid_actionable={} invalid_abstain={} decision={}", same_join_count, valid_actionable_count, invalid_actionable_count, invalid_abstain_count, receipt.decision.status);
    for result in &receipt.diagnostic_rules {
        println!(
            "rule={} valid_loss={} invalid_removed={} qualifies={}",
            result.rule,
            result.valid_actionable_loss,
            result.invalid_actionable_removed,
            result.qualifies_under_frozen_gate
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        class: &str,
        kind: &str,
        marker: u32,
        side: u32,
        left: &str,
        right: &str,
    ) -> FeatureDiff {
        FeatureDiff {
            class: class.to_owned(),
            candidate: "x".to_owned(),
            witness_kind: kind.to_owned(),
            nomination_runner_up: left.to_owned(),
            witness_runner_up: right.to_owned(),
            marker_mask_hamming: marker,
            side_mask_hamming: side,
        }
    }

    #[test]
    fn actionable_filter_excludes_invalid_abstain() {
        let items = [item("INVALID", "ABSTAIN", 3, 3, "finance", "transport")];
        assert_eq!(
            items
                .iter()
                .filter(|item| is_invalid_actionable(item))
                .count(),
            0
        );
    }

    #[test]
    fn runner_up_rule_rejects_conflict() {
        let good = item("VALID_ACTIONABLE", "SUPPORT", 3, 3, "finance", "finance");
        let bad = item("INVALID", "SUPPORT", 3, 3, "finance", "transport");
        assert!(RuleKind::RunnerUpAgrees.accepts(&good));
        assert!(!RuleKind::RunnerUpAgrees.accepts(&bad));
    }

    #[test]
    fn frozen_gate_requires_all_invalid_and_small_valid_loss() {
        let items = [
            item("VALID_ACTIONABLE", "SUPPORT", 0, 0, "finance", "finance"),
            item("VALID_ACTIONABLE", "SUPPORT", 3, 3, "finance", "finance"),
            item("INVALID", "SUPPORT", 3, 3, "finance", "transport"),
        ];
        let result = evaluate(RuleKind::RunnerUpAgrees, &items);
        assert!(result.qualifies_under_frozen_gate);
        assert_eq!(result.valid_actionable_loss, 0);
        assert_eq!(result.invalid_actionable_removed, 1);
    }
}
