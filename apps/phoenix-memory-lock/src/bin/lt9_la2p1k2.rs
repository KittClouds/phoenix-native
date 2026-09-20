//! LT9-LA2-P1K2: unopened-corpus qualification of phenotype assignment.
//!
//! Replays the frozen marker and witness semantics over SciFact, and emits
//! endpoint assignment diagnostics plus one plurality/tie-abstain comparison.
//! It never updates learner authority or changes serving.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1k1/v1";
const SHARDS: usize = 8;
const MAX_PAIR_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
const PHENOTYPES: usize = 4;
const FINANCE: &[&str] = &[
    "finance",
    "financial",
    "bank",
    "loan",
    "credit",
    "debt",
    "interest",
    "mortgage",
    "lender",
    "investment",
    "stock",
    "bond",
    "payment",
    "account",
    "money",
    "insurance",
    "coverage",
];
const GEO: &[&str] = &[
    "river",
    "shore",
    "coast",
    "beach",
    "water",
    "land",
    "geography",
    "geographic",
    "ocean",
    "lake",
];
const TRANSPORT: &[&str] = &[
    "car", "vehicle", "engine", "motor", "auto", "driver", "road", "truck", "tire", "traffic",
];
const SUPPORT: &[&str] = &[
    "also",
    "called",
    "known",
    "means",
    "aka",
    "similar",
    "equivalent",
    "same",
    "like",
];
const NEGATIVE: &[&str] = &[
    "not",
    "never",
    "unlike",
    "different",
    "rather",
    "instead",
    "versus",
    "vs",
    "without",
];

#[derive(Clone, Copy, Debug)]
struct Candidate {
    id: &'static str,
    source: &'static str,
    target: &'static str,
    expected_phi: i8,
}

const CANDIDATES: [Candidate; 12] = [
    Candidate {
        id: "repair_to_fix",
        source: "repair",
        target: "fix",
        expected_phi: -1,
    },
    Candidate {
        id: "engine_to_motor",
        source: "engine",
        target: "motor",
        expected_phi: 2,
    },
    Candidate {
        id: "car_to_vehicle",
        source: "car",
        target: "vehicle",
        expected_phi: 2,
    },
    Candidate {
        id: "vehicle_to_car",
        source: "vehicle",
        target: "car",
        expected_phi: 2,
    },
    Candidate {
        id: "bank_to_shore",
        source: "bank",
        target: "shore",
        expected_phi: 1,
    },
    Candidate {
        id: "bank_to_lender",
        source: "bank",
        target: "lender",
        expected_phi: 0,
    },
    Candidate {
        id: "economic_to_tumor",
        source: "economic",
        target: "tumor",
        expected_phi: -1,
    },
    Candidate {
        id: "loan_to_debt",
        source: "loan",
        target: "debt",
        expected_phi: 0,
    },
    Candidate {
        id: "credit_to_loan",
        source: "credit",
        target: "loan",
        expected_phi: 0,
    },
    Candidate {
        id: "insurance_to_coverage",
        source: "insurance",
        target: "coverage",
        expected_phi: 0,
    },
    Candidate {
        id: "stock_to_bond",
        source: "stock",
        target: "bond",
        expected_phi: 0,
    },
    Candidate {
        id: "bank_to_water",
        source: "bank",
        target: "water",
        expected_phi: 1,
    },
];

#[derive(Clone, Debug, Deserialize)]
struct CorpusDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
struct ContextFeatures {
    counts: [u16; 3],
    marker_masks: [u32; 3],
    before_masks: [u32; 3],
    after_masks: [u32; 3],
    support_count: u16,
    negative_count: u16,
    window_len: u16,
    same_field: bool,
    fingerprint: u64,
}

impl ContextFeatures {
    fn fixed_phi(self) -> u8 {
        if self.counts[1] > 0 {
            1
        } else if self.counts[0] > 0 {
            0
        } else if self.counts[2] > 0 {
            2
        } else {
            3
        }
    }
}

fn phi_name(phi: u8) -> &'static str {
    match phi {
        0 => "finance_markers",
        1 => "geography_markers",
        2 => "transport_markers",
        _ => "general_fallback",
    }
}

#[derive(Clone, Copy, Debug)]
struct Event {
    doc: usize,
    shard: usize,
    candidate: usize,
    distance: usize,
    negative: bool,
    support: bool,
    features: ContextFeatures,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WitnessKind {
    Support,
    Contradiction,
    Abstain,
}

impl WitnessKind {
    fn label(self) -> &'static str {
        match self {
            Self::Support => "SUPPORT",
            Self::Contradiction => "CONTRADICTION",
            Self::Abstain => "ABSTAIN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JoinClass {
    ValidActionable,
    ValidAbstain,
    InvalidActionable,
    InvalidAbstain,
    Other,
}

impl JoinClass {
    fn label(self) -> &'static str {
        match self {
            Self::ValidActionable => "VALID_ACTIONABLE",
            Self::ValidAbstain => "VALID_ABSTAIN",
            Self::InvalidActionable => "INVALID_ACTIONABLE",
            Self::InvalidAbstain => "INVALID_ABSTAIN",
            Self::Other => "OTHER",
        }
    }
    fn invalid(self) -> bool {
        matches!(self, Self::InvalidActionable | Self::InvalidAbstain)
    }
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    shard: usize,
    nomination: Event,
    witness: Event,
    class: JoinClass,
    kind: WitnessKind,
}

#[derive(Clone, Debug, Deserialize)]
struct FrozenFeatureDiff {
    class: String,
    candidate: String,
    nomination_doc: usize,
    witness_doc: usize,
    witness_kind: String,
}

#[derive(Clone, Debug, Deserialize)]
struct P1jReceipt {
    schema: String,
    feature_diffs: Vec<FrozenFeatureDiff>,
}

#[derive(Clone, Debug, Deserialize)]
struct P1j3Receipt {
    schema: String,
    corpus_sha256: String,
    frozen_join_identity_parity: bool,
    frozen_join_count: usize,
    broad_population: P1j3Broad,
}

#[derive(Clone, Debug, Deserialize)]
struct P1j3Broad {
    total_pairs: usize,
    same_phenotype_pairs: usize,
    cross_phenotype_pairs: usize,
    frozen_discovery_pairs_excluded: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct PairRosterRow {
    shard: Option<usize>,
    candidate: String,
    class: String,
    witness_kind: String,
    nomination_doc: usize,
    witness_doc: usize,
    nomination_counts: [u16; 3],
    witness_counts: [u16; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Finance,
    Geography,
    Transport,
}

impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Finance => "finance",
            Self::Geography => "geography",
            Self::Transport => "transport",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct EndpointAudit {
    role: &'static str,
    document: usize,
    counts: [u16; 3],
    current_fixed_priority: String,
    raw_argmax_families: Vec<&'static str>,
    argmax_tie: bool,
    max_count: u16,
    runner_up_count: u16,
    top_runner_up_margin: u16,
    priority_disagrees_with_argmax: Option<bool>,
    priority_inversion: bool,
    plurality_route: Option<&'static str>,
    assignment_stable: bool,
}

#[derive(Clone, Debug, Serialize)]
struct PairAudit {
    shard: usize,
    episode_id: String,
    candidate: &'static str,
    nomination_doc: usize,
    witness_doc: usize,
    witness_kind: &'static str,
    current_outcome: &'static str,
    plurality_outcome: &'static str,
    endpoint_inversion_count: usize,
    pair_has_priority_inversion: bool,
    pair_assignments_stable: bool,
    current_is_finance_local: bool,
    plurality_is_finance_local: bool,
    nomination: EndpointAudit,
    witness: EndpointAudit,
}

#[derive(Clone, Debug, Serialize)]
struct ConditionalRates {
    actionable: Vec<StratumRate>,
    all_conclusive: Vec<StratumRate>,
    episode_actionable: Vec<StratumRate>,
    episode_all_conclusive: Vec<StratumRate>,
}

#[derive(Clone, Debug, Serialize)]
struct StratumRate {
    stratum: &'static str,
    invalid: usize,
    valid: usize,
    mixed_or_unclassified: usize,
    denominator: usize,
    p_invalid: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct EpisodeAudit {
    episode_id: String,
    shard: usize,
    nomination_doc: usize,
    witness_doc: usize,
    directional_pair_count: usize,
    current_invalid_actionable_directions: usize,
    current_valid_actionable_directions: usize,
    current_invalid_any_directions: usize,
    current_valid_any_directions: usize,
    any_priority_inversion: bool,
    inverted_endpoints: usize,
    current_finance_local_directions: usize,
    plurality_finance_local_directions: usize,
    plurality_tie_endpoints: usize,
    plurality_valid_directions: usize,
    plurality_invalid_directions: usize,
    plurality_tie_directions: usize,
    plurality_cross_directions: usize,
    valid_directions_becoming_tie: usize,
    any_valid_direction_changed_phenotype: bool,
}

#[derive(Clone, Debug, Serialize)]
struct RosterValidation {
    corpus_hash_matches_p1j3: bool,
    p1j3_schema: String,
    p1j3_receipt_sha256: String,
    p1j3_pair_roster_sha256: String,
    p1j3_pair_roster_run2_sha256: String,
    p1j3_pair_roster_hashes_match: bool,
    reconstructed_total_pairs: usize,
    reconstructed_same_phi_pairs: usize,
    reconstructed_cross_phi_pairs: usize,
    same_phi_rows_match_frozen_roster: bool,
    excluded_p1j_identity_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Summary {
    directional_join_count: usize,
    distinct_document_pair_episodes: usize,
    endpoint_assignment_count: usize,
    unique_argmax_endpoints: usize,
    tied_argmax_endpoints: usize,
    priority_inversion_endpoint_occurrences: usize,
    same_phi_pairs: usize,
    cross_phi_pairs: usize,
    valid_actionable_pairs: usize,
    invalid_actionable_pairs: usize,
    valid_abstain_pairs: usize,
    invalid_abstain_pairs: usize,
    other_pairs: usize,
    actionable_priority_inversion_pairs: usize,
    unique_priority_inversion_endpoints: usize,
    inversion_pairs_with_any_tie: usize,
    invalid_actionable_episodes: usize,
    invalid_actionable_directional_rows: usize,
    valid_actionable_episodes: usize,
    valid_episodes_with_any_assignment_change: usize,
    valid_episodes_with_plurality_tie: usize,
    invalid_episodes_no_longer_finance_local: usize,
    invalid_episodes_both_endpoints_leave_finance: usize,
    invalid_episodes_repaired_at_both_endpoints: usize,
    invalid_episodes_where_plurality_creates_cross_route: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    hypothesis: &'static str,
    corpus_path: String,
    corpus_sha256: String,
    corpus_document_count: usize,
    p1j_receipt_sha256: String,
    p1j3_receipt_sha256: String,
    roster_validation: RosterValidation,
    summary: Summary,
    conditional_prevalence: ConditionalRates,
    counterfactual: &'static str,
    episodes: Vec<EpisodeAudit>,
    pairs: Vec<PairAudit>,
    interpretation_boundary: &'static str,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

fn positions(words: &[&str], needle: &str) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (*w == needle).then_some(i))
        .collect()
}

fn nearest(words: &[&str], source: &str, target: &str) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for left in positions(words, source) {
        for right in positions(words, target) {
            let distance = left.abs_diff(right);
            if distance <= 40 && best.is_none_or(|old: (usize, usize, usize)| distance < old.2) {
                best = Some((left, right, distance));
            }
        }
    }
    best
}

fn marker_mask(words: &[&str], start: usize, end: usize, markers: &[&str]) -> (u16, u32) {
    let mut count = 0u16;
    let mut mask = 0u32;
    for word in &words[start..end] {
        for (index, marker) in markers.iter().enumerate() {
            if word == marker {
                count = count.saturating_add(1);
                mask |= 1u32 << index;
            }
        }
    }
    (count, mask)
}

fn fingerprint(words: &[&str], start: usize, end: usize, source: &str, target: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for word in &words[start..end] {
        if *word == source || *word == target {
            continue;
        }
        for byte in word.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn context_features(
    words: &[&str],
    left: usize,
    right: usize,
    title_len: usize,
    source: &str,
    target: &str,
) -> ContextFeatures {
    let low = left.min(right).saturating_sub(8);
    let high = (left.max(right) + 9).min(words.len());
    let split = left.min(right);
    let after_split = (left.max(right) + 1).min(words.len());
    let (finance, finance_mask) = marker_mask(words, low, high, FINANCE);
    let (geo, geo_mask) = marker_mask(words, low, high, GEO);
    let (transport, transport_mask) = marker_mask(words, low, high, TRANSPORT);
    let (_, finance_before) = marker_mask(words, low, split, FINANCE);
    let (_, geo_before) = marker_mask(words, low, split, GEO);
    let (_, transport_before) = marker_mask(words, low, split, TRANSPORT);
    let (_, finance_after) = marker_mask(words, after_split, high, FINANCE);
    let (_, geo_after) = marker_mask(words, after_split, high, GEO);
    let (_, transport_after) = marker_mask(words, after_split, high, TRANSPORT);
    let (support_count, _) = marker_mask(words, low, high, SUPPORT);
    let (negative_count, _) = marker_mask(words, low, high, NEGATIVE);
    ContextFeatures {
        counts: [finance, geo, transport],
        marker_masks: [finance_mask, geo_mask, transport_mask],
        before_masks: [finance_before, geo_before, transport_before],
        after_masks: [finance_after, geo_after, transport_after],
        support_count,
        negative_count,
        window_len: (high - low) as u16,
        same_field: (left < title_len) == (right < title_len),
        fingerprint: fingerprint(words, low, high, source, target),
    }
}

fn near_any(words: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(words.len());
    words[start..end]
        .iter()
        .any(|word| markers.iter().any(|marker| word == marker))
}

fn load_events(path: &Path) -> Result<(Vec<Event>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = digest(&bytes);
    let mut events = Vec::new();
    let mut docs = 0usize;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode FiQA corpus record")?;
        let title_len = words(&doc.title.to_ascii_lowercase()).len();
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let tokenized = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&tokenized, spec.source, spec.target) {
                events.push(Event {
                    doc: docs,
                    shard: docs * SHARDS,
                    candidate,
                    distance,
                    negative: near_any(&tokenized, left, right, NEGATIVE),
                    support: near_any(&tokenized, left, right, SUPPORT),
                    features: context_features(
                        &tokenized,
                        left,
                        right,
                        title_len,
                        spec.source,
                        spec.target,
                    ),
                });
            }
        }
        docs += 1;
    }
    ensure!(docs > 0, "corpus is empty");
    for event in &mut events {
        event.shard = (event.shard / docs).min(SHARDS - 1);
    }
    Ok((events, docs, hash))
}

fn witness_kind(event: Event) -> WitnessKind {
    if event.negative {
        WitnessKind::Contradiction
    } else if event.support || event.distance <= SUPPORT_DISTANCE {
        WitnessKind::Support
    } else {
        WitnessKind::Abstain
    }
}

fn classify(spec: Candidate, nomination: Event, witness: Event, kind: WitnessKind) -> JoinClass {
    let nomination_phi = nomination.features.fixed_phi();
    let witness_phi = witness.features.fixed_phi();
    if nomination_phi != witness_phi {
        return JoinClass::Other;
    }
    let valid = spec.expected_phi < 0
        || (nomination_phi == spec.expected_phi as u8 && witness_phi == spec.expected_phi as u8);
    match (valid, kind) {
        (true, WitnessKind::Abstain) => JoinClass::ValidAbstain,
        (true, _) => JoinClass::ValidActionable,
        (false, WitnessKind::Abstain) => JoinClass::InvalidAbstain,
        (false, _) => JoinClass::InvalidActionable,
    }
}

fn pairs(events: &[Event], shard: Option<usize>) -> Vec<Pair> {
    let normalized: Vec<Event> = events
        .iter()
        .copied()
        .filter(|e| e.distance <= MAX_PAIR_DISTANCE && shard.is_none_or(|s| e.shard == s))
        .collect();
    let mut seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut out = Vec::new();
    for (index, event) in normalized.iter().enumerate() {
        let key = event.candidate * PHENOTYPES + usize::from(event.features.fixed_phi());
        if seen[key] {
            continue;
        }
        seen[key] = true;
        let later = normalized
            .iter()
            .skip(index + 1)
            .copied()
            .filter(|candidate| candidate.candidate == event.candidate && candidate.doc > event.doc)
            .collect::<Vec<_>>();
        let witness = later
            .iter()
            .copied()
            .find(|candidate| candidate.features.fixed_phi() == event.features.fixed_phi())
            .or_else(|| later.first().copied());
        if let Some(witness) = witness {
            let kind = witness_kind(witness);
            out.push(Pair {
                shard: shard.unwrap_or(event.shard),
                nomination: *event,
                witness,
                class: classify(CANDIDATES[event.candidate], *event, witness, kind),
                kind,
            });
        }
    }
    out
}

fn family_for_phi(phi: u8) -> Option<Family> {
    match phi {
        0 => Some(Family::Finance),
        1 => Some(Family::Geography),
        2 => Some(Family::Transport),
        _ => None,
    }
}

fn assignment(features: ContextFeatures, role: &'static str, document: usize) -> EndpointAudit {
    let counts = features.counts;
    let max_count = *counts.iter().max().unwrap_or(&0);
    let argmax: Vec<Family> = [Family::Finance, Family::Geography, Family::Transport]
        .into_iter()
        .zip(counts)
        .filter_map(|(family, count)| (count == max_count).then_some(family))
        .collect();
    let tie = argmax.len() != 1;
    let mut ranked = counts;
    ranked.sort_unstable_by(|a, b| b.cmp(a));
    let runner_up_count = ranked.get(1).copied().unwrap_or(0);
    let current_phi = features.fixed_phi();
    let current_family = family_for_phi(current_phi);
    let raw_unique = (!tie).then(|| argmax[0]);
    let priority_disagrees = raw_unique.map(|winner| current_family != Some(winner));
    EndpointAudit {
        role,
        document,
        counts,
        current_fixed_priority: phi_name(current_phi).to_owned(),
        raw_argmax_families: argmax.iter().map(|family| family.name()).collect(),
        argmax_tie: tie,
        max_count,
        runner_up_count,
        top_runner_up_margin: max_count.saturating_sub(runner_up_count),
        priority_disagrees_with_argmax: priority_disagrees,
        priority_inversion: priority_disagrees == Some(true),
        plurality_route: raw_unique.map(Family::name),
        assignment_stable: priority_disagrees == Some(false),
    }
}

fn plurality_class(
    spec: Candidate,
    nomination: Event,
    witness: Event,
    kind: WitnessKind,
) -> &'static str {
    let nomination_phi = assignment(nomination.features, "nomination", nomination.doc)
        .plurality_route
        .and_then(|name| match name {
            "finance" => Some(0),
            "geography" => Some(1),
            "transport" => Some(2),
            _ => None,
        });
    let witness_phi = assignment(witness.features, "witness", witness.doc)
        .plurality_route
        .and_then(|name| match name {
            "finance" => Some(0),
            "geography" => Some(1),
            "transport" => Some(2),
            _ => None,
        });
    let (Some(nomination_phi), Some(witness_phi)) = (nomination_phi, witness_phi) else {
        return "ABSTAIN_TIE";
    };
    if nomination_phi != witness_phi {
        return "CROSS_PHENOTYPE";
    }
    let valid = spec.expected_phi < 0 || nomination_phi == spec.expected_phi as u8;
    match (valid, kind) {
        (true, WitnessKind::Abstain) => "VALID_ABSTAIN",
        (true, _) => "VALID_ACTIONABLE",
        (false, WitnessKind::Abstain) => "INVALID_ABSTAIN",
        (false, _) => "INVALID_ACTIONABLE",
    }
}

fn episode_key(pair: &Pair) -> (usize, usize, usize) {
    (pair.shard, pair.nomination.doc, pair.witness.doc)
}

fn pair_audit(pair: Pair) -> PairAudit {
    let nomination = assignment(pair.nomination.features, "nomination", pair.nomination.doc);
    let witness = assignment(pair.witness.features, "witness", pair.witness.doc);
    let endpoint_inversion_count =
        usize::from(nomination.priority_inversion) + usize::from(witness.priority_inversion);
    let cf_outcome = plurality_class(
        CANDIDATES[pair.nomination.candidate],
        pair.nomination,
        pair.witness,
        pair.kind,
    );
    let plurality_finance_local =
        nomination.plurality_route == Some("finance") && witness.plurality_route == Some("finance");
    PairAudit {
        shard: pair.shard,
        episode_id: format!(
            "s{}:{}->{}",
            pair.shard, pair.nomination.doc, pair.witness.doc
        ),
        candidate: CANDIDATES[pair.nomination.candidate].id,
        nomination_doc: pair.nomination.doc,
        witness_doc: pair.witness.doc,
        witness_kind: pair.kind.label(),
        current_outcome: pair.class.label(),
        plurality_outcome: cf_outcome,
        endpoint_inversion_count,
        pair_has_priority_inversion: endpoint_inversion_count > 0,
        pair_assignments_stable: nomination.assignment_stable && witness.assignment_stable,
        current_is_finance_local: pair.nomination.features.fixed_phi() == 0
            && pair.witness.features.fixed_phi() == 0,
        plurality_is_finance_local: plurality_finance_local,
        nomination,
        witness,
    }
}

fn rows_from_pairs(pairs: &[Pair]) -> Vec<PairRosterRow> {
    pairs
        .iter()
        .map(|pair| PairRosterRow {
            shard: Some(pair.shard),
            candidate: CANDIDATES[pair.nomination.candidate].id.to_owned(),
            class: pair.class.label().to_owned(),
            witness_kind: pair.kind.label().to_owned(),
            nomination_doc: pair.nomination.doc,
            witness_doc: pair.witness.doc,
            nomination_counts: pair.nomination.features.counts,
            witness_counts: pair.witness.features.counts,
        })
        .collect()
}

fn roster_key(
    row: &PairRosterRow,
) -> (
    usize,
    String,
    usize,
    usize,
    String,
    String,
    [u16; 3],
    [u16; 3],
) {
    (
        row.shard.unwrap_or(usize::MAX),
        row.candidate.clone(),
        row.nomination_doc,
        row.witness_doc,
        row.class.clone(),
        row.witness_kind.clone(),
        row.nomination_counts,
        row.witness_counts,
    )
}

fn rate(stratum: &'static str, rows: impl Iterator<Item = (bool, bool)>) -> StratumRate {
    let (mut invalid, mut valid, mut mixed) = (0usize, 0usize, 0usize);
    for (is_invalid, is_valid) in rows {
        match (is_invalid, is_valid) {
            (true, false) => invalid += 1,
            (false, true) => valid += 1,
            _ => mixed += 1,
        }
    }
    let denominator = invalid + valid;
    StratumRate {
        stratum,
        invalid,
        valid,
        mixed_or_unclassified: mixed,
        denominator,
        p_invalid: (denominator > 0).then_some(invalid as f64 / denominator as f64),
    }
}

fn count_episode(pairs: &[PairAudit], key: (usize, usize, usize)) -> EpisodeAudit {
    let rows: Vec<&PairAudit> = pairs
        .iter()
        .filter(|row| (row.shard, row.nomination_doc, row.witness_doc) == key)
        .collect();
    let mut episode = EpisodeAudit {
        episode_id: format!("s{}:{}->{}", key.0, key.1, key.2),
        shard: key.0,
        nomination_doc: key.1,
        witness_doc: key.2,
        directional_pair_count: rows.len(),
        current_invalid_actionable_directions: 0,
        current_valid_actionable_directions: 0,
        current_invalid_any_directions: 0,
        current_valid_any_directions: 0,
        any_priority_inversion: false,
        inverted_endpoints: 0,
        current_finance_local_directions: 0,
        plurality_finance_local_directions: 0,
        plurality_tie_endpoints: 0,
        plurality_valid_directions: 0,
        plurality_invalid_directions: 0,
        plurality_tie_directions: 0,
        plurality_cross_directions: 0,
        valid_directions_becoming_tie: 0,
        any_valid_direction_changed_phenotype: false,
    };
    for row in rows {
        episode.current_invalid_actionable_directions +=
            usize::from(row.current_outcome == "INVALID_ACTIONABLE");
        episode.current_valid_actionable_directions +=
            usize::from(row.current_outcome == "VALID_ACTIONABLE");
        episode.current_invalid_any_directions +=
            usize::from(row.current_outcome.starts_with("INVALID"));
        episode.current_valid_any_directions +=
            usize::from(row.current_outcome.starts_with("VALID"));
        episode.any_priority_inversion |= row.pair_has_priority_inversion;
        episode.inverted_endpoints += row.endpoint_inversion_count;
        episode.current_finance_local_directions += usize::from(row.current_is_finance_local);
        episode.plurality_finance_local_directions += usize::from(row.plurality_is_finance_local);
        episode.plurality_tie_endpoints += usize::from(row.nomination.plurality_route.is_none())
            + usize::from(row.witness.plurality_route.is_none());
        episode.plurality_valid_directions +=
            usize::from(row.plurality_outcome.starts_with("VALID"));
        episode.plurality_invalid_directions +=
            usize::from(row.plurality_outcome.starts_with("INVALID"));
        episode.plurality_tie_directions += usize::from(row.plurality_outcome == "ABSTAIN_TIE");
        episode.plurality_cross_directions +=
            usize::from(row.plurality_outcome == "CROSS_PHENOTYPE");
        if row.current_outcome.starts_with("VALID") {
            episode.any_valid_direction_changed_phenotype |=
                !row.nomination.assignment_stable || !row.witness.assignment_stable;
            episode.valid_directions_becoming_tie +=
                usize::from(row.plurality_outcome == "ABSTAIN_TIE");
        }
    }
    episode
}

fn main_run() -> Result<Receipt> {
    let args: Vec<String> = env::args().collect();
    let corpus_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned());
    let p1j_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1j\\lt9-la2p1j-receipt.json".to_owned());
    let p1j3_path = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1j3\\lt9-la2p1j3-receipt.json".to_owned());
    let roster_path = args.get(4).cloned().unwrap_or_else(|| {
        "D:\\phoenix-evals\\lt9-la2p1j3\\lt9-la2p1j3-pair-context-run1.json".to_owned()
    });
    let (events, document_count, corpus_sha256) = load_events(Path::new(&corpus_path))?;
    let p1j_bytes = fs::read(&p1j_path).with_context(|| format!("read {p1j_path}"))?;
    let p1j: P1jReceipt =
        serde_json::from_slice(&p1j_bytes).context("decode P1J discovery receipt")?;
    let p1j3_bytes = fs::read(&p1j3_path).with_context(|| format!("read {p1j3_path}"))?;
    let p1j3: P1j3Receipt =
        serde_json::from_slice(&p1j3_bytes).context("decode P1J3 sealed receipt")?;
    let roster_bytes = fs::read(&roster_path).with_context(|| format!("read {roster_path}"))?;
    let roster: Vec<PairRosterRow> =
        serde_json::from_slice(&roster_bytes).context("decode P1J3 frozen same-phi roster")?;
    let roster_run2_path =
        Path::new(&roster_path).with_file_name("lt9-la2p1j3-pair-context-run2.json");
    let roster_run2_bytes = fs::read(&roster_run2_path)
        .with_context(|| format!("read {}", roster_run2_path.display()))?;
    let p1j_manifest_bytes =
        fs::read("D:\\phoenix-evals\\lt9-la2p1j3\\lt9-la2p1j3-feature-audit-manifest.json")
            .context("read sealed P1J3 feature audit manifest")?;
    let manifest: serde_json::Value = serde_json::from_slice(&p1j_manifest_bytes)?;
    let artifact_manifest_bytes =
        fs::read("D:\\phoenix-evals\\lt9-la2p1j3\\lt9-la2p1j3-artifact-manifest.json")
            .context("read sealed P1J3 artifact manifest")?;
    let artifact_manifest: serde_json::Value = serde_json::from_slice(&artifact_manifest_bytes)?;
    let p1j_hash = digest(&p1j_bytes);
    let p1j3_hash = digest(&p1j3_bytes);
    ensure!(
        p1j.schema == "phoenix.lexical.lt9la2p1j/v1",
        "unexpected P1J schema"
    );
    let corpus_hash_matches = p1j3.corpus_sha256 == corpus_sha256;
    ensure!(
        corpus_hash_matches,
        "corpus hash differs from sealed P1J3 receipt"
    );
    ensure!(
        p1j3.schema == "phoenix.lexical.lt9la2p1j3/v1",
        "unexpected P1J3 schema"
    );
    ensure!(
        p1j3.frozen_join_identity_parity
            && p1j3.frozen_join_count == 14
            && p1j.feature_diffs.len() == 14,
        "sealed P1J/P1J3 14-join discovery parity is not intact"
    );
    let expected_roster_hash = manifest["files"]["pair_roster_run1"]["sha256"]
        .as_str()
        .unwrap_or("");
    let expected_roster_run2_hash = manifest["files"]["pair_roster_run2"]["sha256"]
        .as_str()
        .unwrap_or("");
    let expected_p1j_hash = artifact_manifest["files"]["p1j_receipt"]["sha256"]
        .as_str()
        .unwrap_or("");
    let expected_p1j3_hash = manifest["files"]["gate_receipt"]["sha256"]
        .as_str()
        .unwrap_or("");
    let actual_roster_hash = digest(&roster_bytes);
    let run2_hash = digest(&roster_run2_bytes);
    ensure!(
        expected_p1j_hash == p1j_hash && expected_p1j3_hash == p1j3_hash,
        "P1J or P1J3 receipt hash differs from the sealed feature audit manifest"
    );
    let roster_hashes_match = !expected_roster_hash.is_empty()
        && expected_roster_hash == actual_roster_hash
        && expected_roster_run2_hash == run2_hash
        && actual_roster_hash == run2_hash;
    ensure!(
        roster_hashes_match,
        "P1J3 pair roster hash/rerun parity failed"
    );

    let full_global = pairs(&events, None);
    let same_global: Vec<Pair> = full_global
        .iter()
        .copied()
        .filter(|pair| pair.nomination.features.fixed_phi() == pair.witness.features.fixed_phi())
        .collect();
    let frozen_ids: BTreeSet<(String, usize, usize)> = p1j
        .feature_diffs
        .iter()
        .map(|row| (row.candidate.clone(), row.nomination_doc, row.witness_doc))
        .collect();
    let recomputed_global: BTreeSet<(String, usize, usize)> = same_global
        .iter()
        .map(|pair| {
            (
                CANDIDATES[pair.nomination.candidate].id.to_owned(),
                pair.nomination.doc,
                pair.witness.doc,
            )
        })
        .collect();
    ensure!(
        frozen_ids == recomputed_global,
        "P1J frozen 14 identities differ from exact recomputation"
    );
    for pair in &same_global {
        let candidate = CANDIDATES[pair.nomination.candidate].id;
        let row = p1j
            .feature_diffs
            .iter()
            .find(|row| {
                row.candidate == candidate
                    && row.nomination_doc == pair.nomination.doc
                    && row.witness_doc == pair.witness.doc
            })
            .context("missing P1J frozen record")?;
        let class = if pair.class.invalid() {
            "INVALID"
        } else {
            pair.class.label()
        };
        ensure!(
            row.class == class && row.witness_kind == pair.kind.label(),
            "P1J frozen semantics mismatch for {candidate}"
        );
    }
    let mut broad = Vec::new();
    let mut excluded = 0usize;
    for shard in 0..SHARDS {
        for pair in pairs(&events, Some(shard)) {
            let key = (
                CANDIDATES[pair.nomination.candidate].id.to_owned(),
                pair.nomination.doc,
                pair.witness.doc,
            );
            if frozen_ids.contains(&key) {
                excluded += 1;
            } else {
                broad.push(pair);
            }
        }
    }
    let same_count = broad
        .iter()
        .filter(|pair| pair.nomination.features.fixed_phi() == pair.witness.features.fixed_phi())
        .count();
    ensure!(
        broad.len() == 79 && same_count == 67 && broad.len() - same_count == 12 && excluded == 9,
        "reconstructed P1J3 roster mismatch: total={} same={} excluded={}",
        broad.len(),
        same_count,
        excluded
    );
    ensure!(
        p1j3.broad_population.total_pairs == broad.len()
            && p1j3.broad_population.same_phenotype_pairs == same_count
            && p1j3.broad_population.cross_phenotype_pairs == 12
            && p1j3.broad_population.frozen_discovery_pairs_excluded == excluded,
        "reconstructed roster differs from immutable P1J3 receipt"
    );
    let recomputed_roster = rows_from_pairs(&broad)
        .into_iter()
        .filter(|row| row.class != "OTHER")
        .map(|row| roster_key(&row))
        .collect::<BTreeSet<_>>();
    let frozen_roster = roster.iter().map(roster_key).collect::<BTreeSet<_>>();
    ensure!(
        roster.len() == 67 && recomputed_roster == frozen_roster,
        "67-row P1J3 same-phi roster parity failed"
    );

    let pair_rows: Vec<PairAudit> = broad.iter().copied().map(pair_audit).collect();
    let episode_keys: BTreeSet<(usize, usize, usize)> = broad.iter().map(episode_key).collect();
    let episodes: Vec<EpisodeAudit> = episode_keys
        .iter()
        .map(|key| count_episode(&pair_rows, *key))
        .collect();
    let mut summary = Summary {
        directional_join_count: pair_rows.len(),
        distinct_document_pair_episodes: episodes.len(),
        endpoint_assignment_count: 0,
        unique_argmax_endpoints: 0,
        tied_argmax_endpoints: 0,
        priority_inversion_endpoint_occurrences: 0,
        same_phi_pairs: same_count,
        cross_phi_pairs: pair_rows.len() - same_count,
        valid_actionable_pairs: 0,
        invalid_actionable_pairs: 0,
        valid_abstain_pairs: 0,
        invalid_abstain_pairs: 0,
        other_pairs: 0,
        actionable_priority_inversion_pairs: 0,
        unique_priority_inversion_endpoints: 0,
        inversion_pairs_with_any_tie: 0,
        invalid_actionable_episodes: 0,
        invalid_actionable_directional_rows: 0,
        valid_actionable_episodes: 0,
        valid_episodes_with_any_assignment_change: 0,
        valid_episodes_with_plurality_tie: 0,
        invalid_episodes_no_longer_finance_local: 0,
        invalid_episodes_both_endpoints_leave_finance: 0,
        invalid_episodes_repaired_at_both_endpoints: 0,
        invalid_episodes_where_plurality_creates_cross_route: 0,
    };
    for row in &pair_rows {
        match row.current_outcome {
            "VALID_ACTIONABLE" => summary.valid_actionable_pairs += 1,
            "INVALID_ACTIONABLE" => summary.invalid_actionable_pairs += 1,
            "VALID_ABSTAIN" => summary.valid_abstain_pairs += 1,
            "INVALID_ABSTAIN" => summary.invalid_abstain_pairs += 1,
            _ => summary.other_pairs += 1,
        }
        summary.actionable_priority_inversion_pairs += usize::from(
            row.pair_has_priority_inversion
                && (row.current_outcome == "VALID_ACTIONABLE"
                    || row.current_outcome == "INVALID_ACTIONABLE"),
        );
        summary.inversion_pairs_with_any_tie += usize::from(
            row.pair_has_priority_inversion
                && (row.nomination.argmax_tie || row.witness.argmax_tie),
        );
    }
    let inverted_endpoints: BTreeSet<(usize, &'static str, usize, &'static str)> = pair_rows
        .iter()
        .flat_map(|row| {
            [(&row.nomination, "nomination"), (&row.witness, "witness")]
                .into_iter()
                .filter(|(endpoint, _)| endpoint.priority_inversion)
                .map(move |(endpoint, role)| (row.shard, row.candidate, endpoint.document, role))
        })
        .collect();
    summary.unique_priority_inversion_endpoints = inverted_endpoints.len();
    summary.endpoint_assignment_count = pair_rows.len() * 2;
    summary.tied_argmax_endpoints = pair_rows
        .iter()
        .map(|row| usize::from(row.nomination.argmax_tie) + usize::from(row.witness.argmax_tie))
        .sum();
    summary.unique_argmax_endpoints =
        summary.endpoint_assignment_count - summary.tied_argmax_endpoints;
    summary.priority_inversion_endpoint_occurrences = pair_rows
        .iter()
        .map(|row| row.endpoint_inversion_count)
        .sum();
    for episode in &episodes {
        if episode.current_invalid_actionable_directions > 0 {
            summary.invalid_actionable_episodes += 1;
            summary.invalid_actionable_directional_rows +=
                episode.current_invalid_actionable_directions;
            let invalid_rows: Vec<&PairAudit> = pair_rows
                .iter()
                .filter(|row| {
                    row.episode_id == episode.episode_id
                        && row.current_outcome == "INVALID_ACTIONABLE"
                })
                .collect();
            if !invalid_rows.is_empty()
                && invalid_rows
                    .iter()
                    .all(|row| row.current_is_finance_local && !row.plurality_is_finance_local)
            {
                summary.invalid_episodes_no_longer_finance_local += 1;
            }
            if !invalid_rows.is_empty()
                && invalid_rows.iter().all(|row| {
                    row.nomination
                        .plurality_route
                        .is_some_and(|phi| phi != "finance")
                        && row
                            .witness
                            .plurality_route
                            .is_some_and(|phi| phi != "finance")
                })
            {
                summary.invalid_episodes_both_endpoints_leave_finance += 1;
            }
            if !invalid_rows.is_empty()
                && invalid_rows
                    .iter()
                    .all(|row| row.plurality_outcome.starts_with("VALID"))
            {
                summary.invalid_episodes_repaired_at_both_endpoints += 1;
            }
            if invalid_rows
                .iter()
                .any(|row| row.plurality_outcome == "CROSS_PHENOTYPE")
            {
                summary.invalid_episodes_where_plurality_creates_cross_route += 1;
            }
        }
        if episode.current_valid_actionable_directions > 0 {
            summary.valid_actionable_episodes += 1;
        }
        if episode.current_valid_any_directions > 0 {
            if episode.any_valid_direction_changed_phenotype {
                summary.valid_episodes_with_any_assignment_change += 1;
            }
            if episode.valid_directions_becoming_tie > 0 {
                summary.valid_episodes_with_plurality_tie += 1;
            }
        }
    }

    let actionable_rates = [true, false]
        .into_iter()
        .map(|inversion| {
            rate(
                if inversion {
                    "priority_inversion"
                } else {
                    "no_priority_inversion"
                },
                pair_rows
                    .iter()
                    .filter(move |row| row.pair_has_priority_inversion == inversion)
                    .filter(|row| {
                        row.current_outcome == "INVALID_ACTIONABLE"
                            || row.current_outcome == "VALID_ACTIONABLE"
                    })
                    .map(|row| {
                        (
                            row.current_outcome == "INVALID_ACTIONABLE",
                            row.current_outcome == "VALID_ACTIONABLE",
                        )
                    }),
            )
        })
        .collect();
    let conclusive_rates = [true, false]
        .into_iter()
        .map(|inversion| {
            rate(
                if inversion {
                    "priority_inversion"
                } else {
                    "no_priority_inversion"
                },
                pair_rows
                    .iter()
                    .filter(move |row| row.pair_has_priority_inversion == inversion)
                    .filter(|row| {
                        row.current_outcome.starts_with("INVALID")
                            || row.current_outcome.starts_with("VALID")
                    })
                    .map(|row| {
                        (
                            row.current_outcome.starts_with("INVALID"),
                            row.current_outcome.starts_with("VALID"),
                        )
                    }),
            )
        })
        .collect();
    let episode_actionable_rates = [true, false]
        .into_iter()
        .map(|inversion| {
            rate(
                if inversion {
                    "priority_inversion"
                } else {
                    "no_priority_inversion"
                },
                episodes
                    .iter()
                    .filter(move |episode| episode.any_priority_inversion == inversion)
                    .filter(|episode| {
                        episode.current_invalid_actionable_directions > 0
                            || episode.current_valid_actionable_directions > 0
                    })
                    .map(|episode| {
                        (
                            episode.current_invalid_actionable_directions > 0,
                            episode.current_valid_actionable_directions > 0,
                        )
                    }),
            )
        })
        .collect();
    let episode_conclusive_rates = [true, false]
        .into_iter()
        .map(|inversion| {
            rate(
                if inversion {
                    "priority_inversion"
                } else {
                    "no_priority_inversion"
                },
                episodes
                    .iter()
                    .filter(move |episode| episode.any_priority_inversion == inversion)
                    .filter(|episode| {
                        episode.current_invalid_any_directions > 0
                            || episode.current_valid_any_directions > 0
                    })
                    .map(|episode| {
                        (
                            episode.current_invalid_any_directions > 0,
                            episode.current_valid_any_directions > 0,
                        )
                    }),
            )
        })
        .collect();

    Ok(Receipt {
        schema: SCHEMA,
        scope: "diagnostic-only reconstruction of the sealed P1J3 discovery roster; no learning, authority, compatibility guard, ranking, or serving changes",
        hypothesis: "fixed family priority can override stronger local marker evidence, routing mixed-marker endpoints into a false same-phenotype join",
        corpus_path,
        corpus_sha256: corpus_sha256.clone(),
        corpus_document_count: document_count,
        p1j_receipt_sha256: p1j_hash,
        p1j3_receipt_sha256: p1j3_hash,
        roster_validation: RosterValidation {
            corpus_hash_matches_p1j3: corpus_hash_matches,
            p1j3_schema: p1j3.schema,
            p1j3_receipt_sha256: digest(&p1j3_bytes),
            p1j3_pair_roster_sha256: actual_roster_hash,
            p1j3_pair_roster_run2_sha256: run2_hash,
            p1j3_pair_roster_hashes_match: roster_hashes_match,
            reconstructed_total_pairs: broad.len(),
            reconstructed_same_phi_pairs: same_count,
            reconstructed_cross_phi_pairs: broad.len() - same_count,
            same_phi_rows_match_frozen_roster: recomputed_roster == frozen_roster,
            excluded_p1j_identity_count: excluded,
        },
        summary,
        conditional_prevalence: ConditionalRates {
            actionable: actionable_rates,
            all_conclusive: conclusive_rates,
            episode_actionable: episode_actionable_rates,
            episode_all_conclusive: episode_conclusive_rates,
        },
        counterfactual: "descriptive only: route to the unique argmax marker family; ties (including all-zero counts) abstain; frozen P1J3 nominations/witnesses and witness outcomes remain unchanged",
        episodes,
        pairs: pair_rows,
        interpretation_boundary: "P1K1 reuses a discovery roster and cannot qualify an assignment policy or compatibility guard; a positive pattern can only motivate preregistered P1K2 on unopened episodes",
    })
}

#[derive(Clone, Debug, Serialize)]
struct K2Gate {
    status: &'static str,
    sufficient: bool,
    current_valid_actionable: usize,
    current_invalid_actionable: usize,
    counterfactual_valid_actionable: usize,
    counterfactual_invalid_actionable: usize,
    invalid_actionable_rejected: usize,
    valid_actionable_loss: usize,
    valid_loss_fraction: f64,
    current_actionable_rows: usize,
    new_tie_abstentions: usize,
    new_abstention_fraction: f64,
    invalid_shards: usize,
    minimum_valid_actionable: usize,
    minimum_invalid_actionable: usize,
    minimum_invalid_shards: usize,
    max_valid_loss_fraction: f64,
    max_new_abstention_fraction: f64,
    reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct K2Receipt {
    schema: &'static str,
    scope: &'static str,
    hypothesis: &'static str,
    corpus_path: String,
    corpus_sha256: String,
    corpus_document_count: usize,
    shard_count: usize,
    pairs: Vec<PairAudit>,
    episodes: Vec<EpisodeAudit>,
    summary: Summary,
    gate: K2Gate,
    counterfactual: &'static str,
    conclusion: &'static str,
}

fn main_run_k2() -> Result<K2Receipt> {
    let args: Vec<String> = env::args().collect();
    let corpus_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "D:\\phoenix-evals\\beir\\scifact\\corpus.jsonl".to_owned());
    let (events, document_count, corpus_sha256) = load_events(Path::new(&corpus_path))?;
    let mut broad = Vec::new();
    for shard in 0..SHARDS {
        broad.extend(pairs(&events, Some(shard)));
    }
    if broad.is_empty() {
        let gate = K2Gate {
            status: "PHENOTYPE_ASSIGNMENT_GATE_NOT_QUALIFIED",
            sufficient: false,
            current_valid_actionable: 0,
            current_invalid_actionable: 0,
            counterfactual_valid_actionable: 0,
            counterfactual_invalid_actionable: 0,
            invalid_actionable_rejected: 0,
            valid_actionable_loss: 0,
            valid_loss_fraction: 0.0,
            current_actionable_rows: 0,
            new_tie_abstentions: 0,
            new_abstention_fraction: 0.0,
            invalid_shards: 0,
            minimum_valid_actionable: 20,
            minimum_invalid_actionable: 5,
            minimum_invalid_shards: 2,
            max_valid_loss_fraction: 0.10,
            max_new_abstention_fraction: 0.10,
            reason:
                "unopened corpus contains no occurrences for the frozen LT9 candidate relation bank",
        };
        return Ok(K2Receipt {
            schema: "phoenix.lexical.lt9la2p1k2/v1",
            scope: "assignment-only qualification on unopened SciFact evidence; no lexical authority or serving changes",
            hypothesis: "count plurality with tie abstention is safer than fixed family priority for mixed-marker endpoint routing",
            corpus_path,
            corpus_sha256,
            corpus_document_count: document_count,
            shard_count: SHARDS,
            pairs: Vec::new(),
            episodes: Vec::new(),
            summary: Summary {
                directional_join_count: 0,
                distinct_document_pair_episodes: 0,
                endpoint_assignment_count: 0,
                unique_argmax_endpoints: 0,
                tied_argmax_endpoints: 0,
                priority_inversion_endpoint_occurrences: 0,
                same_phi_pairs: 0,
                cross_phi_pairs: 0,
                valid_actionable_pairs: 0,
                invalid_actionable_pairs: 0,
                valid_abstain_pairs: 0,
                invalid_abstain_pairs: 0,
                other_pairs: 0,
                actionable_priority_inversion_pairs: 0,
                unique_priority_inversion_endpoints: 0,
                inversion_pairs_with_any_tie: 0,
                invalid_actionable_episodes: 0,
                invalid_actionable_directional_rows: 0,
                valid_actionable_episodes: 0,
                valid_episodes_with_any_assignment_change: 0,
                valid_episodes_with_plurality_tie: 0,
                invalid_episodes_no_longer_finance_local: 0,
                invalid_episodes_both_endpoints_leave_finance: 0,
                invalid_episodes_repaired_at_both_endpoints: 0,
                invalid_episodes_where_plurality_creates_cross_route: 0,
            },
            gate,
            counterfactual: "fixed P1J3 priority versus unique marker-count argmax; ties abstain; no learning or authority updates",
            conclusion: "P1K2_UNDERPOWERED: the unopened corpus cannot exercise the frozen assignment policy",
        });
    }
    let pair_rows: Vec<PairAudit> = broad.iter().copied().map(pair_audit).collect();
    let episode_keys: BTreeSet<(usize, usize, usize)> = broad.iter().map(episode_key).collect();
    let episodes: Vec<EpisodeAudit> = episode_keys
        .iter()
        .map(|key| count_episode(&pair_rows, *key))
        .collect();
    let same_count = pair_rows
        .iter()
        .filter(|row| row.current_outcome != "OTHER")
        .count();
    let mut summary = Summary {
        directional_join_count: pair_rows.len(),
        distinct_document_pair_episodes: episodes.len(),
        endpoint_assignment_count: pair_rows.len() * 2,
        unique_argmax_endpoints: 0,
        tied_argmax_endpoints: 0,
        priority_inversion_endpoint_occurrences: 0,
        same_phi_pairs: same_count,
        cross_phi_pairs: pair_rows.len() - same_count,
        valid_actionable_pairs: 0,
        invalid_actionable_pairs: 0,
        valid_abstain_pairs: 0,
        invalid_abstain_pairs: 0,
        other_pairs: 0,
        actionable_priority_inversion_pairs: 0,
        unique_priority_inversion_endpoints: 0,
        inversion_pairs_with_any_tie: 0,
        invalid_actionable_episodes: 0,
        invalid_actionable_directional_rows: 0,
        valid_actionable_episodes: 0,
        valid_episodes_with_any_assignment_change: 0,
        valid_episodes_with_plurality_tie: 0,
        invalid_episodes_no_longer_finance_local: 0,
        invalid_episodes_both_endpoints_leave_finance: 0,
        invalid_episodes_repaired_at_both_endpoints: 0,
        invalid_episodes_where_plurality_creates_cross_route: 0,
    };
    let inverted_endpoints: BTreeSet<(usize, &'static str, usize, &'static str)> = pair_rows
        .iter()
        .flat_map(|row| {
            [(&row.nomination, "nomination"), (&row.witness, "witness")]
                .into_iter()
                .filter(|(endpoint, _)| endpoint.priority_inversion)
                .map(move |(endpoint, role)| (row.shard, row.candidate, endpoint.document, role))
        })
        .collect();
    summary.unique_priority_inversion_endpoints = inverted_endpoints.len();
    summary.tied_argmax_endpoints = pair_rows
        .iter()
        .map(|row| usize::from(row.nomination.argmax_tie) + usize::from(row.witness.argmax_tie))
        .sum();
    summary.unique_argmax_endpoints =
        summary.endpoint_assignment_count - summary.tied_argmax_endpoints;
    summary.priority_inversion_endpoint_occurrences = pair_rows
        .iter()
        .map(|row| row.endpoint_inversion_count)
        .sum();
    for row in &pair_rows {
        match row.current_outcome {
            "VALID_ACTIONABLE" => summary.valid_actionable_pairs += 1,
            "INVALID_ACTIONABLE" => summary.invalid_actionable_pairs += 1,
            "VALID_ABSTAIN" => summary.valid_abstain_pairs += 1,
            "INVALID_ABSTAIN" => summary.invalid_abstain_pairs += 1,
            _ => summary.other_pairs += 1,
        }
        summary.actionable_priority_inversion_pairs += usize::from(
            row.pair_has_priority_inversion
                && matches!(
                    row.current_outcome,
                    "VALID_ACTIONABLE" | "INVALID_ACTIONABLE"
                ),
        );
        summary.inversion_pairs_with_any_tie += usize::from(
            row.pair_has_priority_inversion
                && (row.nomination.argmax_tie || row.witness.argmax_tie),
        );
    }
    for episode in &episodes {
        summary.invalid_actionable_episodes +=
            usize::from(episode.current_invalid_actionable_directions > 0);
        summary.invalid_actionable_directional_rows +=
            episode.current_invalid_actionable_directions;
        summary.valid_actionable_episodes +=
            usize::from(episode.current_valid_actionable_directions > 0);
        summary.valid_episodes_with_any_assignment_change +=
            usize::from(episode.any_valid_direction_changed_phenotype);
        summary.valid_episodes_with_plurality_tie +=
            usize::from(episode.valid_directions_becoming_tie > 0);
    }
    let current_valid = summary.valid_actionable_pairs;
    let current_invalid = summary.invalid_actionable_pairs;
    let current_actionable = current_valid + current_invalid;
    let cf_valid = pair_rows
        .iter()
        .filter(|row| row.plurality_outcome == "VALID_ACTIONABLE")
        .count();
    let cf_invalid = pair_rows
        .iter()
        .filter(|row| row.plurality_outcome == "INVALID_ACTIONABLE")
        .count();
    let rejected = current_invalid.saturating_sub(cf_invalid);
    let valid_loss = current_valid.saturating_sub(cf_valid);
    let new_ties = pair_rows
        .iter()
        .filter(|row| {
            matches!(
                row.current_outcome,
                "VALID_ACTIONABLE" | "INVALID_ACTIONABLE"
            )
        })
        .filter(|row| row.plurality_outcome == "ABSTAIN_TIE")
        .count();
    let invalid_shards = pair_rows
        .iter()
        .filter(|row| row.current_outcome == "INVALID_ACTIONABLE")
        .map(|row| row.shard)
        .collect::<BTreeSet<_>>()
        .len();
    let valid_loss_fraction = if current_valid == 0 {
        0.0
    } else {
        valid_loss as f64 / current_valid as f64
    };
    let new_abstention_fraction = if current_actionable == 0 {
        0.0
    } else {
        new_ties as f64 / current_actionable as f64
    };
    let sufficient = current_valid >= 20 && current_invalid >= 5 && invalid_shards >= 2;
    let qualified = sufficient
        && rejected > 0
        && valid_loss_fraction <= 0.10
        && new_abstention_fraction <= 0.10;
    let gate = K2Gate {
        status: if qualified {
            "PHENOTYPE_ASSIGNMENT_GATE_QUALIFIED"
        } else {
            "PHENOTYPE_ASSIGNMENT_GATE_NOT_QUALIFIED"
        },
        sufficient,
        current_valid_actionable: current_valid,
        current_invalid_actionable: current_invalid,
        counterfactual_valid_actionable: cf_valid,
        counterfactual_invalid_actionable: cf_invalid,
        invalid_actionable_rejected: rejected,
        valid_actionable_loss: valid_loss,
        valid_loss_fraction,
        current_actionable_rows: current_actionable,
        new_tie_abstentions: new_ties,
        new_abstention_fraction,
        invalid_shards,
        minimum_valid_actionable: 20,
        minimum_invalid_actionable: 5,
        minimum_invalid_shards: 2,
        max_valid_loss_fraction: 0.10,
        max_new_abstention_fraction: 0.10,
        reason: if qualified {
            "unopened corpus sufficiency and the predeclared invalid-reduction, valid-retention, and abstention gates passed"
        } else {
            "unopened corpus was underpowered or the predeclared routing safety gates failed"
        },
    };
    Ok(K2Receipt {
        schema: "phoenix.lexical.lt9la2p1k2/v1",
        scope: "assignment-only qualification on unopened SciFact evidence; no lexical authority or serving changes",
        hypothesis: "count plurality with tie abstention is safer than fixed family priority for mixed-marker endpoint routing",
        corpus_path,
        corpus_sha256,
        corpus_document_count: document_count,
        shard_count: SHARDS,
        pairs: pair_rows,
        episodes,
        summary,
        gate,
        counterfactual: "fixed P1J3 priority versus unique marker-count argmax; ties abstain; no learning or authority updates",
        conclusion: "P1K2_COMPLETE: assignment policy is evaluated only on the unopened corpus and does not promote lexical authority",
    })
}

fn main() -> Result<()> {
    let output_path = env::args()
        .nth(2)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1k2\\lt9-la2p1k2-receipt.json".to_owned());
    let receipt = main_run_k2()?;
    let path = Path::new(&output_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("P1K2 assignment receipt: {}", path.display());
    println!(
        "pairs={} episodes={} inversions={} invalid-actionable={} gate={}",
        receipt.summary.directional_join_count,
        receipt.summary.distinct_document_pair_episodes,
        receipt.summary.actionable_priority_inversion_pairs,
        receipt.summary.invalid_actionable_pairs,
        receipt.gate.status
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features(counts: [u16; 3]) -> ContextFeatures {
        ContextFeatures {
            counts,
            ..ContextFeatures::default()
        }
    }

    #[test]
    fn fixed_priority_inversion_is_unique_plurality_only() {
        let audit = assignment(features([1, 0, 2]), "nomination", 4);
        assert_eq!(audit.current_fixed_priority, "finance_markers");
        assert_eq!(audit.raw_argmax_families, ["transport"]);
        assert_eq!(audit.top_runner_up_margin, 1);
        assert!(audit.priority_inversion);
        assert_eq!(audit.plurality_route, Some("transport"));
    }

    #[test]
    fn tied_plurality_abstains_without_counting_as_priority_inversion() {
        let audit = assignment(features([2, 0, 2]), "witness", 5);
        assert!(audit.argmax_tie);
        assert!(!audit.priority_inversion);
        assert_eq!(audit.plurality_route, None);
    }

    #[test]
    fn plurality_counterfactual_never_joins_a_tied_endpoint() {
        let nomination = Event {
            doc: 1,
            shard: 0,
            candidate: 2,
            distance: 4,
            negative: false,
            support: true,
            features: features([0, 0, 2]),
        };
        let witness = Event {
            doc: 2,
            shard: 0,
            candidate: 2,
            distance: 5,
            negative: false,
            support: true,
            features: features([2, 0, 2]),
        };
        assert_eq!(
            plurality_class(CANDIDATES[2], nomination, witness, WitnessKind::Support),
            "ABSTAIN_TIE"
        );
    }
}
