//! LT9-LA2-P1L2: discovery-only anatomy of the natural tie-resolved tail.
//!
//! Replays the frozen P1L1 candidate/event/episode contract on the same
//! HotpotQA corpus. It compares invalid TIE_RESOLVED episodes with distinct,
//! nearest-in-corpus valid TIE_RESOLVED controls of the same candidate.
//! No routing, learning, authority, ranking, or serving state is changed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1l2/v1";
const MAX_PAIR_DISTANCE: usize = 24;
const SUPPORT_DISTANCE: usize = 8;
const EXPECTED_CORPUS_SHA256: &str =
    "3e776d2343352f83341878202b8c49cc1ebe6e2ad4c2a77a21c116cafa229334";
const EXPECTED_P1L1_SOURCE_SHA256: &str =
    "feda3bfa5e18ac120b158c746348f226949b5426cc64addf9f29af4c7535b6d3";

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

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Features {
    counts: [u16; 3],
    marker_masks: [u32; 3],
    before_masks: [u32; 3],
    after_masks: [u32; 3],
    distance: u16,
    support: bool,
    negative: bool,
    same_field: bool,
}

#[derive(Clone, Copy)]
struct Event {
    doc: u64,
    candidate: usize,
    features: Features,
}

#[derive(Clone, Copy)]
struct Episode {
    candidate: usize,
    nomination: Event,
    witness: Event,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RoutePath {
    UniquePlurality,
    TieResolved,
    TieAbstained,
    UniqueConflictAbstained,
}

impl RoutePath {
    fn label(self) -> &'static str {
        match self {
            Self::UniquePlurality => "UNIQUE_PLURALITY",
            Self::TieResolved => "TIE_RESOLVED",
            Self::TieAbstained => "TIE_ABSTAINED",
            Self::UniqueConflictAbstained => "UNIQUE_CONFLICT_ABSTAINED",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Endpoint {
    document: u64,
    counts: [u16; 3],
    max_count: u16,
    runner_up_count: u16,
    margin: u16,
    tie_set: [bool; 3],
    unique_family: Option<&'static str>,
    fixed_priority: &'static str,
    raw_argmax: Option<&'static str>,
    priority_inversion: bool,
    marker_masks: [u32; 3],
    before_masks: [u32; 3],
    after_masks: [u32; 3],
    support: bool,
    negative: bool,
}

#[derive(Clone, Debug, Serialize)]
struct EpisodeReport {
    key: String,
    candidate: &'static str,
    expected: &'static str,
    route: Option<&'static str>,
    route_path: &'static str,
    invalid: bool,
    polarity: &'static str,
    opportunity_delay: u64,
    mixed_marker: bool,
    priority_inversion: bool,
    marker_mask_hamming: u32,
    side_mask_hamming: u32,
    same_field: bool,
    agreement_shape: &'static str,
    nomination: Endpoint,
    witness: Endpoint,
}

#[derive(Serialize)]
struct MatchedPair {
    invalid_key: String,
    control_key: Option<String>,
    matching: &'static str,
    nomination_gap_docs: Option<u64>,
}

#[derive(Serialize)]
struct FeatureSummary {
    n: usize,
    mixed_marker: usize,
    priority_inversion: usize,
    nomination_unique_witness_tie: usize,
    nomination_tie_witness_unique: usize,
    both_unique_same: usize,
    same_field: usize,
    polarity: BTreeMap<String, usize>,
    count_signatures: BTreeMap<String, usize>,
    endpoint_margin_pairs: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    source_corpus: String,
    source_corpus_sha256: String,
    p1l1_receipt_sha256: String,
    p1l1_manifest_sha256: String,
    p1l1_source_sha256: String,
    document_count: u64,
    replay_episode_count: usize,
    p1l1_invalid_key_parity: usize,
    invalid_tie_resolved_count: usize,
    valid_tie_resolved_pool_count: usize,
    unevaluated_tie_resolved_count: usize,
    matched_control_count: usize,
    matching_rule: &'static str,
    invalid_summary: FeatureSummary,
    matched_control_summary: FeatureSummary,
    matched_pairs: Vec<MatchedPair>,
    invalid_episodes: Vec<EpisodeReport>,
    matched_valid_controls: Vec<EpisodeReport>,
    all_valid_tie_resolved: Vec<EpisodeReport>,
    conclusion: &'static str,
}

fn family(i: usize) -> &'static str {
    match i {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "general",
    }
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
    for l in positions(ws, source) {
        for r in positions(ws, target) {
            let d = l.abs_diff(r);
            if d <= 40 && best.map_or(true, |old: (usize, usize, usize)| d < old.2) {
                best = Some((l, r, d));
            }
        }
    }
    best
}

fn marker_mask(ws: &[&str], start: usize, end: usize, markers: &[&str]) -> (u16, u32) {
    let mut count = 0u16;
    let mut mask = 0u32;
    for word in &ws[start..end] {
        for (i, marker) in markers.iter().enumerate() {
            if word == marker {
                count = count.saturating_add(1);
                mask |= 1u32 << i;
            }
        }
    }
    (count, mask)
}

fn any_marker(ws: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let lo = left.min(right).saturating_sub(8);
    let hi = (left.max(right) + 9).min(ws.len());
    ws[lo..hi].iter().any(|w| markers.iter().any(|m| w == m))
}

fn context(ws: &[&str], left: usize, right: usize, distance: usize, title_len: usize) -> Features {
    let lo = left.min(right).saturating_sub(8);
    let hi = (left.max(right) + 9).min(ws.len());
    let split = left.min(right);
    let after = (left.max(right) + 1).min(ws.len());
    let (f, fm) = marker_mask(ws, lo, hi, FINANCE);
    let (g, gm) = marker_mask(ws, lo, hi, GEO);
    let (t, tm) = marker_mask(ws, lo, hi, TRANSPORT);
    let (_, fb) = marker_mask(ws, lo, split, FINANCE);
    let (_, gb) = marker_mask(ws, lo, split, GEO);
    let (_, tb) = marker_mask(ws, lo, split, TRANSPORT);
    let (_, fa) = marker_mask(ws, after, hi, FINANCE);
    let (_, ga) = marker_mask(ws, after, hi, GEO);
    let (_, ta) = marker_mask(ws, after, hi, TRANSPORT);
    Features {
        counts: [f, g, t],
        marker_masks: [fm, gm, tm],
        before_masks: [fb, gb, tb],
        after_masks: [fa, ga, ta],
        distance: distance as u16,
        support: any_marker(ws, left, right, SUPPORT),
        negative: any_marker(ws, left, right, NEGATIVE),
        same_field: (left < title_len) == (right < title_len),
    }
}

fn ties(c: [u16; 3]) -> Vec<usize> {
    let max = *c.iter().max().unwrap_or(&0);
    (0..3).filter(|&i| c[i] == max).collect()
}

fn route(a: Features, b: Features) -> Option<usize> {
    let at = ties(a.counts);
    let bt = ties(b.counts);
    if at.len() == 1 && bt.len() == 2 && bt.contains(&at[0]) {
        return Some(at[0]);
    }
    if bt.len() == 1 && at.len() == 2 && at.contains(&bt[0]) {
        return Some(bt[0]);
    }
    if at.len() == 1 && bt.len() == 1 && at[0] == bt[0] {
        return Some(at[0]);
    }
    None
}

fn route_path(a: Features, b: Features, r: Option<usize>) -> RoutePath {
    let au = ties(a.counts).len() == 1;
    let bu = ties(b.counts).len() == 1;
    match (r, au, bu) {
        (Some(_), true, true) => RoutePath::UniquePlurality,
        (Some(_), _, _) => RoutePath::TieResolved,
        (None, false, _) | (None, _, false) => RoutePath::TieAbstained,
        (None, true, true) => RoutePath::UniqueConflictAbstained,
    }
}

fn fixed_phi(f: Features) -> usize {
    if f.counts[1] > 0 {
        1
    } else if f.counts[0] > 0 {
        0
    } else if f.counts[2] > 0 {
        2
    } else {
        3
    }
}

fn endpoint(e: Event) -> Endpoint {
    let f = e.features;
    let max = *f.counts.iter().max().unwrap_or(&0);
    let mut sorted = f.counts;
    sorted.sort_unstable();
    let tie = ties(f.counts);
    let unique = (tie.len() == 1).then(|| family(tie[0]));
    let raw = (tie.len() == 1).then(|| family(tie[0]));
    let fixed = fixed_phi(f);
    Endpoint {
        document: e.doc,
        counts: f.counts,
        max_count: max,
        runner_up_count: sorted[1],
        margin: max.saturating_sub(sorted[1]),
        tie_set: [tie.contains(&0), tie.contains(&1), tie.contains(&2)],
        unique_family: unique,
        fixed_priority: family(fixed),
        raw_argmax: raw,
        priority_inversion: unique.is_some_and(|name| name != family(fixed)),
        marker_masks: f.marker_masks,
        before_masks: f.before_masks,
        after_masks: f.after_masks,
        support: f.support,
        negative: f.negative,
    }
}

fn hamming(a: &[u32; 3], b: &[u32; 3]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

fn polarity(f: Features) -> &'static str {
    if f.negative {
        "CONTRADICTION"
    } else if f.support || usize::from(f.distance) <= SUPPORT_DISTANCE {
        "SUPPORT"
    } else {
        "ABSTAIN"
    }
}

fn key(e: Episode) -> String {
    format!(
        "{}:{}->{}",
        CANDIDATES[e.candidate].id, e.nomination.doc, e.witness.doc
    )
}

fn report(e: Episode) -> EpisodeReport {
    let c = CANDIDATES[e.candidate];
    let r = route(e.nomination.features, e.witness.features);
    let p = route_path(e.nomination.features, e.witness.features, r);
    let invalid = r.is_some() && c.expected_phi >= 0 && r != Some(c.expected_phi as usize);
    let n_unique = ties(e.nomination.features.counts);
    let w_unique = ties(e.witness.features.counts);
    let agreement_shape = match (n_unique.len(), w_unique.len()) {
        (1, 1) => "BOTH_UNIQUE_SAME",
        (1, 2) => "NOMINATION_UNIQUE_WITNESS_TIE",
        (2, 1) => "NOMINATION_TIE_WITNESS_UNIQUE",
        _ => "OTHER",
    };
    let mixed = [e.nomination.features, e.witness.features]
        .iter()
        .any(|f| f.counts.iter().filter(|&&x| x > 0).count() > 1);
    EpisodeReport {
        key: key(e),
        candidate: c.id,
        expected: if c.expected_phi < 0 {
            "UNSPECIFIED"
        } else {
            family(c.expected_phi as usize)
        },
        route: r.map(family),
        route_path: p.label(),
        invalid,
        polarity: polarity(e.witness.features),
        opportunity_delay: e.witness.doc.saturating_sub(e.nomination.doc),
        mixed_marker: mixed,
        priority_inversion: endpoint(e.nomination).priority_inversion
            || endpoint(e.witness).priority_inversion,
        marker_mask_hamming: hamming(
            &e.nomination.features.marker_masks,
            &e.witness.features.marker_masks,
        ),
        side_mask_hamming: hamming(
            &e.nomination.features.before_masks,
            &e.witness.features.before_masks,
        ) + hamming(
            &e.nomination.features.after_masks,
            &e.witness.features.after_masks,
        ),
        same_field: e.nomination.features.same_field && e.witness.features.same_field,
        agreement_shape,
        nomination: endpoint(e.nomination),
        witness: endpoint(e.witness),
    }
}

fn load_events(path: &Path) -> Result<(Vec<Event>, u64, String)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut line = String::new();
    let mut hasher = Sha256::new();
    let mut docs = 0u64;
    let mut events = Vec::new();
    while reader.read_line(&mut line)? > 0 {
        hasher.update(line.as_bytes());
        if !line.trim().is_empty() {
            let doc: CorpusDoc = serde_json::from_str(&line)?;
            let combined = format!("{} {}", doc.title, doc.text).to_ascii_lowercase();
            let ws = words(&combined);
            let title_len = words(&doc.title.to_ascii_lowercase()).len();
            for (candidate, spec) in CANDIDATES.iter().enumerate() {
                if let Some((l, r, d)) = nearest(&ws, spec.source, spec.target) {
                    if d <= MAX_PAIR_DISTANCE {
                        events.push(Event {
                            doc: docs,
                            candidate,
                            features: context(&ws, l, r, d, title_len),
                        });
                    }
                }
            }
            docs += 1;
        }
        line.clear();
    }
    ensure!(docs > 0, "empty corpus");
    Ok((events, docs, format!("{:x}", hasher.finalize())))
}

fn make_episodes(events: &[Event]) -> Vec<Episode> {
    let mut per = vec![Vec::new(); CANDIDATES.len()];
    for e in events {
        per[e.candidate].push(*e);
    }
    let mut out = Vec::new();
    for (candidate, stream) in per.iter().enumerate() {
        let mut i = 0;
        while i < stream.len() {
            let nomination = stream[i];
            let mut j = i + 1;
            while j < stream.len() {
                let witness = stream[j];
                if witness.doc > nomination.doc {
                    out.push(Episode {
                        candidate,
                        nomination,
                        witness,
                    });
                    i = j;
                    break;
                }
                j += 1;
            }
            if j >= stream.len() {
                break;
            }
        }
    }
    out.sort_unstable_by_key(|e| (e.nomination.doc, e.witness.doc, e.candidate));
    out
}

fn summarize(rows: &[EpisodeReport]) -> FeatureSummary {
    let mut s = FeatureSummary {
        n: rows.len(),
        mixed_marker: 0,
        priority_inversion: 0,
        nomination_unique_witness_tie: 0,
        nomination_tie_witness_unique: 0,
        both_unique_same: 0,
        same_field: 0,
        polarity: BTreeMap::new(),
        count_signatures: BTreeMap::new(),
        endpoint_margin_pairs: BTreeMap::new(),
    };
    for r in rows {
        s.mixed_marker += usize::from(r.mixed_marker);
        s.priority_inversion += usize::from(r.priority_inversion);
        s.same_field += usize::from(r.same_field);
        match r.agreement_shape {
            "NOMINATION_UNIQUE_WITNESS_TIE" => s.nomination_unique_witness_tie += 1,
            "NOMINATION_TIE_WITNESS_UNIQUE" => s.nomination_tie_witness_unique += 1,
            "BOTH_UNIQUE_SAME" => s.both_unique_same += 1,
            _ => {}
        }
        *s.polarity.entry(r.polarity.to_owned()).or_insert(0) += 1;
        *s.count_signatures
            .entry(format!("{:?}->{:?}", r.nomination.counts, r.witness.counts))
            .or_insert(0) += 1;
        *s.endpoint_margin_pairs
            .entry(format!("{}->{}", r.nomination.margin, r.witness.margin))
            .or_insert(0) += 1;
    }
    s
}

fn matched_controls(
    invalid: &[EpisodeReport],
    valid: &[EpisodeReport],
) -> (Vec<MatchedPair>, Vec<EpisodeReport>) {
    let mut used = BTreeSet::new();
    let mut pairs = Vec::with_capacity(invalid.len());
    let mut controls = Vec::with_capacity(invalid.len());
    for bad in invalid {
        let choice = valid
            .iter()
            .enumerate()
            .filter(|(i, good)| !used.contains(i) && good.candidate == bad.candidate)
            .min_by_key(|(_, good)| {
                (
                    good.nomination.document.abs_diff(bad.nomination.document),
                    good.key.clone(),
                )
            });
        if let Some((i, good)) = choice {
            used.insert(i);
            pairs.push(MatchedPair {
                invalid_key: bad.key.clone(), control_key: Some(good.key.clone()),
                matching: "same_candidate_and_TIE_RESOLVED; nearest_nomination_document; no_control_reuse",
                nomination_gap_docs: Some(good.nomination.document.abs_diff(bad.nomination.document)),
            });
            controls.push(good.clone());
        } else {
            pairs.push(MatchedPair {
                invalid_key: bad.key.clone(), control_key: None,
                matching: "same_candidate_and_TIE_RESOLVED; nearest_nomination_document; no_control_reuse",
                nomination_gap_docs: None,
            });
        }
    }
    (pairs, controls)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!(args.len() == 5, "usage: lt9_la2p1l2 <frozen-hotpotqa-corpus.jsonl> <p1l1-receipt.json> <p1l1-manifest.json> <output.json>");
    let corpus = Path::new(&args[1]);
    let p1l1_path = Path::new(&args[2]);
    let p1l1_manifest_path = Path::new(&args[3]);
    let output = Path::new(&args[4]);
    let p1l1_bytes = std::fs::read(p1l1_path)?;
    let p1l1_hash = format!("{:x}", Sha256::digest(&p1l1_bytes));
    let p1l1: Value = serde_json::from_slice(&p1l1_bytes)?;
    let p1l1_manifest_bytes = std::fs::read(p1l1_manifest_path)?;
    let p1l1_manifest_hash = format!("{:x}", Sha256::digest(&p1l1_manifest_bytes));
    let p1l1_manifest: Value = serde_json::from_slice(&p1l1_manifest_bytes)?;
    ensure!(
        p1l1.get("schema").and_then(Value::as_str) == Some("phoenix.lexical.lt9-la2p1l1/v1"),
        "unexpected P1L1 receipt schema"
    );
    ensure!(
        p1l1_manifest.get("schema").and_then(Value::as_str)
            == Some("phoenix.lexical.lt9la2p1l1-manifest/v1"),
        "unexpected P1L1 manifest schema"
    );
    ensure!(
        p1l1_manifest.get("source_sha256").and_then(Value::as_str)
            == Some(EXPECTED_P1L1_SOURCE_SHA256),
        "P1L1 source manifest differs from frozen version"
    );
    ensure!(
        p1l1_manifest.get("receipt_sha256").and_then(Value::as_str) == Some(p1l1_hash.as_str()),
        "P1L1 receipt hash differs from its manifest"
    );
    ensure!(
        p1l1_manifest
            .get("source_corpus_sha256")
            .and_then(Value::as_str)
            == Some(EXPECTED_CORPUS_SHA256),
        "P1L1 manifest corpus hash differs from frozen source"
    );
    ensure!(
        p1l1.get("source_sha256").and_then(Value::as_str) == Some(EXPECTED_CORPUS_SHA256),
        "P1L1 corpus hash differs from frozen source"
    );
    let declared_source = EXPECTED_P1L1_SOURCE_SHA256;
    let (events, documents, corpus_hash) = load_events(corpus)?;
    ensure!(
        corpus_hash == EXPECTED_CORPUS_SHA256,
        "corpus hash differs from frozen P1L1 input"
    );
    ensure!(
        p1l1.get("document_count").and_then(Value::as_u64) == Some(documents),
        "P1L1 document count differs from replay"
    );
    let episodes = make_episodes(&events);
    let mut invalid = Vec::new();
    let mut valid = Vec::new();
    let mut unevaluated = 0usize;
    for e in episodes.iter().copied() {
        let r = report(e);
        if r.route_path != "TIE_RESOLVED" {
            continue;
        }
        if r.invalid {
            invalid.push(r);
        } else if r.expected != "UNSPECIFIED" && r.route == Some(r.expected) {
            valid.push(r);
        } else {
            unevaluated += 1;
        }
    }
    invalid.sort_by_key(|r| r.nomination.document);
    valid.sort_by_key(|r| r.nomination.document);
    let p1l1_keys: BTreeSet<String> = p1l1
        .get("invalid_reports")
        .and_then(Value::as_array)
        .context("P1L1 invalid reports")?
        .iter()
        .filter(|r| r.get("route_path").and_then(Value::as_str) == Some("TIE_RESOLVED"))
        .filter_map(|r| r.get("key").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let our_keys: BTreeSet<String> = invalid.iter().map(|r| r.key.clone()).collect();
    let parity = our_keys.intersection(&p1l1_keys).count();
    ensure!(
        our_keys == p1l1_keys,
        "invalid TIE_RESOLVED key set differs from P1L1"
    );
    ensure!(
        invalid.len() == 31,
        "expected 31 frozen invalid TIE_RESOLVED episodes"
    );
    let (pairs, controls) = matched_controls(&invalid, &valid);
    let matched = controls.len();
    let invalid_count = invalid.len();
    let valid_count = valid.len();
    let receipt = Receipt {
        schema: SCHEMA,
        scope: "P1L2 discovery-only mixed finance/geography natural-tail analysis; resolver, learning, authority, retrieval, and serving unchanged",
        source_corpus: corpus.display().to_string(), source_corpus_sha256: corpus_hash,
        p1l1_receipt_sha256: p1l1_hash, p1l1_manifest_sha256: p1l1_manifest_hash,
        p1l1_source_sha256: declared_source.to_owned(),
        document_count: documents, replay_episode_count: episodes.len(),
        p1l1_invalid_key_parity: parity, invalid_tie_resolved_count: invalid.len(),
        valid_tie_resolved_pool_count: valid.len(),
        unevaluated_tie_resolved_count: unevaluated, matched_control_count: matched,
        matching_rule: "for each invalid tie-resolved episode, choose the nearest-in-corpus unused valid tie-resolved episode with the same candidate; ties break by episode key",
        invalid_summary: summarize(&invalid), matched_control_summary: summarize(&controls),
        matched_pairs: pairs, invalid_episodes: invalid, matched_valid_controls: controls,
        all_valid_tie_resolved: valid,
        conclusion: "DISCOVERY_ONLY: report structural differences and overlap; no resolver repair or authority update is authorized",
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "P1L2 docs={} episodes={} invalid_ties={} valid_tie_pool={} unevaluated_ties={} matched_controls={} parity={}",
        documents,
        episodes.len(),
        invalid_count,
        valid_count,
        unevaluated,
        matched,
        parity
    );
    Ok(())
}
