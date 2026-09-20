//! LT9-LA2 corpus suitability screen.
//!
//! This is a label-blind preflight. It replays the frozen candidate-event,
//! marker, shard, and witness mechanics, but never reads qrels or classifies
//! a join as valid or invalid. Its only purpose is to measure whether an
//! unopened corpus contains enough lexical events to justify P1K2.

use std::collections::{BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9la2corpus-screen/v1";
const SHARDS: usize = 8;
const MAX_PAIR_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
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

#[derive(Clone, Copy)]
struct Candidate {
    id: &'static str,
    source: &'static str,
    target: &'static str,
}

const CANDIDATES: [Candidate; 12] = [
    Candidate {
        id: "repair_to_fix",
        source: "repair",
        target: "fix",
    },
    Candidate {
        id: "engine_to_motor",
        source: "engine",
        target: "motor",
    },
    Candidate {
        id: "car_to_vehicle",
        source: "car",
        target: "vehicle",
    },
    Candidate {
        id: "vehicle_to_car",
        source: "vehicle",
        target: "car",
    },
    Candidate {
        id: "bank_to_shore",
        source: "bank",
        target: "shore",
    },
    Candidate {
        id: "bank_to_lender",
        source: "bank",
        target: "lender",
    },
    Candidate {
        id: "economic_to_tumor",
        source: "economic",
        target: "tumor",
    },
    Candidate {
        id: "loan_to_debt",
        source: "loan",
        target: "debt",
    },
    Candidate {
        id: "credit_to_loan",
        source: "credit",
        target: "loan",
    },
    Candidate {
        id: "insurance_to_coverage",
        source: "insurance",
        target: "coverage",
    },
    Candidate {
        id: "stock_to_bond",
        source: "stock",
        target: "bond",
    },
    Candidate {
        id: "bank_to_water",
        source: "bank",
        target: "water",
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
    support: bool,
    negative: bool,
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
    fn inversion(self) -> bool {
        let max = *self.counts.iter().max().unwrap_or(&0);
        let winners = self.counts.iter().filter(|count| **count == max).count();
        if winners != 1 {
            return false;
        }
        let argmax = self.counts.iter().position(|count| *count == max).unwrap();
        let priority = self.fixed_phi();
        priority < 3 && argmax != usize::from(priority)
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
enum WitnessKind {
    Support,
    Contradiction,
    Abstain,
}

impl WitnessKind {
    fn actionable(self) -> bool {
        !matches!(self, Self::Abstain)
    }
}

#[derive(Clone, Copy)]
struct Pair {
    shard: usize,
    nomination: Event,
    witness: Event,
    kind: WitnessKind,
}

#[derive(Default, Serialize)]
struct CorpusMetrics {
    path: String,
    corpus_sha256: String,
    document_count: usize,
    observed_event_count: usize,
    candidate_relations: usize,
    nomination_count: usize,
    directional_join_count: usize,
    same_phenotype_join_count: usize,
    same_phenotype_episode_count: usize,
    actionable_witness_count: usize,
    same_phenotype_actionable_witness_count: usize,
    mixed_marker_endpoint_count: usize,
    mixed_marker_endpoint_rate: f64,
    priority_inversion_endpoint_count: usize,
    priority_inversion_endpoint_rate: f64,
    priority_inversion_episode_count: usize,
    phenotype_family_coverage: Vec<String>,
    phenotype_family_event_counts: [usize; 4],
    episode_count: usize,
    viability: Viability,
}

#[derive(Default, Serialize)]
struct Viability {
    candidate_relations_min: usize,
    same_phenotype_episodes_min: usize,
    mixed_marker_endpoints_min: usize,
    priority_inversion_endpoints_min: usize,
    phenotype_families_min: usize,
    eligible: bool,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    protocol: &'static str,
    corpora: Vec<CorpusMetrics>,
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

fn count_markers(words: &[&str], start: usize, end: usize, markers: &[&str]) -> u16 {
    words[start..end]
        .iter()
        .map(|word| markers.iter().filter(|marker| *word == **marker).count() as u16)
        .sum()
}

fn near_any(words: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(words.len());
    words[start..end]
        .iter()
        .any(|word| markers.iter().any(|marker| *word == *marker))
}

fn event_features(words: &[&str], left: usize, right: usize, distance: usize) -> Features {
    let low = left.min(right).saturating_sub(8);
    let high = (left.max(right) + 9).min(words.len());
    Features {
        counts: [
            count_markers(words, low, high, FINANCE),
            count_markers(words, low, high, GEO),
            count_markers(words, low, high, TRANSPORT),
        ],
        support: near_any(words, left, right, SUPPORT),
        negative: near_any(words, left, right, NEGATIVE),
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
        let title_len = words(&doc.title.to_ascii_lowercase()).len();
        let _ = title_len;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let tokenized = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&tokenized, spec.source, spec.target) {
                events.push(Event {
                    doc: documents,
                    shard: 0,
                    candidate,
                    features: event_features(&tokenized, left, right, distance),
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

fn witness_kind(event: Event) -> WitnessKind {
    if event.features.negative {
        WitnessKind::Contradiction
    } else if event.features.support || event.features.distance <= SUPPORT_DISTANCE {
        WitnessKind::Support
    } else {
        WitnessKind::Abstain
    }
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
                kind: witness_kind(witness),
            });
        }
    }
    out
}

fn family_name(phi: u8) -> &'static str {
    match phi {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "general_fallback",
    }
}

fn metrics(path: &Path) -> Result<CorpusMetrics> {
    let (events, document_count, corpus_sha256) = load_events(path)?;
    let mut candidate_ids = BTreeSet::new();
    let mut family_counts = [0usize; 4];
    let mut mixed_endpoint_count = 0usize;
    let mut inversion_endpoint_count = 0usize;
    for event in &events {
        candidate_ids.insert(CANDIDATES[event.candidate].id);
        family_counts[usize::from(event.features.fixed_phi())] += 1;
        mixed_endpoint_count += usize::from(event.features.mixed());
        inversion_endpoint_count += usize::from(event.features.inversion());
    }
    let all_pairs: Vec<Pair> = (0..SHARDS)
        .flat_map(|shard| pairs(&events, shard))
        .collect();
    let mut episode_keys = HashSet::new();
    let mut same_episode_keys = HashSet::new();
    let mut inversion_episode_keys = HashSet::new();
    let mut same_joins = 0usize;
    let mut same_actionable = 0usize;
    let mut actionable = 0usize;
    for pair in &all_pairs {
        let key = (pair.shard, pair.nomination.doc, pair.witness.doc);
        episode_keys.insert(key);
        let same = pair.nomination.features.fixed_phi() == pair.witness.features.fixed_phi();
        if same {
            same_joins += 1;
            same_episode_keys.insert(key);
        }
        if pair.kind.actionable() {
            actionable += 1;
            same_actionable += usize::from(same);
        }
        if pair.nomination.features.inversion() || pair.witness.features.inversion() {
            inversion_episode_keys.insert(key);
        }
    }
    let family_coverage: Vec<String> = family_counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(index, _)| family_name(index as u8).to_owned())
        .collect();
    let mixed_rate = if events.is_empty() {
        0.0
    } else {
        mixed_endpoint_count as f64 / events.len() as f64
    };
    let inversion_rate = if events.is_empty() {
        0.0
    } else {
        inversion_endpoint_count as f64 / events.len() as f64
    };
    let mut viability = Viability {
        candidate_relations_min: 4,
        same_phenotype_episodes_min: 20,
        mixed_marker_endpoints_min: 10,
        priority_inversion_endpoints_min: 5,
        phenotype_families_min: 2,
        ..Viability::default()
    };
    if candidate_ids.len() < viability.candidate_relations_min {
        viability.reasons.push(format!(
            "candidate_relations {} < {}",
            candidate_ids.len(),
            viability.candidate_relations_min
        ));
    }
    if same_episode_keys.len() < viability.same_phenotype_episodes_min {
        viability.reasons.push(format!(
            "same_phenotype_episodes {} < {}",
            same_episode_keys.len(),
            viability.same_phenotype_episodes_min
        ));
    }
    if mixed_endpoint_count < viability.mixed_marker_endpoints_min {
        viability.reasons.push(format!(
            "mixed_marker_endpoints {} < {}",
            mixed_endpoint_count, viability.mixed_marker_endpoints_min
        ));
    }
    if inversion_endpoint_count < viability.priority_inversion_endpoints_min {
        viability.reasons.push(format!(
            "priority_inversion_endpoints {} < {}",
            inversion_endpoint_count, viability.priority_inversion_endpoints_min
        ));
    }
    if family_coverage.len() < viability.phenotype_families_min {
        viability.reasons.push(format!(
            "phenotype_families {} < {}",
            family_coverage.len(),
            viability.phenotype_families_min
        ));
    }
    viability.eligible = viability.reasons.is_empty();
    Ok(CorpusMetrics {
        path: path.display().to_string(),
        corpus_sha256,
        document_count,
        observed_event_count: events.len(),
        candidate_relations: candidate_ids.len(),
        nomination_count: all_pairs.len(),
        directional_join_count: all_pairs.len(),
        same_phenotype_join_count: same_joins,
        same_phenotype_episode_count: same_episode_keys.len(),
        actionable_witness_count: actionable,
        same_phenotype_actionable_witness_count: same_actionable,
        mixed_marker_endpoint_count: mixed_endpoint_count,
        mixed_marker_endpoint_rate: mixed_rate,
        priority_inversion_endpoint_count: inversion_endpoint_count,
        priority_inversion_endpoint_rate: inversion_rate,
        priority_inversion_episode_count: inversion_episode_keys.len(),
        phenotype_family_coverage: family_coverage,
        phenotype_family_event_counts: family_counts,
        episode_count: episode_keys.len(),
        viability,
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    ensure!(!args.is_empty(), "pass one or more corpus.jsonl paths");
    let output = env::var("LT9_SCREEN_OUTPUT")
        .unwrap_or_else(|_| "D:\\phoenix-evals\\lt9-la2corpus-screen\\receipt.json".to_owned());
    let corpora = args
        .iter()
        .map(|path| metrics(Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let receipt = Receipt {
        schema: SCHEMA,
        scope: "label-blind corpus suitability only; no qrels, validity labels, learning, authority, ranking, or serving changes",
        protocol: "frozen floors: >=4 candidate relations, >=20 same-phenotype episodes, >=10 mixed-marker endpoints, >=5 priority-inversion endpoints, >=2 phenotype families",
        corpora,
    };
    let output_path = PathBuf::from(output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, serde_json::to_vec_pretty(&receipt)?)?;
    for corpus in &receipt.corpora {
        println!(
            "{}: events={} joins={} same={} episodes={} inversions={} eligible={}",
            corpus.path,
            corpus.observed_event_count,
            corpus.directional_join_count,
            corpus.same_phenotype_join_count,
            corpus.same_phenotype_episode_count,
            corpus.priority_inversion_endpoint_count,
            corpus.viability.eligible
        );
    }
    println!("screen receipt: {}", output_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_inversion_is_only_unique_non_tied_override() {
        assert!(!Features {
            counts: [1, 0, 0],
            ..Features::default()
        }
        .inversion());
        assert!(Features {
            counts: [1, 0, 2],
            ..Features::default()
        }
        .inversion());
        assert!(!Features {
            counts: [1, 0, 1],
            ..Features::default()
        }
        .inversion());
    }

    #[test]
    fn actionability_excludes_abstain_only() {
        assert!(WitnessKind::Support.actionable());
        assert!(WitnessKind::Contradiction.actionable());
        assert!(!WitnessKind::Abstain.actionable());
    }

    #[test]
    fn fixed_priority_prefers_geography_then_finance_then_transport() {
        assert_eq!(
            Features {
                counts: [1, 2, 3],
                ..Features::default()
            }
            .fixed_phi(),
            1
        );
        assert_eq!(
            Features {
                counts: [2, 0, 3],
                ..Features::default()
            }
            .fixed_phi(),
            0
        );
        assert_eq!(
            Features {
                counts: [0, 0, 3],
                ..Features::default()
            }
            .fixed_phi(),
            2
        );
    }
}
