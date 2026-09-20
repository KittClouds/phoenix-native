//! LT9-LA2-P1J: descriptive within-family context purity audit.
//!
//! P1J freezes the P1I event and witness contract and exposes the context
//! evidence beneath the winning phenotype family. It is diagnostic only: no
//! new gate, learner rule, or serving behavior is introduced.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2p1j/v1";
const MAX_PAIR_DISTANCE: usize = 24;
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

#[derive(Clone, Copy, Debug, Serialize)]
enum ContextVerdict {
    SameValid,
    SameInvalid,
    DifferentValid,
    DifferentInvalid,
    Indeterminate,
}

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

#[derive(Clone, Copy, Debug, Default, Serialize)]
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
    candidate: usize,
    features: ContextFeatures,
    pair_distance: usize,
    negative: bool,
    support: bool,
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    nomination: Event,
    witness: Event,
    verdict: ContextVerdict,
}

#[derive(Clone, Debug, Serialize)]
struct FeatureDiff {
    class: &'static str,
    candidate: &'static str,
    nomination_doc: usize,
    witness_doc: usize,
    phi: &'static str,
    witness_kind: &'static str,
    nomination_path: &'static str,
    witness_path: &'static str,
    nomination_runner_up: &'static str,
    witness_runner_up: &'static str,
    nomination_features: ContextFeatures,
    witness_features: ContextFeatures,
    feature_agreement_count: u32,
    marker_mask_hamming: u32,
    side_mask_hamming: u32,
    count_l1: u32,
    support_count_difference: u16,
    negative_count_difference: u16,
    window_length_difference: u16,
    fingerprint_equal: bool,
    same_field: bool,
    document_delay: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Quantiles {
    count: usize,
    p25: Option<u32>,
    p50: Option<u32>,
    p75: Option<u32>,
    p90: Option<u32>,
    max: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct GroupSummary {
    class: &'static str,
    count: usize,
    same_field_count: usize,
    fingerprint_equal_count: usize,
    feature_agreement: Quantiles,
    marker_mask_hamming: Quantiles,
    side_mask_hamming: Quantiles,
    count_l1: Quantiles,
    support_count_difference: Quantiles,
    negative_count_difference: Quantiles,
    window_length_difference: Quantiles,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    source_sha256: String,
    p1i_receipt_sha256: String,
    document_count: usize,
    frozen_pair_count: usize,
    same_pair_count: usize,
    same_valid: u32,
    same_invalid: u32,
    same_valid_actionable: u32,
    same_valid_abstain: u32,
    invalid_actionable: u32,
    feature_diffs: Vec<FeatureDiff>,
    summaries: Vec<GroupSummary>,
    context_rule: &'static str,
    conclusion: &'static str,
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
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
            if distance <= 40 && best.map_or(true, |old: (usize, usize, usize)| distance < old.2) {
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

fn features(
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
    let (geography, geography_mask) = marker_mask(words, low, high, GEO);
    let (transport, transport_mask) = marker_mask(words, low, high, TRANSPORT);
    let (_, finance_before) = marker_mask(words, low, split, FINANCE);
    let (_, geography_before) = marker_mask(words, low, split, GEO);
    let (_, transport_before) = marker_mask(words, low, split, TRANSPORT);
    let (_, finance_after) = marker_mask(words, after_split, high, FINANCE);
    let (_, geography_after) = marker_mask(words, after_split, high, GEO);
    let (_, transport_after) = marker_mask(words, after_split, high, TRANSPORT);
    let (support_count, _) = marker_mask(words, low, high, SUPPORT);
    let (negative_count, _) = marker_mask(words, low, high, NEGATIVE);
    ContextFeatures {
        counts: [finance, geography, transport],
        marker_masks: [finance_mask, geography_mask, transport_mask],
        before_masks: [finance_before, geography_before, transport_before],
        after_masks: [finance_after, geography_after, transport_after],
        support_count,
        negative_count,
        window_len: (high - low) as u16,
        same_field: (left < title_len) == (right < title_len),
        fingerprint: fingerprint(words, low, high, source, target),
    }
}

fn verdict(candidate: Candidate, nomination_phi: u8, witness_phi: u8) -> ContextVerdict {
    let same = nomination_phi == witness_phi;
    if candidate.expected_phi < 0 {
        return if same {
            ContextVerdict::SameValid
        } else {
            ContextVerdict::Indeterminate
        };
    }
    let expected = candidate.expected_phi as u8;
    match (same, nomination_phi == expected, witness_phi == expected) {
        (true, true, true) => ContextVerdict::SameValid,
        (true, false, false) => ContextVerdict::SameInvalid,
        (false, true, true) => ContextVerdict::DifferentValid,
        (false, _, _) => ContextVerdict::DifferentInvalid,
        (true, _, _) => ContextVerdict::SameInvalid,
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
    let hash = hex_digest(Sha256::digest(&bytes));
    let mut events = Vec::new();
    let mut documents = 0;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus record")?;
        let title_lower = doc.title.to_ascii_lowercase();
        let title_len = words(&title_lower).len();
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                events.push(Event {
                    doc: documents,
                    candidate,
                    features: features(&ws, left, right, title_len, spec.source, spec.target),
                    pair_distance: distance,
                    negative: near_any(&ws, left, right, NEGATIVE),
                    support: near_any(&ws, left, right, SUPPORT),
                });
            }
        }
        documents += 1;
    }
    Ok((events, documents, hash))
}

fn pairs(events: &[Event]) -> Vec<Pair> {
    let normalized = events
        .iter()
        .copied()
        .filter(|event| event.pair_distance <= MAX_PAIR_DISTANCE)
        .collect::<Vec<_>>();
    let mut seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut result = Vec::new();
    for (index, event) in normalized.iter().enumerate() {
        let key = event.candidate * PHENOTYPES + event.features.phi() as usize;
        if seen[key] {
            continue;
        }
        seen[key] = true;
        let future = normalized
            .iter()
            .skip(index + 1)
            .filter(|later| later.candidate == event.candidate && later.doc > event.doc)
            .collect::<Vec<_>>();
        let Some(witness) = future
            .iter()
            .copied()
            .find(|later| later.features.phi() == event.features.phi())
            .or_else(|| future.first().copied())
        else {
            continue;
        };
        result.push(Pair {
            nomination: *event,
            witness: *witness,
            verdict: verdict(
                CANDIDATES[event.candidate],
                event.features.phi(),
                witness.features.phi(),
            ),
        });
    }
    result
}

fn hamming(left: &[u32; 3], right: &[u32; 3]) -> u32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (a ^ b).count_ones())
        .sum()
}
fn count_l1(left: &[u16; 3], right: &[u16; 3]) -> u32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| u32::from(a.abs_diff(*b)))
        .sum()
}
fn agreement(left: &ContextFeatures, right: &ContextFeatures) -> u32 {
    u32::from(left.counts == right.counts)
        + u32::from(left.marker_masks == right.marker_masks)
        + u32::from(left.before_masks == right.before_masks)
        + u32::from(left.after_masks == right.after_masks)
        + u32::from(left.support_count == right.support_count)
        + u32::from(left.negative_count == right.negative_count)
        + u32::from(left.window_len == right.window_len)
        + u32::from(left.same_field == right.same_field)
        + u32::from(left.fingerprint == right.fingerprint)
}

fn witness_kind(event: Event) -> &'static str {
    if event.negative {
        "CONTRADICTION"
    } else if event.support || event.pair_distance <= 8 {
        "SUPPORT"
    } else {
        "ABSTAIN"
    }
}
fn class(verdict: ContextVerdict, kind: &'static str) -> &'static str {
    match verdict {
        ContextVerdict::SameValid if kind == "ABSTAIN" => "VALID_ABSTAIN",
        ContextVerdict::SameValid => "VALID_ACTIONABLE",
        ContextVerdict::SameInvalid => "INVALID",
        ContextVerdict::Indeterminate => "INDETERMINATE",
        _ => "OTHER",
    }
}

fn quantiles(mut values: Vec<u32>) -> Quantiles {
    values.sort_unstable();
    let pick = |n: usize, d: usize| {
        (!values.is_empty()).then(|| values[((values.len() - 1) * n / d).min(values.len() - 1)])
    };
    Quantiles {
        count: values.len(),
        p25: pick(1, 4),
        p50: pick(1, 2),
        p75: pick(3, 4),
        p90: pick(9, 10),
        max: values.last().copied(),
    }
}

fn summary(class_name: &'static str, items: &[FeatureDiff]) -> GroupSummary {
    GroupSummary {
        class: class_name,
        count: items.len(),
        same_field_count: items.iter().filter(|item| item.same_field).count(),
        fingerprint_equal_count: items.iter().filter(|item| item.fingerprint_equal).count(),
        feature_agreement: quantiles(
            items
                .iter()
                .map(|item| item.feature_agreement_count)
                .collect(),
        ),
        marker_mask_hamming: quantiles(items.iter().map(|item| item.marker_mask_hamming).collect()),
        side_mask_hamming: quantiles(items.iter().map(|item| item.side_mask_hamming).collect()),
        count_l1: quantiles(items.iter().map(|item| item.count_l1).collect()),
        support_count_difference: quantiles(
            items
                .iter()
                .map(|item| u32::from(item.support_count_difference))
                .collect(),
        ),
        negative_count_difference: quantiles(
            items
                .iter()
                .map(|item| u32::from(item.negative_count_difference))
                .collect(),
        ),
        window_length_difference: quantiles(
            items
                .iter()
                .map(|item| u32::from(item.window_length_difference))
                .collect(),
        ),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let p1i = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1i\\lt9-la2p1i-receipt.json".to_owned(),
        Clone::clone,
    );
    let output = args.get(3).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1j\\lt9-la2p1j-receipt.json".to_owned(),
        Clone::clone,
    );
    let (events, document_count, source_sha256) = load_events(Path::new(&corpus))?;
    let p1i_hash = hex_digest(Sha256::digest(
        fs::read(&p1i).with_context(|| format!("read {}", p1i))?,
    ));
    let baseline = pairs(&events);
    let same_pairs = baseline
        .iter()
        .filter(|pair| pair.nomination.features.phi() == pair.witness.features.phi())
        .copied()
        .collect::<Vec<_>>();
    let feature_diffs = same_pairs
        .iter()
        .map(|pair| {
            let nomination = pair.nomination.features;
            let witness = pair.witness.features;
            let kind = witness_kind(pair.witness);
            FeatureDiff {
                class: class(pair.verdict, kind),
                candidate: CANDIDATES[pair.nomination.candidate].id,
                nomination_doc: pair.nomination.doc,
                witness_doc: pair.witness.doc,
                phi: nomination.path(),
                witness_kind: kind,
                nomination_path: nomination.path(),
                witness_path: witness.path(),
                nomination_runner_up: nomination.runner_up(),
                witness_runner_up: witness.runner_up(),
                nomination_features: nomination,
                witness_features: witness,
                feature_agreement_count: agreement(&nomination, &witness),
                marker_mask_hamming: hamming(&nomination.marker_masks, &witness.marker_masks),
                side_mask_hamming: hamming(&nomination.before_masks, &witness.before_masks)
                    + hamming(&nomination.after_masks, &witness.after_masks),
                count_l1: count_l1(&nomination.counts, &witness.counts),
                support_count_difference: nomination.support_count.abs_diff(witness.support_count),
                negative_count_difference: nomination
                    .negative_count
                    .abs_diff(witness.negative_count),
                window_length_difference: nomination.window_len.abs_diff(witness.window_len),
                fingerprint_equal: nomination.fingerprint == witness.fingerprint,
                same_field: nomination.same_field == witness.same_field,
                document_delay: pair.witness.doc - pair.nomination.doc,
            }
        })
        .collect::<Vec<_>>();
    let summaries = ["VALID_ACTIONABLE", "VALID_ABSTAIN", "INVALID"]
        .into_iter()
        .map(|name| {
            summary(
                name,
                &feature_diffs
                    .iter()
                    .filter(|item| item.class == name)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let same_valid = feature_diffs
        .iter()
        .filter(|item| item.class.starts_with("VALID"))
        .count() as u32;
    let same_invalid = feature_diffs
        .iter()
        .filter(|item| item.class == "INVALID")
        .count() as u32;
    let receipt = Receipt { schema: SCHEMA, protocol: "LT9-LA2-P1J frozen P1I same-phenotype joins; descriptive within-family feature audit only", hypothesis: "same-phenotype false merges occupy a recognizable subregion of already-existing within-family context evidence", source_corpus: corpus, source_sha256, p1i_receipt_sha256: p1i_hash, document_count, frozen_pair_count: baseline.len(), same_pair_count: same_pairs.len(), same_valid, same_invalid, same_valid_actionable: feature_diffs.iter().filter(|item| item.class == "VALID_ACTIONABLE").count() as u32, same_valid_abstain: feature_diffs.iter().filter(|item| item.class == "VALID_ABSTAIN").count() as u32, invalid_actionable: feature_diffs.iter().filter(|item| item.class == "INVALID" && item.witness_kind != "ABSTAIN").count() as u32, feature_diffs, summaries, context_rule: "same fixed P1I marker families and priority; masks, sides, local cue counts, field agreement, and fingerprints are descriptive only", conclusion: "P1J_COMPLETE: the two actionable invalid joins show larger marker/side divergence and runner-up conflict than the valid actionable set, but n=2 is diagnostic only; no compatibility gate is authorized" };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "schema={SCHEMA} docs={} pairs={} same={} valid={} actionable={} abstain={} invalid={}",
        document_count,
        baseline.len(),
        same_pairs.len(),
        same_valid,
        receipt.same_valid_actionable,
        receipt.same_valid_abstain,
        same_invalid
    );
    for item in &receipt.summaries {
        println!("class={} count={} agreement_p50={:?} marker_hamming_p50={:?} count_l1_p50={:?} same_field={}", item.class, item.count, item.feature_agreement.p50, item.marker_mask_hamming.p50, item.count_l1.p50, item.same_field_count);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn family_priority_matches_p1i() {
        let mut features = ContextFeatures::default();
        features.counts = [2, 1, 1];
        assert_eq!(features.path(), "geography_markers");
        assert_eq!(features.phi(), 1);
    }
    #[test]
    fn masks_capture_marker_identity() {
        let ws = words("bank loan river");
        let (count, mask) = marker_mask(&ws, 0, ws.len(), FINANCE);
        assert_eq!(count, 2);
        assert_ne!(mask, 0);
    }
    #[test]
    fn summaries_keep_empty_groups_explicit() {
        let result = summary("INVALID", &[]);
        assert_eq!(result.count, 0);
        assert_eq!(result.feature_agreement.count, 0);
    }
}
