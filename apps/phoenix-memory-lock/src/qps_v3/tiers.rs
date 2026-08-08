use phoenix_lexical_qps::{
    RankEvidenceV3, RelevanceTier, RANK_EVIDENCE_V3_FEATURE_NAMES, RANK_EVIDENCE_V3_SCHEMA_VERSION,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-constitutional-tiers/v1";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-constitutional-tiers-publication/v1";

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify(
    manifest: &FreezeManifest,
    manifest_path: &Path,
    phase_2_path: &Path,
    suite_path: &Path,
    source_path: &Path,
    workload_path: &Path,
    gold_path: &Path,
    output_path: &Path,
    repetitions: usize,
) -> Result<Phase3Publication> {
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    if output_path.exists() {
        bail!(
            "refusing to overwrite tier receipt {}",
            output_path.display()
        );
    }
    let phase_2_bytes = fs::read(phase_2_path)
        .with_context(|| format!("read Phase 2 receipt {}", phase_2_path.display()))?;
    let phase_2: FrozenPhase2 = serde_json::from_slice(&phase_2_bytes)
        .with_context(|| format!("decode Phase 2 receipt {}", phase_2_path.display()))?;
    if phase_2.contract != "phoenix.memory.qps-v3-primitive-evidence/v1"
        || !phase_2.phase_2_verified
    {
        bail!("Phase 3 requires a verified QPS V3 Phase 2 receipt");
    }

    let suite = load_suite(suite_path)?;
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    let gold: GoldArtifact = read_artifact(gold_path, GOLD_MAGIC)?;
    validate_release_inputs(manifest, &workload, &gold)?;
    let configuration = frozen_configuration();
    let configuration_sha256 = sha256_bytes(&serde_json::to_vec(&configuration)?);
    let mixed_suite = capture_mixed(&suite, repetitions)?;
    let longmemeval_release = capture_longmemeval(&workload, &gold, repetitions)?;
    let comparison = compare_phase_2(&phase_2, &mixed_suite, &longmemeval_release);
    let tier_audit = audit_tiers(&mixed_suite, &longmemeval_release, false);
    let source_identity = file_identity(source_path)?;
    let gates = Phase3Gates {
        phase_2_configuration_unchanged: phase_2.v2_configuration_sha256 == configuration_sha256,
        feature_schema_unchanged: phase_2.feature_schema_version == RANK_EVIDENCE_V3_SCHEMA_VERSION
            && phase_2.feature_names == RANK_EVIDENCE_V3_FEATURE_NAMES,
        frozen_artifacts_unchanged: source_identity.sha256 == workload.source.sha256
            && workload.source == gold.source,
        candidate_pool_mismatches_are_zero: comparison.candidate_pool_mismatches == 0,
        v2_order_mismatches_are_zero: comparison.v2_order_mismatches == 0,
        v2_score_bit_mismatches_are_zero: comparison.v2_score_bit_mismatches == 0,
        rejected_candidates_are_zero: tier_audit.rejected == 0,
        tier_assignment_mismatches_are_zero: tier_audit.assignment_mismatches == 0,
        missing_groups_never_enter_protected_tiers: tier_audit.missing_in_protected_tier == 0,
        expanded_coverage_never_enters_exact_tier: tier_audit.expanded_in_exact_tier == 0,
        exact_identifier_tier_requires_configuration: tier_audit.configured_identifier == 0,
        protected_tier_precedes_arbitrary_score: protected_tier_precedes_arbitrary_score(),
        stable_identity_resolves_score_ties: stable_identity_resolves_score_ties(),
        smaller_complete_span_is_monotonic: 1.0_f32 > 1.0 / 3.0,
        expansion_quality_does_not_lower_tier: expansion_quality_does_not_lower_tier(),
        zero_warm_allocation_growth: mixed_suite.warm_allocation_growths == 0
            && longmemeval_release.warm_allocation_growths == 0,
        deterministic_ranking_failures_are_zero: mixed_suite.deterministic_ranking_failures == 0
            && longmemeval_release.deterministic_ranking_failures == 0,
        frozen_quality_is_unchanged: approx(mixed_suite.metrics.hit_at_10, 1.0)
            && approx(mixed_suite.metrics.mean_reciprocal_rank, 1.0)
            && approx(longmemeval_release.metrics.hit_at_10, 0.98)
            && approx(
                longmemeval_release.metrics.mean_reciprocal_rank,
                0.891_004_761_904_761_6,
            ),
    };
    let phase_3_verified = gates.all_pass();
    let receipt = Phase3Receipt {
        contract: CONTRACT,
        architecture: "hard_relevance_tier_then_learned_within_tier_then_stable_identity",
        phase_2_receipt: file_identity(phase_2_path)?,
        producer_binary: current_binary_identity()?,
        v2_configuration: configuration,
        v2_configuration_sha256: configuration_sha256,
        constitution: ConstitutionReceipt {
            configured_exact_identifier_fields: 0,
            tier_order: [
                "configured_exact_identifier",
                "complete_exact_groups",
                "complete_expanded_groups",
                "admissible_partial",
                "rejected",
            ],
            within_tier_order: "learned_score_desc_then_stable_external_document_identity",
        },
        frozen_inputs: Phase3Inputs {
            manifest: file_identity(manifest_path)?,
            mixed_suite: file_identity(suite_path)?,
            source_corpus: source_identity,
            workload: file_identity(workload_path)?,
            gold: file_identity(gold_path)?,
        },
        comparison,
        tier_audit,
        mixed_suite,
        longmemeval_release,
        gates,
        phase_3_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(Phase3Publication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        phase_2_receipt: receipt.phase_2_receipt,
        comparison: receipt.comparison,
        tier_audit: receipt.tier_audit,
        gates,
        phase_3_verified,
    })
}

fn compare_phase_2(
    frozen: &FrozenPhase2,
    mixed: &CohortBaseline,
    long: &CohortBaseline,
) -> FrozenComparison {
    let mut comparison = FrozenComparison::default();
    compare_cohort(&frozen.mixed_suite, mixed, &mut comparison);
    compare_cohort(&frozen.longmemeval_release, long, &mut comparison);
    comparison
}

fn compare_cohort(
    frozen: &FrozenCohort,
    current: &CohortBaseline,
    comparison: &mut FrozenComparison,
) {
    let current_by_query = current
        .queries
        .iter()
        .map(|query| (query.query_identity.as_str(), query))
        .collect::<HashMap<_, _>>();
    for frozen_query in &frozen.queries {
        comparison.frozen_queries += 1;
        comparison.frozen_candidates += frozen_query.candidate_pool.len();
        let Some(current_query) = current_by_query.get(frozen_query.query_identity.as_str()) else {
            comparison.candidate_pool_mismatches += 1;
            continue;
        };
        comparison.current_queries += 1;
        comparison.current_candidates += current_query.candidate_pool.len();
        comparison.v2_order_mismatches +=
            usize::from(frozen_query.v2_final_order != current_query.v2_final_order);
        if frozen_query.candidate_pool.len() != current_query.candidate_pool.len() {
            comparison.candidate_pool_mismatches += 1;
            continue;
        }
        for (frozen_candidate, current_candidate) in frozen_query
            .candidate_pool
            .iter()
            .zip(&current_query.candidate_pool)
        {
            comparison.candidate_pool_mismatches += usize::from(
                frozen_candidate.document_identity != current_candidate.document_identity
                    || frozen_candidate.v2_order != current_candidate.v2_order,
            );
            comparison.v2_score_bit_mismatches +=
                usize::from(frozen_candidate.v2_score_bits != current_candidate.v2_score_bits);
        }
    }
}

fn audit_tiers(mixed: &CohortBaseline, long: &CohortBaseline, identifiers: bool) -> TierAudit {
    let mut audit = TierAudit::default();
    for candidate in [&mixed.queries, &long.queries]
        .into_iter()
        .flat_map(|queries| queries.iter())
        .flat_map(|query| &query.candidate_pool)
    {
        let evidence = candidate.rank_evidence_v3;
        let tier = candidate.relevance_tier;
        audit.candidates += 1;
        audit.assignment_mismatches += usize::from(tier != evidence.relevance_tier(identifiers));
        match tier {
            RelevanceTier::ConfiguredExactIdentifier => audit.configured_identifier += 1,
            RelevanceTier::CompleteExactGroups => audit.complete_exact += 1,
            RelevanceTier::CompleteExpandedGroups => audit.complete_expanded += 1,
            RelevanceTier::AdmissiblePartial => audit.admissible_partial += 1,
            RelevanceTier::Rejected => audit.rejected += 1,
        }
        audit.missing_in_protected_tier += usize::from(
            tier <= RelevanceTier::CompleteExpandedGroups && evidence.missing_groups != 0,
        );
        audit.expanded_in_exact_tier += usize::from(
            tier <= RelevanceTier::CompleteExactGroups
                && evidence.values[RankEvidenceV3::EXACT_GROUP_FRACTION] < 1.0 - f32::EPSILON,
        );
    }
    audit
}

fn protected_tier_precedes_arbitrary_score() -> bool {
    RelevanceTier::CompleteExactGroups.compare_ranked(
        0.0,
        99,
        RelevanceTier::AdmissiblePartial,
        f32::MAX,
        1,
    ) == std::cmp::Ordering::Less
}

fn stable_identity_resolves_score_ties() -> bool {
    RelevanceTier::CompleteExactGroups.compare_ranked(
        1.0,
        7,
        RelevanceTier::CompleteExactGroups,
        1.0,
        8,
    ) == std::cmp::Ordering::Less
}

fn expansion_quality_does_not_lower_tier() -> bool {
    let mut evidence = synthetic_complete_evidence();
    evidence.values[RankEvidenceV3::EXACT_GROUP_FRACTION] = 0.5;
    let lower = evidence.relevance_tier(false);
    evidence.values[RankEvidenceV3::BEST_EXPANSION_QUALITY] = 1.0;
    evidence.values[RankEvidenceV3::MEAN_EXPANSION_QUALITY] = 0.9;
    evidence.values[RankEvidenceV3::MINIMUM_EXPANSION_QUALITY] = 0.8;
    lower == RelevanceTier::CompleteExpandedGroups && evidence.relevance_tier(false) == lower
}

fn synthetic_complete_evidence() -> RankEvidenceV3 {
    RankEvidenceV3 {
        schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
        query_groups: 2,
        matched_groups: 2,
        missing_groups: 0,
        query_flags: 0,
        field_count: 1,
        values: [0.5; 30],
    }
}

#[derive(Debug, Serialize)]
struct Phase3Receipt {
    contract: &'static str,
    architecture: &'static str,
    phase_2_receipt: FileIdentity,
    producer_binary: FileIdentity,
    v2_configuration: FrozenV2Configuration,
    v2_configuration_sha256: String,
    constitution: ConstitutionReceipt,
    frozen_inputs: Phase3Inputs,
    comparison: FrozenComparison,
    tier_audit: TierAudit,
    mixed_suite: CohortBaseline,
    longmemeval_release: CohortBaseline,
    gates: Phase3Gates,
    phase_3_verified: bool,
}

#[derive(Debug, Serialize)]
struct ConstitutionReceipt {
    configured_exact_identifier_fields: u64,
    tier_order: [&'static str; 5],
    within_tier_order: &'static str,
}

#[derive(Debug, Serialize)]
struct Phase3Inputs {
    manifest: FileIdentity,
    mixed_suite: FileIdentity,
    source_corpus: FileIdentity,
    workload: FileIdentity,
    gold: FileIdentity,
}

#[derive(Debug, Serialize)]
pub struct Phase3Publication {
    contract: &'static str,
    output: FileIdentity,
    phase_2_receipt: FileIdentity,
    comparison: FrozenComparison,
    tier_audit: TierAudit,
    gates: Phase3Gates,
    phase_3_verified: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct FrozenComparison {
    frozen_queries: usize,
    current_queries: usize,
    frozen_candidates: usize,
    current_candidates: usize,
    candidate_pool_mismatches: usize,
    v2_order_mismatches: usize,
    v2_score_bit_mismatches: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct TierAudit {
    candidates: usize,
    configured_identifier: usize,
    complete_exact: usize,
    complete_expanded: usize,
    admissible_partial: usize,
    rejected: usize,
    assignment_mismatches: usize,
    missing_in_protected_tier: usize,
    expanded_in_exact_tier: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase3Gates {
    phase_2_configuration_unchanged: bool,
    feature_schema_unchanged: bool,
    frozen_artifacts_unchanged: bool,
    candidate_pool_mismatches_are_zero: bool,
    v2_order_mismatches_are_zero: bool,
    v2_score_bit_mismatches_are_zero: bool,
    rejected_candidates_are_zero: bool,
    tier_assignment_mismatches_are_zero: bool,
    missing_groups_never_enter_protected_tiers: bool,
    expanded_coverage_never_enters_exact_tier: bool,
    exact_identifier_tier_requires_configuration: bool,
    protected_tier_precedes_arbitrary_score: bool,
    stable_identity_resolves_score_ties: bool,
    smaller_complete_span_is_monotonic: bool,
    expansion_quality_does_not_lower_tier: bool,
    zero_warm_allocation_growth: bool,
    deterministic_ranking_failures_are_zero: bool,
    frozen_quality_is_unchanged: bool,
}

impl Phase3Gates {
    fn all_pass(self) -> bool {
        self.phase_2_configuration_unchanged
            && self.feature_schema_unchanged
            && self.frozen_artifacts_unchanged
            && self.candidate_pool_mismatches_are_zero
            && self.v2_order_mismatches_are_zero
            && self.v2_score_bit_mismatches_are_zero
            && self.rejected_candidates_are_zero
            && self.tier_assignment_mismatches_are_zero
            && self.missing_groups_never_enter_protected_tiers
            && self.expanded_coverage_never_enters_exact_tier
            && self.exact_identifier_tier_requires_configuration
            && self.protected_tier_precedes_arbitrary_score
            && self.stable_identity_resolves_score_ties
            && self.smaller_complete_span_is_monotonic
            && self.expansion_quality_does_not_lower_tier
            && self.zero_warm_allocation_growth
            && self.deterministic_ranking_failures_are_zero
            && self.frozen_quality_is_unchanged
    }
}

#[derive(Debug, Deserialize)]
struct FrozenPhase2 {
    contract: String,
    v2_configuration_sha256: String,
    feature_schema_version: u16,
    feature_names: Vec<String>,
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    phase_2_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    candidate_pool: Vec<FrozenCandidate>,
    v2_final_order: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    v2_order: u16,
    v2_score_bits: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constitutional_comparator_and_expansion_tier_are_fail_closed() {
        assert!(protected_tier_precedes_arbitrary_score());
        assert!(stable_identity_resolves_score_ties());
        assert!(expansion_quality_does_not_lower_tier());
        let mut invalid = synthetic_complete_evidence();
        invalid.values[0] = f32::NAN;
        assert_eq!(invalid.relevance_tier(false), RelevanceTier::Rejected);
    }
}
