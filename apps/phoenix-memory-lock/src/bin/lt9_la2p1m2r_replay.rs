//! LT9-LA2-P1M2: prospective multi-corpus routing and self-vote anatomy.
//! The context-only route is descriptive; no router or learner is changed.

#[allow(dead_code)]
mod lt9_la2p1m1_core;
mod lt9_la2p1m2_sufficiency;
mod lt9_la2p1m2r_identity;

use anyhow::{ensure, Context, Result};
use lt9_la2p1m1_core::*;
use lt9_la2p1m2_sufficiency::{build as build_sufficiency, SufficiencyGate};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const REPLAY_SCHEMA: &str = "phoenix.lexical.lt9-la2p1m2r-outcome-replay/v1";
const DATE: &str = "2026-09-23";
const SCREEN_SCHEMA: &str = "phoenix.lexical.lt9-la2-p1m2r-screen/v1";
const SCREEN_SHA256: &str = "bc32d310b757e8919b45de9c058ebce59892de15da2c5f11527ee800d26b18a0";
const EXPECTED_PROTOCOL_SHA256: &str =
    "cc9e9c0c205ef44a6603eb9f0c3a645258837d3e7c34cacf058200740686a2d0";
const DISCOVERY_IDS: [&str; 12] = [
    "cqadupstack",
    "arguana",
    "nfcorpus",
    "quora",
    "scidocs",
    "trec-covid",
    "scifact",
    "lotte-writing",
    "lotte-recreation",
    "lotte-science",
    "lotte-technology",
    "lotte-lifestyle",
];
const RESERVED_QUALIFICATION_ID: &str = "webis-touche2020";
const SCREEN_MIN_CONTESTED: usize = 240;
const SCREEN_MIN_CORPORA: usize = 3;
const SCREEN_MIN_RELATIONS: usize = 4;
const SCREEN_MIN_SHARDS: usize = 8;
const SHARDS: usize = 8;

fn shard(document: u64, documents: u64) -> usize {
    ((document.saturating_mul(SHARDS as u64)) / documents).min((SHARDS - 1) as u64) as usize
}

fn active_family_count(event: Event) -> usize {
    event
        .features
        .family
        .iter()
        .filter(|f| f.distinct_count() > 0)
        .count()
}

fn route_class(episode: Episode) -> &'static str {
    if active_family_count(episode.nomination) == 1 && active_family_count(episode.witness) == 1 {
        "EXCLUSIVE_UNIQUE"
    } else {
        "CONTESTED_UNIQUE"
    }
}

fn self_vote_count(candidate: usize, family: usize, bit: usize) -> u16 {
    let marker = marker_names(family)[bit];
    let spec = CANDIDATES[candidate];
    u16::from(spec.source.eq_ignore_ascii_case(marker))
        + u16::from(spec.target.eq_ignore_ascii_case(marker))
}

fn context_only_masks(event: Event) -> [u32; 3] {
    std::array::from_fn(|family| {
        marker_names(family)
            .iter()
            .enumerate()
            .fold(0u32, |mask, (bit, _)| {
                let all = event.features.family[family].marker_occurrences[bit];
                let self_votes = self_vote_count(event.candidate, family, bit);
                if all > self_votes {
                    mask | (1u32 << bit)
                } else {
                    mask
                }
            })
    })
}

fn counts_from_masks(masks: [u32; 3]) -> [u16; 3] {
    masks.map(|mask| mask.count_ones() as u16)
}

fn unique_winner(counts: [u16; 3]) -> Option<usize> {
    let max = counts.iter().copied().max()?;
    if max == 0 {
        return None;
    }
    let mut it = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == max);
    let winner = it.next()?.0;
    it.next().is_none().then_some(winner)
}

fn context_only_endpoint_route(event: Event) -> Option<usize> {
    unique_winner(counts_from_masks(context_only_masks(event)))
}

fn context_only_pair_route(episode: Episode) -> Option<usize> {
    match (
        context_only_endpoint_route(episode.nomination),
        context_only_endpoint_route(episode.witness),
    ) {
        (Some(a), Some(b)) if a == b => Some(a),
        _ => None,
    }
}

fn outcome(expected: Option<usize>, route: Option<usize>) -> &'static str {
    match (expected, route) {
        (None, _) => "UNSPECIFIED",
        (Some(_), None) => "ABSTAIN",
        (Some(e), Some(r)) if e == r => "VALID",
        (Some(_), Some(_)) => "INVALID",
    }
}

fn expected_family(candidate: usize) -> Option<usize> {
    let expected = CANDIDATES[candidate].expected_phi;
    (expected >= 0).then_some(expected as usize)
}

fn crop(values: &[u16; MAX_MARKERS], len: usize) -> Vec<u16> {
    values[..len].to_vec()
}

#[derive(Serialize)]
struct FamilyEvidence {
    family: &'static str,
    all_distinct_mask: u32,
    all_distinct_count: u16,
    context_only_distinct_mask: u32,
    context_only_distinct_count: u16,
    raw_occurrences: u16,
    repeated_occurrence_surplus: u16,
    marker_order: Vec<&'static str>,
    candidate_self_vote_identities: Vec<&'static str>,
    marker_occurrences: Vec<u16>,
    before_occurrences: Vec<u16>,
    between_occurrences: Vec<u16>,
    after_occurrences: Vec<u16>,
    all_active_identities: Vec<&'static str>,
    context_only_active_identities: Vec<&'static str>,
}

fn family_evidence(event: Event, family: usize) -> FamilyEvidence {
    let features = event.features.family[family];
    let names = marker_names(family);
    let context_mask = context_only_masks(event)[family];
    let self_vote_ids = names
        .iter()
        .enumerate()
        .filter_map(|(bit, &name)| {
            (self_vote_count(event.candidate, family, bit) > 0).then_some(name)
        })
        .collect();
    let all_ids = names
        .iter()
        .enumerate()
        .filter_map(|(bit, &name)| ((features.distinct_mask & (1u32 << bit)) != 0).then_some(name))
        .collect();
    let context_ids = names
        .iter()
        .enumerate()
        .filter_map(|(bit, &name)| ((context_mask & (1u32 << bit)) != 0).then_some(name))
        .collect();
    FamilyEvidence {
        family: family_name(family),
        all_distinct_mask: features.distinct_mask,
        all_distinct_count: features.distinct_count(),
        context_only_distinct_mask: context_mask,
        context_only_distinct_count: context_mask.count_ones() as u16,
        raw_occurrences: features.raw_count,
        repeated_occurrence_surplus: features.repeated_surplus(),
        marker_order: names.to_vec(),
        candidate_self_vote_identities: self_vote_ids,
        marker_occurrences: crop(&features.marker_occurrences, names.len()),
        before_occurrences: crop(&features.before_occurrences, names.len()),
        between_occurrences: crop(&features.between_occurrences, names.len()),
        after_occurrences: crop(&features.after_occurrences, names.len()),
        all_active_identities: all_ids,
        context_only_active_identities: context_ids,
    }
}

#[derive(Serialize)]
struct EndpointEvidence {
    document: u64,
    all_distinct_family_counts: [u16; 3],
    context_only_family_counts: [u16; 3],
    all_distinct_masks: [u32; 3],
    context_only_distinct_masks: [u32; 3],
    active_family_count: usize,
    all_winner: Option<&'static str>,
    context_only_winner: Option<&'static str>,
    support_cue: bool,
    negative_cue: bool,
    term_distance: u16,
    same_field: bool,
    families: [FamilyEvidence; 3],
}

fn endpoint_evidence(event: Event) -> EndpointEvidence {
    let all_counts = event.features.counts(true);
    let context_masks = context_only_masks(event);
    let context_counts = counts_from_masks(context_masks);
    EndpointEvidence {
        document: event.doc,
        all_distinct_family_counts: all_counts,
        context_only_family_counts: context_counts,
        all_distinct_masks: std::array::from_fn(|i| event.features.family[i].distinct_mask),
        context_only_distinct_masks: context_masks,
        active_family_count: active_family_count(event),
        all_winner: unique_winner(all_counts).map(family_name),
        context_only_winner: unique_winner(context_counts).map(family_name),
        support_cue: event.features.support,
        negative_cue: event.features.negative,
        term_distance: event.features.distance,
        same_field: event.features.same_field,
        families: std::array::from_fn(|i| family_evidence(event, i)),
    }
}

fn marker_ids(mask: u32, family: usize) -> Vec<&'static str> {
    marker_names(family)
        .iter()
        .enumerate()
        .filter_map(|(bit, &name)| ((mask & (1u32 << bit)) != 0).then_some(name))
        .collect()
}

fn overlap_class(intersection: usize, union: usize) -> &'static str {
    match (intersection, union) {
        (_, 0) => "NO_WINNER_MARKERS",
        (0, _) => "NONE",
        (n, total) if n == total => "EXACT",
        _ => "PARTIAL",
    }
}

#[derive(Serialize)]
struct WinnerIdentityEvidence {
    winner_family: Option<&'static str>,
    nomination_winner_markers: Vec<&'static str>,
    witness_winner_markers: Vec<&'static str>,
    shared_winner_markers: Vec<&'static str>,
    winner_marker_overlap: &'static str,
    winner_marker_jaccard: f64,
    persistent_competing_families: Vec<&'static str>,
    identical_persistent_competitor_marker_ids: BTreeMap<String, Vec<&'static str>>,
}

fn winner_identity(episode: Episode, route: Option<usize>) -> WinnerIdentityEvidence {
    let Some(winner) = route else {
        return WinnerIdentityEvidence {
            winner_family: None,
            nomination_winner_markers: Vec::new(),
            witness_winner_markers: Vec::new(),
            shared_winner_markers: Vec::new(),
            winner_marker_overlap: "NO_WINNER_MARKERS",
            winner_marker_jaccard: 0.0,
            persistent_competing_families: Vec::new(),
            identical_persistent_competitor_marker_ids: BTreeMap::new(),
        };
    };
    let left = episode.nomination.features.family[winner].distinct_mask;
    let right = episode.witness.features.family[winner].distinct_mask;
    let shared = left & right;
    let intersection = shared.count_ones() as usize;
    let union = (left | right).count_ones() as usize;
    let mut persistent_competing_families = Vec::new();
    let mut identical_persistent_competitor_marker_ids = BTreeMap::new();
    for family in 0..3 {
        if family == winner {
            continue;
        }
        let a = episode.nomination.features.family[family].distinct_mask;
        let b = episode.witness.features.family[family].distinct_mask;
        if a != 0 && b != 0 {
            persistent_competing_families.push(family_name(family));
            identical_persistent_competitor_marker_ids
                .insert(family_name(family).to_owned(), marker_ids(a & b, family));
        }
    }
    WinnerIdentityEvidence {
        winner_family: Some(family_name(winner)),
        nomination_winner_markers: marker_ids(left, winner),
        witness_winner_markers: marker_ids(right, winner),
        shared_winner_markers: marker_ids(shared, winner),
        winner_marker_overlap: overlap_class(intersection, union),
        winner_marker_jaccard: if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        },
        persistent_competing_families,
        identical_persistent_competitor_marker_ids,
    }
}

#[derive(Default, Serialize)]
struct OutcomeCounts {
    routed_directional_episodes: usize,
    routed_physical_document_pairs: usize,
    exclusive_directional_episodes: usize,
    contested_directional_episodes: usize,
    contested_valid_all: usize,
    contested_invalid_all: usize,
    contested_actionable_valid: usize,
    contested_actionable_invalid: usize,
    contested_actionable_unresolved: usize,
    contested_actionable_physical_valid: usize,
    contested_actionable_physical_invalid: usize,
    contested_actionable_physical_mixed: usize,
    contested_actionable_context_only_valid: usize,
    contested_actionable_context_only_invalid: usize,
    contested_actionable_context_only_abstain: usize,
    self_vote_changed_route_valid: usize,
    self_vote_changed_route_invalid: usize,
    self_vote_changed_route_unresolved: usize,
    self_vote_changed_route_any: usize,
    invalid_relations: BTreeMap<String, usize>,
    valid_relations: BTreeMap<String, usize>,
    invalid_shards: Vec<usize>,
}

#[derive(Serialize)]
pub(crate) struct EpisodeEvidence {
    directional_episode_index: usize,
    physical_document_pair_id: String,
    pub(crate) relation: &'static str,
    source_term: &'static str,
    target_term: &'static str,
    pub(crate) route_class: &'static str,
    pub(crate) shard: usize,
    pub(crate) nomination_document: u64,
    pub(crate) witness_document: u64,
    document_position_delay: u64,
    candidate_opportunity_delay: u64,
    pub(crate) actionable: bool,
    witness_polarity: Option<&'static str>,
    expected_family: Option<&'static str>,
    baseline_route: Option<&'static str>,
    pub(crate) baseline_outcome: &'static str,
    context_only_route: Option<&'static str>,
    context_only_outcome: &'static str,
    self_vote_route_transition: &'static str,
    same_field_at_both_endpoints: bool,
    winner_identity: WinnerIdentityEvidence,
    control_owned_witness: bool,
    resulting_authority_compartment: Option<String>,
    baseline_invalid_compartment: bool,
    nomination: EndpointEvidence,
    witness: EndpointEvidence,
}

#[derive(Serialize)]
pub(crate) struct CorpusReplay {
    pub(crate) corpus_id: String,
    corpus_path: String,
    archive_path: String,
    archive_sha256: String,
    corpus_sha256: String,
    document_count: u64,
    event_count: usize,
    directional_episode_count: usize,
    physical_document_pair_count: usize,
    outcomes: OutcomeCounts,
    baseline_memory: ReplaySummary,
    pub(crate) episodes: Vec<EpisodeEvidence>,
}

#[derive(Serialize)]
struct ReplayReceipt {
    schema: &'static str,
    date: &'static str,
    scope: &'static str,
    protocol_sha256: String,
    prepared_manifest_sha256: String,
    screen_receipt_sha256: String,
    discovery_ids: Vec<&'static str>,
    reserved_qualification_id: &'static str,
    outcome_boundary: &'static str,
    validity_labels_opened: bool,
    discovery_cohort_complete: bool,
    qrels_or_queries_opened: bool,
    reserved_qualification_labels_opened: bool,
    router_selected: bool,
    retrieval_or_ranking_run: bool,
    sufficiency: SufficiencyGate,
    corpora: Vec<CorpusReplay>,
    conclusion: &'static str,
}

fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open archive {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        hasher.update(&chunk[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn validate_screen(path: &Path) -> Result<(Value, String)> {
    let bytes = std::fs::read(path).with_context(|| format!("read screen {}", path.display()))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let receipt: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        receipt["schema"].as_str() == Some(SCREEN_SCHEMA),
        "unexpected P1M2R screen schema"
    );
    ensure!(
        receipt["corpus_text_only"] == true,
        "screen was not corpus-text-only"
    );
    ensure!(
        receipt["validity_labels_opened"] == false,
        "screen opened validity outcomes"
    );
    ensure!(
        receipt["qrels_or_queries_opened"] == false,
        "screen opened qrels or queries"
    );
    ensure!(
        receipt["reserved_qualification_labels_opened"] == false,
        "screen opened reserved qualification outcomes"
    );
    ensure!(
        receipt["frozen_order"] == serde_json::json!(DISCOVERY_IDS),
        "screen corpus order drift"
    );
    ensure!(
        receipt["reserved_qualification_id"].as_str() == Some(RESERVED_QUALIFICATION_ID),
        "screen qualification reservation drift"
    );
    ensure!(
        receipt["structural_preflight"]["passed"] == true,
        "label outcome replay is closed because the frozen structural preflight failed"
    );
    for (field, expected) in [
        ("minimum_contested_episodes", SCREEN_MIN_CONTESTED),
        ("minimum_corpora_with_15_contested", SCREEN_MIN_CORPORA),
        ("minimum_candidate_relations", SCREEN_MIN_RELATIONS),
        ("minimum_corpus_shard_cells", SCREEN_MIN_SHARDS),
    ] {
        ensure!(
            receipt["structural_preflight"][field].as_u64() == Some(expected as u64),
            "structural preflight floor drift in {field}"
        );
    }
    let rows = receipt["corpora"]
        .as_array()
        .context("missing corpus rows")?;
    ensure!(
        rows.len() == DISCOVERY_IDS.len(),
        "screen corpus count drift"
    );
    for (row, id) in rows.iter().zip(DISCOVERY_IDS) {
        ensure!(
            row["corpus_id"].as_str() == Some(id),
            "screen row order drift"
        );
        ensure!(
            row["role"].as_str() == Some("P1M2R_DISCOVERY"),
            "screen role drift for {id}"
        );
    }
    ensure!(
        hash == SCREEN_SHA256,
        "screen does not match the frozen P1M2R receipt identity"
    );
    Ok((receipt, hash))
}

fn outcome_counts(rows: &[EpisodeEvidence]) -> OutcomeCounts {
    let mut out = OutcomeCounts::default();
    let mut physical_pairs = BTreeSet::new();
    let mut physical_outcomes = BTreeMap::<(u64, u64), (bool, bool)>::new();
    for row in rows {
        out.routed_directional_episodes += 1;
        physical_pairs.insert((row.nomination_document, row.witness_document));
        match row.route_class {
            "EXCLUSIVE_UNIQUE" => out.exclusive_directional_episodes += 1,
            _ => {
                out.contested_directional_episodes += 1;
                match row.baseline_outcome {
                    "VALID" => out.contested_valid_all += 1,
                    "INVALID" => out.contested_invalid_all += 1,
                    _ => {}
                }
            }
        }
        if row.actionable && row.route_class == "CONTESTED_UNIQUE" {
            let state = physical_outcomes
                .entry((row.nomination_document, row.witness_document))
                .or_default();
            match row.baseline_outcome {
                "VALID" => {
                    state.0 = true;
                    out.contested_actionable_valid += 1;
                    *out.valid_relations
                        .entry(row.relation.to_owned())
                        .or_default() += 1;
                }
                "INVALID" => {
                    state.1 = true;
                    out.contested_actionable_invalid += 1;
                    *out.invalid_relations
                        .entry(row.relation.to_owned())
                        .or_default() += 1;
                    out.invalid_shards.push(row.shard);
                }
                _ => out.contested_actionable_unresolved += 1,
            }
            match row.context_only_outcome {
                "VALID" => out.contested_actionable_context_only_valid += 1,
                "INVALID" => out.contested_actionable_context_only_invalid += 1,
                _ => out.contested_actionable_context_only_abstain += 1,
            }
            if row.self_vote_route_transition != "UNCHANGED_ROUTE" {
                out.self_vote_changed_route_any += 1;
                match row.baseline_outcome {
                    "VALID" => out.self_vote_changed_route_valid += 1,
                    "INVALID" => out.self_vote_changed_route_invalid += 1,
                    _ => out.self_vote_changed_route_unresolved += 1,
                }
            }
        }
    }
    out.routed_physical_document_pairs = physical_pairs.len();
    for (valid, invalid) in physical_outcomes.into_values() {
        match (valid, invalid) {
            (true, false) => out.contested_actionable_physical_valid += 1,
            (false, true) => out.contested_actionable_physical_invalid += 1,
            (true, true) => out.contested_actionable_physical_mixed += 1,
            (false, false) => {}
        }
    }
    out.invalid_shards.sort_unstable();
    out.invalid_shards.dedup();
    out
}

fn route_transition(before: Option<usize>, after: Option<usize>) -> &'static str {
    match (before, after) {
        (Some(a), Some(b)) if a == b => "UNCHANGED_ROUTE",
        (Some(_), Some(_)) => "ROUTE_CHANGED",
        (Some(_), None) => "ROUTE_TO_ABSTAIN",
        (None, Some(_)) => "ABSTAIN_TO_ROUTE",
        (None, None) => "UNCHANGED_ABSTAIN",
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

fn replay_corpus(screen_row: &Value) -> Result<CorpusReplay> {
    let corpus_id = screen_row["corpus_id"]
        .as_str()
        .context("missing corpus id")?
        .to_owned();
    let corpus_path = screen_row["corpus_path"]
        .as_str()
        .context("missing corpus path")?;
    let archive_path = screen_row["archive_path"]
        .as_str()
        .context("missing archive path")?;
    let archive_path_buf = Path::new(archive_path);
    let corpus_path_buf = Path::new(corpus_path);
    let archive_sha256 = hash_file(archive_path_buf)?;
    ensure!(
        screen_row["archive_sha256"].as_str() == Some(archive_sha256.as_str()),
        "archive hash changed for {corpus_id}"
    );
    let (events, document_count, corpus_sha256) = load_events(corpus_path_buf)?;
    ensure!(
        screen_row["corpus_sha256"].as_str() == Some(corpus_sha256.as_str()),
        "corpus hash changed for {corpus_id}"
    );
    let episodes = make_episodes(&events);
    let ordinals = candidate_opportunity_ordinals(&events);
    let (memory, trace) = run_credit_replay_traced(&episodes, true, TiePolicy::HardAbstain);
    ensure!(
        trace.len() == episodes.len(),
        "credit trace length mismatch"
    );
    let mut rows = Vec::new();
    for (episode_index, episode) in episodes.iter().copied().enumerate() {
        let baseline_route = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::HardAbstain,
        );
        let Some(route) = baseline_route else {
            continue;
        };
        let context_route = context_only_pair_route(episode);
        let expected = expected_family(episode.candidate);
        let step = &trace[episode_index];
        let relation = CANDIDATES[episode.candidate];
        let context_transition = route_transition(baseline_route, context_route);
        let row_shard = shard(episode.nomination.doc, document_count);
        let opportunity_delay = ordinals
            .get(&(episode.candidate, episode.nomination.doc))
            .zip(ordinals.get(&(episode.candidate, episode.witness.doc)))
            .map(|(n, w)| w.saturating_sub(*n))
            .unwrap_or(0);
        let actual_outcome = outcome(expected, baseline_route);
        let invalid_compartment = expected.is_some_and(|e| e != route);
        let compartment_name = format!("{}@{}", relation.id, family_name(route));
        let invalid_authority_compartment = step.owned_witness
            && memory
                .invalid_authority_compartments
                .iter()
                .any(|name| name == &compartment_name);
        rows.push(EpisodeEvidence {
            directional_episode_index: episode_index,
            physical_document_pair_id: format!(
                "s{row_shard}:{}->{}",
                episode.nomination.doc, episode.witness.doc
            ),
            relation: relation.id,
            source_term: relation.source,
            target_term: relation.target,
            route_class: route_class(episode),
            shard: row_shard,
            nomination_document: episode.nomination.doc,
            witness_document: episode.witness.doc,
            document_position_delay: episode.witness.doc.saturating_sub(episode.nomination.doc),
            candidate_opportunity_delay: opportunity_delay,
            actionable: step.witness_polarity.is_some(),
            witness_polarity: step.witness_polarity,
            expected_family: expected.map(family_name),
            baseline_route: Some(family_name(route)),
            baseline_outcome: actual_outcome,
            context_only_route: context_route.map(family_name),
            context_only_outcome: outcome(expected, context_route),
            self_vote_route_transition: context_transition,
            same_field_at_both_endpoints: episode.nomination.features.same_field
                == episode.witness.features.same_field,
            winner_identity: winner_identity(episode, baseline_route),
            control_owned_witness: step.owned_witness,
            resulting_authority_compartment: step.owned_witness.then(|| compartment_name.clone()),
            baseline_invalid_compartment: invalid_compartment && invalid_authority_compartment,
            nomination: endpoint_evidence(episode.nomination),
            witness: endpoint_evidence(episode.witness),
        });
    }
    let outcomes = outcome_counts(&rows);
    Ok(CorpusReplay {
        corpus_id,
        corpus_path: corpus_path.to_owned(),
        archive_path: archive_path.to_owned(),
        archive_sha256,
        corpus_sha256,
        document_count,
        event_count: events.len(),
        directional_episode_count: episodes.len(),
        physical_document_pair_count: episodes
            .iter()
            .map(|e| (e.nomination.doc, e.witness.doc))
            .collect::<BTreeSet<_>>()
            .len(),
        outcomes,
        baseline_memory: memory,
        episodes: rows,
    })
}

fn replay(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 4,
        "usage: --replay <protocol.md> <prepared-manifest.json> <screen.json> <output.json>"
    );
    let protocol_path = Path::new(&args[0]);
    let manifest_path = Path::new(&args[1]);
    let screen_path = Path::new(&args[2]);
    let output = Path::new(&args[3]);
    let (screen, screen_hash) = validate_screen(screen_path)?;
    let prepared_manifest_hash = lt9_la2p1m2r_identity::validate_before_replay(
        protocol_path,
        manifest_path,
        screen_path,
        &screen,
        &screen_hash,
    )?;
    let screen_rows = screen["corpora"]
        .as_array()
        .context("missing screen corpus array")?;
    let mut corpora = Vec::with_capacity(DISCOVERY_IDS.len());
    for id in DISCOVERY_IDS {
        let row = screen_rows
            .iter()
            .find(|row| row["corpus_id"].as_str() == Some(id))
            .with_context(|| format!("discovery corpus {id} missing from screen"))?;
        ensure!(
            row["role"].as_str() == Some("P1M2R_DISCOVERY"),
            "role drift for {id}"
        );
        let corpus = replay_corpus(row)?;
        println!(
            "{}: routed={} contested={} valid={} invalid={} invalid_compartments={}",
            corpus.corpus_id,
            corpus.outcomes.routed_directional_episodes,
            corpus.outcomes.contested_directional_episodes,
            corpus.outcomes.contested_actionable_valid,
            corpus.outcomes.contested_actionable_invalid,
            corpus.baseline_memory.invalid_authority_compartments.len()
        );
        corpora.push(corpus);
    }
    let sufficiency = build_sufficiency(&corpora);
    let conclusion = if sufficiency.sufficient {
        "P1M2R_DISCOVERY_COHORT_SUFFICIENT; anatomy only; no router selection or qualification"
    } else {
        "P1M2R_DISCOVERY_COHORT_OUTCOME_UNDERPOWERED; no router selection; any expansion needs a new sealed protocol"
    };
    let receipt = ReplayReceipt {
        schema: REPLAY_SCHEMA,
        date: DATE,
        scope: "frozen twelve-corpus P1M2R discovery; current hard-abstain distinct-plurality router only; anatomy only; no router or learner update",
        protocol_sha256: EXPECTED_PROTOCOL_SHA256.to_owned(),
        prepared_manifest_sha256: prepared_manifest_hash,
        screen_receipt_sha256: screen_hash,
        discovery_ids: DISCOVERY_IDS.to_vec(),
        reserved_qualification_id: RESERVED_QUALIFICATION_ID,
        outcome_boundary: "candidate expected-context labels inspected only after the prepared replay manifest and all twelve corpus/archive identities are sealed; no BEIR qrels, queries, relevance labels, or LoTTE QAS data are loaded",
        validity_labels_opened: true,
        discovery_cohort_complete: true,
        qrels_or_queries_opened: false,
        reserved_qualification_labels_opened: false,
        router_selected: false,
        retrieval_or_ranking_run: false,
        sufficiency,
        corpora,
        conclusion,
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("--prepare") => {
            ensure!(
                args.len() == 6,
                "usage: --prepare <protocol.md> <screen.json> <source.rs> <manifest.json>"
            );
            lt9_la2p1m2r_identity::prepare(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
            )
        }
        Some("--replay") => replay(&args[2..]),
        _ => anyhow::bail!("usage: lt9_la2p1m2r_replay --prepare ... | --replay ..."),
    }
}

#[cfg(test)]
#[path = "lt9_la2p1m2_tests.rs"]
mod tests;
