//! LT9-LA2-P1L3Q: frozen external qualification of distinct-marker voting.
//!
//! Reads one corpus JSONL plus the label-blind P1L3 state-coverage receipt.
//! It does not load qrels, relevance judgments, or queries and does not change
//! learning, authority, retrieval, or serving state.

#[allow(dead_code)]
mod lt9_la2p1l3_core;

use anyhow::{ensure, Context, Result};
use lt9_la2p1l3_core::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1l3q/v1";
const EXPECTED_CORPUS_SHA256: &str = "abd0a993f69b0abc2a4a5367695bceb081f0c0efd22d65b9986d4ac6ff8baf95";
const EXPECTED_SCREEN_SHA256: &str = "ec267453a7870411803301d0823e2e71003b4087475b43cc3155c5bc6549b41b";
const EXPECTED_P1L3_SOURCE_SHA256: &str = "1b22cada58c79bb565301012ee8c099e20009a1f191d8db66ee43fa01ce75e46";
const SHARDS: usize = 8;
const MIN_VALID: usize = 20;
const MIN_INVALID: usize = 5;
const MIN_INVALID_SHARDS: usize = 2;
const MAX_EPISODE_VALID_LOSS: f64 = 0.10;
const MAX_EPISODE_NEW_TIE: f64 = 0.10;

#[derive(Serialize)]
struct EpisodeOutcome {
    key: String,
    candidate: &'static str,
    shard: usize,
    nomination_document: u64,
    witness_document: u64,
    actionable: bool,
    expected_family: Option<&'static str>,
    raw_route: Option<&'static str>,
    distinct_route: Option<&'static str>,
    distinct_route_path: &'static str,
    raw_outcome: &'static str,
    distinct_outcome: &'static str,
}

#[derive(Default, Serialize)]
struct RoutingSummary {
    episode_count: usize,
    actionable_labeled_episodes: usize,
    raw_valid_actionable_episodes: usize,
    raw_invalid_actionable_episodes: usize,
    raw_invalid_shards: Vec<usize>,
    distinct_valid_actionable_episodes: usize,
    distinct_invalid_episodes: usize,
    repaired_invalid_episodes: usize,
    invalid_abstained_episodes: usize,
    valid_loss_episodes: usize,
    new_tie_abstention_episodes: usize,
    valid_loss_fraction: f64,
    new_tie_fraction: f64,
    current_sufficient: bool,
    zero_invalid_distinct_episodes: bool,
    valid_loss_gate: bool,
    new_tie_gate: bool,
    invalid_shard_gate: bool,
    routing_gate_passed: bool,
}

#[derive(Serialize)]
struct MemoryGate {
    invalid_authority_compartment_count: usize,
    invalid_authority_compartments: Vec<String>,
    zero_invalid_authority_compartments: bool,
    polarity_errors: usize,
    zero_polarity_errors: bool,
    pending_capacity_violations: usize,
    pending_peak: usize,
    pending_capacity_respected: bool,
    owned_witnesses: usize,
    positive_updates: usize,
    negative_updates: usize,
    traceable_owned_witnesses: bool,
    deterministic_replay: bool,
    memory_gate_passed: bool,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    policy: &'static str,
    corpus_path: String,
    corpus_sha256: String,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    p1l3_source_sha256: String,
    p1l3_screen_receipt_sha256: String,
    label_boundary: &'static str,
    preflight_floor_met: bool,
    routing_summary: RoutingSummary,
    memory_gate: MemoryGate,
    overall_status: &'static str,
    episode_outcomes: Vec<EpisodeOutcome>,
    conclusion: &'static str,
}

fn outcome(expected: Option<usize>, route: Option<usize>) -> &'static str {
    match (expected, route) {
        (None, _) => "UNSPECIFIED",
        (Some(_), None) => "ABSTAIN",
        (Some(e), Some(r)) if e == r => "VALID",
        (Some(_), Some(_)) => "INVALID",
    }
}

fn verify_preflight(path: &Path, corpus_hash: &str) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read preflight {}", path.display()))?;
    let receipt_hash = format!("{:x}", Sha256::digest(&bytes));
    ensure!(receipt_hash == EXPECTED_SCREEN_SHA256, "P1L3 screen receipt changed");
    let receipt: Value = serde_json::from_slice(&bytes)?;
    ensure!(receipt.get("schema").and_then(Value::as_str) == Some("phoenix.lexical.lt9-la2p1l3-corpus-screen/v1"), "unexpected preflight schema");
    ensure!(receipt.get("qrels_or_validity_labels_opened").and_then(Value::as_bool) == Some(false), "preflight label boundary not clean");
    let dbpedia = receipt.get("corpora").and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|row| row.get("corpus_id").and_then(Value::as_str) == Some("dbpedia-entity")))
        .context("DBPedia-Entity row absent from frozen preflight")?;
    ensure!(dbpedia.get("corpus_sha256").and_then(Value::as_str) == Some(corpus_hash), "screen/corpus hash mismatch");
    ensure!(dbpedia.get("risk_signature_episode_count").and_then(Value::as_u64).unwrap_or(0) >= 8, "P1L3 risk episode floor not met");
    ensure!(dbpedia.get("risk_signature_candidate_count").and_then(Value::as_u64).unwrap_or(0) >= 2, "P1L3 relation floor not met");
    ensure!(dbpedia.get("viability").and_then(Value::as_str) == Some("RISK_STATE_COVERAGE_FLOOR_MET"), "preflight did not mark DBPedia eligible");
    Ok(receipt_hash)
}

fn classify(episodes: &[Episode], document_count: u64) -> (RoutingSummary, Vec<EpisodeOutcome>) {
    let mut summary = RoutingSummary { episode_count: episodes.len(), ..RoutingSummary::default() };
    let mut invalid_shards = BTreeSet::new();
    let mut outcomes = Vec::with_capacity(episodes.len());
    for episode in episodes {
        let spec = CANDIDATES[episode.candidate];
        let expected = (spec.expected_phi >= 0).then_some(spec.expected_phi as usize);
        let raw = qualified_pair_route(episode.nomination.features, episode.witness.features, false);
        let distinct = qualified_pair_route(episode.nomination.features, episode.witness.features, true);
        let actionable = witness_polarity(episode.witness).is_some();
        let raw_state = outcome(expected, raw);
        let distinct_state = outcome(expected, distinct);
        let shard = ((episode.nomination.doc.saturating_mul(SHARDS as u64)) / document_count).min((SHARDS - 1) as u64) as usize;
        if actionable && expected.is_some() {
            summary.actionable_labeled_episodes += 1;
            match raw_state {
                "VALID" => summary.raw_valid_actionable_episodes += 1,
                "INVALID" => { summary.raw_invalid_actionable_episodes += 1; invalid_shards.insert(shard); }
                _ => {}
            }
            match distinct_state {
                "VALID" => summary.distinct_valid_actionable_episodes += 1,
                _ => {}
            }
            if raw_state == "VALID" && distinct_state != "VALID" { summary.valid_loss_episodes += 1; }
            if matches!(raw_state, "VALID" | "INVALID") && distinct_state == "ABSTAIN" {
                summary.new_tie_abstention_episodes += usize::from(
                    route_path(episode.nomination.features, episode.witness.features, true, distinct) == "TIE_ABSTAINED"
                );
            }
        }
        if expected.is_some() && distinct_state == "INVALID" { summary.distinct_invalid_episodes += 1; }
        if raw_state == "INVALID" && distinct_state == "VALID" { summary.repaired_invalid_episodes += 1; }
        if raw_state == "INVALID" && distinct_state == "ABSTAIN" { summary.invalid_abstained_episodes += 1; }
        outcomes.push(EpisodeOutcome {
            key: format!("{}:{}->{}", spec.id, episode.nomination.doc, episode.witness.doc),
            candidate: spec.id,
            shard,
            nomination_document: episode.nomination.doc,
            witness_document: episode.witness.doc,
            actionable,
            expected_family: expected.map(family_name),
            raw_route: raw.map(family_name),
            distinct_route: distinct.map(family_name),
            distinct_route_path: route_path(episode.nomination.features, episode.witness.features, true, distinct),
            raw_outcome: raw_state,
            distinct_outcome: distinct_state,
        });
    }
    summary.raw_invalid_shards = invalid_shards.iter().copied().collect();
    summary.current_sufficient = summary.raw_valid_actionable_episodes >= MIN_VALID
        && summary.raw_invalid_actionable_episodes >= MIN_INVALID
        && invalid_shards.len() >= MIN_INVALID_SHARDS;
    let current_valid_episodes = outcomes.iter().filter(|r| r.actionable && r.expected_family.is_some() && r.raw_outcome == "VALID").count();
    let current_actionable_episodes = outcomes.iter().filter(|r| r.actionable && r.expected_family.is_some() && matches!(r.raw_outcome, "VALID" | "INVALID")).count();
    summary.valid_loss_fraction = if current_valid_episodes == 0 { 0.0 } else { summary.valid_loss_episodes as f64 / current_valid_episodes as f64 };
    summary.new_tie_fraction = if current_actionable_episodes == 0 { 0.0 } else { summary.new_tie_abstention_episodes as f64 / current_actionable_episodes as f64 };
    summary.zero_invalid_distinct_episodes = summary.distinct_invalid_episodes == 0;
    summary.valid_loss_gate = summary.valid_loss_fraction <= MAX_EPISODE_VALID_LOSS;
    summary.new_tie_gate = summary.new_tie_fraction <= MAX_EPISODE_NEW_TIE;
    summary.invalid_shard_gate = invalid_shards.len() >= MIN_INVALID_SHARDS;
    summary.routing_gate_passed = summary.current_sufficient && summary.zero_invalid_distinct_episodes
        && summary.valid_loss_gate && summary.new_tie_gate && summary.invalid_shard_gate;
    (summary, outcomes)
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    ensure!(args.len() == 4, "usage: lt9_la2p1l3q <dbpedia-corpus.jsonl> <p1l3-screen-receipt.json> <output.json>");
    let corpus_path = Path::new(&args[1]);
    let screen_path = Path::new(&args[2]);
    let output_path = Path::new(&args[3]);
    let (events, document_count, corpus_hash) = load_events(corpus_path)?;
    ensure!(corpus_hash == EXPECTED_CORPUS_SHA256, "DBPedia-Entity corpus hash mismatch");
    let screen_hash = verify_preflight(screen_path, &corpus_hash)?;
    let p1l3_source_sha256 = format!("{:x}", Sha256::digest(include_bytes!("lt9_la2p1l3.rs")));
    ensure!(p1l3_source_sha256 == EXPECTED_P1L3_SOURCE_SHA256, "frozen P1L3 policy source changed");
    let episodes = make_episodes(&events);
    let (routing_summary, episode_outcomes) = classify(&episodes, document_count);
    let memory = run_credit_replay(&episodes, true);
    let memory_repeat = run_credit_replay(&episodes, true);
    let deterministic = serde_json::to_vec(&memory)? == serde_json::to_vec(&memory_repeat)?;
    let zero_invalid_authority_compartments = memory.invalid_authority_compartments.is_empty();
    let zero_polarity_errors = memory.polarity_errors == 0;
    let pending_capacity_respected = memory.pending_capacity_violations == 0 && memory.pending_peak <= PENDING_CAPACITY;
    let traceable_owned_witnesses = memory.owned_witnesses == memory.plus_updates + memory.minus_updates;
    let memory_gate_passed = zero_invalid_authority_compartments && zero_polarity_errors && pending_capacity_respected && traceable_owned_witnesses && deterministic;
    let memory_gate = MemoryGate {
        invalid_authority_compartment_count: memory.invalid_authority_compartments.len(),
        invalid_authority_compartments: memory.invalid_authority_compartments.clone(),
        zero_invalid_authority_compartments,
        polarity_errors: memory.polarity_errors,
        zero_polarity_errors,
        pending_capacity_violations: memory.pending_capacity_violations,
        pending_peak: memory.pending_peak,
        pending_capacity_respected,
        owned_witnesses: memory.owned_witnesses,
        positive_updates: memory.plus_updates,
        negative_updates: memory.minus_updates,
        traceable_owned_witnesses,
        deterministic_replay: deterministic,
        memory_gate_passed,
    };
    let overall_passed = routing_summary.routing_gate_passed && memory_gate_passed;
    let receipt = Receipt {
        schema: SCHEMA,
        scope: "P1L3Q external distinct-marker router qualification only; no qrels, retrieval, authority promotion, or serving",
        policy: "distinct active marker identity count + frozen unique_endpoint_agreement; no tuning",
        corpus_path: corpus_path.display().to_string(),
        corpus_sha256: corpus_hash,
        document_count,
        event_count: events.len(),
        episode_count: episodes.len(),
        p1l3_source_sha256,
        p1l3_screen_receipt_sha256: screen_hash,
        label_boundary: "uses only frozen candidate expected-context classes for post-hoc routing validity; no BEIR qrels, queries, or retrieval judgments loaded",
        preflight_floor_met: true,
        routing_summary,
        memory_gate,
        overall_status: if overall_passed { "DISTINCT_MARKER_GATE_QUALIFIED" } else { "DISTINCT_MARKER_GATE_NOT_QUALIFIED" },
        episode_outcomes,
        conclusion: if overall_passed { "QUALIFICATION_ONLY: no authority or serving promotion" } else { "QUALIFICATION_FAILED_CLOSED: retain current context-routing status and do not tune on this corpus" },
    };
    if let Some(parent) = output_path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("P1L3Q {} docs={} episodes={} raw_invalid={} distinct_invalid={} invalid_authority_compartments={}",
        receipt.overall_status, receipt.document_count, receipt.episode_count,
        receipt.routing_summary.raw_invalid_actionable_episodes,
        receipt.routing_summary.distinct_invalid_episodes,
        receipt.memory_gate.invalid_authority_compartment_count);
    Ok(())
}
