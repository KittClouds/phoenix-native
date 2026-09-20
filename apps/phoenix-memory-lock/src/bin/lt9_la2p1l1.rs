//! LT9-LA2-P1L1: LA2-B integration-failure anatomy.
//!
//! This binary is diagnostic-only. It reconstructs the frozen HotpotQA event
//! and episode stream, reports every B2 invalid route with existing context
//! evidence, compares shared endpoint keys with P1K2/P1K4, and maps invalid
//! routed episodes to the two contaminated authority compartments recorded by
//! LA2-B. It does not change routing, credit, authority, ranking, or serving.
#![allow(clippy::type_complexity)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1l1/v1";
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
struct ContextFeatures {
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
    features: ContextFeatures,
}

#[derive(Clone, Copy)]
struct Episode {
    candidate: usize,
    nomination: Event,
    witness: Event,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct EndpointReport {
    document: u64,
    counts: [u16; 3],
    max_count: u16,
    runner_up_count: u16,
    margin: u16,
    tie_set: [bool; 3],
    unique: bool,
    fixed_priority: &'static str,
    raw_argmax: &'static str,
    priority_inversion: bool,
    marker_masks: [u32; 3],
    before_masks: [u32; 3],
    after_masks: [u32; 3],
    support: bool,
    negative: bool,
    field: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RoutePath {
    UniquePlurality,
    TieResolved,
    TieAbstained,
    UniqueConflictAbstained,
}

impl RoutePath {
    const fn label(self) -> &'static str {
        match self {
            Self::UniquePlurality => "UNIQUE_PLURALITY",
            Self::TieResolved => "TIE_RESOLVED",
            Self::TieAbstained => "TIE_ABSTAINED",
            Self::UniqueConflictAbstained => "UNIQUE_CONFLICT_ABSTAINED",
        }
    }
}

#[derive(Serialize)]
struct EpisodeReport {
    key: String,
    candidate: &'static str,
    expected_phi: i8,
    route: Option<&'static str>,
    route_path: &'static str,
    invalid_routed: bool,
    polarity: &'static str,
    candidate_opportunity_delay: u64,
    mixed_marker: bool,
    priority_inversion: bool,
    side_mask_hamming: u32,
    marker_mask_hamming: u32,
    same_field: bool,
    nomination: EndpointReport,
    witness: EndpointReport,
}

#[derive(Serialize, Default)]
struct CountMap {
    by_candidate: BTreeMap<String, usize>,
    by_route_path: BTreeMap<String, usize>,
    by_phenotype: BTreeMap<String, usize>,
    by_signature: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct ControlPair {
    invalid_key: String,
    valid_key: String,
    route_path: &'static str,
    candidate: &'static str,
}

#[derive(Serialize)]
struct AuthorityCompartment {
    candidate: String,
    phenotype: String,
    a_plus: f64,
    a_minus: f64,
    nominations: u32,
    owned_witnesses: u32,
    causal_episodes: Vec<String>,
}

#[derive(Serialize, Default)]
struct Parity {
    p1k2_matching_keys: usize,
    p1k2_count_matches: usize,
    p1k2_plurality_outcome_matches: usize,
    p1k4_matching_keys: usize,
    p1k4_route_matches: usize,
    p1k4_invalid_key_overlap: usize,
    first_mismatch: Option<String>,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    protocol: &'static str,
    source_corpus: String,
    source_sha256: String,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    invalid_routed_episodes: usize,
    invalid_authority_compartments: usize,
    route_path_counts_invalid: BTreeMap<String, usize>,
    invalid_concentration: CountMap,
    invalid_reports: Vec<EpisodeReport>,
    matched_valid_controls: Vec<ControlPair>,
    contaminated_compartments: Vec<AuthorityCompartment>,
    parity: Parity,
    discovery_boundary: &'static str,
    conclusion: &'static str,
}

fn family_name(phi: usize) -> &'static str {
    match phi {
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
        for (i, m) in markers.iter().enumerate() {
            if word == m {
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
fn context(
    ws: &[&str],
    left: usize,
    right: usize,
    distance: usize,
    title_len: usize,
) -> ContextFeatures {
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
    ContextFeatures {
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
fn fixed_phi(f: ContextFeatures) -> usize {
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
fn tie_set(c: [u16; 3]) -> Vec<usize> {
    let m = *c.iter().max().unwrap_or(&0);
    (0..3).filter(|&i| c[i] == m).collect()
}
fn route(a: ContextFeatures, b: ContextFeatures) -> Option<usize> {
    let at = tie_set(a.counts);
    let bt = tie_set(b.counts);
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
fn path(a: ContextFeatures, b: ContextFeatures, r: Option<usize>) -> RoutePath {
    let au = tie_set(a.counts).len() == 1;
    let bu = tie_set(b.counts).len() == 1;
    match (r, au, bu) {
        (Some(_), true, true) => RoutePath::UniquePlurality,
        (Some(_), _, _) => RoutePath::TieResolved,
        (None, false, _) | (None, _, false) => RoutePath::TieAbstained,
        (None, true, true) => RoutePath::UniqueConflictAbstained,
    }
}
fn report_endpoint(event: Event) -> EndpointReport {
    let f = event.features;
    let m = *f.counts.iter().max().unwrap_or(&0);
    let mut sorted = f.counts;
    sorted.sort_unstable();
    let runner = sorted[1];
    let ties = tie_set(f.counts);
    let raw = ties.first().copied().unwrap_or(3);
    let fixed = fixed_phi(f);
    EndpointReport {
        document: event.doc,
        counts: f.counts,
        max_count: m,
        runner_up_count: runner,
        margin: m.saturating_sub(runner),
        tie_set: [ties.contains(&0), ties.contains(&1), ties.contains(&2)],
        unique: ties.len() == 1,
        fixed_priority: family_name(fixed),
        raw_argmax: family_name(raw),
        priority_inversion: ties.len() == 1 && fixed != raw,
        marker_masks: f.marker_masks,
        before_masks: f.before_masks,
        after_masks: f.after_masks,
        support: f.support,
        negative: f.negative,
        field: "combined_title_text",
    }
}
fn hamming(a: &[u32; 3], b: &[u32; 3]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}
fn polarity(e: Event) -> &'static str {
    if e.features.negative {
        "CONTRADICTION"
    } else if e.features.support || usize::from(e.features.distance) <= SUPPORT_DISTANCE {
        "SUPPORT"
    } else {
        "ABSTAIN"
    }
}
fn load_events(path: &Path) -> Result<(Vec<Event>, u64, String)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut line = String::new();
    let mut hasher = Sha256::new();
    let mut docs = 0;
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
            let nom = stream[i];
            let mut j = i + 1;
            while j < stream.len() {
                let wit = stream[j];
                if wit.doc > nom.doc {
                    out.push(Episode {
                        candidate,
                        nomination: nom,
                        witness: wit,
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
fn key(e: Episode) -> String {
    format!(
        "{}:{}->{}",
        CANDIDATES[e.candidate].id, e.nomination.doc, e.witness.doc
    )
}
fn json_pairs(
    p1k2_path: &Path,
    p1k4_path: &Path,
) -> Result<(BTreeMap<String, Value>, BTreeMap<String, Value>)> {
    let root: Value = serde_json::from_slice(&fs::read(p1k2_path)?)?;
    let mut p1k2 = BTreeMap::new();
    if let Some(rows) = root.get("pairs").and_then(Value::as_array) {
        for row in rows {
            let cand = row.get("candidate").and_then(Value::as_str).unwrap_or("");
            let n = row
                .get("nomination_doc")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let w = row.get("witness_doc").and_then(Value::as_u64).unwrap_or(0);
            p1k2.insert(format!("{}:{}->{}", cand, n, w), row.clone());
        }
    }
    let root: Value = serde_json::from_slice(&fs::read(p1k4_path)?)?;
    let mut p1k4 = BTreeMap::new();
    if let Some(rows) = root.get("tie_decisions").and_then(Value::as_array) {
        for row in rows {
            let cand = row.get("candidate").and_then(Value::as_str).unwrap_or("");
            let id = row.get("episode_id").and_then(Value::as_str).unwrap_or("");
            let mut parts = id.split(':');
            let _ = parts.next();
            if let Some(pair) = parts.next() {
                if let Some((n, w)) = pair.split_once("->") {
                    p1k4.insert(format!("{}:{}->{}", cand, n, w), row.clone());
                }
            }
        }
    }
    Ok((p1k2, p1k4))
}

#[derive(Deserialize)]
struct B2State {
    candidate: String,
    phenotype: String,
    a_plus: f64,
    a_minus: f64,
    nominations: u32,
    owned_witnesses: u32,
}
#[derive(Deserialize)]
struct B2Arm {
    arm_label: String,
    states: Vec<B2State>,
}
#[derive(Deserialize)]
struct B2Receipt {
    arms: Vec<B2Arm>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!(
        args.len() == 6,
        "usage: lt9_la2p1l1 <corpus> <la2b-receipt> <p1k2-receipt> <p1k4-receipt> <output>"
    );
    let corpus = Path::new(&args[1]);
    let la2b = Path::new(&args[2]);
    let p1k2 = Path::new(&args[3]);
    let p1k4 = Path::new(&args[4]);
    let output = Path::new(&args[5]);
    let (events, docs, source_sha) = load_events(corpus)?;
    let episodes = make_episodes(&events);
    let (k2, k4) = json_pairs(p1k2, p1k4)?;
    let mut invalid = Vec::new();
    let mut valid = Vec::new();
    let mut route_counts = BTreeMap::new();
    let mut concentration = CountMap::default();
    for e in episodes.iter().copied() {
        let r = route(e.nomination.features, e.witness.features);
        let p = path(e.nomination.features, e.witness.features, r);
        let expected = CANDIDATES[e.candidate].expected_phi;
        let bad = r.is_some() && expected >= 0 && r != Some(expected as usize);
        let ep = EpisodeReport {
            key: key(e),
            candidate: CANDIDATES[e.candidate].id,
            expected_phi: expected,
            route: r.map(family_name),
            route_path: p.label(),
            invalid_routed: bad,
            polarity: polarity(e.witness),
            candidate_opportunity_delay: e.witness.doc.saturating_sub(e.nomination.doc),
            mixed_marker: e
                .nomination
                .features
                .counts
                .iter()
                .filter(|&&x| x > 0)
                .count()
                > 1
                || e.witness.features.counts.iter().filter(|&&x| x > 0).count() > 1,
            priority_inversion: report_endpoint(e.nomination).priority_inversion
                || report_endpoint(e.witness).priority_inversion,
            side_mask_hamming: hamming(
                &e.nomination.features.before_masks,
                &e.witness.features.before_masks,
            ) + hamming(
                &e.nomination.features.after_masks,
                &e.witness.features.after_masks,
            ),
            marker_mask_hamming: hamming(
                &e.nomination.features.marker_masks,
                &e.witness.features.marker_masks,
            ),
            same_field: e.nomination.features.same_field && e.witness.features.same_field,
            nomination: report_endpoint(e.nomination),
            witness: report_endpoint(e.witness),
        };
        if bad {
            *route_counts.entry(p.label().to_owned()).or_insert(0) += 1;
            *concentration
                .by_candidate
                .entry(CANDIDATES[e.candidate].id.to_owned())
                .or_insert(0) += 1;
            *concentration
                .by_route_path
                .entry(p.label().to_owned())
                .or_insert(0) += 1;
            *concentration
                .by_phenotype
                .entry(r.map(family_name).unwrap_or("abstain").to_owned())
                .or_insert(0) += 1;
            *concentration
                .by_signature
                .entry(format!(
                    "{:?}->{:?}",
                    e.nomination.features.counts, e.witness.features.counts
                ))
                .or_insert(0) += 1;
            invalid.push(ep);
        } else if r.is_some() {
            valid.push(ep);
        }
    }
    let mut controls = Vec::new();
    for bad in &invalid {
        if let Some(v) = valid
            .iter()
            .find(|v| v.candidate == bad.candidate && v.route_path == bad.route_path)
        {
            controls.push(ControlPair {
                invalid_key: bad.key.clone(),
                valid_key: v.key.clone(),
                route_path: bad.route_path,
                candidate: bad.candidate,
            });
        }
    }
    let mut parity = Parity::default();
    let mut first_mismatch = None;
    for e in episodes.iter().copied() {
        let k = key(e);
        if let Some(row) = k2.get(&k) {
            parity.p1k2_matching_keys += 1;
            let n = row
                .get("nomination")
                .and_then(|v| v.get("counts"))
                .and_then(Value::as_array);
            let w = row
                .get("witness")
                .and_then(|v| v.get("counts"))
                .and_then(Value::as_array);
            let same = n.map(|a| {
                a.iter()
                    .filter_map(Value::as_u64)
                    .map(|x| x as u16)
                    .collect::<Vec<_>>()
            }) == Some(e.nomination.features.counts.to_vec())
                && w.map(|a| {
                    a.iter()
                        .filter_map(Value::as_u64)
                        .map(|x| x as u16)
                        .collect::<Vec<_>>()
                }) == Some(e.witness.features.counts.to_vec());
            if same {
                parity.p1k2_count_matches += 1;
            } else if first_mismatch.is_none() {
                first_mismatch = Some(format!("P1K2_COUNTS:{}", k));
            }
            let expected = CANDIDATES[e.candidate].expected_phi;
            let plurality = row
                .get("plurality_outcome")
                .and_then(Value::as_str)
                .unwrap_or("");
            let ours = if route(e.nomination.features, e.witness.features).is_none() {
                "ABSTAIN_TIE"
            } else if expected < 0
                || route(e.nomination.features, e.witness.features) == Some(expected as usize)
            {
                "VALID_ACTIONABLE"
            } else {
                "INVALID_ACTIONABLE"
            };
            if plurality == ours {
                parity.p1k2_plurality_outcome_matches += 1;
            } else if first_mismatch.is_none() {
                first_mismatch = Some(format!("P1K2_ROUTE:{}", k));
            }
        }
        if let Some(row) = k4.get(&k) {
            parity.p1k4_matching_keys += 1;
            let ours = route(e.nomination.features, e.witness.features)
                .map(family_name)
                .unwrap_or("ABSTAIN");
            let theirs = row.get("route").and_then(Value::as_str).unwrap_or("");
            if ours == theirs {
                parity.p1k4_route_matches += 1;
            } else if first_mismatch.is_none() {
                first_mismatch = Some(format!("P1K4_ROUTE:{}", k));
            }
        }
    }
    parity.p1k4_invalid_key_overlap = invalid
        .iter()
        .filter(|row| k4.contains_key(&row.key))
        .count();
    parity.first_mismatch = first_mismatch;
    let la: B2Receipt = serde_json::from_slice(&fs::read(la2b)?)?;
    let b2 = la
        .arms
        .iter()
        .find(|a| a.arm_label.starts_with("B2_"))
        .context("B2 arm")?;
    let mut contaminated = Vec::new();
    let mut invalid_keys_by_comp: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for e in episodes.iter().copied() {
        let r = route(e.nomination.features, e.witness.features);
        if let Some(phi) = r {
            let expected = CANDIDATES[e.candidate].expected_phi;
            if expected >= 0 && phi != expected as usize {
                invalid_keys_by_comp
                    .entry((
                        CANDIDATES[e.candidate].id.to_owned(),
                        family_name(phi).to_owned(),
                    ))
                    .or_default()
                    .push(key(e));
            }
        }
    }
    for s in &b2.states {
        let expected = CANDIDATES
            .iter()
            .find(|c| c.id == s.candidate)
            .map(|c| c.expected_phi)
            .unwrap_or(-2);
        let phi = CANDIDATES
            .iter()
            .position(|c| c.id == s.candidate)
            .map(|_| match s.phenotype.as_str() {
                "finance" => 0,
                "geography" => 1,
                "transport" => 2,
                _ => 3,
            })
            .unwrap_or(3);
        if s.a_plus > 0.01 && expected >= 0 && expected != phi as i8 {
            contaminated.push(AuthorityCompartment {
                candidate: s.candidate.clone(),
                phenotype: s.phenotype.clone(),
                a_plus: s.a_plus,
                a_minus: s.a_minus,
                nominations: s.nominations,
                owned_witnesses: s.owned_witnesses,
                causal_episodes: invalid_keys_by_comp
                    .remove(&(s.candidate.clone(), s.phenotype.clone()))
                    .unwrap_or_default(),
            });
        }
    }
    let invalid_count = invalid.len();
    let contaminated_count = contaminated.len();
    let receipt=Receipt { schema:SCHEMA, scope:"LA2-B discovery-boundary integration anatomy; no routing, credit, authority, ranking, or serving changes", protocol:"LT9-LA2-P1L1 frozen full-replay failure anatomy and P1K4 parity audit", source_corpus:corpus.display().to_string(), source_sha256:source_sha, document_count:docs, event_count:events.len(), episode_count:episodes.len(), invalid_routed_episodes:invalid_count, invalid_authority_compartments:contaminated_count, route_path_counts_invalid:route_counts, invalid_concentration:concentration, invalid_reports:invalid, matched_valid_controls:controls, contaminated_compartments:contaminated, parity, discovery_boundary:"LA2-B HotpotQA integration is frozen discovery evidence; no qualification or repair is authorized by this receipt", conclusion:"P1L1_COMPLETE: full B2 invalid-route anatomy and P1K4 parity audit; next action depends on first failure mechanism" };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    println!("P1L1 documents={} events={} episodes={} invalid={} contaminated={} parity-p1k2={} parity-p1k4={}",docs,events.len(),episodes.len(),invalid_count,contaminated_count,receipt.parity.p1k2_matching_keys,receipt.parity.p1k4_matching_keys);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tie_routes_only_unique_agreement() {
        let a = ContextFeatures {
            counts: [0, 2, 0],
            ..Default::default()
        };
        let b = ContextFeatures {
            counts: [0, 1, 1],
            ..Default::default()
        };
        assert_eq!(route(a, b), Some(1));
        assert_eq!(path(a, b, Some(1)), RoutePath::TieResolved);
    }
    #[test]
    fn unique_conflict_abstains() {
        let a = ContextFeatures {
            counts: [0, 0, 2],
            ..Default::default()
        };
        let b = ContextFeatures {
            counts: [2, 0, 0],
            ..Default::default()
        };
        assert_eq!(route(a, b), None);
        assert_eq!(path(a, b, None), RoutePath::UniqueConflictAbstained);
    }
    #[test]
    fn side_hamming_is_bitwise() {
        let a = [1u32, 0, 4];
        let b = [3u32, 0, 4];
        assert_eq!(hamming(&a, &b), 1);
    }
}
