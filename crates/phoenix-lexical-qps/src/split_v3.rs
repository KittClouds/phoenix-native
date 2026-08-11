use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use crate::{JudgmentIdentity, JudgmentReasonV3, KeyedIdentity, RelevanceLedgerV3};

pub const LEAKAGE_SPLIT_V3_CONTRACT: &str = "phoenix.qps.leakage-split/v3";
pub const LEAKAGE_SPLIT_V3_SCHEMA_VERSION: u16 = 5;
pub const SPLIT_RATIO_TOLERANCE_BPS: u16 = 100;
const MAJOR_FAILURE_CLASS_COUNT: usize = 11;
const FUTURE_HOLDOUT_TARGET_BPS: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimarySplitV3 {
    Training,
    Development,
    BlindTest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JudgmentSplitV3 {
    pub judgment_identity: JudgmentIdentity,
    pub primary_split: PrimarySplitV3,
    pub future_time_holdout: bool,
    pub unseen_source_holdout: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeakageSplitV3 {
    pub contract: String,
    pub schema_version: u16,
    pub assignments: Box<[JudgmentSplitV3]>,
    pub audit: LeakageSplitAuditV3,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LeakageSplitAuditV3 {
    pub eligible_judgments: usize,
    pub connected_components: usize,
    pub training_judgments: usize,
    pub development_judgments: usize,
    pub blind_test_judgments: usize,
    pub training_ratio_bps: u16,
    pub development_ratio_bps: u16,
    pub blind_test_ratio_bps: u16,
    pub maximum_ratio_deviation_bps: u16,
    pub largest_atomic_component_judgments: usize,
    pub nearest_feasible_ratio_tolerance_bps: u16,
    pub query_family_leaks: usize,
    pub source_leaks: usize,
    pub near_duplicate_cluster_leaks: usize,
    pub entity_or_identifier_family_leaks: usize,
    pub collection_cohort_leaks: usize,
    pub future_time_ordering_violations: usize,
    #[serde(default)]
    pub future_time_holdout_judgments: usize,
    #[serde(default)]
    pub unseen_source_holdout_judgments: usize,
    #[serde(default)]
    pub training_major_classes_missing: usize,
    #[serde(default)]
    pub development_major_classes_missing: usize,
    #[serde(default)]
    pub blind_test_major_classes_missing: usize,
    pub frozen_holdout_training_assignments: usize,
    pub duplicate_or_missing_assignments: usize,
}

impl LeakageSplitAuditV3 {
    pub fn is_qualified(self) -> bool {
        self.eligible_judgments > 0
            && self.training_judgments > 0
            && self.development_judgments > 0
            && self.blind_test_judgments > 0
            && self.maximum_ratio_deviation_bps <= self.nearest_feasible_ratio_tolerance_bps
            && self.query_family_leaks == 0
            && self.source_leaks == 0
            && self.near_duplicate_cluster_leaks == 0
            && self.entity_or_identifier_family_leaks == 0
            && self.collection_cohort_leaks == 0
            && self.future_time_ordering_violations == 0
            && self.future_time_holdout_judgments > 0
            && self.unseen_source_holdout_judgments > 0
            && self.training_major_classes_missing == 0
            && self.development_major_classes_missing == 0
            && self.blind_test_major_classes_missing == 0
            && self.frozen_holdout_training_assignments == 0
            && self.duplicate_or_missing_assignments == 0
    }
}

impl LeakageSplitV3 {
    pub fn build(ledger: &RelevanceLedgerV3) -> Result<Self, &'static str> {
        ledger.validate()?;
        let eligible = ledger.active_model_training_indices();
        if eligible.len() < 3 {
            return Err("V3 leakage split requires at least three eligible judgments");
        }

        let mut sets = DisjointSets::new(eligible.len());
        let mut query_families = HashMap::with_capacity(eligible.len());
        let mut sources = HashMap::with_capacity(eligible.len() * 2);
        let mut near_duplicates = HashMap::with_capacity(eligible.len() * 2);
        let mut entity_families = HashMap::with_capacity(eligible.len());
        let mut collection_cohorts = HashMap::with_capacity(eligible.len());
        for (local, &ledger_index) in eligible.iter().enumerate() {
            let groups = ledger.judgments[ledger_index].split_groups;
            union_group(
                &mut sets,
                &mut query_families,
                groups.query_family_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut sources,
                groups.positive_source_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut sources,
                groups.negative_source_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut near_duplicates,
                groups.positive_near_duplicate_cluster_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut near_duplicates,
                groups.negative_near_duplicate_cluster_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut entity_families,
                groups.entity_or_identifier_family_identity,
                local,
            );
            union_group(
                &mut sets,
                &mut collection_cohorts,
                groups.collection_cohort_identity,
                local,
            );
        }

        let mut by_root = HashMap::<usize, Vec<usize>>::new();
        for local in 0..eligible.len() {
            let root = sets.find(local);
            by_root.entry(root).or_default().push(local);
        }
        if by_root.len() < 3 {
            return Err("group constraints leave fewer than three V3 split components");
        }
        let mut components = by_root
            .into_values()
            .map(|members| Component::new(members, &eligible, ledger))
            .collect::<Vec<_>>();
        components.sort_unstable_by(|left, right| {
            left.min_time
                .cmp(&right.min_time)
                .then_with(|| left.max_time.cmp(&right.max_time))
                .then_with(|| left.identity.cmp(&right.identity))
        });

        let future_start = future_holdout_boundary(&components, eligible.len());
        let mut future_components = vec![false; components.len()];
        future_components[future_start..].fill(true);
        let component_splits = assign_primary_splits(&components, future_start, eligible.len());
        let max_non_blind_time = components
            .iter()
            .zip(&component_splits)
            .filter(|(_, split)| **split != PrimarySplitV3::BlindTest)
            .map(|(component, _)| component.max_time)
            .max()
            .unwrap_or(0);
        let mut local_splits = vec![PrimarySplitV3::Training; eligible.len()];
        let mut local_future = vec![false; eligible.len()];
        for (component_index, component) in components.iter().enumerate() {
            let split = component_splits[component_index];
            for &local in &component.members {
                local_splits[local] = split;
                local_future[local] = future_components[component_index]
                    && ledger.judgments[eligible[local]]
                        .split_groups
                        .collected_at_unix_seconds
                        >= max_non_blind_time;
            }
        }

        let mut assignments = eligible
            .iter()
            .enumerate()
            .map(|(local, &ledger_index)| {
                let primary_split = local_splits[local];
                JudgmentSplitV3 {
                    judgment_identity: ledger.judgments[ledger_index].identity,
                    primary_split,
                    future_time_holdout: local_future[local],
                    unseen_source_holdout: primary_split == PrimarySplitV3::BlindTest,
                }
            })
            .collect::<Vec<_>>();
        assignments.sort_unstable_by_key(|assignment| assignment.judgment_identity);
        let largest_atomic_component = components
            .iter()
            .map(|component| component.members.len())
            .max()
            .unwrap_or(0);
        let audit = audit_split(
            ledger,
            &assignments,
            components.len(),
            largest_atomic_component,
        );
        Ok(Self {
            contract: LEAKAGE_SPLIT_V3_CONTRACT.to_owned(),
            schema_version: LEAKAGE_SPLIT_V3_SCHEMA_VERSION,
            assignments: assignments.into_boxed_slice(),
            audit,
        })
    }
}

struct Component {
    members: Vec<usize>,
    min_time: u64,
    max_time: u64,
    identity: [u8; 32],
    reason_counts: [usize; MAJOR_FAILURE_CLASS_COUNT],
}

impl Component {
    fn new(members: Vec<usize>, eligible: &[usize], ledger: &RelevanceLedgerV3) -> Self {
        let mut identities = members
            .iter()
            .map(|&local| ledger.judgments[eligible[local]].identity)
            .collect::<Vec<_>>();
        identities.sort_unstable();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix-qps-v3-split-component\0");
        for identity in identities {
            hasher.update(&identity.as_bytes());
        }
        let min_time = members
            .iter()
            .map(|&local| {
                ledger.judgments[eligible[local]]
                    .split_groups
                    .collected_at_unix_seconds
            })
            .min()
            .unwrap_or(0);
        let max_time = members
            .iter()
            .map(|&local| {
                ledger.judgments[eligible[local]]
                    .split_groups
                    .collected_at_unix_seconds
            })
            .max()
            .unwrap_or(0);
        let mut reason_counts = [0; MAJOR_FAILURE_CLASS_COUNT];
        for &local in &members {
            if let Some(index) = major_reason_index(ledger.judgments[eligible[local]].reason) {
                reason_counts[index] += 1;
            }
        }
        Self {
            members,
            min_time,
            max_time,
            identity: *hasher.finalize().as_bytes(),
            reason_counts,
        }
    }
}

fn component_prefix(components: &[Component]) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(components.len() + 1);
    prefix.push(0);
    for component in components {
        prefix.push(prefix.last().copied().unwrap_or(0) + component.members.len());
    }
    prefix
}

fn future_holdout_boundary(components: &[Component], total: usize) -> usize {
    let prefix = component_prefix(components);
    (1..components.len())
        .min_by_key(|&boundary| {
            let future = total.saturating_sub(prefix[boundary]);
            (future * 10_000).abs_diff(total * FUTURE_HOLDOUT_TARGET_BPS)
        })
        .unwrap_or(components.len() - 1)
}

fn assign_primary_splits(
    components: &[Component],
    future_start: usize,
    total: usize,
) -> Vec<PrimarySplitV3> {
    let mut assignments = vec![PrimarySplitV3::Training; components.len()];
    let mut counts = [0_usize; 3];
    let mut reasons = [[0_usize; MAJOR_FAILURE_CLASS_COUNT]; 3];
    for index in future_start..components.len() {
        assignments[index] = PrimarySplitV3::BlindTest;
        record_component(&components[index], 2, &mut counts, &mut reasons);
    }

    let mut remaining = (0..future_start).collect::<Vec<_>>();
    remaining.sort_unstable_by(|&left, &right| {
        components[right]
            .members
            .len()
            .cmp(&components[left].members.len())
            .then_with(|| components[left].identity.cmp(&components[right].identity))
    });
    let reason_totals = components.iter().fold(
        [0_usize; MAJOR_FAILURE_CLASS_COUNT],
        |mut totals, component| {
            for (total, count) in totals.iter_mut().zip(component.reason_counts) {
                *total += count;
            }
            totals
        },
    );
    for component_index in remaining {
        let component = &components[component_index];
        let split_index = (0..3)
            .min_by_key(|&candidate| {
                assignment_cost(component, candidate, counts, reasons, total, reason_totals)
            })
            .unwrap_or(0);
        assignments[component_index] = split_from_index(split_index);
        record_component(component, split_index, &mut counts, &mut reasons);
    }
    rebalance_primary_sizes(components, future_start, total, &mut assignments);
    rebalance_reason_coverage(components, future_start, total, &mut assignments);
    assignments
}

fn rebalance_reason_coverage(
    components: &[Component],
    future_start: usize,
    total: usize,
    assignments: &mut [PrimarySplitV3],
) {
    let largest_component = components
        .iter()
        .map(|component| component.members.len())
        .max()
        .unwrap_or(0);
    let tolerance_bps = split_ratio_tolerance_bps(largest_component, total);
    loop {
        let (counts, reasons) = assigned_counts(components, assignments);
        let current = coverage_cost(counts, reasons, components, total);
        let mut best_move = None::<(CoverageCost, usize, usize)>;
        for component_index in 0..future_start {
            let source = split_index(assignments[component_index]);
            for target in 0..3 {
                if target == source {
                    continue;
                }
                let mut candidate_counts = counts;
                let size = components[component_index].members.len();
                candidate_counts[source] -= size;
                candidate_counts[target] += size;
                if !ratios_within_tolerance(candidate_counts, total, tolerance_bps) {
                    continue;
                }
                let mut candidate_reasons = reasons;
                for (reason, count) in components[component_index]
                    .reason_counts
                    .into_iter()
                    .enumerate()
                {
                    candidate_reasons[source][reason] -= count;
                    candidate_reasons[target][reason] += count;
                }
                let cost = coverage_cost(candidate_counts, candidate_reasons, components, total);
                let proposal = (cost, component_index, target);
                if cost < current && best_move.is_none_or(|prior| proposal < prior) {
                    best_move = Some(proposal);
                }
            }
        }

        let mut best_swap = None::<(CoverageCost, usize, usize)>;
        for left in 0..future_start {
            for right in left + 1..future_start {
                let left_split = split_index(assignments[left]);
                let right_split = split_index(assignments[right]);
                if left_split == right_split {
                    continue;
                }
                let mut candidate_counts = counts;
                candidate_counts[left_split] = candidate_counts[left_split]
                    - components[left].members.len()
                    + components[right].members.len();
                candidate_counts[right_split] = candidate_counts[right_split]
                    - components[right].members.len()
                    + components[left].members.len();
                if !ratios_within_tolerance(candidate_counts, total, tolerance_bps) {
                    continue;
                }
                let mut candidate_reasons = reasons;
                let (left_reasons, right_reasons) = if left_split < right_split {
                    let (before, after) = candidate_reasons.split_at_mut(right_split);
                    (&mut before[left_split], &mut after[0])
                } else {
                    let (before, after) = candidate_reasons.split_at_mut(left_split);
                    (&mut after[0], &mut before[right_split])
                };
                for (((left_value, right_value), left_count), right_count) in left_reasons
                    .iter_mut()
                    .zip(right_reasons.iter_mut())
                    .zip(components[left].reason_counts.iter())
                    .zip(components[right].reason_counts.iter())
                {
                    *left_value = *left_value - *left_count + *right_count;
                    *right_value = *right_value - *right_count + *left_count;
                }
                let cost = coverage_cost(candidate_counts, candidate_reasons, components, total);
                let proposal = (cost, left, right);
                if cost < current && best_swap.is_none_or(|prior| proposal < prior) {
                    best_swap = Some(proposal);
                }
            }
        }

        match (best_move, best_swap) {
            (Some(movement), Some(swap)) if movement.0 <= swap.0 => {
                assignments[movement.1] = split_from_index(movement.2);
            }
            (Some(_), Some(swap)) | (None, Some(swap)) => {
                assignments.swap(swap.1, swap.2);
            }
            (Some(movement), None) => {
                assignments[movement.1] = split_from_index(movement.2);
            }
            (None, None) => break,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CoverageCost {
    missing_classes: usize,
    reason_error: u128,
    size_error: u128,
}

fn coverage_cost(
    counts: [usize; 3],
    reasons: [[usize; MAJOR_FAILURE_CLASS_COUNT]; 3],
    components: &[Component],
    total: usize,
) -> CoverageCost {
    CoverageCost {
        missing_classes: reasons
            .iter()
            .flatten()
            .filter(|&&count| count == 0)
            .count(),
        reason_error: primary_reason_cost(reasons, components),
        size_error: primary_size_cost(counts, total),
    }
}

fn ratios_within_tolerance(counts: [usize; 3], total: usize, tolerance_bps: u16) -> bool {
    counts
        .into_iter()
        .zip([6_000_u16, 2_000, 2_000])
        .all(|(count, target)| ratio_bps(count, total).abs_diff(target) <= tolerance_bps)
}

fn split_ratio_tolerance_bps(largest_component: usize, total: usize) -> u16 {
    let atomic_component_bps = largest_component.saturating_mul(10_000).div_ceil(total);
    SPLIT_RATIO_TOLERANCE_BPS
        .max(u16::try_from(atomic_component_bps.div_ceil(2)).unwrap_or(u16::MAX))
}

fn rebalance_primary_sizes(
    components: &[Component],
    future_start: usize,
    total: usize,
    assignments: &mut [PrimarySplitV3],
) {
    loop {
        let (counts, reasons) = assigned_counts(components, assignments);
        let current_size = primary_size_cost(counts, total);
        let mut best = None::<(u128, u128, usize, usize)>;
        for component_index in 0..future_start {
            let source = split_index(assignments[component_index]);
            for target in 0..3 {
                if target == source {
                    continue;
                }
                let mut candidate_counts = counts;
                let size = components[component_index].members.len();
                candidate_counts[source] -= size;
                candidate_counts[target] += size;
                let candidate_size = primary_size_cost(candidate_counts, total);
                if candidate_size >= current_size {
                    continue;
                }
                let mut candidate_reasons = reasons;
                for (reason, count) in components[component_index]
                    .reason_counts
                    .into_iter()
                    .enumerate()
                {
                    candidate_reasons[source][reason] -= count;
                    candidate_reasons[target][reason] += count;
                }
                let candidate_reason = primary_reason_cost(candidate_reasons, components);
                let proposal = (candidate_size, candidate_reason, component_index, target);
                if best.is_none_or(|prior| proposal < prior) {
                    best = Some(proposal);
                }
            }
        }
        let Some((_, _, component_index, target)) = best else {
            break;
        };
        assignments[component_index] = split_from_index(target);
    }
}

fn assigned_counts(
    components: &[Component],
    assignments: &[PrimarySplitV3],
) -> ([usize; 3], [[usize; MAJOR_FAILURE_CLASS_COUNT]; 3]) {
    let mut counts = [0; 3];
    let mut reasons = [[0; MAJOR_FAILURE_CLASS_COUNT]; 3];
    for (component, &split) in components.iter().zip(assignments) {
        record_component(component, split_index(split), &mut counts, &mut reasons);
    }
    (counts, reasons)
}

fn primary_size_cost(counts: [usize; 3], total: usize) -> u128 {
    counts
        .into_iter()
        .zip([6_000, 2_000, 2_000])
        .map(|(count, target)| squared_error(count, total, target))
        .sum()
}

fn primary_reason_cost(
    reasons: [[usize; MAJOR_FAILURE_CLASS_COUNT]; 3],
    components: &[Component],
) -> u128 {
    let totals = components.iter().fold(
        [0_usize; MAJOR_FAILURE_CLASS_COUNT],
        |mut totals, component| {
            for (total, count) in totals.iter_mut().zip(component.reason_counts) {
                *total += count;
            }
            totals
        },
    );
    reasons
        .into_iter()
        .zip([6_000, 2_000, 2_000])
        .map(|(split, target)| {
            split
                .into_iter()
                .zip(totals)
                .map(|(count, total)| squared_error(count, total, target))
                .sum::<u128>()
        })
        .sum()
}

fn assignment_cost(
    component: &Component,
    candidate: usize,
    mut counts: [usize; 3],
    mut reasons: [[usize; MAJOR_FAILURE_CLASS_COUNT]; 3],
    total: usize,
    reason_totals: [usize; MAJOR_FAILURE_CLASS_COUNT],
) -> u128 {
    record_component(component, candidate, &mut counts, &mut reasons);
    const TARGETS: [usize; 3] = [6_000, 2_000, 2_000];
    let size_cost = counts
        .into_iter()
        .zip(TARGETS)
        .map(|(count, target)| squared_error(count, total, target))
        .sum::<u128>();
    let reason_cost = reasons
        .into_iter()
        .zip(TARGETS)
        .map(|(split, target)| {
            split
                .into_iter()
                .zip(reason_totals)
                .map(|(count, reason_total)| squared_error(count, reason_total, target))
                .sum::<u128>()
        })
        .sum::<u128>();
    // Keep the 60/20/20 contract primary. Class error resolves assignments
    // among comparably sized choices without buying coverage by violating the
    // ratio gate.
    size_cost.saturating_mul(10_000) + reason_cost
}

fn squared_error(count: usize, total: usize, target_bps: usize) -> u128 {
    let difference = (count as u128 * 10_000).abs_diff(total as u128 * target_bps as u128);
    difference.saturating_mul(difference)
}

fn record_component(
    component: &Component,
    split: usize,
    counts: &mut [usize; 3],
    reasons: &mut [[usize; MAJOR_FAILURE_CLASS_COUNT]; 3],
) {
    counts[split] += component.members.len();
    for (target, count) in reasons[split].iter_mut().zip(component.reason_counts) {
        *target += count;
    }
}

const fn split_from_index(index: usize) -> PrimarySplitV3 {
    match index {
        0 => PrimarySplitV3::Training,
        1 => PrimarySplitV3::Development,
        _ => PrimarySplitV3::BlindTest,
    }
}

const fn split_index(split: PrimarySplitV3) -> usize {
    match split {
        PrimarySplitV3::Training => 0,
        PrimarySplitV3::Development => 1,
        PrimarySplitV3::BlindTest => 2,
    }
}

fn union_group(
    sets: &mut DisjointSets,
    owners: &mut HashMap<KeyedIdentity, usize>,
    identity: KeyedIdentity,
    local: usize,
) {
    if let Some(&prior) = owners.get(&identity) {
        sets.union(prior, local);
    } else {
        owners.insert(identity, local);
    }
}

const fn major_reason_index(reason: JudgmentReasonV3) -> Option<usize> {
    match reason {
        JudgmentReasonV3::PartialMatchSaturation => Some(0),
        JudgmentReasonV3::ScatteredTerms => Some(1),
        JudgmentReasonV3::PhraseOrderFailure => Some(2),
        JudgmentReasonV3::IdentifierCollision => Some(3),
        JudgmentReasonV3::FuzzyCollision => Some(4),
        JudgmentReasonV3::WeakFieldEvidence => Some(5),
        JudgmentReasonV3::CommonTermDominance => Some(6),
        JudgmentReasonV3::LengthPriorFailure => Some(7),
        JudgmentReasonV3::WrongConceptProximity => Some(8),
        JudgmentReasonV3::DocumentConversationConfusion => Some(9),
        JudgmentReasonV3::LongQueryFailure => Some(10),
        JudgmentReasonV3::RealUserCorrection => None,
    }
}

struct DisjointSets {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSets {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            rank: vec![0; len],
        }
    }

    fn find(&mut self, value: usize) -> usize {
        let parent = self.parent[value];
        if parent != value {
            self.parent[value] = self.find(parent);
        }
        self.parent[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parent[right] = left;
        if self.rank[left] == self.rank[right] {
            self.rank[left] += 1;
        }
    }
}

fn audit_split(
    ledger: &RelevanceLedgerV3,
    assignments: &[JudgmentSplitV3],
    connected_components: usize,
    largest_atomic_component_judgments: usize,
) -> LeakageSplitAuditV3 {
    let assignment_by_identity = assignments
        .iter()
        .map(|assignment| (assignment.judgment_identity, assignment))
        .collect::<HashMap<_, _>>();
    let by_identity = assignments
        .iter()
        .map(|assignment| (assignment.judgment_identity, assignment.primary_split))
        .collect::<HashMap<_, _>>();
    let eligible = ledger.active_model_training_judgments();
    let counts = split_counts(assignments);
    let ratios = [
        ratio_bps(counts[0], eligible.len()),
        ratio_bps(counts[1], eligible.len()),
        ratio_bps(counts[2], eligible.len()),
    ];
    let targets = [6_000_u16, 2_000, 2_000];
    let maximum_ratio_deviation_bps = ratios
        .into_iter()
        .zip(targets)
        .map(|(actual, target)| actual.abs_diff(target))
        .max()
        .unwrap_or(u16::MAX);
    let nearest_feasible_ratio_tolerance_bps =
        split_ratio_tolerance_bps(largest_atomic_component_judgments, eligible.len());
    let frozen_holdout_training_assignments = ledger
        .judgments
        .iter()
        .filter(|judgment| {
            judgment.frozen_holdout.is_some()
                && by_identity.get(&judgment.identity) == Some(&PrimarySplitV3::Training)
        })
        .count();
    let unique_assignments = assignments
        .iter()
        .map(|assignment| assignment.judgment_identity)
        .collect::<HashSet<_>>()
        .len();
    let duplicate_or_missing_assignments = assignments.len().abs_diff(unique_assignments)
        + eligible.len().abs_diff(unique_assignments);
    let max_non_future_time = eligible
        .iter()
        .filter(|judgment| {
            assignment_by_identity
                .get(&judgment.identity)
                .is_some_and(|assignment| assignment.primary_split != PrimarySplitV3::BlindTest)
        })
        .map(|judgment| judgment.split_groups.collected_at_unix_seconds)
        .max()
        .unwrap_or(0);
    let future_time_ordering_violations = eligible
        .iter()
        .filter(|judgment| {
            assignment_by_identity
                .get(&judgment.identity)
                .is_some_and(|assignment| assignment.future_time_holdout)
                && judgment.split_groups.collected_at_unix_seconds < max_non_future_time
        })
        .count();
    let future_time_holdout_judgments = assignments
        .iter()
        .filter(|assignment| assignment.future_time_holdout)
        .count();
    let unseen_source_holdout_judgments = assignments
        .iter()
        .filter(|assignment| assignment.unseen_source_holdout)
        .count();
    let missing_classes = major_class_missing_counts(&eligible, &by_identity);
    LeakageSplitAuditV3 {
        eligible_judgments: eligible.len(),
        connected_components,
        training_judgments: counts[0],
        development_judgments: counts[1],
        blind_test_judgments: counts[2],
        training_ratio_bps: ratios[0],
        development_ratio_bps: ratios[1],
        blind_test_ratio_bps: ratios[2],
        maximum_ratio_deviation_bps,
        largest_atomic_component_judgments,
        nearest_feasible_ratio_tolerance_bps,
        query_family_leaks: group_leaks(
            &eligible,
            &by_identity,
            |judgment| {
                [
                    judgment.split_groups.query_family_identity,
                    KeyedIdentity::from_bytes([0; 32]),
                ]
            },
            1,
        ),
        source_leaks: group_leaks(
            &eligible,
            &by_identity,
            |judgment| {
                [
                    judgment.split_groups.positive_source_identity,
                    judgment.split_groups.negative_source_identity,
                ]
            },
            2,
        ),
        near_duplicate_cluster_leaks: group_leaks(
            &eligible,
            &by_identity,
            |judgment| {
                [
                    judgment
                        .split_groups
                        .positive_near_duplicate_cluster_identity,
                    judgment
                        .split_groups
                        .negative_near_duplicate_cluster_identity,
                ]
            },
            2,
        ),
        entity_or_identifier_family_leaks: group_leaks(
            &eligible,
            &by_identity,
            |judgment| {
                [
                    judgment.split_groups.entity_or_identifier_family_identity,
                    KeyedIdentity::from_bytes([0; 32]),
                ]
            },
            1,
        ),
        collection_cohort_leaks: group_leaks(
            &eligible,
            &by_identity,
            |judgment| {
                [
                    judgment.split_groups.collection_cohort_identity,
                    KeyedIdentity::from_bytes([0; 32]),
                ]
            },
            1,
        ),
        future_time_ordering_violations,
        future_time_holdout_judgments,
        unseen_source_holdout_judgments,
        training_major_classes_missing: missing_classes[0],
        development_major_classes_missing: missing_classes[1],
        blind_test_major_classes_missing: missing_classes[2],
        frozen_holdout_training_assignments,
        duplicate_or_missing_assignments,
    }
}

fn major_class_missing_counts(
    judgments: &[&crate::PairwiseJudgmentV3],
    splits: &HashMap<JudgmentIdentity, PrimarySplitV3>,
) -> [usize; 3] {
    let mut present = [[false; MAJOR_FAILURE_CLASS_COUNT]; 3];
    for judgment in judgments {
        let (Some(&split), Some(reason)) = (
            splits.get(&judgment.identity),
            major_reason_index(judgment.reason),
        ) else {
            continue;
        };
        let split = match split {
            PrimarySplitV3::Training => 0,
            PrimarySplitV3::Development => 1,
            PrimarySplitV3::BlindTest => 2,
        };
        present[split][reason] = true;
    }
    present.map(|classes| classes.into_iter().filter(|value| !value).count())
}

fn group_leaks<F>(
    judgments: &[&crate::PairwiseJudgmentV3],
    splits: &HashMap<JudgmentIdentity, PrimarySplitV3>,
    identities: F,
    used: usize,
) -> usize
where
    F: Fn(&crate::PairwiseJudgmentV3) -> [KeyedIdentity; 2],
{
    let mut observed = HashMap::<KeyedIdentity, PrimarySplitV3>::new();
    let mut leaked = HashSet::new();
    for judgment in judgments {
        let Some(&split) = splits.get(&judgment.identity) else {
            continue;
        };
        for identity in identities(judgment).into_iter().take(used) {
            if observed
                .insert(identity, split)
                .is_some_and(|prior| prior != split)
            {
                leaked.insert(identity);
            }
        }
    }
    leaked.len()
}

fn split_counts(assignments: &[JudgmentSplitV3]) -> [usize; 3] {
    let mut counts = [0; 3];
    for assignment in assignments {
        counts[match assignment.primary_split {
            PrimarySplitV3::Training => 0,
            PrimarySplitV3::Development => 1,
            PrimarySplitV3::BlindTest => 2,
        }] += 1;
    }
    counts
}

fn ratio_bps(count: usize, total: usize) -> u16 {
    u16::try_from((count * 10_000 + total / 2) / total.max(1)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        JudgmentReasonV3, JudgmentSourceV3, PairwiseJudgmentDraftV3, PairwiseJudgmentV3,
        RankEvidenceV3, SplitGroupProvenanceV3, RANK_EVIDENCE_V3_SCHEMA_VERSION,
    };

    fn identity(value: u8) -> KeyedIdentity {
        KeyedIdentity::from_bytes([value; 32])
    }

    fn evidence() -> RankEvidenceV3 {
        RankEvidenceV3 {
            schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
            query_groups: 1,
            matched_groups: 1,
            missing_groups: 0,
            query_flags: 0,
            field_count: 1,
            values: [0.5; 30],
        }
    }

    fn ledger(count: u8) -> RelevanceLedgerV3 {
        let mut ledger = RelevanceLedgerV3::default();
        for value in 1..=count {
            let positive = identity(value.saturating_add(80));
            let negative = identity(value.saturating_add(120));
            let judgment = PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
                workspace_identity: identity(250),
                query_identity: identity(value),
                positive_document_version: positive,
                negative_document_version: negative,
                positive_features: evidence(),
                negative_features: evidence(),
                positive_tier: crate::RelevanceTier::CompleteExactGroups,
                negative_tier: crate::RelevanceTier::CompleteExactGroups,
                candidate_pool: vec![positive, negative].into_boxed_slice(),
                positive_position: 0,
                negative_position: 1,
                split_groups: SplitGroupProvenanceV3 {
                    query_family_identity: scoped_identity(value, 1),
                    positive_source_identity: scoped_identity(value, 2),
                    negative_source_identity: scoped_identity(value, 3),
                    positive_near_duplicate_cluster_identity: scoped_identity(value, 4),
                    negative_near_duplicate_cluster_identity: scoped_identity(value, 5),
                    entity_or_identifier_family_identity: scoped_identity(value, 6),
                    collection_cohort_identity: scoped_identity(value, 7),
                    collected_at_unix_seconds: 1_700_000_000 + u64::from(value),
                },
                frozen_holdout: None,
                v2_model_identity: [2; 32],
                challenger_model_identity: [3; 32],
                reason: major_reason(value),
                source: JudgmentSourceV3::CuratedRegressionCase,
                confidence: 1.0,
                weight: 1.0,
                index_generation: u64::from(value),
                supersedes: None,
                contradicts: Box::new([]),
            });
            ledger.append(judgment).unwrap();
        }
        ledger
    }

    fn major_reason(value: u8) -> JudgmentReasonV3 {
        const REASONS: [JudgmentReasonV3; MAJOR_FAILURE_CLASS_COUNT] = [
            JudgmentReasonV3::PartialMatchSaturation,
            JudgmentReasonV3::ScatteredTerms,
            JudgmentReasonV3::PhraseOrderFailure,
            JudgmentReasonV3::IdentifierCollision,
            JudgmentReasonV3::FuzzyCollision,
            JudgmentReasonV3::WeakFieldEvidence,
            JudgmentReasonV3::CommonTermDominance,
            JudgmentReasonV3::LengthPriorFailure,
            JudgmentReasonV3::WrongConceptProximity,
            JudgmentReasonV3::DocumentConversationConfusion,
            JudgmentReasonV3::LongQueryFailure,
        ];
        REASONS[usize::from(value - 1) % REASONS.len()]
    }

    fn scoped_identity(value: u8, domain: u8) -> KeyedIdentity {
        let mut bytes = [value; 32];
        bytes[1] = domain;
        KeyedIdentity::from_bytes(bytes)
    }

    #[test]
    fn split_is_deterministic_grouped_and_class_stratified() {
        let ledger = ledger(55);
        let first = LeakageSplitV3::build(&ledger).unwrap();
        let second = LeakageSplitV3::build(&ledger).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.audit.training_judgments, 33);
        assert_eq!(first.audit.development_judgments, 11);
        assert_eq!(first.audit.blind_test_judgments, 11);
        assert_eq!(first.audit.largest_atomic_component_judgments, 1);
        assert_eq!(first.audit.nearest_feasible_ratio_tolerance_bps, 100);
        assert_eq!(first.audit.training_major_classes_missing, 0);
        assert_eq!(first.audit.development_major_classes_missing, 0);
        assert_eq!(first.audit.blind_test_major_classes_missing, 0);
        assert!(first.audit.is_qualified());
    }

    #[test]
    fn coverage_rebalance_uses_atomic_ratio_tolerance() {
        let component = |size: usize, identity: u8, reason_counts| Component {
            members: vec![0; size],
            min_time: 0,
            max_time: 0,
            identity: [identity; 32],
            reason_counts,
        };
        let mut training_reasons = [0; MAJOR_FAILURE_CLASS_COUNT];
        training_reasons[..MAJOR_FAILURE_CLASS_COUNT - 1].fill(1);
        let all_reasons = [1; MAJOR_FAILURE_CLASS_COUNT];
        let mut rare_reason = [0; MAJOR_FAILURE_CLASS_COUNT];
        rare_reason[MAJOR_FAILURE_CLASS_COUNT - 1] = 1;
        let components = vec![
            component(60, 1, training_reasons),
            component(20, 2, all_reasons),
            component(11, 3, all_reasons),
            component(9, 4, rare_reason),
        ];
        let mut assignments = vec![
            PrimarySplitV3::Training,
            PrimarySplitV3::Development,
            PrimarySplitV3::BlindTest,
            PrimarySplitV3::BlindTest,
        ];

        rebalance_reason_coverage(&components, components.len(), 100, &mut assignments);

        let (counts, reasons) = assigned_counts(&components, &assignments);
        assert_eq!(
            reasons
                .iter()
                .flatten()
                .filter(|&&count| count == 0)
                .count(),
            0
        );
        assert!(ratios_within_tolerance(counts, 100, 3_000));
        assert_eq!(assignments[3], PrimarySplitV3::Training);
    }

    #[test]
    fn frozen_holdouts_are_never_assigned() {
        let mut ledger = ledger(55);
        ledger.judgments[0].frozen_holdout = Some(crate::FrozenHoldoutV3::LongMemEvalRelease);
        let split = LeakageSplitV3::build(&ledger).unwrap();
        assert!(split
            .assignments
            .iter()
            .all(|assignment| assignment.judgment_identity != ledger.judgments[0].identity));
        assert_eq!(split.audit.frozen_holdout_training_assignments, 0);
    }
}
