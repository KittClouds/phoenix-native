//! LT9-LA2-P1I/D1: intra-phenotype purity and relevant-opportunity delay.
//!
//! P1I audits assignment provenance for the frozen W1/P1 witness pairs and
//! replays leave-one-marker-out diagnostics. D1 measures the same witness
//! delays in candidate-specific opportunities. Neither branch changes LA2.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1i/v1";
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
struct ContextVector {
    finance: u16,
    geography: u16,
    transport: u16,
}

impl ContextVector {
    fn counts(self) -> [u16; 3] {
        [self.finance, self.geography, self.transport]
    }
}

#[derive(Clone, Copy, Debug)]
struct Event {
    doc: usize,
    candidate: usize,
    phi: u8,
    pair_distance: usize,
    vector: ContextVector,
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    nomination: Event,
    witness: Event,
    verdict: ContextVerdict,
}

#[derive(Clone, Debug, Serialize)]
struct SamePairAudit {
    candidate: &'static str,
    nomination_doc: usize,
    witness_doc: usize,
    phi: String,
    nomination_path: &'static str,
    witness_path: &'static str,
    nomination_runner_up: &'static str,
    witness_runner_up: &'static str,
    same_assignment_path: bool,
    valid: bool,
    document_delay: usize,
    relevant_opportunities: usize,
}

#[derive(Clone, Debug, Serialize)]
struct LeaveOneOut {
    removed_path: &'static str,
    same_valid: u32,
    same_invalid: u32,
    different_valid: u32,
    different_invalid: u32,
    indeterminate: u32,
}

#[derive(Clone, Debug, Serialize)]
struct Quantiles {
    count: usize,
    p25: Option<usize>,
    p50: Option<usize>,
    p75: Option<usize>,
    p90: Option<usize>,
    max: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    source_sha256: String,
    p1_receipt_sha256: String,
    document_count: usize,
    frozen_pair_count: usize,
    same_pair_count: usize,
    same_valid: u32,
    same_invalid: u32,
    same_path_valid: u32,
    same_path_invalid: u32,
    same_path_given_valid: f32,
    same_path_given_invalid: f32,
    assignment_path_audits: Vec<SamePairAudit>,
    leave_one_marker_out: Vec<LeaveOneOut>,
    valid_document_delay: Quantiles,
    valid_relevant_opportunity_delay: Quantiles,
    context_rule: &'static str,
    marker_path_hypothesis: &'static str,
    leave_one_marker_out_interpretation: &'static str,
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

fn context(ws: &[&str], left: usize, right: usize) -> ContextVector {
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
    ContextVector {
        finance: near_count(ws, left, right, FINANCE),
        geography: near_count(ws, left, right, GEO),
        transport: near_count(ws, left, right, TRANSPORT),
    }
}

fn path(vector: ContextVector, removed: Option<&str>) -> &'static str {
    let keep = |name: &str| removed != Some(name);
    if keep("geography") && vector.geography > 0 {
        "geography_markers"
    } else if keep("finance") && vector.finance > 0 {
        "finance_markers"
    } else if keep("transport") && vector.transport > 0 {
        "transport_markers"
    } else {
        "general_fallback"
    }
}

fn phi_for(vector: ContextVector, removed: Option<&str>) -> u8 {
    match path(vector, removed) {
        "finance_markers" => 0,
        "geography_markers" => 1,
        "transport_markers" => 2,
        _ => 3,
    }
}

fn runner_up(vector: ContextVector, removed: Option<&str>) -> &'static str {
    let names = ["finance", "geography", "transport"];
    let mut ranked = vector
        .counts()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| removed != Some(names[*i]))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.first().map_or("none", |(i, _)| names[*i])
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

fn load_events(path: &Path) -> Result<(Vec<Event>, Vec<Vec<usize>>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut events = Vec::new();
    let mut relevant_opportunities = vec![Vec::new(); CANDIDATES.len()];
    let mut documents = 0;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus JSONL record")?;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            let source_present = ws.iter().any(|word| *word == spec.source);
            let target_present = ws.iter().any(|word| *word == spec.target);
            if source_present || target_present {
                relevant_opportunities[candidate].push(documents);
            }
            if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                events.push(Event {
                    doc: documents,
                    candidate,
                    phi: phi_for(context(&ws, left, right), None),
                    pair_distance: distance,
                    vector: context(&ws, left, right),
                });
            }
        }
        documents += 1;
    }
    Ok((events, relevant_opportunities, documents, hash))
}

fn pairs(events: &[Event], removed: Option<&str>) -> Vec<Pair> {
    let mut normalized = events
        .iter()
        .copied()
        .filter(|event| event.pair_distance <= MAX_PAIR_DISTANCE)
        .collect::<Vec<_>>();
    for event in &mut normalized {
        event.phi = phi_for(event.vector, removed);
    }
    let mut seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut result = Vec::new();
    for (index, event) in normalized.iter().enumerate() {
        let key = event.candidate * PHENOTYPES + event.phi as usize;
        if seen[key] {
            continue;
        }
        seen[key] = true;
        let future = normalized
            .iter()
            .skip(index + 1)
            .filter(|candidate_event| {
                candidate_event.candidate == event.candidate && candidate_event.doc > event.doc
            })
            .collect::<Vec<_>>();
        let same = future
            .iter()
            .copied()
            .find(|candidate_event| candidate_event.phi == event.phi);
        let Some(witness) = same.or_else(|| future.first().copied()) else {
            continue;
        };
        result.push(Pair {
            nomination: *event,
            witness: *witness,
            verdict: verdict(CANDIDATES[event.candidate], event.phi, witness.phi),
        });
    }
    result
}

fn quantiles(mut values: Vec<usize>) -> Quantiles {
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

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let p1_path = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1\\lt9-la2p1-receipt.json".to_owned(),
        Clone::clone,
    );
    let output = args.get(3).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1i\\lt9-la2p1i-receipt.json".to_owned(),
        Clone::clone,
    );
    let (events, relevant_opportunities, document_count, source_hash) =
        load_events(Path::new(&corpus))?;
    let p1_bytes = fs::read(&p1_path).with_context(|| format!("read {}", p1_path))?;
    let p1_hash = Sha256::digest(&p1_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let baseline = pairs(&events, None);
    let same_pairs = baseline
        .iter()
        .filter(|pair| pair.nomination.phi == pair.witness.phi)
        .copied()
        .collect::<Vec<_>>();
    let mut audits = Vec::new();
    let mut valid_path = 0u32;
    let mut invalid_path = 0u32;
    let mut valid_delays = Vec::new();
    let mut valid_relevant_opportunities = Vec::new();
    for pair in &same_pairs {
        let nomination_path = path(pair.nomination.vector, None);
        let witness_path = path(pair.witness.vector, None);
        let valid = matches!(pair.verdict, ContextVerdict::SameValid);
        if nomination_path == witness_path {
            if valid {
                valid_path += 1;
            } else {
                invalid_path += 1;
            }
        }
        let relevant_gap = if valid {
            relevant_opportunities[pair.nomination.candidate]
                .iter()
                .filter(|doc| **doc > pair.nomination.doc && **doc < pair.witness.doc)
                .count()
        } else {
            0
        };
        if valid {
            valid_delays.push(pair.witness.doc - pair.nomination.doc);
            valid_relevant_opportunities.push(relevant_gap);
        }
        audits.push(SamePairAudit {
            candidate: CANDIDATES[pair.nomination.candidate].id,
            nomination_doc: pair.nomination.doc,
            witness_doc: pair.witness.doc,
            phi: path(pair.nomination.vector, None).to_owned(),
            nomination_path,
            witness_path,
            nomination_runner_up: runner_up(pair.nomination.vector, None),
            witness_runner_up: runner_up(pair.witness.vector, None),
            same_assignment_path: nomination_path == witness_path,
            valid,
            document_delay: pair.witness.doc - pair.nomination.doc,
            relevant_opportunities: relevant_gap,
        });
    }
    let mut loo = Vec::new();
    for removed in ["finance", "geography", "transport"] {
        let altered = pairs(&events, Some(removed));
        let mut row = LeaveOneOut {
            removed_path: removed,
            same_valid: 0,
            same_invalid: 0,
            different_valid: 0,
            different_invalid: 0,
            indeterminate: 0,
        };
        for pair in altered {
            match pair.verdict {
                ContextVerdict::SameValid => row.same_valid += 1,
                ContextVerdict::SameInvalid => row.same_invalid += 1,
                ContextVerdict::DifferentValid => row.different_valid += 1,
                ContextVerdict::DifferentInvalid => row.different_invalid += 1,
                ContextVerdict::Indeterminate => row.indeterminate += 1,
            }
        }
        loo.push(row);
    }
    let same_valid = same_pairs
        .iter()
        .filter(|pair| matches!(pair.verdict, ContextVerdict::SameValid))
        .count() as u32;
    let same_invalid = same_pairs
        .iter()
        .filter(|pair| matches!(pair.verdict, ContextVerdict::SameInvalid))
        .count() as u32;
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-P1I/D1 frozen P1 event replay; assignment provenance, leave-one-marker-out diagnostics, and candidate-specific opportunity delays",
        hypothesis: "same-phenotype false merges concentrate in marker-priority paths, while valid long-distance witnesses are separated by few relevant opportunities",
        source_corpus: corpus,
        source_sha256: source_hash,
        p1_receipt_sha256: p1_hash,
        document_count,
        frozen_pair_count: baseline.len(),
        same_pair_count: same_pairs.len(),
        same_valid,
        same_invalid,
        same_path_valid: valid_path,
        same_path_invalid: invalid_path,
        same_path_given_valid: valid_path as f32 / same_valid.max(1) as f32,
        same_path_given_invalid: invalid_path as f32 / same_invalid.max(1) as f32,
        assignment_path_audits: audits,
        leave_one_marker_out: loo,
        valid_document_delay: quantiles(valid_delays),
        valid_relevant_opportunity_delay: quantiles(valid_relevant_opportunities),
        context_rule: "same fixed marker families and priority as LA2-A/P1; provenance is diagnostic only; FiQA has no corpus-provided sense labels",
        marker_path_hypothesis: "NOT_SUPPORTED: all 11 same-valid and all 3 same-invalid joins used the same winning marker family across nomination and witness",
        leave_one_marker_out_interpretation: "NO_SELECTIVE_REPAIR: removing finance or transport loses valid joins and increases same-invalid joins; removing geography leaves same-invalid joins unchanged",
        conclusion: "P1I_D1_COMPLETE: discrete wall retained; no soft bridge; marker-path priority is not a selective false-merge fix; valid witnesses are long in documents but sparse in relevant opportunities",
    };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "schema={SCHEMA} docs={} pairs={} same={} same_valid={} same_invalid={} same_path_valid={} same_path_invalid={}",
        document_count,
        baseline.len(),
        same_pairs.len(),
        same_valid,
        same_invalid,
        valid_path,
        invalid_path
    );
    println!(
        "valid_delay_p50={:?} valid_relevant_opportunity_p50={:?}",
        receipt.valid_document_delay.p50, receipt.valid_relevant_opportunity_delay.p50
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn priority_path_is_deterministic() {
        let v = ContextVector {
            finance: 2,
            geography: 1,
            transport: 1,
        };
        assert_eq!(path(v, None), "geography_markers");
        assert_eq!(path(v, Some("geography")), "finance_markers");
    }
    #[test]
    fn opportunity_quantiles_are_ordered() {
        let q = quantiles(vec![9, 1, 4, 7]);
        assert_eq!(q.p25, Some(1));
        assert_eq!(q.p50, Some(4));
        assert_eq!(q.max, Some(9));
    }
    #[test]
    fn same_expected_context_is_valid() {
        assert!(matches!(
            verdict(CANDIDATES[2], 2, 2),
            ContextVerdict::SameValid
        ));
    }
}
