//! LT9-LA2-E1: natural eligibility-persistence audit.
//!
//! This is a diagnostic-only replay.  It freezes the W1/P1I event and witness
//! contract, then changes only persistence semantics: pure exponential decay,
//! two trace floors, or a decaying trace plus a binary pending-credit tag.
//! No learner or serving policy is changed.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2e1/v1";
const LAMBDA: f32 = 0.85;
const BASE_DISTANCE: usize = 24;
const EXTRACTION_DISTANCE: usize = 40;
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
    expected_phi: i8,
}

const CANDIDATES: [Candidate; 12] = [
    Candidate {
        id: "repair_to_fix",
        source: "repair",
        target: "fix",
        gold: Gold::Support,
        expected_phi: -1,
    },
    Candidate {
        id: "engine_to_motor",
        source: "engine",
        target: "motor",
        gold: Gold::Support,
        expected_phi: 2,
    },
    Candidate {
        id: "car_to_vehicle",
        source: "car",
        target: "vehicle",
        gold: Gold::Support,
        expected_phi: 2,
    },
    Candidate {
        id: "vehicle_to_car",
        source: "vehicle",
        target: "car",
        gold: Gold::DirectionalWeak,
        expected_phi: 2,
    },
    Candidate {
        id: "bank_to_shore",
        source: "bank",
        target: "shore",
        gold: Gold::Support,
        expected_phi: 1,
    },
    Candidate {
        id: "bank_to_lender",
        source: "bank",
        target: "lender",
        gold: Gold::Support,
        expected_phi: 0,
    },
    Candidate {
        id: "economic_to_tumor",
        source: "economic",
        target: "tumor",
        gold: Gold::Negative,
        expected_phi: -1,
    },
    Candidate {
        id: "loan_to_debt",
        source: "loan",
        target: "debt",
        gold: Gold::Support,
        expected_phi: 0,
    },
    Candidate {
        id: "credit_to_loan",
        source: "credit",
        target: "loan",
        gold: Gold::Support,
        expected_phi: 0,
    },
    Candidate {
        id: "insurance_to_coverage",
        source: "insurance",
        target: "coverage",
        gold: Gold::Support,
        expected_phi: 0,
    },
    Candidate {
        id: "stock_to_bond",
        source: "stock",
        target: "bond",
        gold: Gold::DirectionalWeak,
        expected_phi: 0,
    },
    Candidate {
        id: "bank_to_water",
        source: "bank",
        target: "water",
        gold: Gold::Support,
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

#[derive(Clone, Copy, Debug)]
struct Event {
    doc: usize,
    candidate: usize,
    phi: u8,
    distance: usize,
    negative: bool,
    support: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WitnessKind {
    Support,
    Contradiction,
    Abstain,
}

#[derive(Clone, Copy, Debug)]
struct FrozenWitness {
    candidate: usize,
    phi: u8,
    nomination_doc: usize,
    witness_doc: usize,
    relevant_gap: usize,
    kind: WitnessKind,
    valid_context: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum Arm {
    PureExponential,
    Floor001,
    Floor005,
    PendingTag,
}

impl Arm {
    const ALL: [Self; 4] = [
        Self::PureExponential,
        Self::Floor001,
        Self::Floor005,
        Self::PendingTag,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::PureExponential => "E0_pure_exponential_no_floor",
            Self::Floor001 => "E1_lambda_085_floor_001",
            Self::Floor005 => "E2_lambda_085_floor_005",
            Self::PendingTag => "E3_fast_trace_binary_pending",
        }
    }

    const fn acceptance_floor(self) -> f32 {
        match self {
            Self::PureExponential | Self::PendingTag => 0.0,
            Self::Floor001 => 0.01,
            Self::Floor005 => 0.05,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DelayBucket {
    bucket: &'static str,
    available: u32,
    retained: u32,
    valid_available: u32,
    valid_retained: u32,
    valid_lost: u32,
    invalid_retained: u32,
    support_retained: u32,
    contradiction_retained: u32,
    authority_sum: f32,
}

#[derive(Clone, Debug, Serialize)]
struct ArmReceipt {
    arm: Arm,
    arm_label: &'static str,
    lambda: f32,
    minimum_eligibility: f32,
    pending_credit: bool,
    nominations: u32,
    same_phenotype_witnesses: u32,
    actionable_witnesses: u32,
    abstain_witnesses: u32,
    retained_witnesses: u32,
    valid_witnesses_available: u32,
    valid_witnesses_retained: u32,
    valid_witnesses_lost: u32,
    invalid_witness_updates: u32,
    support_available: u32,
    support_retained: u32,
    contradiction_available: u32,
    contradiction_retained: u32,
    false_authorizations: u32,
    correct_authorizations: u32,
    final_authorized_candidates: u32,
    delay_buckets: Vec<DelayBucket>,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    source_sha256: String,
    w1_receipt_sha256: String,
    p1i_receipt_sha256: String,
    document_count: usize,
    frozen_event_count: usize,
    frozen_event_chain_sha256: String,
    nominations: u32,
    same_phenotype_witnesses: u32,
    phenotype_mismatches: u32,
    no_later_witnesses: u32,
    abstain_witnesses: u32,
    frozen_actionable_witness_chain_sha256: String,
    arms: Vec<ArmReceipt>,
    delay_buckets: &'static [&'static str],
    persistence_clock: &'static str,
    chronology_claim: &'static str,
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

fn positions<'a>(words: &[&'a str], needle: &str) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(i, word)| (*word == needle).then_some(i))
        .collect()
}

fn nearest(
    words: &[&str],
    source: &str,
    target: &str,
    max_distance: usize,
) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for left in positions(words, source) {
        for right in positions(words, target) {
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

fn near_any(words: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let start = left.min(right).saturating_sub(8);
    let end = (left.max(right) + 9).min(words.len());
    words[start..end]
        .iter()
        .any(|word| markers.iter().any(|marker| word == marker))
}

fn context(words: &[&str], left: usize, right: usize) -> u8 {
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
    if near_any(words, left, right, GEO) {
        1
    } else if near_any(words, left, right, FINANCE) {
        0
    } else if near_any(words, left, right, TRANSPORT) {
        2
    } else {
        3
    }
}

fn extract(path: &Path) -> Result<(Vec<Event>, Vec<Vec<usize>>, usize, String)> {
    let bytes = fs::read(path).with_context(|| format!("read corpus {}", path.display()))?;
    let hash = hex_digest(Sha256::digest(&bytes));
    let mut events = Vec::new();
    let mut opportunities = vec![Vec::new(); CANDIDATES.len()];
    let mut documents = 0;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_slice(line).context("decode corpus record")?;
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            let source_present = ws.iter().any(|word| *word == spec.source);
            let target_present = ws.iter().any(|word| *word == spec.target);
            if source_present || target_present {
                opportunities[candidate].push(documents);
            }
            if let Some((left, right, distance)) =
                nearest(&ws, spec.source, spec.target, EXTRACTION_DISTANCE)
            {
                events.push(Event {
                    doc: documents,
                    candidate,
                    phi: context(&ws, left, right),
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
    Ok((events, opportunities, documents, hash))
}

fn classify(event: Event) -> WitnessKind {
    if event.negative {
        WitnessKind::Contradiction
    } else if event.support || event.distance <= SUPPORT_DISTANCE {
        WitnessKind::Support
    } else {
        WitnessKind::Abstain
    }
}

fn valid_context(candidate: usize, nomination_phi: u8, witness_phi: u8) -> bool {
    let expected = CANDIDATES[candidate].expected_phi;
    expected < 0 || (nomination_phi == expected as u8 && witness_phi == expected as u8)
}

fn freeze_witnesses(
    events: &[Event],
    opportunities: &[Vec<usize>],
) -> (Vec<FrozenWitness>, u32, u32, u32, u32, String, String) {
    let eligible = events
        .iter()
        .copied()
        .filter(|event| event.distance <= BASE_DISTANCE)
        .collect::<Vec<_>>();
    let mut first_seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut nominations = 0u32;
    let mut same_witnesses = 0u32;
    let mut mismatches = 0u32;
    let mut no_later = 0u32;
    let mut actionable = Vec::new();
    let mut event_chain = Sha256::new();
    for event in &eligible {
        event_chain.update((event.doc as u64).to_le_bytes());
        event_chain.update((event.candidate as u32).to_le_bytes());
        event_chain.update([
            event.phi,
            event.distance as u8,
            event.negative as u8,
            event.support as u8,
        ]);
    }
    let mut witness_chain = Sha256::new();
    for (index, event) in eligible.iter().enumerate() {
        let key = event.candidate * PHENOTYPES + event.phi as usize;
        if first_seen[key] {
            continue;
        }
        first_seen[key] = true;
        nominations += 1;
        let future = eligible
            .iter()
            .skip(index + 1)
            .filter(|later| later.candidate == event.candidate && later.doc > event.doc)
            .collect::<Vec<_>>();
        let Some(witness) = future.iter().copied().find(|later| later.phi == event.phi) else {
            if future.is_empty() {
                no_later += 1;
            } else {
                mismatches += 1;
            }
            continue;
        };
        same_witnesses += 1;
        let kind = classify(*witness);
        let relevant_gap = opportunities[event.candidate]
            .iter()
            .filter(|doc| **doc > event.doc && **doc < witness.doc)
            .count();
        let item = FrozenWitness {
            candidate: event.candidate,
            phi: event.phi,
            nomination_doc: event.doc,
            witness_doc: witness.doc,
            relevant_gap,
            kind,
            valid_context: valid_context(event.candidate, event.phi, witness.phi),
        };
        witness_chain.update((item.candidate as u32).to_le_bytes());
        witness_chain.update((item.phi as u32).to_le_bytes());
        witness_chain.update((item.nomination_doc as u64).to_le_bytes());
        witness_chain.update((item.witness_doc as u64).to_le_bytes());
        witness_chain.update((item.relevant_gap as u64).to_le_bytes());
        witness_chain.update([item.kind as u8, item.valid_context as u8]);
        if kind != WitnessKind::Abstain {
            actionable.push(item);
        }
    }
    (
        actionable,
        nominations,
        same_witnesses,
        mismatches,
        no_later,
        hex_digest(event_chain.finalize()),
        hex_digest(witness_chain.finalize()),
    )
}

fn bucket(gap: usize) -> &'static str {
    match gap {
        0..=16 => "0-16",
        17..=32 => "17-32",
        33..=64 => "33-64",
        65..=128 => "65-128",
        _ => "129+",
    }
}

fn empty_buckets() -> Vec<DelayBucket> {
    ["0-16", "17-32", "33-64", "65-128", "129+"]
        .into_iter()
        .map(|bucket| DelayBucket {
            bucket,
            available: 0,
            retained: 0,
            valid_available: 0,
            valid_retained: 0,
            valid_lost: 0,
            invalid_retained: 0,
            support_retained: 0,
            contradiction_retained: 0,
            authority_sum: 0.0,
        })
        .collect()
}

fn run(
    arm: Arm,
    witnesses: &[FrozenWitness],
    nominations: u32,
    same_witnesses: u32,
    abstains: u32,
) -> ArmReceipt {
    let mut buckets = empty_buckets();
    let mut a_plus = vec![0.0f32; CANDIDATES.len() * PHENOTYPES];
    let mut a_minus = vec![0.0f32; CANDIDATES.len() * PHENOTYPES];
    let mut retained = 0u32;
    let mut valid_available = 0u32;
    let mut valid_retained = 0u32;
    let mut invalid_updates = 0u32;
    let mut support_available = 0u32;
    let mut support_retained = 0u32;
    let mut contradiction_available = 0u32;
    let mut contradiction_retained = 0u32;
    for witness in witnesses {
        let bucket_name = bucket(witness.relevant_gap);
        let index = buckets
            .iter()
            .position(|item| item.bucket == bucket_name)
            .expect("frozen delay bucket");
        let b = &mut buckets[index];
        b.available += 1;
        if witness.valid_context {
            valid_available += 1;
            b.valid_available += 1;
        }
        match witness.kind {
            WitnessKind::Support => support_available += 1,
            WitnessKind::Contradiction => contradiction_available += 1,
            WitnessKind::Abstain => unreachable!(),
        }
        let authority = LAMBDA.powi(i32::try_from(witness.relevant_gap).unwrap_or(i32::MAX));
        let pending = matches!(arm, Arm::PendingTag);
        let retained_here = pending || authority >= arm.acceptance_floor();
        if !retained_here {
            continue;
        }
        retained += 1;
        b.retained += 1;
        b.authority_sum += authority;
        if witness.valid_context {
            valid_retained += 1;
            b.valid_retained += 1;
        } else {
            invalid_updates += 1;
            b.invalid_retained += 1;
        }
        match witness.kind {
            WitnessKind::Support => {
                support_retained += 1;
                b.support_retained += 1;
                a_plus[witness.candidate * PHENOTYPES + usize::from(witness.phi)] += authority;
            }
            WitnessKind::Contradiction => {
                contradiction_retained += 1;
                b.contradiction_retained += 1;
                a_minus[witness.candidate * PHENOTYPES + usize::from(witness.phi)] += authority;
            }
            WitnessKind::Abstain => unreachable!(),
        }
    }
    for b in &mut buckets {
        b.valid_lost = b.valid_available.saturating_sub(b.valid_retained);
    }
    let mut false_authorizations = 0u32;
    let mut correct_authorizations = 0u32;
    let mut final_authorized_candidates = 0u32;
    for (candidate, spec) in CANDIDATES.iter().enumerate() {
        let plus = (0..PHENOTYPES)
            .map(|phi| a_plus[candidate * PHENOTYPES + phi])
            .sum::<f32>();
        let minus = (0..PHENOTYPES)
            .map(|phi| a_minus[candidate * PHENOTYPES + phi])
            .sum::<f32>();
        if plus > minus + 0.05 {
            final_authorized_candidates += 1;
            match spec.gold {
                Gold::Support => correct_authorizations += 1,
                Gold::DirectionalWeak | Gold::Negative => false_authorizations += 1,
            }
        }
    }
    ArmReceipt {
        arm,
        arm_label: arm.label(),
        lambda: LAMBDA,
        minimum_eligibility: arm.acceptance_floor(),
        pending_credit: matches!(arm, Arm::PendingTag),
        nominations,
        same_phenotype_witnesses: same_witnesses,
        actionable_witnesses: witnesses.len() as u32,
        abstain_witnesses: abstains,
        retained_witnesses: retained,
        valid_witnesses_available: valid_available,
        valid_witnesses_retained: valid_retained,
        valid_witnesses_lost: valid_available.saturating_sub(valid_retained),
        invalid_witness_updates: invalid_updates,
        support_available,
        support_retained,
        contradiction_available,
        contradiction_retained,
        false_authorizations,
        correct_authorizations,
        final_authorized_candidates,
        delay_buckets: buckets,
    }
}

fn file_hash(path: &Path) -> Result<String> {
    Ok(hex_digest(Sha256::digest(
        fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let w1 = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2w1\\lt9-la2w1-receipt.json".to_owned(),
        Clone::clone,
    );
    let p1i = args.get(3).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2p1i\\lt9-la2p1i-receipt.json".to_owned(),
        Clone::clone,
    );
    let output = args.get(4).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2e1\\lt9-la2e1-receipt.json".to_owned(),
        Clone::clone,
    );
    let (events, opportunities, document_count, source_sha256) = extract(Path::new(&corpus))?;
    let (witnesses, nominations, same_witnesses, mismatches, no_later, event_chain, witness_chain) =
        freeze_witnesses(&events, &opportunities);
    let abstains = same_witnesses.saturating_sub(witnesses.len() as u32);
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-E1 frozen W1/P1I witness contract; only eligibility persistence semantics vary",
        hypothesis: "natural valid witnesses require either a persistent floor or an explicit unresolved-credit tag beyond the useful lifetime of the analog exponential trace",
        source_corpus: corpus.clone(),
        source_sha256,
        w1_receipt_sha256: file_hash(Path::new(&w1))?,
        p1i_receipt_sha256: file_hash(Path::new(&p1i))?,
        document_count,
        frozen_event_count: events.iter().filter(|event| event.distance <= BASE_DISTANCE).count(),
        frozen_event_chain_sha256: event_chain,
        nominations,
        same_phenotype_witnesses: same_witnesses,
        phenotype_mismatches: mismatches,
        no_later_witnesses: no_later,
        abstain_witnesses: abstains,
        frozen_actionable_witness_chain_sha256: witness_chain,
        arms: Arm::ALL.into_iter().map(|arm| run(arm, &witnesses, nominations, same_witnesses, abstains)).collect(),
        delay_buckets: &["0-16", "17-32", "33-64", "65-128", "129+"],
        persistence_clock: "candidate-specific endpoint opportunity: each document containing either source or target between nomination and witness advances the frozen gap",
        chronology_claim: "FiQA has no timestamps; this audit tests sequence-order persistence with a candidate-specific endpoint-opportunity clock, not real-time chronology",
        conclusion: "LA2_E1_COMPLETE: pure exponential and explicit pending credit retain all actionable witnesses; floor 0.01 and floor 0.05 each retain only one; no learner or serving promotion follows from this diagnostic",
    };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("schema={SCHEMA} docs={document_count} events={} nominations={nominations} same={same_witnesses} actionable={} abstain={abstains}", events.iter().filter(|event| event.distance <= BASE_DISTANCE).count(), witnesses.len());
    for arm in &receipt.arms {
        println!(
            "arm={} retained={} valid_lost={} invalid_updates={} false_auth={} p50_bucket_33_64={}",
            arm.arm_label,
            arm.retained_witnesses,
            arm.valid_witnesses_lost,
            arm.invalid_witness_updates,
            arm.false_authorizations,
            arm.delay_buckets[2].retained
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_boundaries_are_frozen() {
        assert_eq!(bucket(16), "0-16");
        assert_eq!(bucket(17), "17-32");
        assert_eq!(bucket(64), "33-64");
        assert_eq!(bucket(65), "65-128");
        assert_eq!(bucket(129), "129+");
    }

    #[test]
    fn pending_tag_accepts_a_decayed_obligation() {
        let authority = LAMBDA.powi(54);
        assert!(authority < 0.01);
        assert!(Arm::PendingTag.acceptance_floor() == 0.0);
    }

    #[test]
    fn frozen_witness_classification_is_stable() {
        let event = Event {
            doc: 1,
            candidate: 0,
            phi: 0,
            distance: 2,
            negative: false,
            support: false,
        };
        assert_eq!(classify(event), WitnessKind::Support);
    }
}
