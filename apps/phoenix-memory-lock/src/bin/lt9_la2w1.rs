//! LT9-LA2-W1: natural witness-funnel audit.
//!
//! This instrument does not change LA2-A's learning rule.  It replays the
//! same FiQA JSONL stream, records one exclusive terminal reason per frozen
//! nomination, and evaluates single-gate diagnostic relaxations.  Relation
//! classes are consulted only after the audit for descriptive concentration
//! and false-authorization receipts.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2w1/v1";
const LAMBDA: f32 = 0.85;
const BASE_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
const PHENOTYPES: usize = 4;

#[derive(Clone, Copy, Debug, Serialize)]
enum Gold {
    Support,
    DirectionalWeak,
    Negative,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Candidate {
    id: &'static str,
    source: &'static str,
    target: &'static str,
    gold: Gold,
}

const CANDIDATES: [Candidate; 12] = [
    Candidate {
        id: "repair_to_fix",
        source: "repair",
        target: "fix",
        gold: Gold::Support,
    },
    Candidate {
        id: "engine_to_motor",
        source: "engine",
        target: "motor",
        gold: Gold::Support,
    },
    Candidate {
        id: "car_to_vehicle",
        source: "car",
        target: "vehicle",
        gold: Gold::Support,
    },
    Candidate {
        id: "vehicle_to_car",
        source: "vehicle",
        target: "car",
        gold: Gold::DirectionalWeak,
    },
    Candidate {
        id: "bank_to_shore",
        source: "bank",
        target: "shore",
        gold: Gold::Support,
    },
    Candidate {
        id: "bank_to_lender",
        source: "bank",
        target: "lender",
        gold: Gold::Support,
    },
    Candidate {
        id: "economic_to_tumor",
        source: "economic",
        target: "tumor",
        gold: Gold::Negative,
    },
    Candidate {
        id: "loan_to_debt",
        source: "loan",
        target: "debt",
        gold: Gold::Support,
    },
    Candidate {
        id: "credit_to_loan",
        source: "credit",
        target: "loan",
        gold: Gold::Support,
    },
    Candidate {
        id: "insurance_to_coverage",
        source: "insurance",
        target: "coverage",
        gold: Gold::Support,
    },
    Candidate {
        id: "stock_to_bond",
        source: "stock",
        target: "bond",
        gold: Gold::DirectionalWeak,
    },
    Candidate {
        id: "bank_to_water",
        source: "bank",
        target: "water",
        gold: Gold::Support,
    },
];

#[derive(Clone, Debug, Deserialize)]
struct CorpusDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    doc: usize,
    candidate: usize,
    phi: u8,
    distance: usize,
    negative: bool,
    support: bool,
}

#[derive(Clone, Copy, Debug)]
struct Gate {
    name: &'static str,
    max_distance: usize,
    any_phenotype: bool,
    min_trace: f32,
    horizon: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
struct CandidateFunnel {
    candidate: &'static str,
    nominations: u32,
    owned_support: u32,
    owned_contradiction: u32,
    terminal_share: f32,
}

#[derive(Clone, Debug, Serialize)]
struct GateReceipt {
    name: &'static str,
    max_distance: usize,
    phenotype_rule: &'static str,
    min_trace: f32,
    horizon_docs: Option<usize>,
    observed_pairs: u32,
    nominations: u32,
    eligibility_opened: u32,
    later_candidate_witness: u32,
    same_phenotype_witness: u32,
    phenotype_mismatch: u32,
    abstain_witness: u32,
    eligibility_decay: u32,
    owned_witnesses: u32,
    support_owned: u32,
    contradiction_owned: u32,
    no_later_witness: u32,
    median_witness_delay_docs: Option<usize>,
    terminal_reasons: BTreeMap<String, u32>,
    false_authorizations: u32,
    correct_authorizations: u32,
    top1_update_share: f32,
    top3_update_share: f32,
    top5_update_share: f32,
    candidate_funnel: Vec<CandidateFunnel>,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    source_sha256: String,
    document_count: usize,
    corpus_has_timestamps: bool,
    chronology_claim: &'static str,
    visibility_audit: &'static str,
    baseline: GateReceipt,
    counterfactuals: Vec<GateReceipt>,
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

fn nearest(
    ws: &[&str],
    source: &str,
    target: &str,
    max_distance: usize,
) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for left in positions(ws, source) {
        for right in positions(ws, target) {
            let distance = left.abs_diff(right);
            if distance <= max_distance
                && best.map_or(true, |old: (usize, usize, usize)| distance < old.2)
            {
                best = Some((left, right, distance));
            }
        }
    }
    best
}

fn near_any(ws: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(ws.len());
    ws[start..end]
        .iter()
        .any(|w| markers.iter().any(|m| w == m))
}

fn phenotype(ws: &[&str], left: usize, right: usize) -> u8 {
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
    if near_any(ws, left, right, GEO) {
        1
    } else if near_any(ws, left, right, FINANCE) {
        0
    } else if near_any(ws, left, right, TRANSPORT) {
        2
    } else {
        3
    }
}

fn extract(path: &Path) -> Result<(Vec<Event>, usize, String)> {
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
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus record")?;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target, 40) {
                events.push(Event {
                    doc: documents,
                    candidate,
                    phi: phenotype(&ws, left, right),
                    distance,
                    negative: near_any(
                        &ws,
                        left,
                        right,
                        &[
                            "not",
                            "never",
                            "unlike",
                            "different",
                            "rather",
                            "instead",
                            "versus",
                            "vs",
                            "without",
                        ],
                    ),
                    support: near_any(
                        &ws,
                        left,
                        right,
                        &[
                            "also",
                            "called",
                            "known",
                            "means",
                            "aka",
                            "similar",
                            "equivalent",
                            "same",
                            "like",
                        ],
                    ),
                });
            }
        }
        documents += 1;
    }
    Ok((events, documents, hash))
}

fn median(values: &mut [usize]) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

fn gate(events: &[Event], gate: Gate) -> GateReceipt {
    let eligible_events: Vec<Event> = events
        .iter()
        .copied()
        .filter(|event| event.distance <= gate.max_distance)
        .collect();
    let mut first_seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut a_plus = vec![0.0f32; CANDIDATES.len() * PHENOTYPES];
    let mut a_minus = vec![0.0f32; CANDIDATES.len() * PHENOTYPES];
    let mut nominations = vec![0u32; CANDIDATES.len()];
    let mut support_owned = vec![0u32; CANDIDATES.len()];
    let mut contradiction_owned = vec![0u32; CANDIDATES.len()];
    let mut terminal = BTreeMap::<String, u32>::new();
    let mut delays = Vec::new();
    let mut observed = 0u32;
    let mut later_candidate = 0u32;
    let mut same_phi = 0u32;
    let mut mismatch = 0u32;
    let mut abstain = 0u32;
    let mut decay_count = 0u32;
    let mut owned = 0u32;
    for event in &eligible_events {
        observed += 1;
        if first_seen[event.candidate * PHENOTYPES + event.phi as usize] {
            continue;
        }
        first_seen[event.candidate * PHENOTYPES + event.phi as usize] = true;
        nominations[event.candidate] += 1;
    }
    for event in eligible_events
        .iter()
        .filter(|event| first_seen[event.candidate * PHENOTYPES + event.phi as usize])
    {
        let key = event.candidate * PHENOTYPES + event.phi as usize;
        // Only the first occurrence for each candidate/phenotype is a nomination.
        let first_index = eligible_events
            .iter()
            .position(|candidate_event| {
                candidate_event.doc == event.doc
                    && candidate_event.candidate == event.candidate
                    && candidate_event.phi == event.phi
            })
            .unwrap_or(usize::MAX);
        if eligible_events
            .iter()
            .take(first_index)
            .any(|candidate_event| {
                candidate_event.candidate == event.candidate && candidate_event.phi == event.phi
            })
        {
            continue;
        }
        let future = eligible_events
            .iter()
            .filter(|candidate_event| {
                candidate_event.candidate == event.candidate
                    && candidate_event.doc > event.doc
                    && gate
                        .horizon
                        .is_none_or(|h| candidate_event.doc - event.doc <= h)
            })
            .collect::<Vec<_>>();
        let same = future
            .iter()
            .copied()
            .find(|candidate_event| candidate_event.phi == event.phi);
        if future.is_empty() {
            *terminal.entry("NO_LATER_WITNESS".to_owned()).or_default() += 1;
            continue;
        }
        later_candidate += 1;
        if same.is_none() && !gate.any_phenotype {
            mismatch += 1;
            *terminal.entry("PHENOTYPE_MISMATCH".to_owned()).or_default() += 1;
            continue;
        }
        let witness = same.or_else(|| future.first().copied()).unwrap();
        if same.is_some() {
            same_phi += 1;
        }
        let kind = if witness.negative {
            "CONTRADICTION"
        } else if witness.support || witness.distance <= SUPPORT_DISTANCE {
            "SUPPORT"
        } else {
            "ABSTAIN"
        };
        if kind == "ABSTAIN" {
            abstain += 1;
            *terminal.entry("WITNESS_ABSTAIN".to_owned()).or_default() += 1;
            continue;
        }
        let relevant_gap = eligible_events
            .iter()
            .filter(|candidate_event| {
                candidate_event.candidate == event.candidate
                    && candidate_event.phi == event.phi
                    && candidate_event.doc > event.doc
                    && candidate_event.doc < witness.doc
            })
            .count();
        let authority = LAMBDA.powi(relevant_gap as i32);
        if authority < gate.min_trace {
            decay_count += 1;
            *terminal.entry("ELIGIBILITY_DECAY".to_owned()).or_default() += 1;
            continue;
        }
        delays.push(witness.doc - event.doc);
        owned += 1;
        if kind == "SUPPORT" {
            support_owned[event.candidate] += 1;
            a_plus[key] += authority;
        } else {
            contradiction_owned[event.candidate] += 1;
            a_minus[key] += authority;
        }
        *terminal.entry(format!("OWNED_{kind}")).or_default() += 1;
    }
    let mut false_auth = 0u32;
    let mut correct_auth = 0u32;
    for (candidate, spec) in CANDIDATES.iter().enumerate() {
        let plus = (0..PHENOTYPES)
            .map(|phi| a_plus[candidate * PHENOTYPES + phi])
            .sum::<f32>();
        let minus = (0..PHENOTYPES)
            .map(|phi| a_minus[candidate * PHENOTYPES + phi])
            .sum::<f32>();
        let authorized = plus > minus + 0.05;
        match spec.gold {
            Gold::Support if authorized => correct_auth += 1,
            Gold::DirectionalWeak | Gold::Negative if authorized => false_auth += 1,
            _ => {}
        }
    }
    let total_updates = owned.max(1) as f32;
    let mut update_counts = support_owned
        .iter()
        .zip(&contradiction_owned)
        .map(|(a, b)| a + b)
        .collect::<Vec<_>>();
    update_counts.sort_unstable_by(|a, b| b.cmp(a));
    let share = |n: usize| update_counts.iter().take(n).sum::<u32>() as f32 / total_updates;
    let candidate_funnel = CANDIDATES
        .iter()
        .enumerate()
        .map(|(i, spec)| CandidateFunnel {
            candidate: spec.id,
            nominations: nominations[i],
            owned_support: support_owned[i],
            owned_contradiction: contradiction_owned[i],
            terminal_share: (support_owned[i] + contradiction_owned[i]) as f32 / total_updates,
        })
        .collect();
    GateReceipt {
        name: gate.name,
        max_distance: gate.max_distance,
        phenotype_rule: if gate.any_phenotype {
            "candidate-compatible phenotype"
        } else {
            "same phenotype"
        },
        min_trace: gate.min_trace,
        horizon_docs: gate.horizon,
        observed_pairs: observed,
        nominations: nominations.iter().sum(),
        eligibility_opened: nominations.iter().sum(),
        later_candidate_witness: later_candidate,
        same_phenotype_witness: same_phi,
        phenotype_mismatch: mismatch,
        abstain_witness: abstain,
        eligibility_decay: decay_count,
        owned_witnesses: owned,
        support_owned: support_owned.iter().sum(),
        contradiction_owned: contradiction_owned.iter().sum(),
        no_later_witness: *terminal.get("NO_LATER_WITNESS").unwrap_or(&0),
        median_witness_delay_docs: median(&mut delays),
        terminal_reasons: terminal,
        false_authorizations: false_auth,
        correct_authorizations: correct_auth,
        top1_update_share: share(1),
        top3_update_share: share(3),
        top5_update_share: share(5),
        candidate_funnel,
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let output = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2w1\\lt9-la2w1-receipt.json".to_owned(),
        Clone::clone,
    );
    let (events, document_count, source_hash) = extract(Path::new(&corpus))?;
    let baseline = gate(
        &events,
        Gate {
            name: "baseline",
            max_distance: BASE_DISTANCE,
            any_phenotype: false,
            min_trace: 0.05,
            horizon: None,
        },
    );
    let counterfactuals = vec![
        gate(
            &events,
            Gate {
                name: "nomination_distance_relaxed_40",
                max_distance: 40,
                any_phenotype: false,
                min_trace: 0.05,
                horizon: None,
            },
        ),
        gate(
            &events,
            Gate {
                name: "phenotype_compatibility_relaxed",
                max_distance: BASE_DISTANCE,
                any_phenotype: true,
                min_trace: 0.05,
                horizon: None,
            },
        ),
        gate(
            &events,
            Gate {
                name: "support_mass_threshold_relaxed_0.01",
                max_distance: BASE_DISTANCE,
                any_phenotype: false,
                min_trace: 0.01,
                horizon: None,
            },
        ),
        gate(
            &events,
            Gate {
                name: "witness_horizon_512_docs_audit",
                max_distance: BASE_DISTANCE,
                any_phenotype: false,
                min_trace: 0.05,
                horizon: Some(512),
            },
        ),
    ];
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-W1 exclusive natural witness funnel; exact LA2-A frozen markers and dual-trace constants; one gate relaxed per diagnostic arm",
        hypothesis: "natural witness sparsity is attributable to identifiable nomination, phenotype, delay, or eligibility gates",
        source_corpus: corpus.clone(),
        source_sha256: source_hash.clone(),
        document_count,
        corpus_has_timestamps: false,
        chronology_claim: "FiQA records have empty metadata; this instrument supports sequence-order claims, not real-time chronology claims",
        visibility_audit: "events are emitted in JSONL line order; future lines are never consulted during event extraction; witness searches are post hoc diagnostics only",
        baseline,
        counterfactuals,
        conclusion: "NATURAL_FUNNEL_AUDIT_COMPLETE: use yield/safety/concentration to decide whether LA2-B is authorized",
    };
    let path = Path::new(&output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "schema={SCHEMA} docs={} events={} source_sha256={source_hash}",
        document_count,
        events.len()
    );
    println!(
        "baseline nominations={} owned={} no_later={} mismatch={} decay={} abstain={} top1={:.3} false_auth={}",
        receipt.baseline.nominations,
        receipt.baseline.owned_witnesses,
        receipt.baseline.no_later_witness,
        receipt.baseline.phenotype_mismatch,
        receipt.baseline.eligibility_decay,
        receipt.baseline.abstain_witness,
        receipt.baseline.top1_update_share,
        receipt.baseline.false_authorizations
    );
    for arm in &receipt.counterfactuals {
        println!(
            "arm={} nominations={} owned={} false_auth={} top1={:.3}",
            arm.name,
            arm.nominations,
            arm.owned_witnesses,
            arm.false_authorizations,
            arm.top1_update_share
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_has_exclusive_nomination_and_witness_reasons() {
        let events = vec![
            Event {
                doc: 0,
                candidate: 2,
                phi: 2,
                distance: 4,
                negative: false,
                support: false,
            },
            Event {
                doc: 5,
                candidate: 2,
                phi: 2,
                distance: 4,
                negative: false,
                support: true,
            },
        ];
        let result = gate(
            &events,
            Gate {
                name: "test",
                max_distance: 24,
                any_phenotype: false,
                min_trace: 0.05,
                horizon: None,
            },
        );
        assert_eq!(result.nominations, 1);
        assert_eq!(result.owned_witnesses, 1);
        assert_eq!(result.support_owned, 1);
    }
    #[test]
    fn phenotype_mismatch_is_distinct_from_no_witness() {
        let events = vec![
            Event {
                doc: 0,
                candidate: 4,
                phi: 1,
                distance: 4,
                negative: false,
                support: false,
            },
            Event {
                doc: 2,
                candidate: 4,
                phi: 0,
                distance: 4,
                negative: false,
                support: true,
            },
        ];
        let result = gate(
            &events,
            Gate {
                name: "test",
                max_distance: 24,
                any_phenotype: false,
                min_trace: 0.05,
                horizon: None,
            },
        );
        assert_eq!(result.phenotype_mismatch, 1);
        assert_eq!(result.owned_witnesses, 0);
    }
    #[test]
    fn horizon_is_a_single_gate() {
        let events = vec![
            Event {
                doc: 0,
                candidate: 0,
                phi: 0,
                distance: 4,
                negative: false,
                support: false,
            },
            Event {
                doc: 100,
                candidate: 0,
                phi: 0,
                distance: 4,
                negative: false,
                support: true,
            },
        ];
        let result = gate(
            &events,
            Gate {
                name: "test",
                max_distance: 24,
                any_phenotype: false,
                min_trace: 0.05,
                horizon: Some(10),
            },
        );
        assert_eq!(result.no_later_witness, 1);
    }
}
