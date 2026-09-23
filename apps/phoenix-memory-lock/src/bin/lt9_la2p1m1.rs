//! LT9-LA2-P1M1: anatomy of confidently routed, mixed-family evidence.
//! Corpus screening is label-blind; replay uses frozen candidate context only.

#[allow(dead_code)]
mod lt9_la2p1m1_core;
mod lt9_la2p1m1_screen;

use anyhow::{ensure, Context, Result};
use lt9_la2p1m1_core::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

const SCREEN_SCHEMA: &str = "phoenix.lexical.lt9-la2p1m1-screen/v1";
const REPLAY_SCHEMA: &str = "phoenix.lexical.lt9-la2p1m1-replay/v1";
const SHARDS: usize = 8;
const MIN_VALID_CONTESTED_ACTIONABLE: usize = 20;
const MIN_INVALID_CONTESTED_ACTIONABLE: usize = 5;
const MIN_INVALID_SHARDS: usize = 2;
const MAX_VALID_LOSS: f64 = 0.10;
const MAX_NEW_ABSTENTION: f64 = 0.10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteClass {
    ExclusiveUnique,
    ContestedUnique,
}

impl RouteClass {
    fn label(self) -> &'static str {
        match self {
            Self::ExclusiveUnique => "EXCLUSIVE_UNIQUE",
            Self::ContestedUnique => "CONTESTED_UNIQUE",
        }
    }
}

fn active_family_count(features: Features) -> usize {
    features
        .family
        .iter()
        .filter(|f| f.distinct_count() > 0)
        .count()
}

fn route_class(episode: Episode) -> RouteClass {
    if active_family_count(episode.nomination.features) == 1
        && active_family_count(episode.witness.features) == 1
    {
        RouteClass::ExclusiveUnique
    } else {
        RouteClass::ContestedUnique
    }
}

fn shard(document: u64, documents: u64) -> usize {
    ((document.saturating_mul(SHARDS as u64)) / documents).min((SHARDS - 1) as u64) as usize
}

#[derive(Serialize)]
struct FamilyEvidence {
    family: &'static str,
    distinct_mask: u32,
    distinct_count: u16,
    raw_occurrences: u16,
    repeated_surplus: u16,
    marker_order: Vec<&'static str>,
    marker_occurrences: Vec<u16>,
    before_occurrences: Vec<u16>,
    between_occurrences: Vec<u16>,
    after_occurrences: Vec<u16>,
    active_marker_identities: Vec<&'static str>,
}

#[derive(Serialize)]
struct EndpointEvidence {
    document: u64,
    distinct_family_counts: [u16; 3],
    active_family_count: usize,
    winner: Option<&'static str>,
    winner_distinct_count: u16,
    runner_up_distinct_count: u16,
    runner_up_families: Vec<&'static str>,
    distinct_masks: [u32; 3],
    repeated_surplus: [u16; 3],
    support_cue: bool,
    negative_cue: bool,
    term_distance: u16,
    same_field: bool,
    families: [FamilyEvidence; 3],
}

fn crop(values: &[u16; MAX_MARKERS], len: usize) -> Vec<u16> {
    values[..len].to_vec()
}

fn family_evidence(index: usize, f: FamilyFeatures) -> FamilyEvidence {
    let order = marker_names(index);
    let active_marker_identities = order
        .iter()
        .enumerate()
        .filter_map(|(bit, &name)| ((f.distinct_mask & (1u32 << bit)) != 0).then_some(name))
        .collect();
    FamilyEvidence {
        family: family_name(index),
        distinct_mask: f.distinct_mask,
        distinct_count: f.distinct_count(),
        raw_occurrences: f.raw_count,
        repeated_surplus: f.repeated_surplus(),
        marker_order: order.to_vec(),
        marker_occurrences: crop(&f.marker_occurrences, order.len()),
        before_occurrences: crop(&f.before_occurrences, order.len()),
        between_occurrences: crop(&f.between_occurrences, order.len()),
        after_occurrences: crop(&f.after_occurrences, order.len()),
        active_marker_identities,
    }
}

fn endpoint_evidence(event: Event) -> EndpointEvidence {
    let features = event.features;
    let counts = features.counts(true);
    let winner = qualified_pair_route(features, features, true, TiePolicy::HardAbstain);
    let winner_count = winner.map_or(0, |i| counts[i]);
    let runner = (0..3)
        .filter(|&i| Some(i) != winner)
        .map(|i| counts[i])
        .max()
        .unwrap_or(0);
    let runner_up_families = (0..3)
        .filter(|&i| Some(i) != winner && counts[i] == runner && runner > 0)
        .map(family_name)
        .collect();
    EndpointEvidence {
        document: event.doc,
        distinct_family_counts: counts,
        active_family_count: active_family_count(features),
        winner: winner.map(family_name),
        winner_distinct_count: winner_count,
        runner_up_distinct_count: runner,
        runner_up_families,
        distinct_masks: std::array::from_fn(|i| features.family[i].distinct_mask),
        repeated_surplus: std::array::from_fn(|i| features.family[i].repeated_surplus()),
        support_cue: features.support,
        negative_cue: features.negative,
        term_distance: features.distance,
        same_field: features.same_field,
        families: std::array::from_fn(|i| family_evidence(i, features.family[i])),
    }
}

fn shared_marker_ids(a: Features, b: Features) -> [Vec<&'static str>; 3] {
    std::array::from_fn(|family| {
        let shared = a.family[family].distinct_mask & b.family[family].distinct_mask;
        marker_names(family)
            .iter()
            .enumerate()
            .filter_map(|(bit, &name)| ((shared & (1u32 << bit)) != 0).then_some(name))
            .collect()
    })
}

#[derive(Clone, Default, Serialize)]
struct ClassOutcomes {
    episodes: usize,
    actionable: usize,
    control_valid_actionable: usize,
    control_invalid_actionable: usize,
    candidate_valid_actionable: usize,
    candidate_invalid_actionable: usize,
    candidate_abstained_actionable: usize,
    invalid_shards: Vec<usize>,
}

#[derive(Default, Serialize)]
struct OutcomeCounts {
    unique_plurality: usize,
    exclusive_unique: usize,
    contested_unique: usize,
    control_valid: usize,
    control_invalid: usize,
    candidate_valid: usize,
    candidate_invalid: usize,
    valid_actionable_control: usize,
    invalid_actionable_control: usize,
    valid_actionable_candidate: usize,
    invalid_actionable_candidate: usize,
    valid_actionable_sacrificed: usize,
    invalid_actionable_prevented: usize,
    new_actionable_abstentions: usize,
    control_by_phenotype: [usize; PHENOTYPES],
    candidate_by_phenotype: [usize; PHENOTYPES],
    control_invalid_by_relation: BTreeMap<String, usize>,
    candidate_invalid_by_relation: BTreeMap<String, usize>,
    class: BTreeMap<String, ClassOutcomes>,
    invalid_shards: Vec<usize>,
    contested_invalid_shards: Vec<usize>,
}

#[derive(Serialize)]
struct EpisodeEvidence {
    episode_index: usize,
    relation: &'static str,
    route_class: &'static str,
    shard: usize,
    nomination_document: u64,
    witness_document: u64,
    document_position_delay: u64,
    candidate_opportunity_delay: u64,
    actionable: bool,
    same_field_agreement: bool,
    witness_polarity: Option<&'static str>,
    expected_family: Option<&'static str>,
    control_route: Option<&'static str>,
    control_route_compartment: Option<String>,
    candidate_route: Option<&'static str>,
    candidate_route_compartment: Option<String>,
    control_outcome: &'static str,
    candidate_outcome: &'static str,
    shared_marker_identities_by_family: [Vec<&'static str>; 3],
    nomination: EndpointEvidence,
    witness: EndpointEvidence,
    control_owned_witness: bool,
    control_updated_compartment: Option<String>,
    candidate_owned_witness: bool,
    candidate_updated_compartment: Option<String>,
}

#[derive(Serialize)]
struct ReplayDecision {
    contested_outcome_sufficient: bool,
    contested_valid_actionable: usize,
    contested_invalid_actionable: usize,
    zero_invalid_episodes: bool,
    zero_invalid_authority_compartments: bool,
    valid_loss_fraction: f64,
    new_abstention_fraction: f64,
    credit_integrity_passed: bool,
    exclusive_only_selected_or_qualified: bool,
    status: &'static str,
}

#[derive(Serialize)]
struct MemoryComparison {
    control: ReplaySummary,
    candidate: ReplaySummary,
    authority_updates_lost: i64,
    traceable_owned_updates: bool,
    deterministic_replay: bool,
    credit_integrity_passed: bool,
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
    unique_plurality_detail_count: usize,
    label_boundary: &'static str,
    control_policy: &'static str,
    candidate_policy: &'static str,
    outcomes: OutcomeCounts,
    decision: ReplayDecision,
    memory: MemoryComparison,
    episodes: Vec<EpisodeEvidence>,
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

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn candidate_opportunity_ordinals(events: &[Event]) -> HashMap<(usize, u64), u64> {
    let mut ordinals = HashMap::with_capacity(events.len());
    let mut next = [0u64; CANDIDATES.len()];
    for event in events {
        ordinals.insert((event.candidate, event.doc), next[event.candidate]);
        next[event.candidate] += 1;
    }
    ordinals
}

fn count_outcomes(rows: &[EpisodeEvidence], control_memory: &ReplaySummary) -> OutcomeCounts {
    let mut out = OutcomeCounts::default();
    out.unique_plurality = rows.len();
    for row in rows {
        let key = row.route_class.to_owned();
        let class = out.class.entry(key).or_default();
        class.episodes += 1;
        match row.route_class {
            "EXCLUSIVE_UNIQUE" => out.exclusive_unique += 1,
            _ => out.contested_unique += 1,
        }
        if row.actionable {
            class.actionable += 1;
        }
        if row.control_outcome == "VALID" {
            out.control_valid += 1;
            if row.actionable {
                out.valid_actionable_control += 1;
                class.control_valid_actionable += 1;
            }
        } else if row.control_outcome == "INVALID" {
            out.control_invalid += 1;
            *out.control_invalid_by_relation
                .entry(row.relation.to_owned())
                .or_default() += 1;
            if row.actionable {
                out.invalid_actionable_control += 1;
                class.control_invalid_actionable += 1;
                class.invalid_shards.push(row.shard);
            }
        }
        if row.candidate_outcome == "VALID" {
            out.candidate_valid += 1;
            if row.actionable {
                out.valid_actionable_candidate += 1;
                class.candidate_valid_actionable += 1;
            }
        } else if row.candidate_outcome == "INVALID" {
            out.candidate_invalid += 1;
            *out.candidate_invalid_by_relation
                .entry(row.relation.to_owned())
                .or_default() += 1;
            if row.actionable {
                out.invalid_actionable_candidate += 1;
                class.candidate_invalid_actionable += 1;
            }
        }
        if row.actionable && row.control_outcome == "VALID" && row.candidate_outcome == "ABSTAIN" {
            out.valid_actionable_sacrificed += 1;
        }
        if row.actionable && row.control_outcome == "INVALID" && row.candidate_outcome == "ABSTAIN"
        {
            out.invalid_actionable_prevented += 1;
            if row.route_class == "CONTESTED_UNIQUE" {
                out.contested_invalid_shards.push(row.shard);
            }
        }
        if row.actionable && row.control_route.is_some() && row.candidate_route.is_none() {
            out.new_actionable_abstentions += 1;
            class.candidate_abstained_actionable += 1;
        }
        if let Some(phi) = row.control_route {
            if let Some(index) = (0..PHENOTYPES).find(|&i| family_name(i) == phi) {
                out.control_by_phenotype[index] += 1;
            }
        }
        if let Some(phi) = row.candidate_route {
            if let Some(index) = (0..PHENOTYPES).find(|&i| family_name(i) == phi) {
                out.candidate_by_phenotype[index] += 1;
            }
        }
    }
    out.invalid_shards = control_memory
        .invalid_routed_episodes
        .gt(&0)
        .then(|| {
            rows.iter()
                .filter(|r| r.control_outcome == "INVALID")
                .map(|r| r.shard)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    for class in out.class.values_mut() {
        class.invalid_shards.sort_unstable();
        class.invalid_shards.dedup();
    }
    out.contested_invalid_shards.sort_unstable();
    out.contested_invalid_shards.dedup();
    out
}

fn screen_hash(path: &Path, corpus_id: &str, corpus_sha256: &str, role: &str) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read screen {}", path.display()))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let receipt: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        receipt["schema"].as_str() == Some(SCREEN_SCHEMA),
        "unexpected P1M1 screen schema"
    );
    ensure!(
        receipt["corpus_text_only"].as_bool() == Some(true),
        "screen was not restricted to corpus text"
    );
    ensure!(
        receipt["validity_labels_opened"].as_bool() == Some(false),
        "screen opened validity labels"
    );
    ensure!(
        receipt["qrels_or_queries_opened"].as_bool() == Some(false),
        "screen opened qrels or queries"
    );
    lt9_la2p1m1_screen::validate_frozen_screen(&receipt)?;
    let role_field = if role == "P1M1_DISCOVERY" {
        "discovery_corpus"
    } else {
        "qualification_corpus"
    };
    ensure!(
        receipt[role_field].as_str() == Some(corpus_id),
        "corpus role differs from frozen screen"
    );
    let corpus = receipt["corpora"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["corpus_id"].as_str() == Some(corpus_id))
        })
        .context("corpus missing from screen receipt")?;
    ensure!(
        corpus["corpus_sha256"].as_str() == Some(corpus_sha256),
        "screen/corpus hash mismatch"
    );
    ensure!(
        corpus["eligible"].as_bool() == Some(true),
        "corpus missed P1M1 structural floor"
    );
    ensure!(
        corpus["assigned_role"].as_str() == Some(role),
        "role assignment is not frozen"
    );
    Ok(hash)
}

fn replay(args: &[String]) -> Result<()> {
    ensure!(args.len() == 5 || args.len() == 6,
        "usage: --replay <discovery|qualification> <id> <corpus.jsonl> <screen.json> <output.json> [discovery.json]");
    let mode: &'static str = match args[0].as_str() {
        "discovery" => "discovery",
        "qualification" => "qualification",
        _ => anyhow::bail!("invalid replay mode"),
    };
    let corpus_id = args[1].clone();
    let corpus_path = Path::new(&args[2]);
    let output_path = Path::new(&args[4]);
    let (events, document_count, corpus_hash) = load_events(corpus_path)?;
    let role = if mode == "discovery" {
        "P1M1_DISCOVERY"
    } else {
        "P1M1Q_QUALIFICATION"
    };
    let screen_hash = screen_hash(Path::new(&args[3]), &corpus_id, &corpus_hash, role)?;
    let mut discovery_receipt_hash = None;
    if mode == "qualification" {
        ensure!(args.len() == 6, "qualification needs its discovery receipt");
        let bytes = std::fs::read(&args[5])?;
        let discovery: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            discovery["mode"].as_str() == Some("discovery"),
            "not a P1M1 discovery receipt"
        );
        ensure!(
            discovery["decision"]["exclusive_only_selected_or_qualified"].as_bool() == Some(true),
            "discovery did not select exclusive-only; qualification is closed"
        );
        ensure!(
            discovery["screen_receipt_sha256"].as_str() == Some(screen_hash.as_str()),
            "discovery and qualification do not share the same frozen screen receipt"
        );
        ensure!(
            discovery["corpus_id"].as_str()
                == serde_json::from_slice::<Value>(&std::fs::read(&args[3])?)?["discovery_corpus"]
                    .as_str(),
            "discovery receipt corpus differs from the screen-assigned discovery corpus"
        );
        ensure!(
            discovery["corpus_id"].as_str() != Some(corpus_id.as_str()),
            "qualification corpus must be distinct from discovery corpus"
        );
        discovery_receipt_hash = Some(format!("{:x}", Sha256::digest(bytes)));
    }

    let episodes = make_episodes(&events);
    let ordinals = candidate_opportunity_ordinals(&events);
    let (control_memory, control_trace) =
        run_credit_replay_traced(&episodes, true, TiePolicy::HardAbstain);
    let (control_repeat, _) = run_credit_replay_traced(&episodes, true, TiePolicy::HardAbstain);
    let (candidate_memory, candidate_trace) =
        run_credit_replay_traced(&episodes, true, TiePolicy::ExclusiveOnly);
    let (candidate_repeat, _) = run_credit_replay_traced(&episodes, true, TiePolicy::ExclusiveOnly);
    let deterministic = serde_json::to_vec(&control_memory)?
        == serde_json::to_vec(&control_repeat)?
        && serde_json::to_vec(&candidate_memory)? == serde_json::to_vec(&candidate_repeat)?
        && serde_json::to_vec(&control_trace)?
            == serde_json::to_vec(
                &run_credit_replay_traced(&episodes, true, TiePolicy::HardAbstain).1,
            )?
        && serde_json::to_vec(&candidate_trace)?
            == serde_json::to_vec(
                &run_credit_replay_traced(&episodes, true, TiePolicy::ExclusiveOnly).1,
            )?;
    let traceable = control_trace.iter().filter(|s| s.owned_witness).count()
        == control_memory.plus_updates + control_memory.minus_updates
        && candidate_trace.iter().filter(|s| s.owned_witness).count()
            == candidate_memory.plus_updates + candidate_memory.minus_updates;
    let integrity = deterministic
        && traceable
        && [(&control_memory), (&candidate_memory)].iter().all(|m| {
            m.polarity_errors == 0
                && m.pending_capacity_violations == 0
                && m.pending_peak <= PENDING_CAPACITY
                && m.owned_witnesses == m.plus_updates + m.minus_updates
        });

    let mut rows = Vec::new();
    for (episode_index, episode) in episodes.iter().copied().enumerate() {
        let control = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::HardAbstain,
        );
        if control.is_none() {
            continue;
        }
        let class = route_class(episode);
        let candidate = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::ExclusiveOnly,
        );
        let expected = (CANDIDATES[episode.candidate].expected_phi >= 0)
            .then_some(CANDIDATES[episode.candidate].expected_phi as usize);
        let control_step = &control_trace[episode_index];
        let candidate_step = &candidate_trace[episode_index];
        let relation = CANDIDATES[episode.candidate].id;
        let control_compartment = control_step
            .owned_witness
            .then(|| format!("{}@{}", relation, family_name(control_step.route.unwrap())));
        let control_route_compartment =
            control.map(|phi| format!("{}@{}", relation, family_name(phi)));
        let candidate_compartment = candidate_step.owned_witness.then(|| {
            format!(
                "{}@{}",
                relation,
                family_name(candidate_step.route.unwrap())
            )
        });
        let candidate_route_compartment =
            candidate.map(|phi| format!("{}@{}", relation, family_name(phi)));
        let delay = episode.witness.doc.saturating_sub(episode.nomination.doc);
        let opportunity_delay = ordinals
            .get(&(episode.candidate, episode.witness.doc))
            .zip(ordinals.get(&(episode.candidate, episode.nomination.doc)))
            .map_or(0, |(w, n)| w.saturating_sub(*n));
        rows.push(EpisodeEvidence {
            episode_index,
            relation,
            route_class: class.label(),
            shard: shard(episode.nomination.doc, document_count),
            nomination_document: episode.nomination.doc,
            witness_document: episode.witness.doc,
            document_position_delay: delay,
            candidate_opportunity_delay: opportunity_delay,
            actionable: control_step.witness_polarity.is_some(),
            same_field_agreement: episode.nomination.features.same_field
                == episode.witness.features.same_field,
            witness_polarity: control_step.witness_polarity,
            expected_family: expected.map(family_name),
            control_route: control.map(family_name),
            control_route_compartment,
            candidate_route: candidate.map(family_name),
            candidate_route_compartment,
            control_outcome: outcome(expected, control),
            candidate_outcome: outcome(expected, candidate),
            shared_marker_identities_by_family: shared_marker_ids(
                episode.nomination.features,
                episode.witness.features,
            ),
            nomination: endpoint_evidence(episode.nomination),
            witness: endpoint_evidence(episode.witness),
            control_owned_witness: control_step.owned_witness,
            control_updated_compartment: control_compartment,
            candidate_owned_witness: candidate_step.owned_witness,
            candidate_updated_compartment: candidate_compartment,
        });
    }
    let outcomes = count_outcomes(&rows, &control_memory);
    let contested = outcomes
        .class
        .get("CONTESTED_UNIQUE")
        .cloned()
        .unwrap_or_default();
    let contested_invalid_shards = outcomes.contested_invalid_shards.len();
    let contested_sufficient = contested.control_valid_actionable >= MIN_VALID_CONTESTED_ACTIONABLE
        && contested.control_invalid_actionable >= MIN_INVALID_CONTESTED_ACTIONABLE
        && contested_invalid_shards >= MIN_INVALID_SHARDS;
    let valid_loss_fraction = ratio(
        outcomes.valid_actionable_sacrificed,
        outcomes.valid_actionable_control,
    );
    let actionable_control_routed = rows.iter().filter(|r| r.actionable).count();
    let new_abstention_fraction = ratio(
        outcomes.new_actionable_abstentions,
        actionable_control_routed,
    );
    let zero_invalid = outcomes.candidate_invalid == 0;
    let zero_invalid_compartments = candidate_memory.invalid_authority_compartments.is_empty();
    let budgets_pass =
        valid_loss_fraction <= MAX_VALID_LOSS && new_abstention_fraction <= MAX_NEW_ABSTENTION;
    let selected = contested_sufficient
        && zero_invalid
        && zero_invalid_compartments
        && budgets_pass
        && integrity;
    let status = match (
        mode,
        contested_sufficient,
        zero_invalid,
        zero_invalid_compartments,
        budgets_pass,
        integrity,
    ) {
        ("discovery", false, _, _, _, _) => {
            "P1M1_DISCOVERY_UNDERPOWERED_CONTESTED_ACTIONABLE_OUTCOMES"
        }
        ("discovery", true, false, _, _, _) | ("discovery", true, _, false, _, _) => {
            "EXCLUSIVE_ONLY_UNSAFE; STOP_ROUTER_RULE_REFINEMENT_AUDIT_MARKER_ONTOLOGY"
        }
        ("discovery", true, true, true, false, _) => {
            "EXCLUSIVE_ONLY_SAFE_BUT_USEFULNESS_BUDGET_FAILED; P1M2_SPATIAL_ANATOMY_ELIGIBLE"
        }
        ("discovery", true, true, true, true, false) => {
            "DISCOVERY_CREDIT_OR_TRACE_INTEGRITY_FAILED"
        }
        ("discovery", true, true, true, true, true) => {
            "EXCLUSIVE_ONLY_SELECTED_FOR_FROZEN_QUALIFICATION"
        }
        ("qualification", false, _, _, _, _) => "P1M1Q_UNDERPOWERED_FAILED_CLOSED",
        ("qualification", true, true, true, true, true) => "P1M1Q_EXCLUSIVE_ONLY_QUALIFIED",
        ("qualification", _, _, _, _, _) => {
            "P1M1Q_NOT_QUALIFIED_FAILED_CLOSED_DO_NOT_TUNE_ON_THIS_CORPUS"
        }
        _ => unreachable!(),
    };
    let decision = ReplayDecision {
        contested_outcome_sufficient: contested_sufficient,
        contested_valid_actionable: contested.control_valid_actionable,
        contested_invalid_actionable: contested.control_invalid_actionable,
        zero_invalid_episodes: zero_invalid,
        zero_invalid_authority_compartments: zero_invalid_compartments,
        valid_loss_fraction,
        new_abstention_fraction,
        credit_integrity_passed: integrity,
        exclusive_only_selected_or_qualified: selected,
        status,
    };
    let receipt = ReplayReceipt {
        schema: REPLAY_SCHEMA,
        date: "2026-09-23",
        mode,
        corpus_id: corpus_id.clone(),
        corpus_path: corpus_path.display().to_string(),
        corpus_sha256: corpus_hash,
        screen_receipt_sha256: screen_hash,
        discovery_receipt_sha256: discovery_receipt_hash,
        document_count,
        event_count: events.len(),
        episode_count: episodes.len(),
        unique_plurality_detail_count: rows.len(),
        label_boundary: "post-screen validity uses only frozen candidate expected context; no BEIR qrels, query files, or retrieval judgments loaded",
        control_policy: "distinct-marker plurality with hard abstention on ties; route only same unique endpoint winner",
        candidate_policy: "exclusive-only; both endpoints must each have exactly one active distinct-marker family and the same family",
        outcomes,
        decision,
        memory: MemoryComparison {
            authority_updates_lost: (control_memory.plus_updates + control_memory.minus_updates) as i64
                - (candidate_memory.plus_updates + candidate_memory.minus_updates) as i64,
            control: control_memory,
            candidate: candidate_memory,
            traceable_owned_updates: traceable,
            deterministic_replay: deterministic,
            credit_integrity_passed: integrity,
        },
        episodes: rows,
        conclusion: if mode == "discovery" {
            "P1M1 discovery only; qualification allowed only if exclusive-only is selected"
        } else {
            "P1M1Q qualification only; no promotion, LA2-B integration, retrieval, or serving"
        },
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "P1M1 {} corpus={} unique={} exclusive={} contested={} invalid={}->{} compartments={}",
        status,
        corpus_id,
        receipt.unique_plurality_detail_count,
        receipt.outcomes.exclusive_unique,
        receipt.outcomes.contested_unique,
        receipt.outcomes.control_invalid,
        receipt.outcomes.candidate_invalid,
        receipt
            .memory
            .candidate
            .invalid_authority_compartments
            .len()
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("--screen") => lt9_la2p1m1_screen::run_screen(&args[2..]),
        Some("--replay") => replay(&args[2..]),
        _ => anyhow::bail!("usage: lt9_la2p1m1 --screen ... | --replay ..."),
    }
}

#[cfg(test)]
mod p1m1_contract_tests {
    use super::*;

    fn valid_screen() -> Value {
        serde_json::json!({
            "schema": SCREEN_SCHEMA,
            "frozen_order": lt9_la2p1m1_screen::SCREEN_ORDER,
            "min_unique_plurality": lt9_la2p1m1_screen::MIN_UNIQUE,
            "min_contested_unique": lt9_la2p1m1_screen::MIN_CONTESTED,
            "min_exclusive_unique": lt9_la2p1m1_screen::MIN_EXCLUSIVE,
            "min_contested_per_relation": lt9_la2p1m1_screen::MIN_CONTESTED_PER_RELATION,
            "min_exclusive_per_relation": lt9_la2p1m1_screen::MIN_EXCLUSIVE_PER_RELATION,
            "min_relations": lt9_la2p1m1_screen::MIN_RELATIONS,
            "min_document_shards": lt9_la2p1m1_screen::MIN_SHARDS,
        })
    }

    #[test]
    fn screen_contract_rejects_order_or_floor_drift() {
        let receipt = valid_screen();
        lt9_la2p1m1_screen::validate_frozen_screen(&receipt).unwrap();

        let mut changed_floor = receipt.clone();
        changed_floor["min_unique_plurality"] = serde_json::json!(39);
        assert!(lt9_la2p1m1_screen::validate_frozen_screen(&changed_floor).is_err());

        let mut changed_order = receipt;
        changed_order["frozen_order"] = serde_json::json!(["climate-fever", "cqadupstack", "nq"]);
        assert!(lt9_la2p1m1_screen::validate_frozen_screen(&changed_order).is_err());
    }
}
