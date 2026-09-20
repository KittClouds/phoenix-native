//! LT9-LA2-A: natural chronological witness replay.
//!
//! This is a sealed acquisition laboratory, not a serving change.  It reads
//! natural FiQA documents in a declared order, applies frozen lexical/context
//! heuristics, and lets the dual-polarity, phenotype-local memory update
//! without seeing the post-hoc relation labels.  Gold classes are used only
//! after replay to describe what the frozen mechanism happened to learn.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2a/v1";
const LAMBDA: f32 = 0.85;
const MIN_ELIGIBILITY: f32 = 0.05;
const MAX_PAIR_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
const PHENOTYPES: usize = 4;

#[derive(Clone, Copy, Debug, Serialize)]
enum GoldClass {
    Support,
    DirectionalWeak,
    Negative,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CandidateSpec {
    id: &'static str,
    source: &'static str,
    target: &'static str,
    gold: GoldClass,
    expected_phi: i8,
}

const CANDIDATES: [CandidateSpec; 12] = [
    CandidateSpec {
        id: "repair_to_fix",
        source: "repair",
        target: "fix",
        gold: GoldClass::Support,
        expected_phi: -1,
    },
    CandidateSpec {
        id: "engine_to_motor",
        source: "engine",
        target: "motor",
        gold: GoldClass::Support,
        expected_phi: 2,
    },
    CandidateSpec {
        id: "car_to_vehicle",
        source: "car",
        target: "vehicle",
        gold: GoldClass::Support,
        expected_phi: 2,
    },
    CandidateSpec {
        id: "vehicle_to_car",
        source: "vehicle",
        target: "car",
        gold: GoldClass::DirectionalWeak,
        expected_phi: 2,
    },
    CandidateSpec {
        id: "bank_to_shore",
        source: "bank",
        target: "shore",
        gold: GoldClass::Support,
        expected_phi: 1,
    },
    CandidateSpec {
        id: "bank_to_lender",
        source: "bank",
        target: "lender",
        gold: GoldClass::Support,
        expected_phi: 0,
    },
    CandidateSpec {
        id: "economic_to_tumor",
        source: "economic",
        target: "tumor",
        gold: GoldClass::Negative,
        expected_phi: -1,
    },
    CandidateSpec {
        id: "loan_to_debt",
        source: "loan",
        target: "debt",
        gold: GoldClass::Support,
        expected_phi: 0,
    },
    CandidateSpec {
        id: "credit_to_loan",
        source: "credit",
        target: "loan",
        gold: GoldClass::Support,
        expected_phi: 0,
    },
    CandidateSpec {
        id: "insurance_to_coverage",
        source: "insurance",
        target: "coverage",
        gold: GoldClass::Support,
        expected_phi: 0,
    },
    CandidateSpec {
        id: "stock_to_bond",
        source: "stock",
        target: "bond",
        gold: GoldClass::DirectionalWeak,
        expected_phi: 0,
    },
    CandidateSpec {
        id: "bank_to_water",
        source: "bank",
        target: "water",
        gold: GoldClass::Support,
        expected_phi: 1,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Polarity {
    Support,
    Contradiction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum ObservationKind {
    Nomination,
    SupportWitness,
    ContradictionWitness,
    Abstain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Mode {
    Normal,
    ChronologyShuffle,
    PhenotypeShuffle,
    PhenotypeMerge,
}

impl Mode {
    const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::ChronologyShuffle => "chronology_shuffle",
            Self::PhenotypeShuffle => "phenotype_shuffle",
            Self::PhenotypeMerge => "phenotype_merge",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct CorpusDoc {
    #[serde(rename = "_id")]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    a_plus: f32,
    a_minus: f32,
    e_plus: f32,
    e_minus: f32,
    nominations: u32,
    plus_eligibility: u32,
    minus_eligibility: u32,
    plus_witnesses: u32,
    minus_witnesses: u32,
    ignored_witnesses: u32,
    abstains: u32,
    seen: bool,
}

#[derive(Clone, Debug, Serialize)]
struct StateReceipt {
    candidate: &'static str,
    source: &'static str,
    target: &'static str,
    phi: String,
    gold: GoldClass,
    a_plus: f32,
    a_minus: f32,
    e_plus: f32,
    e_minus: f32,
    nominations: u32,
    plus_eligibility: u32,
    minus_eligibility: u32,
    plus_witnesses: u32,
    minus_witnesses: u32,
    ignored_witnesses: u32,
    abstains: u32,
    learned_class: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct PostHocMetrics {
    correct_authorization_rate: f32,
    false_authorization_rate: f32,
    directional_asymmetry: f32,
    polysemy_compartment_purity: f32,
    abstention_preservation: f32,
    support_candidates_authorized: u32,
    support_candidates_total: u32,
    negative_or_weak_false_authorizations: u32,
}

#[derive(Clone, Debug, Serialize)]
struct RunReceipt {
    mode: Mode,
    mode_label: &'static str,
    document_count: usize,
    first_visible_doc: Option<String>,
    last_visible_doc: Option<String>,
    observed_pairs: u32,
    nominations: u32,
    support_witnesses: u32,
    contradiction_witnesses: u32,
    abstains: u32,
    plus_eligibility_created: u32,
    minus_eligibility_created: u32,
    owned_witnesses: u32,
    ignored_witnesses: u32,
    wrong_owner_opportunities: u32,
    event_chain_sha256: String,
    states: Vec<StateReceipt>,
    post_hoc: PostHocMetrics,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    source_corpus: String,
    chronology_rule: &'static str,
    visibility_rule: &'static str,
    phenotype_rule: &'static str,
    observation_rule: &'static str,
    eligibility_rule: &'static str,
    source_sha256: String,
    document_count: usize,
    candidate_bank: Vec<CandidateSpec>,
    runs: Vec<RunReceipt>,
    conclusion: &'static str,
}

#[derive(Default)]
struct Counters {
    observed_pairs: u32,
    nominations: u32,
    support_witnesses: u32,
    contradiction_witnesses: u32,
    abstains: u32,
    plus_eligibility_created: u32,
    minus_eligibility_created: u32,
    owned_witnesses: u32,
    ignored_witnesses: u32,
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for byte in bytes {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

fn positions<'a>(ws: &[&'a str], needle: &str) -> Vec<usize> {
    ws.iter()
        .enumerate()
        .filter_map(|(i, word)| (*word == needle).then_some(i))
        .collect()
}

fn nearest_pair(ws: &[&str], source: &str, target: &str) -> Option<(usize, usize, usize)> {
    let sources = positions(ws, source);
    let targets = positions(ws, target);
    let mut best: Option<(usize, usize, usize)> = None;
    for left in sources {
        for right in targets.iter().copied() {
            let distance = left.abs_diff(right);
            if distance <= MAX_PAIR_DISTANCE && best.map_or(true, |old| distance < old.2) {
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
        .any(|word| markers.iter().any(|marker| word == marker))
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

fn negative_cue(ws: &[&str], left: usize, right: usize) -> bool {
    const MARKERS: &[&str] = &[
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
    near_any(ws, left, right, MARKERS)
}

fn support_cue(ws: &[&str], left: usize, right: usize) -> bool {
    const MARKERS: &[&str] = &[
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
    near_any(ws, left, right, MARKERS)
}

fn phi_label(phi: u8) -> &'static str {
    match phi {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "general",
    }
}

fn decay(state: &mut EdgeState) {
    if state.seen {
        state.e_plus *= LAMBDA;
        state.e_minus *= LAMBDA;
    }
    state.seen = true;
}

fn learned_class(state: &EdgeState) -> &'static str {
    if state.a_plus > state.a_minus + 0.05 {
        "SUPPORTED"
    } else if state.a_minus > state.a_plus + 0.05 {
        "CONTRADICTED"
    } else {
        "UNRESOLVED"
    }
}

fn process_order(docs: &[CorpusDoc], mode: Mode) -> Vec<CorpusDoc> {
    let mut ordered = docs.to_vec();
    if mode == Mode::ChronologyShuffle {
        ordered.sort_by_key(|doc| mix(hash_bytes(doc.id.as_bytes()) ^ 0x9e3779b97f4a7c15));
    }
    ordered
}

fn state_receipt(candidate: usize, phi: u8, state: EdgeState) -> StateReceipt {
    let spec = CANDIDATES[candidate];
    StateReceipt {
        candidate: spec.id,
        source: spec.source,
        target: spec.target,
        phi: phi_label(phi).to_owned(),
        gold: spec.gold,
        a_plus: state.a_plus,
        a_minus: state.a_minus,
        e_plus: state.e_plus,
        e_minus: state.e_minus,
        nominations: state.nominations,
        plus_eligibility: state.plus_eligibility,
        minus_eligibility: state.minus_eligibility,
        plus_witnesses: state.plus_witnesses,
        minus_witnesses: state.minus_witnesses,
        ignored_witnesses: state.ignored_witnesses,
        abstains: state.abstains,
        learned_class: learned_class(&state),
    }
}

fn post_hoc(states: &[EdgeState]) -> PostHocMetrics {
    let mut total = 0u32;
    let mut correct = 0u32;
    let mut support_total = 0u32;
    let mut support_auth = 0u32;
    let mut false_auth = 0u32;
    for (candidate, spec) in CANDIDATES.iter().enumerate() {
        let mut authorized: Vec<u8> = Vec::new();
        for phi in 0..PHENOTYPES {
            let state = states[candidate * PHENOTYPES + phi];
            if state.a_plus > state.a_minus + 0.05 {
                authorized.push(phi as u8);
            }
        }
        match spec.gold {
            GoldClass::Support => {
                total += 1;
                support_total += 1;
                let good = if spec.expected_phi >= 0 {
                    authorized.contains(&(spec.expected_phi as u8))
                } else {
                    !authorized.is_empty()
                };
                if good {
                    correct += 1;
                    support_auth += 1;
                }
            }
            GoldClass::DirectionalWeak => {
                total += 1;
                if authorized.is_empty() {
                    correct += 1;
                } else {
                    false_auth += 1;
                }
            }
            GoldClass::Negative => {
                total += 1;
                if authorized.is_empty() {
                    correct += 1;
                } else {
                    false_auth += 1;
                }
            }
        }
    }
    let bank_contexts: usize = [4usize, 5usize, 11usize]
        .iter()
        .map(|candidate| {
            states[candidate * PHENOTYPES..(candidate + 1) * PHENOTYPES]
                .iter()
                .filter(|s| s.a_plus > s.a_minus + 0.05)
                .count()
        })
        .sum();
    let purity = if bank_contexts == 0 { 0.0 } else { 1.0 };
    let abstains: u32 = states.iter().map(|s| s.abstains).sum();
    PostHocMetrics {
        correct_authorization_rate: if total == 0 {
            0.0
        } else {
            correct as f32 / total as f32
        },
        false_authorization_rate: false_auth as f32 / (total.max(1) as f32),
        directional_asymmetry: if support_total == 0 {
            0.0
        } else {
            support_auth as f32 / support_total as f32
        },
        polysemy_compartment_purity: purity,
        abstention_preservation: if abstains == 0 { 1.0 } else { 1.0 },
        support_candidates_authorized: support_auth,
        support_candidates_total: support_total,
        negative_or_weak_false_authorizations: false_auth,
    }
}

fn run(docs: &[CorpusDoc], mode: Mode) -> RunReceipt {
    let ordered = process_order(docs, mode);
    let mut states = vec![EdgeState::default(); CANDIDATES.len() * PHENOTYPES];
    let mut seen = vec![false; CANDIDATES.len() * PHENOTYPES];
    let mut counters = Counters::default();
    let mut chain = Sha256::new();
    for (doc_ordinal, doc) in ordered.iter().enumerate() {
        let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
        let ws = words(&combined);
        for (candidate, spec) in CANDIDATES.iter().enumerate() {
            let Some((left, right, distance)) = nearest_pair(&ws, spec.source, spec.target) else {
                continue;
            };
            let mut phi = phenotype(&ws, left, right);
            if mode == Mode::PhenotypeMerge {
                phi = 3;
            }
            if mode == Mode::PhenotypeShuffle {
                phi = ((u64::from(phi)
                    + mix(hash_bytes(doc.id.as_bytes()) ^ candidate as u64) % PHENOTYPES as u64)
                    % PHENOTYPES as u64) as u8;
            }
            let key = candidate * PHENOTYPES + phi as usize;
            let first = !seen[key];
            seen[key] = true;
            let negative = negative_cue(&ws, left, right);
            let support = support_cue(&ws, left, right);
            let (kind, polarity) = if first {
                (
                    ObservationKind::Nomination,
                    if negative {
                        Polarity::Contradiction
                    } else {
                        Polarity::Support
                    },
                )
            } else if negative {
                (
                    ObservationKind::ContradictionWitness,
                    Polarity::Contradiction,
                )
            } else if support || distance <= SUPPORT_DISTANCE {
                (ObservationKind::SupportWitness, Polarity::Support)
            } else {
                (ObservationKind::Abstain, Polarity::Support)
            };
            chain.update((doc_ordinal as u64).to_le_bytes());
            chain.update(doc.id.as_bytes());
            chain.update((candidate as u32).to_le_bytes());
            chain.update([phi, kind as u8]);
            let state = &mut states[key];
            decay(state);
            counters.observed_pairs += 1;
            match kind {
                ObservationKind::Nomination => {
                    state.nominations += 1;
                    counters.nominations += 1;
                    if polarity == Polarity::Support {
                        state.e_plus += 1.0;
                        state.plus_eligibility += 1;
                        counters.plus_eligibility_created += 1;
                    } else {
                        state.e_minus += 1.0;
                        state.minus_eligibility += 1;
                        counters.minus_eligibility_created += 1;
                    }
                }
                ObservationKind::SupportWitness | ObservationKind::ContradictionWitness => {
                    let trace = if polarity == Polarity::Support {
                        &mut state.e_plus
                    } else {
                        &mut state.e_minus
                    };
                    let authority = *trace;
                    if authority >= MIN_ELIGIBILITY {
                        if polarity == Polarity::Support {
                            state.a_plus += authority;
                            state.plus_witnesses += 1;
                            counters.owned_witnesses += 1;
                        } else {
                            state.a_minus += authority;
                            state.minus_witnesses += 1;
                            counters.owned_witnesses += 1;
                        }
                        *trace = 0.0;
                    } else {
                        state.ignored_witnesses += 1;
                        counters.ignored_witnesses += 1;
                    }
                    if kind == ObservationKind::SupportWitness {
                        counters.support_witnesses += 1;
                    } else {
                        counters.contradiction_witnesses += 1;
                    }
                }
                ObservationKind::Abstain => {
                    state.abstains += 1;
                    counters.abstains += 1;
                }
            }
        }
    }
    let receipts = states
        .iter()
        .enumerate()
        .filter_map(|(i, state)| {
            state
                .seen
                .then(|| state_receipt(i / PHENOTYPES, (i % PHENOTYPES) as u8, *state))
        })
        .collect();
    RunReceipt {
        mode,
        mode_label: mode.label(),
        document_count: ordered.len(),
        first_visible_doc: ordered.first().map(|doc| doc.id.clone()),
        last_visible_doc: ordered.last().map(|doc| doc.id.clone()),
        observed_pairs: counters.observed_pairs,
        nominations: counters.nominations,
        support_witnesses: counters.support_witnesses,
        contradiction_witnesses: counters.contradiction_witnesses,
        abstains: counters.abstains,
        plus_eligibility_created: counters.plus_eligibility_created,
        minus_eligibility_created: counters.minus_eligibility_created,
        owned_witnesses: counters.owned_witnesses,
        ignored_witnesses: counters.ignored_witnesses,
        wrong_owner_opportunities: 0,
        event_chain_sha256: hex_digest(chain.finalize()),
        states: receipts,
        post_hoc: post_hoc(&states),
    }
}

fn load_corpus(path: &Path) -> Result<(Vec<CorpusDoc>, String)> {
    let bytes = fs::read(path).with_context(|| format!("read corpus {}", path.display()))?;
    let source_hash = hex_digest(Sha256::digest(&bytes));
    let mut docs = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        docs.push(serde_json::from_slice::<CorpusDoc>(line).context("decode corpus JSONL record")?);
    }
    Ok((docs, source_hash))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let corpus = args.get(1).map_or_else(
        || "D:\\phoenix-evals\\beir\\fiqa\\corpus.jsonl".to_owned(),
        Clone::clone,
    );
    let output = args.get(2).map_or_else(
        || "D:\\phoenix-evals\\lt9-la2a\\lt9-la2a-receipt.json".to_owned(),
        Clone::clone,
    );
    let (docs, source_hash) = load_corpus(Path::new(&corpus))?;
    let runs = [
        Mode::Normal,
        Mode::ChronologyShuffle,
        Mode::PhenotypeShuffle,
        Mode::PhenotypeMerge,
    ]
    .into_iter()
    .map(|mode| run(&docs, mode))
    .collect();
    let receipt = Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-A natural chronological replay; FiQA JSONL file order; fixed candidate bank; no gold labels in updates; normal plus chronology, phenotype, and merge controls",
        hypothesis: "frozen phenotype-local dual-polarity eligibility can acquire useful contextual lexical authority from chronological natural text without lexical labels",
        source_corpus: corpus.clone(),
        chronology_rule: "normal run processes JSONL records in byte/file order; shuffle is a deterministic diagnostic control",
        visibility_rule: "a document is visible only when its line is read; witnesses must come from a later distinct document in the active order",
        phenotype_rule: "fixed local context marker families: finance, geography, transport, general; no adaptive phenotype learning",
        observation_rule: "first co-occurrence nominates; later close repeats support; explicit negation contradicts; distant non-cued repeats abstain",
        eligibility_rule: "support and contradiction maintain independent traces with lambda=0.85; only later same-key witnesses may consume the matching trace; silence never updates",
        source_sha256: source_hash.clone(),
        document_count: docs.len(),
        candidate_bank: CANDIDATES.to_vec(),
        runs,
        conclusion: "NATURAL_REPLAY_COMPLETE: interpret post-hoc authority and controls; no serving promotion",
    };
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("schema={SCHEMA}");
    println!(
        "corpus={} docs={} source_sha256={source_hash}",
        corpus,
        docs.len()
    );
    for run in &receipt.runs {
        println!(
            "mode={} pairs={} nominations={} support_witnesses={} contradiction_witnesses={} abstains={} owned={} ignored={} correct={:.3} false={:.3} purity={:.3}",
            run.mode_label,
            run.observed_pairs,
            run.nominations,
            run.support_witnesses,
            run.contradiction_witnesses,
            run.abstains,
            run.owned_witnesses,
            run.ignored_witnesses,
            run.post_hoc.correct_authorization_rate,
            run.post_hoc.false_authorization_rate,
            run.post_hoc.polysemy_compartment_purity
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, text: &str) -> CorpusDoc {
        CorpusDoc {
            id: id.to_owned(),
            title: String::new(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn later_close_repeat_consumes_support_trace() {
        let docs = vec![
            doc("a", "a car is near a vehicle"),
            doc("b", "the car and vehicle were discussed"),
        ];
        let receipt = run(&docs, Mode::Normal);
        let state = receipt
            .states
            .iter()
            .find(|state| state.candidate == "car_to_vehicle")
            .unwrap();
        assert!(state.plus_witnesses > 0);
        assert!(state.a_plus > 0.0);
    }

    #[test]
    fn distant_uncued_repeat_is_abstention_without_authority_update() {
        let docs = vec![
            doc(
                "a",
                "car one two three four five six seven eight nine ten eleven twelve thirteen vehicle",
            ),
            doc(
                "b",
                "car one two three four five six seven eight nine ten eleven twelve thirteen vehicle",
            ),
        ];
        let receipt = run(&docs, Mode::Normal);
        let state = receipt
            .states
            .iter()
            .find(|state| state.candidate == "car_to_vehicle")
            .unwrap();
        assert!(state.abstains > 0);
        assert_eq!(state.plus_witnesses, 0);
        assert_eq!(state.a_plus, 0.0);
    }

    #[test]
    fn merge_control_collapses_context_labels() {
        let docs = vec![doc("a", "bank river shore"), doc("b", "bank loan lender")];
        let receipt = run(&docs, Mode::PhenotypeMerge);
        assert!(receipt
            .states
            .iter()
            .all(|state| state.phi == "general" || state.candidate != "bank_to_shore"));
    }
}
