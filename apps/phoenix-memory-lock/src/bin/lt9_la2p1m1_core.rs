//! Frozen natural-text event and credit replay primitives for P1M1.
//! This module adds marker-identity accounting but preserves the P1L2/LA2-B
//! tokenizer, nearest-pair, context-window, episode, router, and credit rules.

use anyhow::{ensure, Context, Result};
use memchr::memchr_iter;
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

pub const MAX_PAIR_DISTANCE: usize = 24;
pub const SUPPORT_DISTANCE: usize = 8;
pub const PHENOTYPES: usize = 4;
pub const LAMBDA: f32 = 0.85;
pub const MIN_CONFIDENCE: f32 = 0.01;
pub const DEADLINE: u64 = 32;
pub const PENDING_CAPACITY: usize = 7;
pub const MAX_MARKERS: usize = 17;

pub const FINANCE: &[&str] = &[
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
pub const GEO: &[&str] = &[
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
pub const TRANSPORT: &[&str] = &[
    "car", "vehicle", "engine", "motor", "auto", "driver", "road", "truck", "tire", "traffic",
];
pub const SUPPORT: &[&str] = &[
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
pub const NEGATIVE: &[&str] = &[
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

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub id: &'static str,
    pub source: &'static str,
    pub target: &'static str,
    pub expected_phi: i8,
}

pub const CANDIDATES: [Candidate; 12] = [
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
struct CorpusDoc<'a> {
    #[serde(default, borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct FamilyFeatures {
    pub raw_count: u16,
    pub distinct_mask: u32,
    pub marker_occurrences: [u16; MAX_MARKERS],
    pub before_occurrences: [u16; MAX_MARKERS],
    pub between_occurrences: [u16; MAX_MARKERS],
    pub after_occurrences: [u16; MAX_MARKERS],
}

impl FamilyFeatures {
    pub fn distinct_count(self) -> u16 {
        self.distinct_mask.count_ones() as u16
    }
    pub fn repeated_surplus(self) -> u16 {
        self.raw_count.saturating_sub(self.distinct_count())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Features {
    pub family: [FamilyFeatures; 3],
    pub distance: u16,
    pub support: bool,
    pub negative: bool,
    pub same_field: bool,
}

impl Features {
    pub fn counts(self, distinct_vote: bool) -> [u16; 3] {
        std::array::from_fn(|i| {
            if distinct_vote {
                self.family[i].distinct_count()
            } else {
                self.family[i].raw_count
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Event {
    pub doc: u64,
    pub candidate: usize,
    pub features: Features,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Episode {
    pub candidate: usize,
    pub nomination: Event,
    pub witness: Event,
}

pub fn family_name(phi: usize) -> &'static str {
    match phi {
        0 => "finance",
        1 => "geography",
        2 => "transport",
        _ => "general",
    }
}

pub fn marker_names(family: usize) -> &'static [&'static str] {
    match family {
        0 => FINANCE,
        1 => GEO,
        _ => TRANSPORT,
    }
}

fn positions(ws: &[&str], needle: &str) -> Vec<usize> {
    ws.iter()
        .enumerate()
        .filter_map(|(i, w)| w.eq_ignore_ascii_case(needle).then_some(i))
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

fn in_any(word: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| word.eq_ignore_ascii_case(m))
}

fn push_words<'a>(text: &'a str, output: &mut Vec<&'a str>) {
    output.extend(
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty()),
    );
}

fn family_features(
    ws: &[&str],
    low: usize,
    left: usize,
    right: usize,
    high: usize,
    markers: &[&str],
) -> FamilyFeatures {
    let mut f = FamilyFeatures::default();
    for (i, marker) in markers.iter().enumerate() {
        let mut before = 0u16;
        let mut between = 0u16;
        let mut after = 0u16;
        for word in &ws[low..left] {
            before += u16::from(word.eq_ignore_ascii_case(marker));
        }
        for word in &ws[left..=right] {
            between += u16::from(word.eq_ignore_ascii_case(marker));
        }
        for word in &ws[right + 1..high] {
            after += u16::from(word.eq_ignore_ascii_case(marker));
        }
        let total = before.saturating_add(between).saturating_add(after);
        f.before_occurrences[i] = before;
        f.between_occurrences[i] = between;
        f.after_occurrences[i] = after;
        f.marker_occurrences[i] = total;
        if total > 0 {
            f.distinct_mask |= 1u32 << i;
        }
        f.raw_count = f.raw_count.saturating_add(total);
    }
    f
}

fn near_any(ws: &[&str], left: usize, right: usize, markers: &[&str]) -> bool {
    let low = left.min(right).saturating_sub(8);
    let high = (left.max(right) + 9).min(ws.len());
    ws[low..high].iter().any(|w| in_any(w, markers))
}

fn context(ws: &[&str], left: usize, right: usize, distance: usize, title_len: usize) -> Features {
    let first = left.min(right);
    let last = left.max(right);
    let low = first.saturating_sub(8);
    let high = (last + 9).min(ws.len());
    Features {
        family: [
            family_features(ws, low, first, last, high, FINANCE),
            family_features(ws, low, first, last, high, GEO),
            family_features(ws, low, first, last, high, TRANSPORT),
        ],
        distance: distance as u16,
        support: near_any(ws, left, right, SUPPORT),
        negative: near_any(ws, left, right, NEGATIVE),
        same_field: (left < title_len) == (right < title_len),
    }
}

pub fn load_events(path: &Path) -> Result<(Vec<Event>, u64, String)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    // The mapped corpus stays read-only; newline offsets and token slices borrow
    // it, and one scratch vector is reused for each document.
    let mmap = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("mmap {}", path.display()))?;
    let corpus_sha256 = format!("{:x}", Sha256::digest(&mmap));
    let mut docs = 0u64;
    let mut events = Vec::new();
    let mut start = 0usize;
    for end in memchr_iter(b'\n', mmap.as_ref()).chain(std::iter::once(mmap.len())) {
        let line = &mmap[start..end];
        start = end.saturating_add(1);
        if !std::str::from_utf8(line)?.trim().is_empty() {
            let doc: CorpusDoc<'_> =
                serde_json::from_slice(line).context("decode corpus JSONL record")?;
            let title = doc.title.as_deref().unwrap_or("");
            let text = doc.text.as_deref().unwrap_or("");
            let mut ws = Vec::with_capacity((title.len() + text.len()) / 8);
            push_words(title, &mut ws);
            let title_len = ws.len();
            push_words(text, &mut ws);
            for (candidate, spec) in CANDIDATES.iter().enumerate() {
                if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                    if distance <= MAX_PAIR_DISTANCE {
                        events.push(Event {
                            doc: docs,
                            candidate,
                            features: context(&ws, left, right, distance, title_len),
                        });
                    }
                }
            }
            docs += 1;
        }
    }
    ensure!(docs > 0, "corpus is empty");
    Ok((events, docs, corpus_sha256))
}

pub fn make_episodes(events: &[Event]) -> Vec<Episode> {
    let mut streams = vec![Vec::<Event>::new(); CANDIDATES.len()];
    for event in events {
        streams[event.candidate].push(*event);
    }
    let mut out = Vec::new();
    for (candidate, stream) in streams.iter().enumerate() {
        let mut i = 0usize;
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

fn unique_winner(counts: [u16; 3]) -> Option<usize> {
    let max = counts.iter().copied().max()?;
    let mut winners = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == max);
    let winner = winners.next()?.0;
    winners.next().is_none().then_some(winner)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TiePolicy {
    HardAbstain,
    ExclusiveOnly,
}

pub fn exclusive_family(counts: [u16; 3]) -> Option<usize> {
    let mut family = None;
    for (index, count) in counts.into_iter().enumerate() {
        if count == 0 {
            continue;
        }
        if family.replace(index).is_some() {
            return None;
        }
    }
    family
}

pub fn qualified_pair_route(
    a: Features,
    b: Features,
    distinct_vote: bool,
    tie_policy: TiePolicy,
) -> Option<usize> {
    if tie_policy == TiePolicy::ExclusiveOnly {
        let left = exclusive_family(a.counts(distinct_vote))?;
        let right = exclusive_family(b.counts(distinct_vote))?;
        return (left == right).then_some(left);
    }
    let left = unique_winner(a.counts(distinct_vote))?;
    let right = unique_winner(b.counts(distinct_vote))?;
    (left == right).then_some(left)
}

pub fn witness_polarity(event: Event) -> Option<Polarity> {
    if event.features.negative {
        Some(Polarity::Contradiction)
    } else if event.features.support || usize::from(event.features.distance) <= SUPPORT_DISTANCE {
        Some(Polarity::Support)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Polarity {
    Support,
    Contradiction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ExpiryCause {
    Deadline,
    SupersededByOpposite,
    CapacityEviction,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pending {
    active: bool,
    created: u64,
    confidence: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    a_plus: f32,
    a_minus: f32,
    e_plus: f32,
    e_minus: f32,
    p_plus: Pending,
    p_minus: Pending,
    last_closed_plus: Option<ExpiryCause>,
    last_closed_minus: Option<ExpiryCause>,
    nominations: u32,
    owned_witnesses: u32,
    stale_rejected: u32,
}

#[derive(Default, Debug, Serialize)]
pub struct ReplaySummary {
    pub routed_episodes: usize,
    pub abstained_episodes: usize,
    pub invalid_routed_episodes: usize,
    pub valid_routed_episodes: usize,
    pub nominations: usize,
    pub owned_witnesses: usize,
    pub ignored_witnesses: usize,
    pub polarity_errors: usize,
    pub stale_credit_rejections: usize,
    pub deadline_expiries: usize,
    pub supersessions: usize,
    pub capacity_evictions: usize,
    pub pending_capacity_violations: usize,
    pub pending_peak: usize,
    pub unresolved_final: usize,
    pub plus_updates: usize,
    pub minus_updates: usize,
    pub positive_authority_mass: f64,
    pub negative_authority_mass: f64,
    pub positive_updates_by_phenotype: [usize; PHENOTYPES],
    pub negative_updates_by_phenotype: [usize; PHENOTYPES],
    pub invalid_authority_compartments: Vec<String>,
    pub positive_invalid_authority_cells: Vec<String>,
}

fn pow_decay(value: &mut f32) {
    *value *= LAMBDA;
}

fn pending_count(states: &[EdgeState]) -> usize {
    states
        .iter()
        .map(|s| usize::from(s.p_plus.active) + usize::from(s.p_minus.active))
        .sum()
}

fn close_pending(
    state: &mut EdgeState,
    polarity: Polarity,
    cause: ExpiryCause,
    summary: &mut ReplaySummary,
) {
    let pending = match polarity {
        Polarity::Support => &mut state.p_plus,
        Polarity::Contradiction => &mut state.p_minus,
    };
    if pending.active {
        pending.active = false;
        match cause {
            ExpiryCause::Deadline => summary.deadline_expiries += 1,
            ExpiryCause::SupersededByOpposite => summary.supersessions += 1,
            ExpiryCause::CapacityEviction => summary.capacity_evictions += 1,
        }
        match polarity {
            Polarity::Support => state.last_closed_plus = Some(cause),
            Polarity::Contradiction => state.last_closed_minus = Some(cause),
        }
    }
}

fn expire_and_decay(states: &mut [EdgeState], tick: u64, summary: &mut ReplaySummary) {
    for state in states {
        pow_decay(&mut state.e_plus);
        pow_decay(&mut state.e_minus);
        if state.p_plus.active && tick.saturating_sub(state.p_plus.created) > DEADLINE {
            close_pending(state, Polarity::Support, ExpiryCause::Deadline, summary);
        }
        if state.p_minus.active && tick.saturating_sub(state.p_minus.created) > DEADLINE {
            close_pending(
                state,
                Polarity::Contradiction,
                ExpiryCause::Deadline,
                summary,
            );
        }
    }
}

fn enforce_capacity(states: &mut [EdgeState], summary: &mut ReplaySummary) {
    while pending_count(states) > PENDING_CAPACITY {
        let mut oldest: Option<(usize, Polarity, u64)> = None;
        for (i, state) in states.iter().enumerate() {
            if state.p_plus.active && oldest.is_none_or(|old| state.p_plus.created < old.2) {
                oldest = Some((i, Polarity::Support, state.p_plus.created));
            }
            if state.p_minus.active && oldest.is_none_or(|old| state.p_minus.created < old.2) {
                oldest = Some((i, Polarity::Contradiction, state.p_minus.created));
            }
        }
        if let Some((i, polarity, _)) = oldest {
            close_pending(
                &mut states[i],
                polarity,
                ExpiryCause::CapacityEviction,
                summary,
            );
        } else {
            break;
        }
    }
    let n = pending_count(states);
    summary.pending_peak = summary.pending_peak.max(n);
    if n > PENDING_CAPACITY {
        summary.pending_capacity_violations += 1;
    }
}

fn update_state(
    states: &mut [EdgeState],
    candidate: usize,
    phi: usize,
    episode: Episode,
    tick: u64,
    summary: &mut ReplaySummary,
) {
    let key = candidate * PHENOTYPES + phi;
    summary.nominations += 1;
    let nomination_polarity = witness_polarity(episode.nomination).unwrap_or(Polarity::Support);
    {
        let state = &mut states[key];
        state.nominations += 1;
        match nomination_polarity {
            Polarity::Support => {
                if state.p_minus.active {
                    close_pending(
                        state,
                        Polarity::Contradiction,
                        ExpiryCause::SupersededByOpposite,
                        summary,
                    );
                }
                state.e_plus = state.e_plus.max(1.0);
                state.p_plus = Pending {
                    active: true,
                    created: tick,
                    confidence: 1.0,
                };
            }
            Polarity::Contradiction => {
                if state.p_plus.active {
                    close_pending(
                        state,
                        Polarity::Support,
                        ExpiryCause::SupersededByOpposite,
                        summary,
                    );
                }
                state.e_minus = state.e_minus.max(1.0);
                state.p_minus = Pending {
                    active: true,
                    created: tick,
                    confidence: 1.0,
                };
            }
        }
    }
    enforce_capacity(states, summary);
    let Some(polarity) = witness_polarity(episode.witness) else {
        summary.ignored_witnesses += 1;
        return;
    };
    let state = &mut states[key];
    let (pending, trace, closed) = match polarity {
        Polarity::Support => (
            &mut state.p_plus,
            &mut state.e_plus,
            &mut state.last_closed_plus,
        ),
        Polarity::Contradiction => (
            &mut state.p_minus,
            &mut state.e_minus,
            &mut state.last_closed_minus,
        ),
    };
    if pending.active && *trace >= MIN_CONFIDENCE {
        let authority = (*trace).max(pending.confidence);
        match polarity {
            Polarity::Support => {
                state.a_plus += authority;
                summary.plus_updates += 1;
                summary.positive_authority_mass += f64::from(authority);
                summary.positive_updates_by_phenotype[phi] += 1;
            }
            Polarity::Contradiction => {
                state.a_minus += authority;
                summary.minus_updates += 1;
                summary.negative_authority_mass += f64::from(authority);
                summary.negative_updates_by_phenotype[phi] += 1;
            }
        }
        state.owned_witnesses += 1;
        summary.owned_witnesses += 1;
        pending.active = false;
        *trace = 0.0;
        *closed = None;
    } else {
        if pending.active {
            summary.polarity_errors += 1;
        } else if closed.is_some() {
            summary.stale_credit_rejections += 1;
            state.stale_rejected += 1;
        }
        summary.ignored_witnesses += 1;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CreditStep {
    pub episode_index: usize,
    pub route: Option<usize>,
    pub witness_polarity: Option<&'static str>,
    pub owned_witness: bool,
}

fn polarity_name(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Support => "SUPPORT",
        Polarity::Contradiction => "CONTRADICTION",
    }
}

pub fn run_credit_replay_traced(
    episodes: &[Episode],
    distinct_vote: bool,
    tie_policy: TiePolicy,
) -> (ReplaySummary, Vec<CreditStep>) {
    let mut states = vec![EdgeState::default(); CANDIDATES.len() * PHENOTYPES];
    let mut summary = ReplaySummary::default();
    let mut trace = Vec::with_capacity(episodes.len());
    for (tick, episode) in episodes.iter().copied().enumerate() {
        let route = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            distinct_vote,
            tie_policy,
        );
        let polarity = witness_polarity(episode.witness).map(polarity_name);
        match route {
            None => {
                summary.abstained_episodes += 1;
                trace.push(CreditStep {
                    episode_index: tick,
                    route: None,
                    witness_polarity: polarity,
                    owned_witness: false,
                });
            }
            Some(phi) => {
                summary.routed_episodes += 1;
                if CANDIDATES[episode.candidate].expected_phi >= 0
                    && CANDIDATES[episode.candidate].expected_phi != phi as i8
                {
                    summary.invalid_routed_episodes += 1;
                } else {
                    summary.valid_routed_episodes += 1;
                }
                let owned_before = summary.owned_witnesses;
                expire_and_decay(&mut states, tick as u64, &mut summary);
                update_state(
                    &mut states,
                    episode.candidate,
                    phi,
                    episode,
                    tick as u64,
                    &mut summary,
                );
                trace.push(CreditStep {
                    episode_index: tick,
                    route: Some(phi),
                    witness_polarity: polarity,
                    owned_witness: summary.owned_witnesses > owned_before,
                });
            }
        }
    }
    summary.unresolved_final = pending_count(&states);
    for (index, state) in states.iter().enumerate() {
        if state.a_plus > MIN_CONFIDENCE {
            let candidate = index / PHENOTYPES;
            let phi = index % PHENOTYPES;
            let key = format!("{}@{}", CANDIDATES[candidate].id, family_name(phi));
            if CANDIDATES[candidate].expected_phi < 0
                || CANDIDATES[candidate].expected_phi == phi as i8
            {
                continue;
            }
            summary.invalid_authority_compartments.push(key.clone());
            summary.positive_invalid_authority_cells.push(key);
        }
    }
    summary.invalid_authority_compartments.sort();
    summary.positive_invalid_authority_cells.sort();
    (summary, trace)
}

#[cfg(test)]
#[path = "lt9_la2p1m1_core_tests.rs"]
mod tests;
