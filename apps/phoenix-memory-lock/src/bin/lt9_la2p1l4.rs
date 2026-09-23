//! LT9-LA2-P1L4: tie-resolver necessity and hard-abstention replay.
//!
//! Screen reads corpus text only. Replay uses frozen candidate context classes
//! for post-hoc route diagnostics; it never opens BEIR queries or qrels.

#[allow(dead_code)]
mod lt9_la2p1l4_core;

use anyhow::{ensure, Context, Result};
use lt9_la2p1l4_core::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SCREEN_SCHEMA: &str = "phoenix.lexical.lt9-la2p1l4-screen/v1";
const REPLAY_SCHEMA: &str = "phoenix.lexical.lt9-la2p1l4-replay/v1";
const SCREEN_ORDER: [&str; 5] = ["fever", "msmarco", "cqadupstack", "nq", "climate-fever"];
const MIN_TIE_RESOLVED: usize = 40;
const MIN_PER_RELATION: usize = 10;
const MIN_RELATIONS: usize = 2;
const MIN_SHARDS: usize = 3;
const SHARDS: usize = 8;
const MIN_VALID_TIE_ACTIONABLE: usize = 20;
const MIN_INVALID_TIE_ACTIONABLE: usize = 5;
const MIN_INVALID_TIE_SHARDS: usize = 2;
const MAX_VALID_LOSS: f64 = 0.10;
const MAX_NEW_ABSTENTION: f64 = 0.10;

#[derive(Serialize)]
struct ScreenRow {
    corpus_id: String,
    corpus_path: String,
    corpus_sha256: String,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    tie_resolved_episode_count: usize,
    tie_resolved_by_candidate: BTreeMap<String, usize>,
    tie_resolved_by_shard: [usize; SHARDS],
    candidate_relation_count: usize,
    shard_count: usize,
    eligible: bool,
    assigned_role: &'static str,
}

#[derive(Serialize)]
struct ScreenReceipt {
    schema: &'static str,
    date: &'static str,
    scope: &'static str,
    corpus_text_only: bool,
    qrels_or_validity_labels_opened: bool,
    frozen_order: Vec<&'static str>,
    min_tie_resolved_episodes: usize,
    min_tie_resolved_per_relation: usize,
    min_candidate_relations: usize,
    min_document_shards: usize,
    discovery_corpus: Option<String>,
    qualification_corpus: Option<String>,
    corpora: Vec<ScreenRow>,
    conclusion: &'static str,
}

#[derive(Default, Serialize)]
struct RouteCounts {
    episodes: usize,
    tie_resolved: usize,
    routed: usize,
    abstained: usize,
    valid_labeled: usize,
    invalid_labeled: usize,
    valid_actionable: usize,
    invalid_actionable: usize,
    tie_valid_actionable: usize,
    tie_invalid_actionable: usize,
    invalid_shards: Vec<usize>,
    tie_invalid_shards: Vec<usize>,
    invalid_by_candidate: BTreeMap<String, usize>,
    routed_by_phenotype: [usize; PHENOTYPES],
    abstained_tie: usize,
}

#[derive(Serialize)]
struct EpisodeRow {
    key: String,
    candidate: &'static str,
    shard: usize,
    nomination_document: u64,
    witness_document: u64,
    actionable: bool,
    expected_family: Option<&'static str>,
    baseline_route: Option<&'static str>,
    baseline_path: &'static str,
    hard_abstain_route: Option<&'static str>,
    hard_abstain_path: &'static str,
    baseline_outcome: &'static str,
    hard_abstain_outcome: &'static str,
}

#[derive(Serialize)]
struct MemoryComparison {
    baseline: ReplaySummary,
    hard_abstain: ReplaySummary,
    authority_updates_lost: i64,
    baseline_total_authority_volume: f64,
    hard_abstain_total_authority_volume: f64,
    authority_volume_lost: f64,
    deterministic_replay: bool,
    credit_integrity_passed: bool,
}

#[derive(Serialize)]
struct DiscoveryDecision {
    tie_state_sufficient: bool,
    valid_loss_fraction: f64,
    new_abstention_fraction: f64,
    zero_invalid_episodes: bool,
    zero_invalid_authority_compartments: bool,
    credit_integrity_passed: bool,
    hard_abstention_selected: bool,
    status: &'static str,
}

#[derive(Serialize)]
struct ReplayReceipt {
    schema: &'static str,
    date: &'static str,
    mode: &'static str,
    corpus_id: String,
    corpus_path: String,
    corpus_sha256: String,
    screen_receipt_sha256: String,
    discovery_receipt_sha256: Option<String>,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    label_boundary: &'static str,
    control_policy: &'static str,
    candidate_policy: &'static str,
    baseline: RouteCounts,
    hard_abstain: RouteCounts,
    invalid_episodes_prevented: usize,
    invalid_actionable_episodes_prevented: usize,
    valid_episodes_sacrificed: usize,
    actionable_valid_episodes_sacrificed: usize,
    new_abstentions: usize,
    new_actionable_abstentions: usize,
    valid_loss_fraction: f64,
    new_abstention_fraction: f64,
    memory: MemoryComparison,
    decision: DiscoveryDecision,
    episodes: Vec<EpisodeRow>,
    conclusion: &'static str,
}

fn shard(document: u64, documents: u64) -> usize {
    ((document.saturating_mul(SHARDS as u64)) / documents).min((SHARDS - 1) as u64) as usize
}

fn tie_resolved_counts(
    events: &[Event],
    documents: u64,
) -> (BTreeMap<String, usize>, [usize; SHARDS]) {
    let mut by_candidate = BTreeMap::new();
    let mut by_shard = [0usize; SHARDS];
    for episode in make_episodes(events) {
        let route = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::UniqueEndpointAgreement,
        );
        if route_path(
            episode.nomination.features,
            episode.witness.features,
            true,
            route,
        ) != "TIE_RESOLVED"
        {
            continue;
        }
        *by_candidate
            .entry(CANDIDATES[episode.candidate].id.to_owned())
            .or_insert(0) += 1;
        by_shard[shard(episode.nomination.doc, documents)] += 1;
    }
    (by_candidate, by_shard)
}

fn screen_eligible(by_candidate: &BTreeMap<String, usize>, by_shard: &[usize; SHARDS]) -> bool {
    let total: usize = by_candidate.values().sum();
    let relations = by_candidate
        .values()
        .filter(|&&count| count >= MIN_PER_RELATION)
        .count();
    let shard_count = by_shard.iter().filter(|&&count| count > 0).count();
    total >= MIN_TIE_RESOLVED && relations >= MIN_RELATIONS && shard_count >= MIN_SHARDS
}

fn run_screen(args: &[String]) -> Result<()> {
    ensure!(
        args.len() >= 2,
        "usage: lt9_la2p1l4 --screen <receipt.json> <corpus-id=corpus.jsonl>..."
    );
    let output_path = Path::new(&args[0]);
    let mut rows = Vec::with_capacity(args.len() - 1);
    let mut discovery = None;
    let mut qualification = None;

    for (index, spec) in args[1..].iter().enumerate() {
        ensure!(index < SCREEN_ORDER.len(), "screen order exhausted");
        ensure!(
            discovery.is_none() || qualification.is_none(),
            "stop after the second eligible corpus"
        );
        let (id, path) = spec
            .split_once('=')
            .context("screen argument must be corpus-id=corpus.jsonl")?;
        ensure!(
            id == SCREEN_ORDER[index],
            "corpus {id} violates frozen screen order at slot {index}"
        );
        let corpus_path = Path::new(path);
        let (events, document_count, corpus_sha256) = load_events(corpus_path)?;
        let episodes = make_episodes(&events);
        let (by_candidate, by_shard) = tie_resolved_counts(&events, document_count);
        let eligible = screen_eligible(&by_candidate, &by_shard);
        let assigned_role = if eligible && discovery.is_none() {
            discovery = Some(id.to_owned());
            "P1L4_DISCOVERY"
        } else if eligible && qualification.is_none() {
            qualification = Some(id.to_owned());
            "P1L4Q_QUALIFICATION"
        } else {
            "NOT_ASSIGNED"
        };
        let tie_resolved_episode_count = by_candidate.values().sum();
        let candidate_relation_count = by_candidate
            .values()
            .filter(|&&n| n >= MIN_PER_RELATION)
            .count();
        let shard_count = by_shard.iter().filter(|&&n| n > 0).count();
        println!("{id}: documents={document_count} episodes={} tie_resolved={tie_resolved_episode_count} relations={candidate_relation_count} shards={shard_count} eligible={eligible} role={assigned_role}",
            episodes.len());
        rows.push(ScreenRow {
            corpus_id: id.to_owned(),
            corpus_path: corpus_path.display().to_string(),
            corpus_sha256,
            document_count,
            event_count: events.len(),
            episode_count: episodes.len(),
            tie_resolved_episode_count,
            tie_resolved_by_candidate: by_candidate,
            tie_resolved_by_shard: by_shard,
            candidate_relation_count,
            shard_count,
            eligible,
            assigned_role,
        });
    }

    let conclusion = if qualification.is_some() {
        "TWO_CORPORA_PREASSIGNED_LABEL_BLIND; first eligible is P1L4 discovery and second is P1L4Q qualification"
    } else {
        "INSUFFICIENT_UNOPENED_CORPORA; do not open labels"
    };
    let receipt = ScreenReceipt {
        schema: SCREEN_SCHEMA,
        date: "2026-09-23",
        scope: "P1L4 label-blind tie-tail structural coverage only",
        corpus_text_only: true,
        qrels_or_validity_labels_opened: false,
        frozen_order: SCREEN_ORDER.to_vec(),
        min_tie_resolved_episodes: MIN_TIE_RESOLVED,
        min_tie_resolved_per_relation: MIN_PER_RELATION,
        min_candidate_relations: MIN_RELATIONS,
        min_document_shards: MIN_SHARDS,
        discovery_corpus: discovery,
        qualification_corpus: qualification,
        corpora: rows,
        conclusion,
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn outcome(expected: Option<usize>, route: Option<usize>) -> &'static str {
    match (expected, route) {
        (None, _) => "UNSPECIFIED",
        (Some(_), None) => "ABSTAIN",
        (Some(e), Some(r)) if e == r => "VALID",
        (Some(_), Some(_)) => "INVALID",
    }
}

fn verify_screen(path: &Path, corpus_id: &str, corpus_hash: &str, role: &str) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read screen receipt {}", path.display()))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let receipt: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        receipt.get("schema").and_then(Value::as_str) == Some(SCREEN_SCHEMA),
        "unexpected P1L4 screen schema"
    );
    ensure!(
        receipt
            .get("qrels_or_validity_labels_opened")
            .and_then(Value::as_bool)
            == Some(false),
        "screen did not preserve label boundary"
    );
    ensure!(
        receipt
            .get("min_tie_resolved_episodes")
            .and_then(Value::as_u64)
            == Some(MIN_TIE_RESOLVED as u64),
        "tie coverage floor changed"
    );
    let field = if role == "P1L4_DISCOVERY" {
        "discovery_corpus"
    } else {
        "qualification_corpus"
    };
    ensure!(
        receipt.get(field).and_then(Value::as_str) == Some(corpus_id),
        "corpus role does not match frozen screen order"
    );
    let row = receipt
        .get("corpora")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("corpus_id").and_then(Value::as_str) == Some(corpus_id))
        })
        .context("selected corpus missing from screen receipt")?;
    ensure!(
        row.get("corpus_sha256").and_then(Value::as_str) == Some(corpus_hash),
        "screen and replay corpus hashes differ"
    );
    ensure!(
        row.get("eligible").and_then(Value::as_bool) == Some(true),
        "selected corpus did not meet structural floor"
    );
    ensure!(
        row.get("assigned_role").and_then(Value::as_str) == Some(role),
        "selected role was not frozen in the screen receipt"
    );
    Ok(hash)
}

fn route_outcome_counts(rows: &[EpisodeRow], hard: bool) -> RouteCounts {
    let mut counts = RouteCounts {
        episodes: rows.len(),
        ..RouteCounts::default()
    };
    let mut invalid_shards = BTreeSet::new();
    let mut tie_invalid_shards = BTreeSet::new();
    for row in rows {
        let route = if hard {
            row.hard_abstain_route
        } else {
            row.baseline_route
        };
        let state = if hard {
            row.hard_abstain_outcome
        } else {
            row.baseline_outcome
        };
        let path = if hard {
            row.hard_abstain_path
        } else {
            row.baseline_path
        };
        if path == "TIE_RESOLVED" {
            counts.tie_resolved += 1;
        }
        if route.is_some() {
            counts.routed += 1;
            if let Some(phi) = CANDIDATES
                .iter()
                .find(|c| c.id == row.candidate)
                .and_then(|_| match route? {
                    "finance" => Some(0),
                    "geography" => Some(1),
                    "transport" => Some(2),
                    _ => Some(3),
                })
            {
                counts.routed_by_phenotype[phi] += 1;
            }
        } else {
            counts.abstained += 1;
            if path == "TIE_ABSTAINED" {
                counts.abstained_tie += 1;
            }
        }
        match state {
            "VALID" => counts.valid_labeled += 1,
            "INVALID" => {
                counts.invalid_labeled += 1;
                *counts
                    .invalid_by_candidate
                    .entry(row.candidate.to_owned())
                    .or_insert(0) += 1;
                if row.actionable {
                    invalid_shards.insert(row.shard);
                }
            }
            _ => {}
        }
        if row.actionable {
            match state {
                "VALID" => counts.valid_actionable += 1,
                "INVALID" => counts.invalid_actionable += 1,
                _ => {}
            }
            if row.baseline_path == "TIE_RESOLVED" {
                match row.baseline_outcome {
                    "VALID" if !hard || state == "VALID" => counts.tie_valid_actionable += 1,
                    "INVALID" => {
                        counts.tie_invalid_actionable += 1;
                        tie_invalid_shards.insert(row.shard);
                    }
                    _ => {}
                }
            }
        }
    }
    counts.invalid_shards = invalid_shards.into_iter().collect();
    counts.tie_invalid_shards = tie_invalid_shards.into_iter().collect();
    counts
}

fn replay_main(args: &[String]) -> Result<()> {
    ensure!(args.len() == 5 || args.len() == 6,
        "usage: lt9_la2p1l4 --replay <discovery|qualification> <corpus-id> <corpus.jsonl> <screen.json> <output.json> [discovery.json]");
    let mode: &'static str = match args[0].as_str() {
        "discovery" => "discovery",
        "qualification" => "qualification",
        _ => anyhow::bail!("unknown replay mode"),
    };
    let corpus_id = args[1].clone();
    let corpus_path = Path::new(&args[2]);
    let screen_path = Path::new(&args[3]);
    let output_path = Path::new(&args[4]);
    let discovery_path = if mode == "qualification" {
        ensure!(
            args.len() == 6,
            "qualification mode requires the sealed discovery receipt"
        );
        Some(Path::new(&args[5]))
    } else {
        ensure!(
            args.len() == 5,
            "discovery mode takes no prior discovery receipt"
        );
        None
    };
    let (events, document_count, corpus_hash) = load_events(corpus_path)?;
    let role = if mode == "discovery" {
        "P1L4_DISCOVERY"
    } else {
        "P1L4Q_QUALIFICATION"
    };
    let screen_hash = verify_screen(screen_path, &corpus_id, &corpus_hash, role)?;
    let mut discovery_hash = None;
    if let Some(path) = discovery_path {
        let bytes = std::fs::read(path)?;
        discovery_hash = Some(format!("{:x}", Sha256::digest(&bytes)));
        let d: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            d.get("schema").and_then(Value::as_str) == Some(REPLAY_SCHEMA),
            "wrong discovery receipt schema"
        );
        ensure!(
            d.get("mode").and_then(Value::as_str) == Some("discovery"),
            "prior receipt is not P1L4 discovery"
        );
        ensure!(
            d.get("decision")
                .and_then(|v| v.get("hard_abstention_selected"))
                .and_then(Value::as_bool)
                == Some(true),
            "P1L4 discovery did not select hard tie-abstention; qualification is not authorized"
        );
    }

    let episodes = make_episodes(&events);
    let mut rows = Vec::with_capacity(episodes.len());
    for episode in episodes.iter().copied() {
        let control = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::UniqueEndpointAgreement,
        );
        let candidate = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::HardAbstain,
        );
        let expected = (CANDIDATES[episode.candidate].expected_phi >= 0)
            .then_some(CANDIDATES[episode.candidate].expected_phi as usize);
        let actionable = witness_polarity(episode.witness).is_some();
        let baseline_path = route_path(
            episode.nomination.features,
            episode.witness.features,
            true,
            control,
        );
        let hard_path = route_path(
            episode.nomination.features,
            episode.witness.features,
            true,
            candidate,
        );
        rows.push(EpisodeRow {
            key: format!(
                "{}:{}->{}",
                CANDIDATES[episode.candidate].id, episode.nomination.doc, episode.witness.doc
            ),
            candidate: CANDIDATES[episode.candidate].id,
            shard: shard(episode.nomination.doc, document_count),
            nomination_document: episode.nomination.doc,
            witness_document: episode.witness.doc,
            actionable,
            expected_family: expected.map(family_name),
            baseline_route: control.map(family_name),
            baseline_path,
            hard_abstain_route: candidate.map(family_name),
            hard_abstain_path: hard_path,
            baseline_outcome: outcome(expected, control),
            hard_abstain_outcome: outcome(expected, candidate),
        });
    }

    let baseline = route_outcome_counts(&rows, false);
    let hard = route_outcome_counts(&rows, true);
    let invalid_episodes_prevented = rows
        .iter()
        .filter(|r| r.baseline_outcome == "INVALID" && r.hard_abstain_outcome == "ABSTAIN")
        .count();
    let invalid_actionable_episodes_prevented = rows
        .iter()
        .filter(|r| {
            r.actionable && r.baseline_outcome == "INVALID" && r.hard_abstain_outcome == "ABSTAIN"
        })
        .count();
    let valid_episodes_sacrificed = rows
        .iter()
        .filter(|r| r.baseline_outcome == "VALID" && r.hard_abstain_outcome == "ABSTAIN")
        .count();
    let actionable_valid_episodes_sacrificed = rows
        .iter()
        .filter(|r| {
            r.actionable && r.baseline_outcome == "VALID" && r.hard_abstain_outcome == "ABSTAIN"
        })
        .count();
    let new_abstentions = rows
        .iter()
        .filter(|r| r.baseline_route.is_some() && r.hard_abstain_route.is_none())
        .count();
    let new_actionable_abstentions = rows
        .iter()
        .filter(|r| {
            r.actionable
                && r.expected_family.is_some()
                && r.baseline_route.is_some()
                && r.hard_abstain_route.is_none()
        })
        .count();
    let actionable_valid_baseline = rows
        .iter()
        .filter(|r| r.actionable && r.expected_family.is_some() && r.baseline_outcome == "VALID")
        .count();
    let actionable_routed_baseline = rows
        .iter()
        .filter(|r| r.actionable && r.expected_family.is_some() && r.baseline_route.is_some())
        .count();
    let valid_loss_fraction = ratio(
        actionable_valid_episodes_sacrificed,
        actionable_valid_baseline,
    );
    let new_abstention_fraction = ratio(new_actionable_abstentions, actionable_routed_baseline);

    let baseline_memory = run_credit_replay(&episodes, true, TiePolicy::UniqueEndpointAgreement);
    let baseline_repeat = run_credit_replay(&episodes, true, TiePolicy::UniqueEndpointAgreement);
    let candidate_memory = run_credit_replay(&episodes, true, TiePolicy::HardAbstain);
    let candidate_repeat = run_credit_replay(&episodes, true, TiePolicy::HardAbstain);
    let deterministic = serde_json::to_vec(&baseline_memory)?
        == serde_json::to_vec(&baseline_repeat)?
        && serde_json::to_vec(&candidate_memory)? == serde_json::to_vec(&candidate_repeat)?;
    let integrity = [(&baseline_memory), (&candidate_memory)].iter().all(|m| {
        m.polarity_errors == 0
            && m.pending_capacity_violations == 0
            && m.pending_peak <= PENDING_CAPACITY
            && m.owned_witnesses == m.plus_updates + m.minus_updates
    }) && deterministic;
    let baseline_authority = baseline_memory.plus_updates + baseline_memory.minus_updates;
    let hard_authority = candidate_memory.plus_updates + candidate_memory.minus_updates;
    let baseline_volume =
        baseline_memory.positive_authority_mass + baseline_memory.negative_authority_mass;
    let hard_volume =
        candidate_memory.positive_authority_mass + candidate_memory.negative_authority_mass;
    let tie_invalid_shards = baseline.tie_invalid_shards.len();
    let tie_state_sufficient = baseline.tie_valid_actionable >= MIN_VALID_TIE_ACTIONABLE
        && baseline.tie_invalid_actionable >= MIN_INVALID_TIE_ACTIONABLE
        && tie_invalid_shards >= MIN_INVALID_TIE_SHARDS;
    let zero_invalid_episodes = hard.invalid_labeled == 0;
    let zero_invalid_compartments = candidate_memory.invalid_authority_compartments.is_empty();
    let selected = tie_state_sufficient
        && valid_loss_fraction <= MAX_VALID_LOSS
        && new_abstention_fraction <= MAX_NEW_ABSTENTION
        && zero_invalid_episodes
        && zero_invalid_compartments
        && integrity;
    let status = match (mode, tie_state_sufficient, selected) {
        ("discovery", false, _) => "DISCOVERY_UNDERPOWERED_ON_ACTIONABLE_TIE_OUTCOMES",
        ("discovery", true, true) => "HARD_TIE_ABSTENTION_SELECTED_FOR_FROZEN_QUALIFICATION",
        ("discovery", true, false) => {
            "HARD_TIE_ABSTENTION_NOT_SELECTED; P1L5_TIE_INFORMATION_ANATOMY_ELIGIBLE"
        }
        ("qualification", false, _) => "QUALIFICATION_UNDERPOWERED_FAILED_CLOSED",
        ("qualification", true, true) => "P1L4Q_HARD_TIE_ABSTENTION_QUALIFIED",
        ("qualification", true, false) => {
            "P1L4Q_NOT_QUALIFIED_FAILED_CLOSED; DO_NOT_TUNE_ON_THIS_CORPUS"
        }
        _ => unreachable!("mode validated above"),
    };
    let status_for_print = status;
    let baseline_invalid_for_print = baseline.invalid_labeled;
    let hard_invalid_for_print = hard.invalid_labeled;
    let baseline_ties_for_print = baseline.tie_resolved;
    let decision = DiscoveryDecision {
        tie_state_sufficient,
        valid_loss_fraction,
        new_abstention_fraction,
        zero_invalid_episodes,
        zero_invalid_authority_compartments: zero_invalid_compartments,
        credit_integrity_passed: integrity,
        hard_abstention_selected: selected,
        status,
    };
    let receipt = ReplayReceipt {
        schema: REPLAY_SCHEMA,
        date: "2026-09-23",
        mode,
        corpus_id,
        corpus_path: corpus_path.display().to_string(),
        corpus_sha256: corpus_hash,
        screen_receipt_sha256: screen_hash,
        discovery_receipt_sha256: discovery_hash,
        document_count,
        event_count: events.len(),
        episode_count: rows.len(),
        label_boundary: "uses only frozen candidate expected-context classes for post-hoc router validity; no BEIR qrels, queries, or retrieval judgments loaded",
        control_policy: "distinct-marker plurality with frozen unique_endpoint_agreement",
        candidate_policy: "distinct-marker plurality; any exact endpoint plurality tie abstains; otherwise require matching unique winners",
        baseline,
        hard_abstain: hard,
        invalid_episodes_prevented,
        invalid_actionable_episodes_prevented,
        valid_episodes_sacrificed,
        actionable_valid_episodes_sacrificed,
        new_abstentions,
        new_actionable_abstentions,
        valid_loss_fraction,
        new_abstention_fraction,
        memory: MemoryComparison {
            authority_updates_lost: baseline_authority as i64 - hard_authority as i64,
            baseline_total_authority_volume: baseline_volume,
            hard_abstain_total_authority_volume: hard_volume,
            authority_volume_lost: baseline_volume - hard_volume,
            baseline: baseline_memory,
            hard_abstain: candidate_memory,
            deterministic_replay: deterministic,
            credit_integrity_passed: integrity,
        },
        decision,
        episodes: rows,
        conclusion: if mode == "discovery" {
            "P1L4 discovery only; qualification is allowed only when discovery selects the frozen hard-abstention policy"
        } else {
            "P1L4Q qualification only; no authority promotion, natural integration, retrieval, or serving"
        },
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("P1L4 {} corpus={} episodes={} ties={} invalid={}->{} valid_lost={} authority_lost={} invalid_compartments={}",
        status_for_print, receipt.corpus_id, receipt.episode_count, baseline_ties_for_print,
        baseline_invalid_for_print, hard_invalid_for_print, actionable_valid_episodes_sacrificed,
        receipt.memory.authority_updates_lost, receipt.memory.hard_abstain.invalid_authority_compartments.len());
    Ok(())
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("--screen") => run_screen(&args[2..]),
        Some("--replay") => replay_main(&args[2..]),
        _ => anyhow::bail!("usage: lt9_la2p1l4 --screen ... | --replay ..."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tie_screen_floor_requires_relations_and_shards() {
        let relations = BTreeMap::from([("a".to_owned(), 30), ("b".to_owned(), 10)]);
        let shards = [10, 10, 10, 10, 0, 0, 0, 0];
        assert!(screen_eligible(&relations, &shards));
        let one_relation = BTreeMap::from([("a".to_owned(), 39), ("b".to_owned(), 1)]);
        assert!(!screen_eligible(&one_relation, &shards));
        let two_shards = [20, 20, 0, 0, 0, 0, 0, 0];
        assert!(!screen_eligible(&relations, &two_shards));
    }

    #[test]
    fn distinct_marker_tie_abstention_removes_only_the_rescued_route() {
        let f = |raw, mask| FamilyFeatures {
            raw_count: raw,
            distinct_mask: mask,
            ..FamilyFeatures::default()
        };
        let unique = Features {
            family: [f(2, 0b11), f(1, 0b01), f(0, 0)],
            ..Features::default()
        };
        let tied = Features {
            family: [f(1, 0b01), f(1, 0b10), f(0, 0)],
            ..Features::default()
        };
        assert_eq!(
            qualified_pair_route(unique, tied, true, TiePolicy::UniqueEndpointAgreement),
            Some(0)
        );
        assert_eq!(
            qualified_pair_route(unique, tied, true, TiePolicy::HardAbstain),
            None
        );
        let same_unique = Features {
            family: [f(3, 0b111), f(0, 0), f(0, 0)],
            ..Features::default()
        };
        assert_eq!(
            qualified_pair_route(unique, same_unique, true, TiePolicy::HardAbstain),
            Some(0)
        );
    }
}
