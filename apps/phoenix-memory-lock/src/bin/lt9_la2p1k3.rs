//! LT9-LA2-P1K3: NQ tie anatomy, diagnostic-only.
//!
//! The NQ P1K2 result is discovery evidence. This binary inspects only the
//! existing endpoint features for actionable plurality ties and evaluates a
//! small preregistered set of categorical tie-break views. It does not alter
//! the router, learner, authority, ranking, or serving behavior.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1k3/v1";
const SHARDS: usize = 8;
const MAX_PAIR_DISTANCE: usize = 24;
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

#[derive(Clone, Copy)]
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

#[derive(Deserialize)]
struct CorpusDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Clone, Copy, Default)]
struct Features {
    counts: [u16; 3],
    marker_masks: [u32; 3],
    before_masks: [u32; 3],
    after_masks: [u32; 3],
    distance: usize,
}

impl Features {
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
    fn mixed(self) -> bool {
        self.counts.iter().filter(|count| **count > 0).count() > 1
    }
    fn popcount(value: u32) -> u32 {
        value.count_ones()
    }
    fn side_score(self, family: usize) -> u32 {
        u32::from(self.before_masks[family] != 0) + u32::from(self.after_masks[family] != 0)
    }
    fn concentration(self, family: usize) -> u32 {
        Self::popcount(self.marker_masks[family])
    }
}

#[derive(Clone, Copy)]
struct Event {
    doc: usize,
    shard: usize,
    candidate: usize,
    features: Features,
}

#[derive(Clone, Copy)]
struct Pair {
    shard: usize,
    nomination: Event,
    witness: Event,
}

#[derive(Deserialize)]
struct K2Receipt {
    corpus_sha256: String,
    pairs: Vec<K2Pair>,
}

#[derive(Deserialize)]
struct K2Pair {
    shard: usize,
    candidate: String,
    nomination_doc: usize,
    witness_doc: usize,
    witness_kind: String,
    current_outcome: String,
    plurality_outcome: String,
}

#[derive(Serialize)]
struct EndpointView {
    role: &'static str,
    document: usize,
    counts: [u16; 3],
    fixed_route: String,
    tied_families: Vec<String>,
    marker_mask_bits: [u32; 3],
    before_mask_bits: [u32; 3],
    after_mask_bits: [u32; 3],
    side_scores: [u32; 3],
    concentration_scores: [u32; 3],
    mixed_marker: bool,
}

#[derive(Serialize)]
struct TieRow {
    episode_id: String,
    candidate: String,
    expected_phi: i8,
    current_outcome: String,
    witness_kind: String,
    current_validity: &'static str,
    nomination: EndpointView,
    witness: EndpointView,
    resolver_routes: ResolverRoutes,
}

#[derive(Default, Serialize)]
struct ResolverRoutes {
    side_context: String,
    marker_concentration: String,
    before_after_asymmetry: String,
    fixed_priority_control: String,
}

#[derive(Default, Serialize)]
struct ResolverSummary {
    actionable_tie_rows: usize,
    actionable_tie_episodes: usize,
    valid_tie_rows: usize,
    invalid_tie_rows: usize,
    valid_tie_episodes: usize,
    invalid_tie_episodes: usize,
    resolver_valid_rows: [usize; 4],
    resolver_invalid_rows: [usize; 4],
    resolver_abstain_rows: [usize; 4],
    resolver_valid_episodes: [usize; 4],
    resolver_invalid_episodes: [usize; 4],
    resolver_abstain_episodes: [usize; 4],
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    hypothesis: &'static str,
    corpus_path: String,
    corpus_sha256: String,
    source_p1k2_receipt_sha256: String,
    summary: ResolverSummary,
    ties: Vec<TieRow>,
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
        .filter(|word| !word.is_empty())
        .collect()
}

fn positions(words: &[&str], needle: &str) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(i, word)| (*word == needle).then_some(i))
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

fn marker(words: &[&str], start: usize, end: usize, markers: &[&str]) -> (u16, u32) {
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

fn feature(words: &[&str], left: usize, right: usize, distance: usize) -> Features {
    let low = left.min(right).saturating_sub(8);
    let high = (left.max(right) + 9).min(words.len());
    let split = left.min(right);
    let after_split = (left.max(right) + 1).min(words.len());
    let (finance, finance_mask) = marker(words, low, high, FINANCE);
    let (geo, geo_mask) = marker(words, low, high, GEO);
    let (transport, transport_mask) = marker(words, low, high, TRANSPORT);
    let (_, finance_before) = marker(words, low, split, FINANCE);
    let (_, geo_before) = marker(words, low, split, GEO);
    let (_, transport_before) = marker(words, low, split, TRANSPORT);
    let (_, finance_after) = marker(words, after_split, high, FINANCE);
    let (_, geo_after) = marker(words, after_split, high, GEO);
    let (_, transport_after) = marker(words, after_split, high, TRANSPORT);
    Features {
        counts: [finance, geo, transport],
        marker_masks: [finance_mask, geo_mask, transport_mask],
        before_masks: [finance_before, geo_before, transport_before],
        after_masks: [finance_after, geo_after, transport_after],
        distance,
    }
}

fn load_events(path: &Path) -> Result<(Vec<Event>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = digest(&bytes);
    let mut events = Vec::new();
    let mut documents = 0usize;
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus record")?;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let tokenized = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&tokenized, spec.source, spec.target) {
                events.push(Event {
                    doc: documents,
                    shard: 0,
                    candidate,
                    features: feature(&tokenized, left, right, distance),
                });
            }
        }
        documents += 1;
    }
    ensure!(documents > 0, "corpus is empty");
    for event in &mut events {
        event.shard = (event.doc * SHARDS / documents).min(SHARDS - 1);
    }
    Ok((events, documents, hash))
}

fn pairs(events: &[Event], shard: usize) -> Vec<Pair> {
    let normalized: Vec<Event> = events
        .iter()
        .copied()
        .filter(|event| event.shard == shard && event.features.distance <= MAX_PAIR_DISTANCE)
        .collect();
    let mut seen = vec![false; CANDIDATES.len() * 4];
    let mut out = Vec::new();
    for (index, event) in normalized.iter().enumerate() {
        let key = event.candidate * 4 + usize::from(event.features.fixed_phi());
        if seen[key] {
            continue;
        }
        seen[key] = true;
        let later = normalized
            .iter()
            .skip(index + 1)
            .copied()
            .filter(|candidate| {
                candidate.candidate == event.candidate && candidate.doc > event.doc
            });
        let witness = later
            .clone()
            .find(|candidate| candidate.features.fixed_phi() == event.features.fixed_phi())
            .or_else(|| later.into_iter().next());
        if let Some(witness) = witness {
            out.push(Pair {
                shard,
                nomination: *event,
                witness,
            });
        }
    }
    out
}

fn family_name(index: usize) -> &'static str {
    match index {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "general_fallback",
    }
}

fn fixed_route(counts: [u16; 3]) -> u8 {
    if counts[1] > 0 {
        1
    } else if counts[0] > 0 {
        0
    } else if counts[2] > 0 {
        2
    } else {
        3
    }
}

fn tied_families(counts: [u16; 3]) -> Vec<String> {
    let max = *counts.iter().max().unwrap_or(&0);
    counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == max)
        .map(|(i, _)| family_name(i).to_owned())
        .collect()
}

fn tied_indices(counts: [u16; 3]) -> Vec<usize> {
    let max = *counts.iter().max().unwrap_or(&0);
    counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == max)
        .map(|(i, _)| i)
        .collect()
}

fn unique_tie_route(scores: [u32; 3], tied: &[usize]) -> Option<&'static str> {
    if tied.len() != 2 {
        return None;
    }
    let a = tied[0];
    let b = tied[1];
    if scores[a] == scores[b] {
        None
    } else if scores[a] > scores[b] {
        Some(family_name(a))
    } else {
        Some(family_name(b))
    }
}

fn endpoint_view(role: &'static str, event: Event) -> EndpointView {
    EndpointView {
        role,
        document: event.doc,
        counts: event.features.counts,
        fixed_route: family_name(usize::from(event.features.fixed_phi())).to_owned(),
        tied_families: tied_families(event.features.counts),
        marker_mask_bits: event.features.marker_masks.map(Features::popcount),
        before_mask_bits: event.features.before_masks.map(Features::popcount),
        after_mask_bits: event.features.after_masks.map(Features::popcount),
        side_scores: [
            event.features.side_score(0),
            event.features.side_score(1),
            event.features.side_score(2),
        ],
        concentration_scores: [
            event.features.concentration(0),
            event.features.concentration(1),
            event.features.concentration(2),
        ],
        mixed_marker: event.features.mixed(),
    }
}

fn resolver_routes(nomination: Event, witness: Event) -> ResolverRoutes {
    fn route_for(a: Event, b: Event, mode: usize) -> String {
        let a_tied = tied_indices(a.features.counts);
        let b_tied = tied_indices(b.features.counts);
        if a_tied.len() != 2 || b_tied.len() != 2 || a_tied != b_tied {
            return "ABSTAIN".to_owned();
        }
        let scores = |family: usize| -> u32 {
            match mode {
                0 => a.features.side_score(family) + b.features.side_score(family),
                1 => a.features.concentration(family) + b.features.concentration(family),
                2 => {
                    (u32::from(a.features.before_masks[family] != 0)
                        + u32::from(b.features.before_masks[family] != 0))
                        * 2
                        + u32::from(a.features.after_masks[family] != 0)
                        + u32::from(b.features.after_masks[family] != 0)
                }
                _ => 0,
            }
        };
        let tied = a_tied;
        let values = [scores(tied[0]), scores(tied[1]), 0];
        unique_tie_route(values, &tied)
            .unwrap_or("ABSTAIN")
            .to_owned()
    }
    let fixed = {
        let a = fixed_route(nomination.features.counts);
        let b = fixed_route(witness.features.counts);
        if a < 3 && b < 3 && a == b {
            family_name(usize::from(a)).to_owned()
        } else {
            "ABSTAIN".to_owned()
        }
    };
    ResolverRoutes {
        side_context: route_for(nomination, witness, 0),
        marker_concentration: route_for(nomination, witness, 1),
        before_after_asymmetry: route_for(nomination, witness, 2),
        fixed_priority_control: fixed,
    }
}

fn main() -> Result<()> {
    let corpus_path = env::args().nth(1).unwrap_or_else(|| {
        "D:\\phoenix-evals\\beir\\screen-candidates\\nq\\corpus.jsonl".to_owned()
    });
    let p1k2_path = env::args()
        .nth(2)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1k2-nq\\receipt.json".to_owned());
    let output_path = env::args()
        .nth(3)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2p1k3-nq\\receipt.json".to_owned());
    let p1k2_bytes = fs::read(&p1k2_path).with_context(|| format!("read {p1k2_path}"))?;
    let p1k2: K2Receipt = serde_json::from_slice(&p1k2_bytes).context("decode P1K2 receipt")?;
    let (events, _documents, corpus_sha256) = load_events(Path::new(&corpus_path))?;
    ensure!(
        corpus_sha256 == p1k2.corpus_sha256,
        "corpus differs from sealed P1K2 receipt"
    );
    let pairs: Vec<Pair> = (0..SHARDS)
        .flat_map(|shard| pairs(&events, shard))
        .collect();
    let by_key: HashMap<(usize, usize, usize), Pair> = pairs
        .iter()
        .copied()
        .map(|pair| {
            (
                (
                    pair.nomination.candidate,
                    pair.nomination.doc,
                    pair.witness.doc,
                ),
                pair,
            )
        })
        .collect();
    let mut summary = ResolverSummary::default();
    let mut episode_keys = BTreeSet::new();
    let mut episode_outcomes: HashMap<(usize, usize, usize), [HashSet<&'static str>; 4]> =
        HashMap::new();
    let mut ties = Vec::new();
    for row in &p1k2.pairs {
        if row.plurality_outcome != "ABSTAIN_TIE"
            || !matches!(
                row.current_outcome.as_str(),
                "VALID_ACTIONABLE" | "INVALID_ACTIONABLE"
            )
        {
            continue;
        }
        let candidate_index = CANDIDATES
            .iter()
            .position(|candidate| candidate.id == row.candidate)
            .context("unknown candidate in P1K2 receipt")?;
        let pair = by_key
            .get(&(candidate_index, row.nomination_doc, row.witness_doc))
            .context("P1K2 pair not reproducible")?;
        ensure!(pair.shard == row.shard, "P1K2 shard mismatch");
        let candidate = CANDIDATES[candidate_index];
        let current_validity = if row.current_outcome == "VALID_ACTIONABLE" {
            "VALID"
        } else {
            "INVALID"
        };
        summary.actionable_tie_rows += 1;
        summary.valid_tie_rows += usize::from(current_validity == "VALID");
        summary.invalid_tie_rows += usize::from(current_validity == "INVALID");
        let key = (pair.shard, pair.nomination.doc, pair.witness.doc);
        episode_keys.insert(key);
        let routes = resolver_routes(pair.nomination, pair.witness);
        let names = [
            &routes.side_context,
            &routes.marker_concentration,
            &routes.before_after_asymmetry,
            &routes.fixed_priority_control,
        ];
        let episode = episode_outcomes
            .entry(key)
            .or_insert_with(|| std::array::from_fn(|_| HashSet::new()));
        for (index, route) in names.iter().enumerate() {
            let outcome = if route.as_str() == "ABSTAIN" {
                "ABSTAIN"
            } else if route.as_str() == family_name(candidate.expected_phi.max(0) as usize)
                || candidate.expected_phi < 0
            {
                "VALID"
            } else {
                "INVALID"
            };
            episode[index].insert(outcome);
            if outcome == "VALID" {
                summary.resolver_valid_rows[index] += 1;
            } else if outcome == "INVALID" {
                summary.resolver_invalid_rows[index] += 1;
            } else {
                summary.resolver_abstain_rows[index] += 1;
            }
        }
        ties.push(TieRow {
            episode_id: format!(
                "s{}:{}->{}",
                pair.shard, pair.nomination.doc, pair.witness.doc
            ),
            candidate: candidate.id.to_owned(),
            expected_phi: candidate.expected_phi,
            current_outcome: row.current_outcome.clone(),
            witness_kind: row.witness_kind.clone(),
            current_validity,
            nomination: endpoint_view("nomination", pair.nomination),
            witness: endpoint_view("witness", pair.witness),
            resolver_routes: routes,
        });
    }
    summary.actionable_tie_episodes = episode_keys.len();
    for (key, outcomes) in episode_outcomes {
        let _ = key;
        for index in 0..4 {
            if outcomes[index].contains("VALID") {
                summary.resolver_valid_episodes[index] += 1;
            } else if outcomes[index].contains("INVALID") {
                summary.resolver_invalid_episodes[index] += 1;
            } else {
                summary.resolver_abstain_episodes[index] += 1;
            }
        }
    }
    ties.sort_by_key(|tie| tie.episode_id.clone());
    let receipt = Receipt {
        schema: SCHEMA,
        scope: "NQ discovery-only tie anatomy; no router, learner, authority, ranking, or serving changes",
        hypothesis: "plurality ties may contain recoverable contextual ownership, while some ties should remain abstentions",
        corpus_path,
        corpus_sha256,
        source_p1k2_receipt_sha256: digest(&p1k2_bytes),
        summary,
        ties,
        interpretation_boundary: "P1K3 is descriptive discovery on NQ and cannot qualify a tie resolver; any policy must be frozen before P1K4 on HotpotQA",
    };
    let output = Path::new(&output_path);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "P1K3 tie anatomy receipt: {} rows={} episodes={}",
        output.display(),
        receipt.summary.actionable_tie_rows,
        receipt.summary.actionable_tie_episodes
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_priority_is_geography_first() {
        assert_eq!(fixed_route([1, 1, 2]), 1);
    }
    #[test]
    fn side_route_requires_unique_tied_family() {
        let scores = [2, 1, 0];
        assert_eq!(unique_tie_route(scores, &[0, 1]), Some("finance"));
        assert_eq!(unique_tie_route([1, 1, 0], &[0, 1]), None);
    }
    #[test]
    fn feature_mixed_and_fixed_phi_are_deterministic() {
        let value = Features {
            counts: [1, 0, 2],
            ..Features::default()
        };
        assert!(value.mixed());
        assert_eq!(value.fixed_phi(), 0);
    }
}
