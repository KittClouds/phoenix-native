use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use crate::{JudgmentIdentity, KeyedIdentity, RelevanceLedgerV3};

pub const LEAKAGE_SPLIT_V3_CONTRACT: &str = "phoenix.qps.leakage-split/v3";
pub const LEAKAGE_SPLIT_V3_SCHEMA_VERSION: u16 = 4;
pub const SPLIT_RATIO_TOLERANCE_BPS: u16 = 100;

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

        let prefix = component_prefix(&components);
        let train_end = closest_boundary(&prefix, eligible.len(), 6_000, 1, components.len() - 2);
        let dev_end = closest_boundary(
            &prefix,
            eligible.len(),
            8_000,
            train_end + 1,
            components.len() - 1,
        );
        let mut local_splits = vec![PrimarySplitV3::Training; eligible.len()];
        for (component_index, component) in components.iter().enumerate() {
            let split = if component_index < train_end {
                PrimarySplitV3::Training
            } else if component_index < dev_end {
                PrimarySplitV3::Development
            } else {
                PrimarySplitV3::BlindTest
            };
            for &local in &component.members {
                local_splits[local] = split;
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
                    future_time_holdout: primary_split == PrimarySplitV3::BlindTest,
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
        Self {
            members,
            min_time,
            max_time,
            identity: *hasher.finalize().as_bytes(),
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

fn closest_boundary(
    prefix: &[usize],
    total: usize,
    target_bps: usize,
    first: usize,
    last: usize,
) -> usize {
    (first..=last)
        .min_by_key(|&boundary| (prefix[boundary] * 10_000).abs_diff(total * target_bps))
        .unwrap_or(first)
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
    let atomic_component_bps = largest_atomic_component_judgments
        .saturating_mul(10_000)
        .div_ceil(eligible.len());
    let nearest_feasible_ratio_tolerance_bps = SPLIT_RATIO_TOLERANCE_BPS
        .max(u16::try_from(atomic_component_bps.div_ceil(2)).unwrap_or(u16::MAX));
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
    let max_non_test_time = eligible
        .iter()
        .filter(|judgment| by_identity.get(&judgment.identity) != Some(&PrimarySplitV3::BlindTest))
        .map(|judgment| judgment.split_groups.collected_at_unix_seconds)
        .max()
        .unwrap_or(0);
    let future_time_ordering_violations = eligible
        .iter()
        .filter(|judgment| {
            by_identity.get(&judgment.identity) == Some(&PrimarySplitV3::BlindTest)
                && judgment.split_groups.collected_at_unix_seconds < max_non_test_time
        })
        .count();
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
        frozen_holdout_training_assignments,
        duplicate_or_missing_assignments,
    }
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
                    query_family_identity: identity(value),
                    positive_source_identity: identity(value.saturating_add(20)),
                    negative_source_identity: identity(value.saturating_add(40)),
                    positive_near_duplicate_cluster_identity: identity(value.saturating_add(60)),
                    negative_near_duplicate_cluster_identity: identity(value.saturating_add(100)),
                    entity_or_identifier_family_identity: identity(value.saturating_add(140)),
                    collection_cohort_identity: identity(value.saturating_add(180)),
                    collected_at_unix_seconds: 1_700_000_000 + u64::from(value),
                },
                frozen_holdout: None,
                v2_model_identity: [2; 32],
                challenger_model_identity: [3; 32],
                reason: JudgmentReasonV3::RealUserCorrection,
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

    #[test]
    fn split_is_deterministic_grouped_and_exact_for_twenty_components() {
        let ledger = ledger(20);
        let first = LeakageSplitV3::build(&ledger).unwrap();
        let second = LeakageSplitV3::build(&ledger).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.audit.training_judgments, 12);
        assert_eq!(first.audit.development_judgments, 4);
        assert_eq!(first.audit.blind_test_judgments, 4);
        assert_eq!(first.audit.largest_atomic_component_judgments, 1);
        assert_eq!(first.audit.nearest_feasible_ratio_tolerance_bps, 250);
        assert!(first.audit.is_qualified());
    }

    #[test]
    fn frozen_holdouts_are_never_assigned() {
        let mut ledger = ledger(20);
        ledger.judgments[0].frozen_holdout = Some(crate::FrozenHoldoutV3::LongMemEvalRelease);
        let split = LeakageSplitV3::build(&ledger).unwrap();
        assert!(split
            .assignments
            .iter()
            .all(|assignment| assignment.judgment_identity != ledger.judgments[0].identity));
        assert_eq!(split.audit.frozen_holdout_training_assignments, 0);
    }
}
