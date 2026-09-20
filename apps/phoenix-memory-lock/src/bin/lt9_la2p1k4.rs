//! LT9-LA2-P1K4: frozen categorical tie resolver qualification.
//!
//! This postprocessor consumes an unchanged P1K2 receipt from an unopened
//! corpus. Plurality remains the primary assignment rule. Only an exact tie
//! may enter the preregistered unique-endpoint agreement resolver:
//!
//!   unique(nomination) + tied(witness containing it) => that family
//!   unique(witness) + tied(nomination containing it) => that family
//!   all other ties => abstain
//!
//! No numeric fitting, learner updates, authority changes, ranking changes, or
//! serving changes occur here. Qualification is episode-primary because two
//! directional rows can arise from one document-pair episode.
#![allow(clippy::type_complexity)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1k4/v1";
const MIN_VALID: usize = 20;
const MIN_INVALID: usize = 5;
const MIN_INVALID_SHARDS: usize = 2;
const MAX_EPISODE_VALID_LOSS: f64 = 0.10;
const MAX_EPISODE_NEW_TIE: f64 = 0.10;

#[derive(Clone, Copy)]
struct Candidate {
    id: &'static str,
    expected_phi: i8,
}

const CANDIDATES: [Candidate; 12] = [
    Candidate {
        id: "repair_to_fix",
        expected_phi: -1,
    },
    Candidate {
        id: "engine_to_motor",
        expected_phi: 2,
    },
    Candidate {
        id: "car_to_vehicle",
        expected_phi: 2,
    },
    Candidate {
        id: "vehicle_to_car",
        expected_phi: 2,
    },
    Candidate {
        id: "bank_to_shore",
        expected_phi: 1,
    },
    Candidate {
        id: "bank_to_lender",
        expected_phi: 0,
    },
    Candidate {
        id: "economic_to_tumor",
        expected_phi: -1,
    },
    Candidate {
        id: "loan_to_debt",
        expected_phi: 0,
    },
    Candidate {
        id: "credit_to_loan",
        expected_phi: 0,
    },
    Candidate {
        id: "insurance_to_coverage",
        expected_phi: 0,
    },
    Candidate {
        id: "stock_to_bond",
        expected_phi: 0,
    },
    Candidate {
        id: "bank_to_water",
        expected_phi: 1,
    },
];

#[derive(Deserialize)]
struct K2Receipt {
    schema: String,
    corpus_sha256: String,
    pairs: Vec<K2Pair>,
}

#[derive(Deserialize)]
struct K2Pair {
    shard: usize,
    episode_id: String,
    candidate: String,
    current_outcome: String,
    plurality_outcome: String,
    nomination: Endpoint,
    witness: Endpoint,
}

#[derive(Deserialize)]
struct Endpoint {
    counts: [u16; 3],
}

#[derive(Default)]
struct EpisodeState {
    current_valid: usize,
    current_invalid: usize,
    resolved_valid: usize,
    resolved_invalid: usize,
    resolved_tie: usize,
}

#[derive(Default, Serialize)]
struct Summary {
    pairs_total: usize,
    actionable_rows_current: usize,
    current_valid_actionable: usize,
    current_invalid_actionable: usize,
    resolved_valid_actionable: usize,
    resolved_invalid_actionable: usize,
    resolved_tie_abstain_rows: usize,
    invalid_actionable_rejected: usize,
    valid_actionable_loss: usize,
    new_tie_rows: usize,
    current_actionable_episodes: usize,
    current_valid_episodes: usize,
    current_invalid_episodes: usize,
    resolved_invalid_episodes: usize,
    invalid_episodes_repaired_as_valid: usize,
    invalid_episodes_abstaining: usize,
    valid_loss_episodes: usize,
    active_episodes_with_new_tie: usize,
    episode_valid_loss_fraction: f64,
    episode_new_tie_fraction: f64,
    invalid_shards_current: usize,
    current_sufficient: bool,
}

#[derive(Serialize)]
struct TieDecision {
    episode_id: String,
    shard: usize,
    candidate: String,
    expected_phi: i8,
    nomination_counts: [u16; 3],
    witness_counts: [u16; 3],
    nomination_tie_set: Vec<String>,
    witness_tie_set: Vec<String>,
    route: String,
    resolved_outcome: String,
    current_outcome: String,
    plurality_outcome: String,
}

#[derive(Serialize)]
struct Receipt<'a> {
    schema: &'static str,
    scope: &'static str,
    policy: &'static str,
    rule: &'static str,
    corpus_path: &'a str,
    source_receipt_sha256: String,
    source_corpus_sha256: &'a str,
    source_schema: &'a str,
    summary: Summary,
    gate: Gate,
    tie_decisions: Vec<TieDecision>,
    conclusion: &'static str,
}

#[derive(Serialize)]
struct Gate {
    status: &'static str,
    current_sufficient: bool,
    no_resolved_invalid_episodes: bool,
    episode_valid_loss_ok: bool,
    episode_new_tie_ok: bool,
    invalid_shards_ok: bool,
    reason: &'static str,
}

fn family_name(index: usize) -> &'static str {
    match index {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "unknown",
    }
}

fn expected_phi(candidate: &str) -> i8 {
    CANDIDATES
        .iter()
        .find(|spec| spec.id == candidate)
        .map(|spec| spec.expected_phi)
        .unwrap_or(-1)
}

fn tie_set(counts: [u16; 3]) -> Vec<usize> {
    let max = counts.iter().copied().max().unwrap_or(0);
    (0..3).filter(|&index| counts[index] == max).collect()
}

fn unique_endpoint_route(nomination: [u16; 3], witness: [u16; 3]) -> Option<usize> {
    let nomination_tie = tie_set(nomination);
    let witness_tie = tie_set(witness);
    if nomination_tie.len() == 1
        && witness_tie.len() == 2
        && witness_tie.contains(&nomination_tie[0])
    {
        return Some(nomination_tie[0]);
    }
    if witness_tie.len() == 1
        && nomination_tie.len() == 2
        && nomination_tie.contains(&witness_tie[0])
    {
        return Some(witness_tie[0]);
    }
    None
}

fn classify(expected: i8, route: Option<usize>) -> &'static str {
    match route {
        None => "ABSTAIN_TIE",
        Some(phi) if expected < 0 || expected == phi as i8 => "VALID_ACTIONABLE",
        Some(_) => "INVALID_ACTIONABLE",
    }
}

fn is_valid(outcome: &str) -> bool {
    outcome == "VALID_ACTIONABLE"
}

fn is_invalid(outcome: &str) -> bool {
    outcome == "INVALID_ACTIONABLE"
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 3,
        "usage: lt9_la2p1k4 <p1k2-receipt> <output>"
    );
    let source_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);
    let source_bytes =
        fs::read(source_path).with_context(|| format!("read {}", source_path.display()))?;
    let source: K2Receipt = serde_json::from_slice(&source_bytes).context("parse P1K2 receipt")?;
    ensure!(
        source.schema.contains("lt9la2p1k2"),
        "unexpected P1K2 schema: {}",
        source.schema
    );

    let mut summary = Summary {
        pairs_total: source.pairs.len(),
        ..Summary::default()
    };
    let mut episodes: BTreeMap<String, EpisodeState> = BTreeMap::new();
    let mut invalid_shards = BTreeSet::new();
    let mut tie_decisions = Vec::new();

    for pair in &source.pairs {
        let episode = episodes.entry(pair.episode_id.clone()).or_default();
        let current = pair.current_outcome.as_str();
        if is_valid(current) {
            summary.current_valid_actionable += 1;
            episode.current_valid += 1;
        } else if is_invalid(current) {
            summary.current_invalid_actionable += 1;
            episode.current_invalid += 1;
            invalid_shards.insert(pair.shard);
        }

        let resolved = if pair.plurality_outcome == "ABSTAIN_TIE" {
            let expected = expected_phi(&pair.candidate);
            let nomination_tie = tie_set(pair.nomination.counts);
            let witness_tie = tie_set(pair.witness.counts);
            let route = unique_endpoint_route(pair.nomination.counts, pair.witness.counts);
            let resolved_outcome = classify(expected, route);
            tie_decisions.push(TieDecision {
                episode_id: pair.episode_id.clone(),
                shard: pair.shard,
                candidate: pair.candidate.clone(),
                expected_phi: expected,
                nomination_counts: pair.nomination.counts,
                witness_counts: pair.witness.counts,
                nomination_tie_set: nomination_tie
                    .iter()
                    .map(|&i| family_name(i).to_owned())
                    .collect(),
                witness_tie_set: witness_tie
                    .iter()
                    .map(|&i| family_name(i).to_owned())
                    .collect(),
                route: route.map(family_name).unwrap_or("ABSTAIN").to_owned(),
                resolved_outcome: resolved_outcome.to_owned(),
                current_outcome: pair.current_outcome.clone(),
                plurality_outcome: pair.plurality_outcome.clone(),
            });
            resolved_outcome.to_owned()
        } else {
            pair.plurality_outcome.clone()
        };

        if is_valid(&resolved) {
            summary.resolved_valid_actionable += 1;
            episode.resolved_valid += 1;
        } else if is_invalid(&resolved) {
            summary.resolved_invalid_actionable += 1;
            episode.resolved_invalid += 1;
        } else if resolved == "ABSTAIN_TIE" {
            summary.resolved_tie_abstain_rows += 1;
            episode.resolved_tie += 1;
        }
    }

    summary.actionable_rows_current =
        summary.current_valid_actionable + summary.current_invalid_actionable;
    summary.invalid_actionable_rejected = summary
        .current_invalid_actionable
        .saturating_sub(summary.resolved_invalid_actionable);
    summary.valid_actionable_loss = summary
        .current_valid_actionable
        .saturating_sub(summary.resolved_valid_actionable);
    summary.new_tie_rows = summary.resolved_tie_abstain_rows;
    summary.current_actionable_episodes = episodes
        .values()
        .filter(|e| e.current_valid + e.current_invalid > 0)
        .count();
    summary.current_valid_episodes = episodes.values().filter(|e| e.current_valid > 0).count();
    summary.current_invalid_episodes = episodes.values().filter(|e| e.current_invalid > 0).count();
    summary.resolved_invalid_episodes =
        episodes.values().filter(|e| e.resolved_invalid > 0).count();
    summary.invalid_episodes_repaired_as_valid = episodes
        .values()
        .filter(|e| e.current_invalid > 0 && e.resolved_invalid == 0 && e.resolved_valid > 0)
        .count();
    summary.invalid_episodes_abstaining = episodes
        .values()
        .filter(|e| e.current_invalid > 0 && e.resolved_invalid == 0 && e.resolved_valid == 0)
        .count();
    summary.valid_loss_episodes = episodes
        .values()
        .filter(|e| e.current_valid > 0 && e.resolved_valid < e.current_valid)
        .count();
    summary.active_episodes_with_new_tie = episodes
        .values()
        .filter(|e| e.current_valid + e.current_invalid > 0 && e.resolved_tie > 0)
        .count();
    summary.episode_valid_loss_fraction = if summary.current_valid_episodes == 0 {
        0.0
    } else {
        summary.valid_loss_episodes as f64 / summary.current_valid_episodes as f64
    };
    summary.episode_new_tie_fraction = if summary.current_actionable_episodes == 0 {
        0.0
    } else {
        summary.active_episodes_with_new_tie as f64 / summary.current_actionable_episodes as f64
    };
    summary.invalid_shards_current = invalid_shards.len();
    summary.current_sufficient = summary.current_valid_actionable >= MIN_VALID
        && summary.current_invalid_actionable >= MIN_INVALID
        && summary.invalid_shards_current >= MIN_INVALID_SHARDS;

    let no_invalid = summary.resolved_invalid_episodes == 0;
    let valid_loss_ok = summary.episode_valid_loss_fraction <= MAX_EPISODE_VALID_LOSS;
    let new_tie_ok = summary.episode_new_tie_fraction <= MAX_EPISODE_NEW_TIE;
    let shards_ok = summary.invalid_shards_current >= MIN_INVALID_SHARDS;
    let qualified =
        summary.current_sufficient && no_invalid && valid_loss_ok && new_tie_ok && shards_ok;
    let current_sufficient = summary.current_sufficient;
    let status = if qualified {
        "PHENOTYPE_ASSIGNMENT_GATE_QUALIFIED"
    } else {
        "PHENOTYPE_ASSIGNMENT_GATE_NOT_QUALIFIED"
    };
    let reason = if qualified {
        "frozen unique-endpoint agreement passed the episode-primary routing gate"
    } else {
        "frozen tie resolver failed one or more predeclared episode-primary routing gates"
    };

    let receipt = Receipt {
        schema: SCHEMA,
        scope: "HotpotQA qualification of frozen unique-endpoint tie resolver; no learner, authority, ranking, or serving changes",
        policy: "unique_endpoint_agreement",
        rule: "plurality first; on a tie, route only when one endpoint has a unique family and the other endpoint's two-way tie contains it; otherwise abstain",
        corpus_path: "D:\\phoenix-evals\\beir\\screen-candidates\\hotpotqa\\corpus.jsonl",
        source_receipt_sha256: sha256_file(source_path)?,
        source_corpus_sha256: &source.corpus_sha256,
        source_schema: &source.schema,
        summary,
        gate: Gate { status, current_sufficient, no_resolved_invalid_episodes: no_invalid, episode_valid_loss_ok: valid_loss_ok, episode_new_tie_ok: new_tie_ok, invalid_shards_ok: shards_ok, reason },
        tie_decisions,
        conclusion: "P1K4_COMPLETE: tie resolver evaluated unchanged on unopened HotpotQA; no authority or serving promotion",
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output_path.display()))?;
    println!("P1K4 qualification receipt: {}", output_path.display());
    println!("pairs={} episodes={} current-invalid={} resolved-invalid-episodes={} valid-loss-episodes={} new-tie-episodes={} gate={}", receipt.summary.pairs_total, receipt.summary.current_actionable_episodes, receipt.summary.current_invalid_actionable, receipt.summary.resolved_invalid_episodes, receipt.summary.valid_loss_episodes, receipt.summary.active_episodes_with_new_tie, receipt.gate.status);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tie_set_is_deterministic() {
        assert_eq!(tie_set([1, 1, 0]), vec![0, 1]);
        assert_eq!(tie_set([0, 0, 0]), vec![0, 1, 2]);
        assert_eq!(tie_set([0, 2, 1]), vec![1]);
    }

    #[test]
    fn unique_endpoint_agreement_routes_only_supported_shape() {
        assert_eq!(unique_endpoint_route([0, 2, 0], [0, 1, 1]), Some(1));
        assert_eq!(unique_endpoint_route([0, 1, 1], [0, 2, 0]), Some(1));
        assert_eq!(unique_endpoint_route([1, 1, 0], [1, 1, 0]), None);
    }

    #[test]
    fn unresolved_tie_abstains() {
        assert_eq!(classify(2, None), "ABSTAIN_TIE");
        assert_eq!(classify(2, Some(2)), "VALID_ACTIONABLE");
        assert_eq!(classify(2, Some(1)), "INVALID_ACTIONABLE");
        assert_eq!(classify(-1, Some(0)), "VALID_ACTIONABLE");
    }
}
