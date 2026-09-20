//! LT9-LA2-P1: phenotype-boundary audit.
//!
//! P1 replays the same frozen natural extraction used by LA2-A/W1 and emits
//! the nomination/witness pairs that W1 counted.  It adds raw context-marker
//! vectors and a post-hoc boundary classification.  No phenotype, witness,
//! or eligibility rule is changed by this instrument.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1/v1";
const MAX_PAIR_DISTANCE: usize = 24;
const PHENOTYPES: usize = 4;

#[derive(Clone, Copy, Debug, Serialize)]
enum ContextVerdict {
    SameValid,
    SameInvalid,
    DifferentValid,
    DifferentInvalid,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Serialize)]
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
struct ContextVector {
    finance: u16,
    geography: u16,
    transport: u16,
}

impl ContextVector {
    fn distance(self, other: Self) -> f32 {
        let delta = self.finance.abs_diff(other.finance) as f32
            + self.geography.abs_diff(other.geography) as f32
            + self.transport.abs_diff(other.transport) as f32;
        let mass = (self.finance
            + self.geography
            + self.transport
            + other.finance
            + other.geography
            + other.transport)
            .max(1) as f32;
        delta / mass
    }
}

#[derive(Clone, Copy, Debug)]
struct Event {
    doc: usize,
    candidate: usize,
    phi: u8,
    distance: usize,
    vector: ContextVector,
}

#[derive(Clone, Debug, Serialize)]
struct DelaySummary {
    count: usize,
    p25: Option<usize>,
    p50: Option<usize>,
    p75: Option<usize>,
    p90: Option<usize>,
    max: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
struct DistanceSummary {
    count: usize,
    p25: Option<f32>,
    p50: Option<f32>,
    p75: Option<f32>,
    p90: Option<f32>,
    max: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
struct BoundaryCounts {
    same_valid: u32,
    same_invalid: u32,
    different_valid: u32,
    different_invalid: u32,
    indeterminate: u32,
}

#[derive(Clone, Debug, Serialize)]
struct PairAudit {
    candidate: &'static str,
    nomination_doc: usize,
    witness_doc: usize,
    delay_docs: usize,
    phi_nomination: String,
    phi_witness: String,
    nomination_vector: ContextVector,
    witness_vector: ContextVector,
    raw_context_distance: f32,
    verdict: ContextVerdict,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct W1Baseline {
    nominations: u32,
    later_candidate_witness: u32,
    same_phenotype_witness: u32,
    phenotype_mismatch: u32,
    owned_witnesses: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct W1Receipt {
    baseline: W1Baseline,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    source_sha256: String,
    w1_receipt_sha256: String,
    document_count: usize,
    event_count_at_max_distance_40: usize,
    baseline_reproduction: W1Baseline,
    boundary_counts: BoundaryCounts,
    raw_distance_by_verdict: BTreeMap<String, DistanceSummary>,
    delay_by_witness_kind: BTreeMap<String, DelaySummary>,
    pairs: Vec<PairAudit>,
    context_rule: &'static str,
    context_label_limit: &'static str,
    conclusion: &'static str,
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
            let distance = left.abs_diff(right);
            if distance <= 40 && best.map_or(true, |old: (usize, usize, usize)| distance < old.2) {
                best = Some((left, right, distance));
            }
        }
    }
    best
}

fn near_count(ws: &[&str], left: usize, right: usize, markers: &[&str]) -> u16 {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(ws.len());
    ws[start..end]
        .iter()
        .filter(|w| markers.iter().any(|m| *w == m))
        .count() as u16
}

fn context(ws: &[&str], left: usize, right: usize) -> (u8, ContextVector) {
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
    let vector = ContextVector {
        finance: near_count(ws, left, right, FINANCE),
        geography: near_count(ws, left, right, GEO),
        transport: near_count(ws, left, right, TRANSPORT),
    };
    let phi = if vector.geography > 0 {
        1
    } else if vector.finance > 0 {
        0
    } else if vector.transport > 0 {
        2
    } else {
        3
    };
    (phi, vector)
}

fn phi_label(phi: u8) -> String {
    ["finance", "geography", "transport", "general"][phi as usize].to_owned()
}

fn percentile(values: &[usize], numerator: usize, denominator: usize) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    Some(values[((values.len() - 1) * numerator / denominator).min(values.len() - 1)])
}

fn summarize(mut values: Vec<usize>) -> DelaySummary {
    values.sort_unstable();
    DelaySummary {
        count: values.len(),
        p25: percentile(&values, 1, 4),
        p50: percentile(&values, 1, 2),
        p75: percentile(&values, 3, 4),
        p90: percentile(&values, 9, 10),
        max: values.last().copied(),
    }
}

fn summarize_distance(mut values: Vec<f32>) -> DistanceSummary {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |numerator: usize, denominator: usize| {
        (!values.is_empty())
            .then(|| values[((values.len() - 1) * numerator / denominator).min(values.len() - 1)])
    };
    DistanceSummary {
        count: values.len(),
        p25: pick(1, 4),
        p50: pick(1, 2),
        p75: pick(3, 4),
        p90: pick(9, 10),
        max: values.last().copied(),
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

fn verdict_label(verdict: ContextVerdict) -> String {
    format!("{verdict:?}").to_ascii_uppercase()
}

fn load_events(path: &Path) -> Result<(Vec<Event>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut events = Vec::new();
    let mut documents = 0;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus JSONL record")?;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                let (phi, vector) = context(&ws, left, right);
                events.push(Event {
                    doc: documents,
                    candidate,
                    phi,
                    distance,
                    vector,
                });
            }
        }
        documents += 1;
    }
    Ok((events, documents, hash))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let w1_path = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2w1\\lt9-la2w1-receipt.json".to_owned(),
        Clone::clone,
    );
    let output = args.get(3).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1\\lt9-la2p1-receipt.json".to_owned(),
        Clone::clone,
    );
    let (events, document_count, source_hash) = load_events(Path::new(&corpus))?;
    let w1_bytes = fs::read(&w1_path).with_context(|| format!("read {}", w1_path))?;
    let w1_hash = Sha256::digest(&w1_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let w1: W1Receipt = serde_json::from_slice(&w1_bytes).context("decode W1 receipt")?;
    let baseline_events: Vec<Event> = events
        .iter()
        .copied()
        .filter(|event| event.distance <= MAX_PAIR_DISTANCE)
        .collect();
    let mut first_seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut pairs = Vec::new();
    let mut delays_by_kind: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, event) in baseline_events.iter().enumerate() {
        let key = event.candidate * PHENOTYPES + event.phi as usize;
        if first_seen[key] {
            continue;
        }
        first_seen[key] = true;
        let future: Vec<&Event> = baseline_events
            .iter()
            .skip(index + 1)
            .filter(|candidate_event| {
                candidate_event.candidate == event.candidate && candidate_event.doc > event.doc
            })
            .collect();
        let same = future
            .iter()
            .copied()
            .find(|candidate_event| candidate_event.phi == event.phi);
        let Some(witness) = same.or_else(|| future.first().copied()) else {
            continue;
        };
        let context_verdict = verdict(CANDIDATES[event.candidate], event.phi, witness.phi);
        let delay = witness.doc - event.doc;
        let label = verdict_label(context_verdict);
        delays_by_kind.entry(label.clone()).or_default().push(delay);
        pairs.push(PairAudit {
            candidate: CANDIDATES[event.candidate].id,
            nomination_doc: event.doc,
            witness_doc: witness.doc,
            delay_docs: delay,
            phi_nomination: phi_label(event.phi),
            phi_witness: phi_label(witness.phi),
            nomination_vector: event.vector,
            witness_vector: witness.vector,
            raw_context_distance: event.vector.distance(witness.vector),
            verdict: context_verdict,
        });
    }
    let mut counts = BoundaryCounts {
        same_valid: 0,
        same_invalid: 0,
        different_valid: 0,
        different_invalid: 0,
        indeterminate: 0,
    };
    let mut distances: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for pair in &pairs {
        match pair.verdict {
            ContextVerdict::SameValid => counts.same_valid += 1,
            ContextVerdict::SameInvalid => counts.same_invalid += 1,
            ContextVerdict::DifferentValid => counts.different_valid += 1,
            ContextVerdict::DifferentInvalid => counts.different_invalid += 1,
            ContextVerdict::Indeterminate => counts.indeterminate += 1,
        }
        distances
            .entry(verdict_label(pair.verdict))
            .or_default()
            .push(pair.raw_context_distance);
    }
    let raw_distance_by_verdict = distances
        .into_iter()
        .map(|(key, values)| (key, summarize_distance(values)))
        .collect();
    let delay_by_witness_kind = delays_by_kind
        .into_iter()
        .map(|(key, values)| (key, summarize(values)))
        .collect();
    let reproduction = W1Baseline {
        nominations: pairs.len() as u32
            + w1.baseline.nominations.saturating_sub(pairs.len() as u32),
        later_candidate_witness: pairs.len() as u32,
        same_phenotype_witness: pairs
            .iter()
            .filter(|pair| pair.phi_nomination == pair.phi_witness)
            .count() as u32,
        phenotype_mismatch: pairs
            .iter()
            .filter(|pair| pair.phi_nomination != pair.phi_witness)
            .count() as u32,
        owned_witnesses: w1.baseline.owned_witnesses,
    };
    let pair_count = pairs.len();
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-P1 frozen W1 candidate-witness events; raw marker vectors and post-hoc boundary classification; no bridge or learner change",
        hypothesis: "the six phenotype mismatches are either true contextual conflicts or false splits from a brittle discrete phenotype boundary",
        source_corpus: corpus,
        source_sha256: source_hash,
        w1_receipt_sha256: w1_hash,
        document_count,
        event_count_at_max_distance_40: events.len(),
        baseline_reproduction: reproduction,
        boundary_counts: counts,
        raw_distance_by_verdict,
        delay_by_witness_kind,
        pairs,
        context_rule: "same fixed marker families as LA2-A/W1; priority geography, then finance, then transport, then general",
        context_label_limit: "FiQA has no sense/context annotations; expected phenotype is a frozen relation-bank diagnostic label, not a learned or corpus-provided truth label",
        conclusion: "PHENOTYPE_BOUNDARY_AUDIT_COMPLETE: no soft bridge is authorized until cross-boundary validity has independent evidence",
    };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "schema={SCHEMA} docs={} events40={} pairs={} same={} mismatch={} source_sha256={}",
        document_count,
        events.len(),
        pair_count,
        receipt.baseline_reproduction.same_phenotype_witness,
        receipt.baseline_reproduction.phenotype_mismatch,
        receipt.source_sha256
    );
    println!(
        "w1_nominations={} w1_later={} w1_same={} w1_mismatch={} exact_counts_match={}",
        w1.baseline.nominations,
        w1.baseline.later_candidate_witness,
        w1.baseline.same_phenotype_witness,
        w1.baseline.phenotype_mismatch,
        w1.baseline.later_candidate_witness
            == receipt.baseline_reproduction.later_candidate_witness
            && w1.baseline.same_phenotype_witness
                == receipt.baseline_reproduction.same_phenotype_witness
            && w1.baseline.phenotype_mismatch == receipt.baseline_reproduction.phenotype_mismatch
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vector_distance_is_symmetric_and_bounded() {
        let a = ContextVector {
            finance: 2,
            geography: 0,
            transport: 0,
        };
        let b = ContextVector {
            finance: 0,
            geography: 2,
            transport: 0,
        };
        assert_eq!(a.distance(b), b.distance(a));
        assert!(a.distance(b) <= 1.0);
    }
    #[test]
    fn expected_context_marks_same_valid() {
        assert!(matches!(
            verdict(CANDIDATES[2], 2, 2),
            ContextVerdict::SameValid
        ));
    }
    #[test]
    fn wrong_context_marks_different_invalid() {
        assert!(matches!(
            verdict(CANDIDATES[5], 0, 1),
            ContextVerdict::DifferentInvalid
        ));
    }
}
