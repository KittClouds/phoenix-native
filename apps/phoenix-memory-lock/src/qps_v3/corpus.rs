use phoenix_lexical_qps::{
    JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity, RankEvidenceV3, RelevanceLedgerV3,
    RelevanceTier, WorkspaceIdentityKey,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-corpus-readiness/v2";

pub(crate) fn audit(
    phase_4_path: &Path,
    phase_3_path: &Path,
    workspace_key_path: &Path,
    output_path: &Path,
) -> Result<Phase5Publication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite corpus audit {}",
            output_path.display()
        );
    }
    let phase_4: FrozenPhase4 = serde_json::from_slice(&fs::read(phase_4_path)?)
        .with_context(|| format!("decode Phase 4 receipt {}", phase_4_path.display()))?;
    if phase_4.contract != "phoenix.memory.qps-v3-ledger-qualification/v1"
        || !phase_4.phase_4_verified
    {
        bail!("Phase 5 requires a verified QPS V3 Phase 4 ledger");
    }
    let phase_3: FrozenPhase3 = serde_json::from_slice(&fs::read(phase_3_path)?)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("Phase 5 requires the verified Phase 3 release-cohort identities");
    }
    let key = WorkspaceIdentityKey::new(super::ledger::read_workspace_key(workspace_key_path)?)
        .map_err(anyhow::Error::msg)?;
    let ledger_audit = phase_4.ledger.validate().map_err(anyhow::Error::msg)?;
    let counts = corpus_counts(&phase_4.ledger);
    let technical_failure_classes = technical_failure_class_counts(&phase_4.ledger);
    let operational_evidence = operational_evidence(&phase_4.ledger);
    let release_query_identities = phase_3
        .longmemeval_release
        .queries
        .iter()
        .map(|query| KeyedIdentity::derive(&key, b"private-query", query.query_identity.as_bytes()))
        .collect::<HashSet<_>>();
    let release_cohort_intersections = phase_4
        .ledger
        .judgments
        .iter()
        .filter(|judgment| release_query_identities.contains(&judgment.query_identity))
        .count();
    let release_evidence = release_evidence_fingerprints(&phase_3.longmemeval_release);
    let release_evidence_intersections = phase_4
        .ledger
        .judgments
        .iter()
        .filter(|judgment| {
            release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.positive_features,
                judgment.positive_tier,
            )) || release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.negative_features,
                judgment.negative_tier,
            ))
        })
        .count();
    let hard_negative_groups = hard_negative_groups(&phase_4.ledger);
    let integrity = IntegrityGates {
        duplicate_ledger_identities_are_zero: unique_judgments(&phase_4.ledger),
        unresolved_authoritative_contradictions_are_zero: ledger_audit
            .unresolved_authoritative_contradictions
            == 0,
        feature_schema_and_provenance_complete: phase_4.ledger.judgments.iter().all(|judgment| {
            judgment.positive_features.is_valid()
                && judgment.negative_features.is_valid()
                && judgment.split_groups.is_valid()
                && judgment.index_generation > 0
                && judgment.v2_model_identity != [0; 32]
                && judgment.challenger_model_identity != [0; 32]
        }),
        release_cohort_intersections_are_zero: release_cohort_intersections == 0,
        release_evidence_intersections_are_zero: release_evidence_intersections == 0,
    };
    let shadow = CorpusTargetGates {
        judgments: counts.judgments >= 2_000,
        unique_queries: counts.unique_queries >= 500,
        independent_sources: counts.independent_sources >= 100,
        three_to_five_hard_negatives_where_available: hard_negative_groups.eligible_groups == 0
            || hard_negative_groups.groups_with_three_to_five
                == hard_negative_groups.eligible_groups,
        one_hundred_reviewed_per_technical_failure_class: technical_failure_classes
            .iter()
            .all(|class| class.authoritative_reviewed >= 100),
        explicit_or_curator_confirmed: counts.explicit_or_curator_confirmed >= 250,
    };
    let promotion = CorpusTargetGates {
        judgments: counts.judgments >= 5_000,
        unique_queries: counts.unique_queries >= 1_000,
        independent_sources: counts.independent_sources >= 200,
        three_to_five_hard_negatives_where_available: shadow
            .three_to_five_hard_negatives_where_available,
        one_hundred_reviewed_per_technical_failure_class: shadow
            .one_hundred_reviewed_per_technical_failure_class,
        explicit_or_curator_confirmed: counts.explicit_or_curator_confirmed >= 500,
    };
    let deficits = CorpusDeficits {
        shadow_judgments: 2_000_usize.saturating_sub(counts.judgments),
        shadow_unique_queries: 500_usize.saturating_sub(counts.unique_queries),
        shadow_independent_sources: 100_usize.saturating_sub(counts.independent_sources),
        shadow_explicit_or_curator_confirmed: 250_usize
            .saturating_sub(counts.explicit_or_curator_confirmed),
        promotion_judgments: 5_000_usize.saturating_sub(counts.judgments),
        promotion_unique_queries: 1_000_usize.saturating_sub(counts.unique_queries),
        promotion_independent_sources: 200_usize.saturating_sub(counts.independent_sources),
        promotion_explicit_or_curator_confirmed: 500_usize
            .saturating_sub(counts.explicit_or_curator_confirmed),
        technical_failure_classes_below_100_reviewed: technical_failure_classes
            .iter()
            .filter(|class| class.authoritative_reviewed < 100)
            .count(),
    };
    let phase_5_verified = integrity.all_pass() && shadow.all_pass() && promotion.all_pass();
    let receipt = Phase5Receipt {
        contract: CONTRACT,
        phase_4_receipt: file_identity(phase_4_path)?,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        counts,
        technical_failure_classes: technical_failure_classes.clone(),
        operational_evidence,
        hard_negative_groups,
        release_cohort_queries: release_query_identities.len(),
        release_cohort_intersections,
        release_evidence_intersections,
        integrity,
        shadow,
        promotion,
        deficits,
        phase_5_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(Phase5Publication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        counts,
        technical_failure_classes,
        operational_evidence,
        integrity,
        shadow,
        promotion,
        deficits,
        phase_5_verified,
    })
}

fn release_evidence_fingerprints(cohort: &FrozenCohort) -> HashSet<[u8; 32]> {
    cohort
        .queries
        .iter()
        .flat_map(|query| query.candidate_pool.iter())
        .map(|candidate| {
            super::provenance::evidence_fingerprint(
                candidate.rank_evidence_v3,
                candidate.relevance_tier,
            )
        })
        .collect()
}

fn corpus_counts(ledger: &RelevanceLedgerV3) -> CorpusCounts {
    let active = ledger.active_model_training_judgments();
    let unique_queries = active
        .iter()
        .map(|judgment| judgment.query_identity)
        .collect::<HashSet<_>>()
        .len();
    let independent_sources = active
        .iter()
        .flat_map(|judgment| {
            [
                judgment.split_groups.positive_source_identity,
                judgment.split_groups.negative_source_identity,
            ]
        })
        .collect::<HashSet<_>>()
        .len();
    let explicit_or_curator_confirmed = active
        .iter()
        .filter(|judgment| {
            matches!(
                judgment.source,
                JudgmentSourceV3::ExplicitUserCorrection | JudgmentSourceV3::CuratedRegressionCase
            )
        })
        .count();
    CorpusCounts {
        judgments: active.len(),
        unique_queries,
        independent_sources,
        explicit_or_curator_confirmed,
    }
}

fn technical_failure_class_counts(ledger: &RelevanceLedgerV3) -> Vec<FailureClassCount> {
    const REASONS: [JudgmentReasonV3; 11] = [
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
    let active = ledger.active_model_training_judgments();
    REASONS
        .into_iter()
        .map(|reason| FailureClassCount {
            reason,
            total: active
                .iter()
                .filter(|judgment| judgment.reason == reason)
                .count(),
            authoritative_reviewed: active
                .iter()
                .filter(|judgment| {
                    judgment.reason == reason
                        && matches!(
                            judgment.source,
                            JudgmentSourceV3::ExplicitUserCorrection
                                | JudgmentSourceV3::CuratedRegressionCase
                        )
                })
                .count(),
        })
        .collect()
}

fn operational_evidence(ledger: &RelevanceLedgerV3) -> OperationalEvidenceGate {
    let active = ledger.active_model_training_judgments();
    let explicit_user_corrections = active
        .iter()
        .filter(|judgment| judgment.source == JudgmentSourceV3::ExplicitUserCorrection)
        .count();
    OperationalEvidenceGate {
        explicit_user_corrections,
        post_bootstrap_minimum: 100,
        post_bootstrap_minimum_met: explicit_user_corrections >= 100,
        included_in_initial_bootstrap_verification: false,
        rationale: "real-user correction is judgment provenance, not a retrieval failure class; it remains mandatory evidence for post-bootstrap operational retraining",
    }
}

fn hard_negative_groups(ledger: &RelevanceLedgerV3) -> HardNegativeGroupAudit {
    let judgments_by_identity = ledger
        .judgments
        .iter()
        .map(|judgment| (judgment.identity, judgment))
        .collect::<HashMap<_, _>>();
    let mut groups = HashMap::<(KeyedIdentity, KeyedIdentity), usize>::new();
    for judgment in ledger.active_model_training_judgments() {
        let mut root = judgment;
        while let Some(parent_identity) = root.supersedes {
            let Some(parent) = judgments_by_identity.get(&parent_identity).copied() else {
                break;
            };
            root = parent;
        }
        *groups
            .entry((root.query_identity, root.positive_document_version))
            .or_default() += 1;
    }
    let reviewed_groups = groups.len();
    let partial_review_groups = groups
        .values()
        .filter(|count| (1..=2).contains(*count))
        .count();
    let eligible_groups = groups.values().filter(|count| **count >= 3).count();
    HardNegativeGroupAudit {
        eligible_groups,
        groups_with_three_to_five: groups
            .values()
            .filter(|count| (3..=5).contains(*count))
            .count(),
        reviewed_groups,
        partial_review_groups,
    }
}

fn unique_judgments(ledger: &RelevanceLedgerV3) -> bool {
    ledger
        .judgments
        .iter()
        .map(|judgment| judgment.identity)
        .collect::<HashSet<_>>()
        .len()
        == ledger.judgments.len()
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CorpusCounts {
    judgments: usize,
    unique_queries: usize,
    independent_sources: usize,
    explicit_or_curator_confirmed: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct FailureClassCount {
    reason: JudgmentReasonV3,
    total: usize,
    authoritative_reviewed: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct HardNegativeGroupAudit {
    eligible_groups: usize,
    groups_with_three_to_five: usize,
    reviewed_groups: usize,
    partial_review_groups: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct OperationalEvidenceGate {
    explicit_user_corrections: usize,
    post_bootstrap_minimum: usize,
    post_bootstrap_minimum_met: bool,
    included_in_initial_bootstrap_verification: bool,
    rationale: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct IntegrityGates {
    duplicate_ledger_identities_are_zero: bool,
    unresolved_authoritative_contradictions_are_zero: bool,
    feature_schema_and_provenance_complete: bool,
    release_cohort_intersections_are_zero: bool,
    release_evidence_intersections_are_zero: bool,
}

impl IntegrityGates {
    fn all_pass(self) -> bool {
        self.duplicate_ledger_identities_are_zero
            && self.unresolved_authoritative_contradictions_are_zero
            && self.feature_schema_and_provenance_complete
            && self.release_cohort_intersections_are_zero
            && self.release_evidence_intersections_are_zero
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CorpusTargetGates {
    judgments: bool,
    unique_queries: bool,
    independent_sources: bool,
    three_to_five_hard_negatives_where_available: bool,
    one_hundred_reviewed_per_technical_failure_class: bool,
    explicit_or_curator_confirmed: bool,
}

impl CorpusTargetGates {
    fn all_pass(self) -> bool {
        self.judgments
            && self.unique_queries
            && self.independent_sources
            && self.three_to_five_hard_negatives_where_available
            && self.one_hundred_reviewed_per_technical_failure_class
            && self.explicit_or_curator_confirmed
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CorpusDeficits {
    shadow_judgments: usize,
    shadow_unique_queries: usize,
    shadow_independent_sources: usize,
    shadow_explicit_or_curator_confirmed: usize,
    promotion_judgments: usize,
    promotion_unique_queries: usize,
    promotion_independent_sources: usize,
    promotion_explicit_or_curator_confirmed: usize,
    technical_failure_classes_below_100_reviewed: usize,
}

#[derive(Debug, Serialize)]
struct Phase5Receipt {
    contract: &'static str,
    phase_4_receipt: FileIdentity,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    counts: CorpusCounts,
    technical_failure_classes: Vec<FailureClassCount>,
    operational_evidence: OperationalEvidenceGate,
    hard_negative_groups: HardNegativeGroupAudit,
    release_cohort_queries: usize,
    release_cohort_intersections: usize,
    release_evidence_intersections: usize,
    integrity: IntegrityGates,
    shadow: CorpusTargetGates,
    promotion: CorpusTargetGates,
    deficits: CorpusDeficits,
    phase_5_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct Phase5Publication {
    contract: &'static str,
    output: FileIdentity,
    counts: CorpusCounts,
    technical_failure_classes: Vec<FailureClassCount>,
    operational_evidence: OperationalEvidenceGate,
    integrity: IntegrityGates,
    shadow: CorpusTargetGates,
    promotion: CorpusTargetGates,
    deficits: CorpusDeficits,
    phase_5_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase4 {
    contract: String,
    ledger: RelevanceLedgerV3,
    phase_4_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    contract: String,
    longmemeval_release: FrozenCohort,
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    candidate_pool: Vec<FrozenCandidate>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_lexical_qps::{
        PairwiseJudgmentDraftV3, PairwiseJudgmentV3, SplitGroupProvenanceV3,
    };

    #[test]
    fn target_gate_requires_every_dimension() {
        let gates = CorpusTargetGates {
            judgments: true,
            unique_queries: true,
            independent_sources: true,
            three_to_five_hard_negatives_where_available: true,
            one_hundred_reviewed_per_technical_failure_class: false,
            explicit_or_curator_confirmed: true,
        };
        assert!(!gates.all_pass());
    }

    #[test]
    fn real_user_evidence_is_not_a_technical_failure_class() {
        let ledger = RelevanceLedgerV3::default();
        let classes = technical_failure_class_counts(&ledger);
        assert_eq!(classes.len(), 11);
        assert!(classes
            .iter()
            .all(|class| class.reason != JudgmentReasonV3::RealUserCorrection));
        let operational = operational_evidence(&ledger);
        assert_eq!(operational.explicit_user_corrections, 0);
        assert!(!operational.post_bootstrap_minimum_met);
        assert!(!operational.included_in_initial_bootstrap_verification);
    }

    #[test]
    fn reversed_review_remains_in_the_mined_root_group() {
        let mut ledger = RelevanceLedgerV3::default();
        append_review(&mut ledger, 7, 20, 30, 1, false);
        append_review(&mut ledger, 7, 20, 31, 3, false);
        append_review(&mut ledger, 7, 20, 32, 5, true);

        let audit = hard_negative_groups(&ledger);
        assert_eq!(audit.reviewed_groups, 1);
        assert_eq!(audit.partial_review_groups, 0);
        assert_eq!(audit.eligible_groups, 1);
        assert_eq!(audit.groups_with_three_to_five, 1);
    }

    #[test]
    fn partial_review_groups_are_reported_but_not_eligible() {
        let mut ledger = RelevanceLedgerV3::default();
        append_review(&mut ledger, 7, 20, 30, 1, false);
        append_review(&mut ledger, 7, 20, 31, 3, false);
        append_review(&mut ledger, 8, 40, 50, 5, false);
        append_review(&mut ledger, 8, 40, 51, 7, true);
        append_review(&mut ledger, 8, 40, 52, 9, false);

        let audit = hard_negative_groups(&ledger);
        assert_eq!(audit.reviewed_groups, 2);
        assert_eq!(audit.partial_review_groups, 1);
        assert_eq!(audit.eligible_groups, 1);
        assert_eq!(audit.groups_with_three_to_five, 1);
    }

    fn append_review(
        ledger: &mut RelevanceLedgerV3,
        query: u8,
        positive: u8,
        negative: u8,
        generation: u64,
        reversed: bool,
    ) {
        let root = judgment(query, positive, negative, generation, None, false);
        let root_identity = root.identity;
        ledger.append(root).expect("append mined root");
        let review = judgment(
            query,
            positive,
            negative,
            generation + 1,
            Some(root_identity),
            reversed,
        );
        ledger.append(review).expect("append reviewed judgment");
    }

    fn judgment(
        query: u8,
        positive: u8,
        negative: u8,
        generation: u64,
        supersedes: Option<phoenix_lexical_qps::JudgmentIdentity>,
        reversed: bool,
    ) -> PairwiseJudgmentV3 {
        let positive_identity = keyed(positive);
        let negative_identity = keyed(negative);
        let split_groups = SplitGroupProvenanceV3 {
            query_family_identity: keyed(query.saturating_add(80)),
            positive_source_identity: keyed(positive.saturating_add(100)),
            negative_source_identity: keyed(negative.saturating_add(100)),
            positive_near_duplicate_cluster_identity: keyed(positive.saturating_add(140)),
            negative_near_duplicate_cluster_identity: keyed(negative.saturating_add(140)),
            entity_or_identifier_family_identity: keyed(query.saturating_add(180)),
            collection_cohort_identity: keyed(query.saturating_add(200)),
            collected_at_unix_seconds: 1_800_000_000 + generation,
        };
        let (preferred, rejected, preferred_position, rejected_position, split_groups) = if reversed
        {
            (
                negative_identity,
                positive_identity,
                1,
                0,
                reverse_split_groups(split_groups),
            )
        } else {
            (positive_identity, negative_identity, 0, 1, split_groups)
        };
        PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
            workspace_identity: keyed(250),
            query_identity: keyed(query),
            positive_document_version: preferred,
            negative_document_version: rejected,
            positive_features: evidence(),
            negative_features: evidence(),
            positive_tier: RelevanceTier::CompleteExactGroups,
            negative_tier: RelevanceTier::CompleteExactGroups,
            candidate_pool: vec![positive_identity, negative_identity].into_boxed_slice(),
            positive_position: preferred_position,
            negative_position: rejected_position,
            split_groups,
            frozen_holdout: None,
            v2_model_identity: [1; 32],
            challenger_model_identity: [2; 32],
            reason: JudgmentReasonV3::WrongConceptProximity,
            source: if supersedes.is_some() {
                JudgmentSourceV3::CuratedRegressionCase
            } else {
                JudgmentSourceV3::AutomaticallyMinedNegative
            },
            confidence: if supersedes.is_some() { 0.9 } else { 0.5 },
            weight: if supersedes.is_some() { 1.5 } else { 0.5 },
            index_generation: generation,
            supersedes,
            contradicts: if reversed {
                supersedes
                    .into_iter()
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            } else {
                Box::new([])
            },
        })
    }

    fn evidence() -> RankEvidenceV3 {
        RankEvidenceV3 {
            schema_version: 3,
            query_groups: 1,
            matched_groups: 1,
            missing_groups: 0,
            query_flags: 0,
            field_count: 1,
            values: [0.0; 30],
        }
    }

    fn keyed(value: u8) -> KeyedIdentity {
        KeyedIdentity::from_bytes([value; 32])
    }

    fn reverse_split_groups(groups: SplitGroupProvenanceV3) -> SplitGroupProvenanceV3 {
        SplitGroupProvenanceV3 {
            positive_source_identity: groups.negative_source_identity,
            negative_source_identity: groups.positive_source_identity,
            positive_near_duplicate_cluster_identity: groups
                .negative_near_duplicate_cluster_identity,
            negative_near_duplicate_cluster_identity: groups
                .positive_near_duplicate_cluster_identity,
            ..groups
        }
    }
}
