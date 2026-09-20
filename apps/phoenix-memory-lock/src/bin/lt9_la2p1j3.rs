//! LT9-LA2-P1J3: frozen purity-guard routing qualification.
//!
//! The two P1J2 guards and their conjunction are replayed over the sealed
//! fourteen joins and eight fixed, contiguous FiQA shards. Shards reuse the
//! P1J first-nomination/earliest-same-phenotype witness contract. This is a
//! diagnostic routing assay only: no learner, ranking, or serving state moves.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1j3/v1";
const SHARDS: usize = 8;
const MAX_PAIR_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
const PHENOTYPES: usize = 4;
const BROAD_MIN_VALID: usize = 20;
const BROAD_MIN_INVALID: usize = 5;
const MAX_VALID_LOSS_FRACTION: f64 = 0.10;
const MAX_NEW_ABSTENTION_FRACTION: f64 = 0.10;

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
    fn path(self) -> &'static str {
        if self.counts[1] > 0 {
            "geography_markers"
        } else if self.counts[0] > 0 {
            "finance_markers"
        } else if self.counts[2] > 0 {
            "transport_markers"
        } else {
            "general_fallback"
        }
    }
    fn phi(self) -> u8 {
        match self.path() {
            "finance_markers" => 0,
            "geography_markers" => 1,
            "transport_markers" => 2,
            _ => 3,
        }
    }
    fn runner_up(self) -> &'static str {
        let mut values = [
            (self.counts[0], "finance"),
            (self.counts[1], "geography"),
            (self.counts[2], "transport"),
        ];
        values.sort_by(|a, b| b.0.cmp(&a.0));
        values[0].1
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

#[derive(Clone, Copy, Debug)]
struct Pair {
    shard: Option<usize>,
    nomination: Event,
    witness: Event,
    class: JoinClass,
    kind: WitnessKind,
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
    fn actionable(self) -> bool {
        matches!(self, Self::ValidActionable | Self::InvalidActionable)
    }
    fn valid(self) -> bool {
        matches!(self, Self::ValidActionable | Self::ValidAbstain)
    }
    fn invalid(self) -> bool {
        matches!(self, Self::InvalidActionable | Self::InvalidAbstain)
    }
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

#[derive(Clone, Copy, Debug, Serialize)]
enum Guard {
    SideMaskLe2,
    RouteAgreement,
    Both,
}

impl Guard {
    const ALL: [Self; 3] = [Self::SideMaskLe2, Self::RouteAgreement, Self::Both];
    fn label(self) -> &'static str {
        match self {
            Self::SideMaskLe2 => "side_mask_hamming <= 2",
            Self::RouteAgreement => "nomination_runner_up == witness_runner_up",
            Self::Both => "side_mask_hamming <= 2 && runner_up_agreement",
        }
    }
    fn accepts(self, pair: Pair) -> bool {
        let side_ok = side_distance(pair) <= 2;
        let route_ok = pair.nomination.features.runner_up() == pair.witness.features.runner_up();
        match self {
            Self::SideMaskLe2 => side_ok,
            Self::RouteAgreement => route_ok,
            Self::Both => side_ok && route_ok,
        }
    }
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
struct P1j2Decision {
    status: String,
    selected_rule: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct P1j2Receipt {
    decision: P1j2Decision,
}

#[derive(Clone, Debug, Serialize)]
struct FamilyCount {
    phenotype_family: &'static str,
    count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct GuardMetrics {
    guard: &'static str,
    join_count: usize,
    same_phenotype_join_count: usize,
    preexisting_cross_phenotype_rejections: usize,
    valid_join_count_before: usize,
    valid_join_count_retained: usize,
    invalid_join_count_before: usize,
    invalid_join_count_rejected: usize,
    nominations_before: usize,
    nominations_after: usize,
    nominations_suppressed: usize,
    valid_actionable_before: usize,
    valid_actionable_retained: usize,
    valid_actionable_loss: usize,
    invalid_actionable_before: usize,
    invalid_actionable_rejected: usize,
    actionable_witnesses_before: usize,
    actionable_witnesses_retained: usize,
    actionable_witnesses_rejected: usize,
    existing_abstentions: usize,
    new_abstentions: usize,
    new_abstentions_from_valid: usize,
    new_abstentions_from_invalid: usize,
    downstream_authority_updates_before: usize,
    downstream_authority_updates_after: usize,
    downstream_authority_updates_prevented: usize,
    false_authorized_join_count_before: usize,
    false_authorized_join_count_after: usize,
    false_authorized_candidate_count_before: usize,
    false_authorized_candidate_count_after: usize,
    family_distribution_before: Vec<FamilyCount>,
    family_distribution_after: Vec<FamilyCount>,
    valid_loss_fraction: f64,
    new_abstention_fraction: f64,
}

#[derive(Clone, Debug, Serialize)]
struct DatasetResult {
    population: &'static str,
    fixed_slice_definition: &'static str,
    total_pairs: usize,
    same_phenotype_pairs: usize,
    cross_phenotype_pairs: usize,
    actionable_same_phenotype_pairs: usize,
    valid_actionable_pairs: usize,
    invalid_actionable_pairs: usize,
    frozen_discovery_pairs_excluded: usize,
    distinct_shards_with_pairs: usize,
    distinct_shards_with_invalid_actionable_pairs: usize,
    guards: Vec<GuardMetrics>,
}

#[derive(Clone, Debug, Serialize)]
struct Equivalence {
    left: &'static str,
    right: &'static str,
    compared_same_phenotype_pairs: usize,
    decisions_identical: bool,
    both_accept: usize,
    side_accept_route_reject: usize,
    route_accept_side_reject: usize,
    both_reject: usize,
    inspection: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct GateDecision {
    status: &'static str,
    selected_guard: Option<&'static str>,
    valid_actionable_retained: usize,
    invalid_actionable_rejected: usize,
    broad_population_sufficient: bool,
    frozen_gate: &'static str,
    reason: &'static str,
    la2b_unblocked: bool,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    corpus_path: String,
    corpus_sha256: String,
    corpus_document_count: usize,
    p1j_receipt_sha256: String,
    p1j2_receipt_sha256: String,
    p1j_schema: String,
    p1j2_decision: String,
    p1j2_selected_rule: Option<String>,
    frozen_join_identity_parity: bool,
    frozen_join_count: usize,
    shard_count: usize,
    shard_doc_ranges: Vec<(usize, usize)>,
    guard_semantics: &'static str,
    broad_min_valid: usize,
    broad_min_invalid: usize,
    max_valid_loss_fraction: f64,
    max_new_abstention_fraction: f64,
    frozen_population: DatasetResult,
    broad_population: DatasetResult,
    equivalence: Vec<Equivalence>,
    context_routing_gate: GateDecision,
    scope: &'static str,
    conclusion: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct PairContextAudit {
    shard: Option<usize>,
    candidate: &'static str,
    class: &'static str,
    witness_kind: &'static str,
    nomination_doc: usize,
    witness_doc: usize,
    nomination_phi: &'static str,
    witness_phi: &'static str,
    nomination_runner_up: &'static str,
    witness_runner_up: &'static str,
    side_mask_hamming: u32,
    side_guard_accepts: bool,
    route_guard_accepts: bool,
    marker_mask_hamming: u32,
    count_l1: u32,
    nomination_counts: [u16; 3],
    witness_counts: [u16; 3],
    nomination_marker_masks: [u32; 3],
    witness_marker_masks: [u32; 3],
    nomination_before_masks: [u32; 3],
    nomination_after_masks: [u32; 3],
    witness_before_masks: [u32; 3],
    witness_after_masks: [u32; 3],
    support_count_difference: u16,
    negative_count_difference: u16,
    window_length_difference: u16,
    same_field: bool,
    fingerprint_equal: bool,
    document_delay: usize,
}

fn pair_context_audit(pairs: &[Pair]) -> Vec<PairContextAudit> {
    pairs
        .iter()
        .filter(|pair| pair.nomination.features.phi() == pair.witness.features.phi())
        .map(|pair| {
            let nomination = pair.nomination.features;
            let witness = pair.witness.features;
            PairContextAudit {
                shard: pair.shard,
                candidate: CANDIDATES[pair.nomination.candidate].id,
                class: pair.class.label(),
                witness_kind: pair.kind.label(),
                nomination_doc: pair.nomination.doc,
                witness_doc: pair.witness.doc,
                nomination_phi: nomination.path(),
                witness_phi: witness.path(),
                nomination_runner_up: nomination.runner_up(),
                witness_runner_up: witness.runner_up(),
                side_mask_hamming: side_distance(*pair),
                side_guard_accepts: Guard::SideMaskLe2.accepts(*pair),
                route_guard_accepts: Guard::RouteAgreement.accepts(*pair),
                marker_mask_hamming: nomination
                    .marker_masks
                    .iter()
                    .zip(witness.marker_masks)
                    .map(|(left, right)| (left ^ right).count_ones())
                    .sum(),
                count_l1: nomination
                    .counts
                    .iter()
                    .zip(witness.counts)
                    .map(|(left, right)| u32::from(left.abs_diff(right)))
                    .sum(),
                nomination_counts: nomination.counts,
                witness_counts: witness.counts,
                nomination_marker_masks: nomination.marker_masks,
                witness_marker_masks: witness.marker_masks,
                nomination_before_masks: nomination.before_masks,
                nomination_after_masks: nomination.after_masks,
                witness_before_masks: witness.before_masks,
                witness_after_masks: witness.after_masks,
                support_count_difference: nomination.support_count.abs_diff(witness.support_count),
                negative_count_difference: nomination
                    .negative_count
                    .abs_diff(witness.negative_count),
                window_length_difference: nomination.window_len.abs_diff(witness.window_len),
                same_field: nomination.same_field == witness.same_field,
                fingerprint_equal: nomination.fingerprint == witness.fingerprint,
                document_delay: pair.witness.doc - pair.nomination.doc,
            }
        })
        .collect()
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
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
fn positions(ws: &[&str], needle: &str) -> Vec<usize> {
    ws.iter()
        .enumerate()
        .filter_map(|(i, w)| (*w == needle).then_some(i))
        .collect()
}

fn nearest(ws: &[&str], source: &str, target: &str) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for left in positions(ws, source) {
        for right in positions(ws, target) {
            let d = left.abs_diff(right);
            if d <= 40 && best.is_none_or(|old: (usize, usize, usize)| d < old.2) {
                best = Some((left, right, d));
            }
        }
    }
    best
}

fn marker_mask(ws: &[&str], start: usize, end: usize, markers: &[&str]) -> (u16, u32) {
    let mut count = 0u16;
    let mut mask = 0u32;
    for word in &ws[start..end] {
        for (index, marker) in markers.iter().enumerate() {
            if word == marker {
                count = count.saturating_add(1);
                mask |= 1u32 << index;
            }
        }
    }
    (count, mask)
}

fn fingerprint(ws: &[&str], start: usize, end: usize, source: &str, target: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for word in &ws[start..end] {
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
    ws: &[&str],
    left: usize,
    right: usize,
    title_len: usize,
    source: &str,
    target: &str,
) -> ContextFeatures {
    let low = left.min(right).saturating_sub(8);
    let high = (left.max(right) + 9).min(ws.len());
    let split = left.min(right);
    let after_split = (left.max(right) + 1).min(ws.len());
    let (finance, finance_mask) = marker_mask(ws, low, high, FINANCE);
    let (geo, geo_mask) = marker_mask(ws, low, high, GEO);
    let (transport, transport_mask) = marker_mask(ws, low, high, TRANSPORT);
    let (_, finance_before) = marker_mask(ws, low, split, FINANCE);
    let (_, geo_before) = marker_mask(ws, low, split, GEO);
    let (_, transport_before) = marker_mask(ws, low, split, TRANSPORT);
    let (_, finance_after) = marker_mask(ws, after_split, high, FINANCE);
    let (_, geo_after) = marker_mask(ws, after_split, high, GEO);
    let (_, transport_after) = marker_mask(ws, after_split, high, TRANSPORT);
    let (support_count, _) = marker_mask(ws, low, high, SUPPORT);
    let (negative_count, _) = marker_mask(ws, low, high, NEGATIVE);
    ContextFeatures {
        counts: [finance, geo, transport],
        marker_masks: [finance_mask, geo_mask, transport_mask],
        before_masks: [finance_before, geo_before, transport_before],
        after_masks: [finance_after, geo_after, transport_after],
        support_count,
        negative_count,
        window_len: (high - low) as u16,
        same_field: (left < title_len) == (right < title_len),
        fingerprint: fingerprint(ws, low, high, source, target),
    }
}

fn near_any(ws: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(ws.len());
    ws[start..end]
        .iter()
        .any(|word| markers.iter().any(|marker| word == marker))
}

fn load_events(path: &Path) -> Result<(Vec<Event>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_hash = digest_hex(&bytes);
    let mut events = Vec::new();
    let mut docs = 0usize;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode FiQA corpus record")?;
        let title_len = words(&doc.title.to_ascii_lowercase()).len();
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        let shard = docs * SHARDS;
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                let local = ContextFeatures {
                    ..context_features(&ws, left, right, title_len, spec.source, spec.target)
                };
                events.push(Event {
                    doc: docs,
                    shard,
                    candidate,
                    distance,
                    negative: near_any(&ws, left, right, NEGATIVE),
                    support: near_any(&ws, left, right, SUPPORT),
                    features: local,
                });
            }
        }
        docs += 1;
    }
    for event in &mut events {
        event.shard = (event.shard / docs).min(SHARDS - 1);
    }
    Ok((events, docs, source_hash))
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
    let same = nomination.features.phi() == witness.features.phi();
    let expected = spec.expected_phi;
    let valid = if expected < 0 {
        same
    } else if same {
        nomination.features.phi() == expected as u8 && witness.features.phi() == expected as u8
    } else {
        nomination.features.phi() == expected as u8 && witness.features.phi() == expected as u8
    };
    if !same {
        return JoinClass::Other;
    }
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
        let key = event.candidate * PHENOTYPES + usize::from(event.features.phi());
        if seen[key] {
            continue;
        }
        seen[key] = true;
        let future: Vec<Event> = normalized
            .iter()
            .skip(index + 1)
            .copied()
            .filter(|later| later.candidate == event.candidate && later.doc > event.doc)
            .collect();
        let witness = future
            .iter()
            .copied()
            .find(|later| later.features.phi() == event.features.phi())
            .or_else(|| future.first().copied());
        if let Some(witness) = witness {
            let kind = witness_kind(witness);
            let class = classify(CANDIDATES[event.candidate], *event, witness, kind);
            out.push(Pair {
                shard,
                nomination: *event,
                witness,
                class,
                kind,
            });
        }
    }
    out
}

fn side_distance(pair: Pair) -> u32 {
    let hamming = |left: &[u32; 3], right: &[u32; 3]| {
        left.iter()
            .zip(right)
            .map(|(a, b)| (a ^ b).count_ones())
            .sum::<u32>()
    };
    hamming(
        &pair.nomination.features.before_masks,
        &pair.witness.features.before_masks,
    ) + hamming(
        &pair.nomination.features.after_masks,
        &pair.witness.features.after_masks,
    )
}

fn family_distribution<'a>(pairs: impl Iterator<Item = &'a Pair>) -> Vec<FamilyCount> {
    let mut counts = BTreeMap::<&'static str, usize>::new();
    for pair in pairs {
        *counts.entry(pair.nomination.features.path()).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(phenotype_family, count)| FamilyCount {
            phenotype_family,
            count,
        })
        .collect()
}

fn metrics(all_pairs: &[Pair], guard: Guard) -> GuardMetrics {
    let same: Vec<Pair> = all_pairs
        .iter()
        .copied()
        .filter(|p| p.nomination.features.phi() == p.witness.features.phi())
        .collect();
    let actionable: Vec<Pair> = same
        .iter()
        .copied()
        .filter(|p| p.class.actionable())
        .collect();
    let valid: Vec<Pair> = actionable
        .iter()
        .copied()
        .filter(|p| p.class.valid())
        .collect();
    let invalid: Vec<Pair> = actionable
        .iter()
        .copied()
        .filter(|p| p.class.invalid())
        .collect();
    let valid_joins: Vec<Pair> = same.iter().copied().filter(|p| p.class.valid()).collect();
    let invalid_joins: Vec<Pair> = same.iter().copied().filter(|p| p.class.invalid()).collect();
    let existing_abstentions = same.iter().filter(|p| !p.class.actionable()).count();
    let valid_lost = valid.iter().filter(|p| !guard.accepts(**p)).count();
    let invalid_rejected = invalid.iter().filter(|p| !guard.accepts(**p)).count();
    let valid_joins_lost = valid_joins.iter().filter(|p| !guard.accepts(**p)).count();
    let invalid_joins_rejected = invalid_joins.iter().filter(|p| !guard.accepts(**p)).count();
    let rejected: Vec<Pair> = actionable
        .iter()
        .copied()
        .filter(|p| !guard.accepts(*p))
        .collect();
    let after: Vec<Pair> = actionable
        .iter()
        .copied()
        .filter(|p| guard.accepts(*p))
        .collect();
    let candidates_before: BTreeSet<&'static str> = invalid
        .iter()
        .map(|p| CANDIDATES[p.nomination.candidate].id)
        .collect();
    let candidates_after: BTreeSet<&'static str> = invalid
        .iter()
        .filter(|p| guard.accepts(**p))
        .map(|p| CANDIDATES[p.nomination.candidate].id)
        .collect();
    let new_abstentions = rejected.len();
    let denominator = actionable.len().max(1) as f64;
    GuardMetrics {
        guard: guard.label(),
        join_count: all_pairs.len(),
        same_phenotype_join_count: same.len(),
        preexisting_cross_phenotype_rejections: all_pairs.len() - same.len(),
        valid_join_count_before: valid_joins.len(),
        valid_join_count_retained: valid_joins.len() - valid_joins_lost,
        invalid_join_count_before: invalid_joins.len(),
        invalid_join_count_rejected: invalid_joins_rejected,
        nominations_before: all_pairs.len(),
        nominations_after: all_pairs.len(),
        nominations_suppressed: 0,
        valid_actionable_before: valid.len(),
        valid_actionable_retained: valid.len() - valid_lost,
        valid_actionable_loss: valid_lost,
        invalid_actionable_before: invalid.len(),
        invalid_actionable_rejected: invalid_rejected,
        actionable_witnesses_before: actionable.len(),
        actionable_witnesses_retained: after.len(),
        actionable_witnesses_rejected: rejected.len(),
        existing_abstentions,
        new_abstentions,
        new_abstentions_from_valid: valid_lost,
        new_abstentions_from_invalid: invalid_rejected,
        downstream_authority_updates_before: actionable.len(),
        downstream_authority_updates_after: after.len(),
        downstream_authority_updates_prevented: rejected.len(),
        false_authorized_join_count_before: invalid.len(),
        false_authorized_join_count_after: invalid.len() - invalid_rejected,
        false_authorized_candidate_count_before: candidates_before.len(),
        false_authorized_candidate_count_after: candidates_after.len(),
        family_distribution_before: family_distribution(actionable.iter()),
        family_distribution_after: family_distribution(after.iter()),
        valid_loss_fraction: valid_lost as f64 / valid.len().max(1) as f64,
        new_abstention_fraction: new_abstentions as f64 / denominator,
    }
}

fn dataset_result(
    population: &'static str,
    slice: &'static str,
    all_pairs: &[Pair],
    excluded: usize,
) -> DatasetResult {
    let same: Vec<Pair> = all_pairs
        .iter()
        .copied()
        .filter(|p| p.nomination.features.phi() == p.witness.features.phi())
        .collect();
    let actionable: Vec<Pair> = same
        .iter()
        .copied()
        .filter(|p| p.class.actionable())
        .collect();
    let valid = actionable.iter().filter(|p| p.class.valid()).count();
    let invalid = actionable.iter().filter(|p| p.class.invalid()).count();
    let distinct_shards: BTreeSet<usize> = all_pairs.iter().filter_map(|p| p.shard).collect();
    let invalid_shards: BTreeSet<usize> = actionable
        .iter()
        .filter(|p| p.class.invalid())
        .filter_map(|p| p.shard)
        .collect();
    DatasetResult {
        population,
        fixed_slice_definition: slice,
        total_pairs: all_pairs.len(),
        same_phenotype_pairs: same.len(),
        cross_phenotype_pairs: all_pairs.len() - same.len(),
        actionable_same_phenotype_pairs: actionable.len(),
        valid_actionable_pairs: valid,
        invalid_actionable_pairs: invalid,
        frozen_discovery_pairs_excluded: excluded,
        distinct_shards_with_pairs: distinct_shards.len(),
        distinct_shards_with_invalid_actionable_pairs: invalid_shards.len(),
        guards: Guard::ALL
            .into_iter()
            .map(|guard| metrics(all_pairs, guard))
            .collect(),
    }
}

fn pair_key(candidate: &str, nomination_doc: usize, witness_doc: usize) -> (String, usize, usize) {
    (candidate.to_owned(), nomination_doc, witness_doc)
}

fn decisions_identical(pairs: &[Pair], left: Guard, right: Guard) -> Equivalence {
    let same = pairs
        .iter()
        .filter(|p| p.nomination.features.phi() == p.witness.features.phi())
        .copied()
        .collect::<Vec<_>>();
    let mut side_accept_route_reject = 0;
    let mut route_accept_side_reject = 0;
    let mut both_accept = 0;
    let mut both_reject = 0;
    for pair in &same {
        let s = Guard::SideMaskLe2.accepts(*pair);
        let r = Guard::RouteAgreement.accepts(*pair);
        match (s, r) {
            (true, true) => both_accept += 1,
            (true, false) => side_accept_route_reject += 1,
            (false, true) => route_accept_side_reject += 1,
            (false, false) => both_reject += 1,
        }
    }
    Equivalence { left: left.label(), right: right.label(), compared_same_phenotype_pairs: same.len(), decisions_identical: side_accept_route_reject == 0 && route_accept_side_reject == 0, both_accept, side_accept_route_reject, route_accept_side_reject, both_reject,
        inspection: "side distance is Hamming distance over nomination/witness before-and-after marker masks; route agreement compares the existing runner-up marker-family labels" }
}

fn qualify(metrics: &GuardMetrics, enough: bool) -> bool {
    enough
        && metrics.invalid_actionable_rejected > 0
        && metrics.false_authorized_candidate_count_after
            < metrics.false_authorized_candidate_count_before
        && metrics.valid_loss_fraction <= MAX_VALID_LOSS_FRACTION
        && metrics.new_abstention_fraction <= MAX_NEW_ABSTENTION_FRACTION
}

fn run() -> Result<Receipt> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let p1j_path = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1j\\lt9-la2p1j-receipt.json".to_owned(),
        Clone::clone,
    );
    let p1j2_path = args.get(3).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1j2\\lt9-la2p1j2-receipt-link.json".to_owned(),
        Clone::clone,
    );
    let (events, doc_count, corpus_hash) = load_events(Path::new(&corpus))?;
    ensure!(doc_count > 0, "corpus is empty");
    let p1j_bytes = fs::read(&p1j_path).with_context(|| format!("read {p1j_path}"))?;
    let p1j_hash = digest_hex(&p1j_bytes);
    let p1j: P1jReceipt =
        serde_json::from_slice(&p1j_bytes).context("decode frozen P1J receipt")?;
    let p1j2_bytes = fs::read(&p1j2_path).with_context(|| format!("read {p1j2_path}"))?;
    let p1j2_hash = digest_hex(&p1j2_bytes);
    let p1j2: P1j2Receipt = serde_json::from_slice(&p1j2_bytes).context("decode P1J2 decision")?;
    ensure!(
        p1j2.decision.status == "PURITY_GUARD_EXPERIMENT_AUTHORIZED",
        "P1J2 did not authorize a narrow experiment"
    );
    let p1j2_identity = format!("{}", p1j2.decision.selected_rule.as_deref().unwrap_or(""));
    ensure!(
        p1j2_identity == "nomination_runner_up == witness_runner_up",
        "unexpected P1J2 selected rule: {p1j2_identity}"
    );

    let full_pairs = pairs(&events, None);
    let frozen_same: Vec<Pair> = full_pairs
        .iter()
        .copied()
        .filter(|p| p.nomination.features.phi() == p.witness.features.phi())
        .collect();
    let from_receipt: BTreeSet<(String, usize, usize)> = p1j
        .feature_diffs
        .iter()
        .map(|p| pair_key(&p.candidate, p.nomination_doc, p.witness_doc))
        .collect();
    let recomputed: BTreeSet<(String, usize, usize)> = frozen_same
        .iter()
        .map(|p| {
            pair_key(
                CANDIDATES[p.nomination.candidate].id,
                p.nomination.doc,
                p.witness.doc,
            )
        })
        .collect();
    let frozen_by_id: BTreeMap<(String, usize, usize), &FrozenFeatureDiff> = p1j
        .feature_diffs
        .iter()
        .map(|row| {
            (
                pair_key(&row.candidate, row.nomination_doc, row.witness_doc),
                row,
            )
        })
        .collect();
    let frozen_semantics_match = frozen_same.iter().all(|pair| {
        frozen_by_id
            .get(&pair_key(
                CANDIDATES[pair.nomination.candidate].id,
                pair.nomination.doc,
                pair.witness.doc,
            ))
            .is_some_and(|row| {
                let p1j_class = match pair.class {
                    JoinClass::InvalidActionable | JoinClass::InvalidAbstain => "INVALID",
                    other => other.label(),
                };
                row.class == p1j_class && row.witness_kind == pair.kind.label()
            })
    });
    let frozen_match =
        from_receipt == recomputed && p1j.feature_diffs.len() == 14 && frozen_semantics_match;
    ensure!(
        frozen_match,
        "recomputed P1J join identities do not exactly match frozen 14: receipt={} recomputed={}",
        from_receipt.len(),
        recomputed.len()
    );
    let frozen_result = dataset_result(
        "P1J_FROZEN_14",
        "the exact global first-nomination/earliest-same-phenotype witness pairs from P1J",
        &frozen_same,
        0,
    );

    let frozen_ids = from_receipt;
    let mut broad_pairs = Vec::new();
    let mut excluded = 0usize;
    for shard in 0..SHARDS {
        for pair in pairs(&events, Some(shard)) {
            let key = pair_key(
                CANDIDATES[pair.nomination.candidate].id,
                pair.nomination.doc,
                pair.witness.doc,
            );
            if frozen_ids.contains(&key) {
                excluded += 1;
            } else {
                broad_pairs.push(pair);
            }
        }
    }
    let broad = dataset_result("FIQA_8_SHARD_REPLAY_EX_FROZEN", "eight equal contiguous JSONL line shards; within each shard use P1J's first candidate/phenotype nomination and earliest later same-phenotype witness, falling back to earliest later candidate event; exact 14 P1J join identities excluded", &broad_pairs, excluded);
    if let Some(audit_path) = args.get(5) {
        let audit_bytes = serde_json::to_vec_pretty(&pair_context_audit(&broad_pairs))?;
        let audit_file = Path::new(audit_path);
        if let Some(parent) = audit_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(audit_file, audit_bytes)?;
    }
    let enough = broad.valid_actionable_pairs >= BROAD_MIN_VALID
        && broad.invalid_actionable_pairs >= BROAD_MIN_INVALID
        && broad.distinct_shards_with_invalid_actionable_pairs >= 2;
    let broad_side = broad
        .guards
        .iter()
        .find(|g| g.guard == Guard::SideMaskLe2.label())
        .expect("side guard");
    let broad_route = broad
        .guards
        .iter()
        .find(|g| g.guard == Guard::RouteAgreement.label())
        .expect("route guard");
    let broad_both = broad
        .guards
        .iter()
        .find(|g| g.guard == Guard::Both.label())
        .expect("combined guard");
    let equivalence = vec![
        decisions_identical(&frozen_same, Guard::SideMaskLe2, Guard::RouteAgreement),
        decisions_identical(&broad_pairs, Guard::SideMaskLe2, Guard::RouteAgreement),
    ];
    let side_qualified = qualify(broad_side, enough);
    let route_qualified = qualify(broad_route, enough);
    let both_qualified = qualify(broad_both, enough);
    let selected = match (side_qualified, route_qualified, both_qualified) {
        (true, true, _) if equivalence[1].decisions_identical => Some(broad_route),
        (true, true, _) => [broad_side, broad_route].into_iter().min_by_key(|metrics| {
            (
                metrics.valid_actionable_loss,
                metrics.false_authorized_candidate_count_after,
                metrics.new_abstentions,
            )
        }),
        (true, false, _) => Some(broad_side),
        (false, true, _) => Some(broad_route),
        (false, false, true) => Some(broad_both),
        _ => None,
    };
    let (status, selected_name, valid_retained, invalid_removed, reason) = if let Some(result) =
        selected
    {
        ("CONTEXT_ROUTING_GATE_QUALIFIED", Some(result.guard), result.valid_actionable_retained, result.invalid_actionable_rejected, "the fixed broad-slice sufficiency floor and predeclared safety/abstention gates passed; guard remains a routing-only qualified mechanism")
    } else {
        ("CONTEXT_ROUTING_GATE_NOT_QUALIFIED", None, 0, 0, "broad evidence did not satisfy the frozen sample floor or invalid-reduction/valid-retention/abstention criteria")
    };
    let decision = GateDecision { status, selected_guard: selected_name, valid_actionable_retained: valid_retained, invalid_actionable_rejected: invalid_removed,
        broad_population_sufficient: enough,
        frozen_gate: "broad >=20 valid and >=5 invalid actionable same-phenotype joins, invalid joins and false-authorized candidates decrease, valid loss <=10%, new abstentions <=10%, and invalids span >=2 shards",
        reason, la2b_unblocked: false };
    let ranges = (0..SHARDS)
        .map(|s| {
            let start = s * doc_count / SHARDS;
            let end = (s + 1) * doc_count / SHARDS;
            (start, end)
        })
        .collect();
    Ok(Receipt {
        schema: SCHEMA,
        protocol: "P1J3: exact P1J/P1J2 guard replay; only side-mask <=2, existing runner-up route agreement, and their conjunction; no threshold search or retrieval-quality tuning",
        hypothesis: "within-family side-context or route agreement is a context-purity invariant beyond the original fourteen selected joins",
        corpus_path: corpus, corpus_sha256: corpus_hash, corpus_document_count: doc_count,
        p1j_receipt_sha256: p1j_hash, p1j2_receipt_sha256: p1j2_hash, p1j_schema: p1j.schema,
        p1j2_decision: p1j2.decision.status, p1j2_selected_rule: p1j2.decision.selected_rule,
        frozen_join_identity_parity: frozen_match, frozen_join_count: p1j.feature_diffs.len(), shard_count: SHARDS, shard_doc_ranges: ranges,
        guard_semantics: "side-mask distance sums Hamming distances over before/after family marker masks; route agreement is equality of the existing P1J nomination/witness runner-up family labels; apply only after witness evidence arrives; rejection preserves the nomination and produces an abstention",
        broad_min_valid: BROAD_MIN_VALID, broad_min_invalid: BROAD_MIN_INVALID, max_valid_loss_fraction: MAX_VALID_LOSS_FRACTION, max_new_abstention_fraction: MAX_NEW_ABSTENTION_FRACTION,
        frozen_population: frozen_result, broad_population: broad, equivalence, context_routing_gate: decision,
        scope: "Diagnostic-only. No natural learner, lexical authority, ranking, or serving changes. Passing here authorizes integration investigation only; LA2-B remains separately gated.",
        conclusion: "P1J3_COMPLETE: context guard qualification is determined only from the frozen broad natural-corpus replay and predeclared local routing metrics",
    })
}

fn main() -> Result<()> {
    let receipt = run()?;
    let output = env::args()
        .nth(4)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1j3\\lt9-la2p1j3-receipt.json".to_owned());
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    let path = Path::new(&output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &bytes)?;
    println!("schema={SCHEMA} corpus_docs={} frozen={} broad_pairs={} broad_valid={} broad_invalid={} gate={}", receipt.corpus_document_count, receipt.frozen_join_count, receipt.broad_population.total_pairs, receipt.broad_population.valid_actionable_pairs, receipt.broad_population.invalid_actionable_pairs, receipt.context_routing_gate.status);
    for result in receipt.broad_population.guards.iter() {
        println!("guard={} valid={}/{} invalid_rejected={}/{} new_abstentions={} false_candidates={}->{}", result.guard, result.valid_actionable_retained, result.valid_actionable_before, result.invalid_actionable_rejected, result.invalid_actionable_before, result.new_abstentions, result.false_authorized_candidate_count_before, result.false_authorized_candidate_count_after);
    }
    println!("receipt_sha256={}", digest_hex(bytes));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(doc: usize, candidate: usize, phi: u8, distance: usize) -> Event {
        let mut features = ContextFeatures::default();
        match phi {
            0 => features.counts[0] = 1,
            1 => features.counts[1] = 1,
            2 => features.counts[2] = 1,
            _ => {}
        }
        Event {
            doc,
            shard: 0,
            candidate,
            distance,
            negative: false,
            support: true,
            features,
        }
    }

    #[test]
    fn guard_family_is_exactly_the_two_preregistered_rules_and_their_conjunction() {
        assert_eq!(
            Guard::ALL.map(Guard::label),
            [
                "side_mask_hamming <= 2",
                "nomination_runner_up == witness_runner_up",
                "side_mask_hamming <= 2 && runner_up_agreement"
            ]
        );
    }

    #[test]
    fn shard_replay_keeps_first_nomination_and_same_phi_preferred_witness() {
        let events = [
            event(0, 0, 0, 2),
            event(1, 0, 1, 2),
            event(2, 0, 0, 2),
            event(3, 0, 0, 2),
        ];
        let result = pairs(&events, Some(0));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].nomination.doc, 0);
        assert_eq!(result[0].witness.doc, 2);
    }

    #[test]
    fn rejected_actionable_witness_becomes_abstention_without_suppressing_nomination() {
        let mut n = event(0, 0, 2, 2);
        let mut w = event(1, 0, 2, 2);
        n.features.counts = [2, 0, 2];
        w.features.counts = [1, 0, 2];
        n.features.before_masks = [0, 0, 0];
        w.features.before_masks = [1, 0, 0];
        let pair = Pair {
            shard: None,
            nomination: n,
            witness: w,
            class: JoinClass::InvalidActionable,
            kind: WitnessKind::Support,
        };
        let stats = metrics(&[pair], Guard::RouteAgreement);
        assert_eq!(stats.nominations_suppressed, 0);
        assert_eq!(stats.new_abstentions, 1);
        assert_eq!(stats.downstream_authority_updates_prevented, 1);
        let audit = pair_context_audit(&[pair]);
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].class, "INVALID_ACTIONABLE");
        assert_eq!(audit[0].side_mask_hamming, 1);
        assert!(audit[0].side_guard_accepts);
        assert!(!audit[0].route_guard_accepts);
    }
}
