use phoenix_lexical_qps::{
    RankEvidenceV3, RANK_EVIDENCE_V3_FEATURE_COUNT, RANK_EVIDENCE_V3_FEATURE_NAMES,
    RANK_EVIDENCE_V3_SCHEMA_VERSION,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-primitive-evidence/v1";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-primitive-evidence-publication/v1";

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify(
    manifest: &FreezeManifest,
    manifest_path: &Path,
    phase_1_path: &Path,
    suite_path: &Path,
    source_path: &Path,
    workload_path: &Path,
    gold_path: &Path,
    output_path: &Path,
    repetitions: usize,
) -> Result<Phase2Publication> {
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    if output_path.exists() {
        bail!(
            "refusing to overwrite evidence receipt {}",
            output_path.display()
        );
    }
    let phase_1_bytes = fs::read(phase_1_path)
        .with_context(|| format!("read Phase 1 baseline {}", phase_1_path.display()))?;
    let phase_1: FrozenBaseline = serde_json::from_slice(&phase_1_bytes)
        .with_context(|| format!("decode Phase 1 baseline {}", phase_1_path.display()))?;
    if phase_1.contract != BASELINE_CONTRACT || !phase_1.phase_1_verified {
        bail!("Phase 2 requires a verified QPS V3 Phase 1 baseline");
    }

    let suite = load_suite(suite_path)?;
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    let gold: GoldArtifact = read_artifact(gold_path, GOLD_MAGIC)?;
    validate_release_inputs(manifest, &workload, &gold)?;
    let configuration = frozen_configuration();
    let configuration_sha256 = sha256_bytes(&serde_json::to_vec(&configuration)?);
    let mixed_suite = capture_mixed(&suite, repetitions)?;
    let longmemeval_release = capture_longmemeval(&workload, &gold, repetitions)?;
    let comparison = compare_phase_1(&phase_1, &mixed_suite, &longmemeval_release);
    let audit = audit_evidence(&mixed_suite, &longmemeval_release);
    let schema_identity = schema_identity();
    let source_identity = file_identity(source_path)?;
    let gates = Phase2Gates {
        phase_1_configuration_unchanged: phase_1.v2_configuration_sha256 == configuration_sha256,
        frozen_artifacts_unchanged: source_identity.sha256 == workload.source.sha256
            && workload.source == gold.source,
        candidate_pool_mismatches_are_zero: comparison.candidate_pool_mismatches == 0,
        v2_order_mismatches_are_zero: comparison.v2_order_mismatches == 0,
        v2_score_bit_mismatches_are_zero: comparison.v2_score_bit_mismatches == 0,
        evidence_schema_is_versioned: RANK_EVIDENCE_V3_SCHEMA_VERSION == 3
            && RANK_EVIDENCE_V3_FEATURE_NAMES.len() == RANK_EVIDENCE_V3_FEATURE_COUNT,
        baseline_score_is_excluded: !RANK_EVIDENCE_V3_FEATURE_NAMES.contains(&"baseline_score"),
        candidate_strength_is_excluded: !RANK_EVIDENCE_V3_FEATURE_NAMES
            .contains(&"candidate_strength"),
        all_evidence_is_finite_and_bounded: audit.invalid_evidence == 0,
        per_field_bm25f_decomposition_is_exact: audit.field_decomposition_failures == 0,
        every_candidate_has_v3_evidence: audit.candidates == comparison.phase_1_candidates,
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
    let phase_2_verified = gates.all_pass();
    let receipt = Phase2Receipt {
        contract: CONTRACT,
        architecture: "primitive_v3_evidence_over_byte_frozen_v2_candidate_pools",
        phase_1_baseline: file_identity(phase_1_path)?,
        producer_binary: current_binary_identity()?,
        v2_configuration: configuration,
        v2_configuration_sha256: configuration_sha256,
        feature_schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
        feature_schema_identity_sha256: schema_identity,
        feature_names: RANK_EVIDENCE_V3_FEATURE_NAMES,
        frozen_inputs: Phase2Inputs {
            manifest: file_identity(manifest_path)?,
            mixed_suite: file_identity(suite_path)?,
            source_corpus: source_identity,
            workload: file_identity(workload_path)?,
            gold: file_identity(gold_path)?,
        },
        comparison,
        evidence_audit: audit,
        mixed_suite,
        longmemeval_release,
        gates,
        phase_2_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(Phase2Publication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        phase_1_baseline: receipt.phase_1_baseline,
        feature_schema_identity_sha256: receipt.feature_schema_identity_sha256,
        comparison: receipt.comparison,
        evidence_audit: receipt.evidence_audit,
        gates,
        phase_2_verified,
    })
}

fn compare_phase_1(
    frozen: &FrozenBaseline,
    mixed: &CohortBaseline,
    long: &CohortBaseline,
) -> Phase1Comparison {
    let mut comparison = Phase1Comparison::default();
    compare_cohort(&frozen.mixed_suite, mixed, &mut comparison);
    compare_cohort(&frozen.longmemeval_release, long, &mut comparison);
    comparison
}

fn compare_cohort(
    frozen: &FrozenCohort,
    current: &CohortBaseline,
    comparison: &mut Phase1Comparison,
) {
    let current_by_query = current
        .queries
        .iter()
        .map(|query| (query.query_identity.as_str(), query))
        .collect::<HashMap<_, _>>();
    for frozen_query in &frozen.queries {
        comparison.phase_1_queries += 1;
        comparison.phase_1_candidates += frozen_query.candidate_pool.len();
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
    comparison.candidate_pool_mismatches += current.queries.len().abs_diff(frozen.queries.len());
}

fn audit_evidence(mixed: &CohortBaseline, long: &CohortBaseline) -> EvidenceAudit {
    let mut audit = EvidenceAudit::default();
    for candidate in [&mixed.queries, &long.queries]
        .into_iter()
        .flat_map(|queries| queries.iter())
        .flat_map(|query| &query.candidate_pool)
    {
        audit.candidates += 1;
        let evidence = candidate.rank_evidence_v3;
        audit.invalid_evidence += usize::from(!evidence.is_valid());
        audit.field_decomposition_failures += usize::from(!field_decomposition_matches(evidence));
    }
    audit
}

fn field_decomposition_matches(evidence: RankEvidenceV3) -> bool {
    let total = invert_unit(evidence.values[RankEvidenceV3::BM25F_LEXICAL]);
    let fields = (RankEvidenceV3::FIELD_LEXICAL_0..=RankEvidenceV3::FIELD_LEXICAL_3)
        .map(|slot| invert_unit(evidence.values[slot]))
        .sum::<f32>()
        + invert_unit(evidence.values[RankEvidenceV3::FIELD_LEXICAL_OVERFLOW]);
    (total - fields).abs() <= 2.0e-4 * total.max(1.0)
}

fn invert_unit(value: f32) -> f32 {
    if value >= 1.0 {
        f32::INFINITY
    } else {
        value / (1.0 - value)
    }
}

fn schema_identity() -> String {
    let bytes = serde_json::to_vec(&(
        RANK_EVIDENCE_V3_SCHEMA_VERSION,
        RANK_EVIDENCE_V3_FEATURE_NAMES,
    ))
    .expect("static feature schema serializes");
    sha256_bytes(&bytes)
}

#[derive(Debug, Serialize)]
struct Phase2Receipt {
    contract: &'static str,
    architecture: &'static str,
    phase_1_baseline: FileIdentity,
    producer_binary: FileIdentity,
    v2_configuration: FrozenV2Configuration,
    v2_configuration_sha256: String,
    feature_schema_version: u16,
    feature_schema_identity_sha256: String,
    feature_names: [&'static str; RANK_EVIDENCE_V3_FEATURE_COUNT],
    frozen_inputs: Phase2Inputs,
    comparison: Phase1Comparison,
    evidence_audit: EvidenceAudit,
    mixed_suite: CohortBaseline,
    longmemeval_release: CohortBaseline,
    gates: Phase2Gates,
    phase_2_verified: bool,
}

#[derive(Debug, Serialize)]
struct Phase2Inputs {
    manifest: FileIdentity,
    mixed_suite: FileIdentity,
    source_corpus: FileIdentity,
    workload: FileIdentity,
    gold: FileIdentity,
}

#[derive(Debug, Serialize)]
pub struct Phase2Publication {
    contract: &'static str,
    output: FileIdentity,
    phase_1_baseline: FileIdentity,
    feature_schema_identity_sha256: String,
    comparison: Phase1Comparison,
    evidence_audit: EvidenceAudit,
    gates: Phase2Gates,
    phase_2_verified: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase1Comparison {
    phase_1_queries: usize,
    current_queries: usize,
    phase_1_candidates: usize,
    current_candidates: usize,
    candidate_pool_mismatches: usize,
    v2_order_mismatches: usize,
    v2_score_bit_mismatches: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct EvidenceAudit {
    candidates: usize,
    invalid_evidence: usize,
    field_decomposition_failures: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase2Gates {
    phase_1_configuration_unchanged: bool,
    frozen_artifacts_unchanged: bool,
    candidate_pool_mismatches_are_zero: bool,
    v2_order_mismatches_are_zero: bool,
    v2_score_bit_mismatches_are_zero: bool,
    evidence_schema_is_versioned: bool,
    baseline_score_is_excluded: bool,
    candidate_strength_is_excluded: bool,
    all_evidence_is_finite_and_bounded: bool,
    per_field_bm25f_decomposition_is_exact: bool,
    every_candidate_has_v3_evidence: bool,
    zero_warm_allocation_growth: bool,
    deterministic_ranking_failures_are_zero: bool,
    frozen_quality_is_unchanged: bool,
}

impl Phase2Gates {
    fn all_pass(self) -> bool {
        self.phase_1_configuration_unchanged
            && self.frozen_artifacts_unchanged
            && self.candidate_pool_mismatches_are_zero
            && self.v2_order_mismatches_are_zero
            && self.v2_score_bit_mismatches_are_zero
            && self.evidence_schema_is_versioned
            && self.baseline_score_is_excluded
            && self.candidate_strength_is_excluded
            && self.all_evidence_is_finite_and_bounded
            && self.per_field_bm25f_decomposition_is_exact
            && self.every_candidate_has_v3_evidence
            && self.zero_warm_allocation_growth
            && self.deterministic_ranking_failures_are_zero
            && self.frozen_quality_is_unchanged
    }
}

#[derive(Debug, Deserialize)]
struct FrozenBaseline {
    contract: String,
    v2_configuration_sha256: String,
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    phase_1_verified: bool,
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
    fn schema_identity_is_stable_and_has_no_v2_composite_features() {
        assert_eq!(schema_identity(), schema_identity());
        assert!(!RANK_EVIDENCE_V3_FEATURE_NAMES.contains(&"baseline_score"));
        assert!(!RANK_EVIDENCE_V3_FEATURE_NAMES.contains(&"candidate_strength"));
    }
}
