use phoenix_lexical_qps::{
    FeatureNormalizationV3, LinearRankerV3, SearchHit, RANK_EVIDENCE_V3_FEATURE_COUNT,
};

use super::train::LinearModelArtifactV3;
use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-e2e-performance-readiness/v1";

pub(crate) fn benchmark(
    manifest: &FreezeManifest,
    workload_path: &Path,
    model_path: Option<&Path>,
    output_path: &Path,
    repetitions: usize,
) -> Result<E2ePublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite E2E performance receipt {}",
            output_path.display()
        );
    }
    if repetitions == 0 || repetitions > 4_096 {
        bail!("E2E benchmark repetitions must be in 1..=4096");
    }
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    if workload.contract != WORKLOAD_CONTRACT {
        bail!("unsupported LongMemEval workload contract");
    }
    verify_binding(manifest, &workload.source)?;
    let (model, model_kind, model_artifact) = load_model(model_path)?;
    let mut v2_total_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut v3_total_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut paired_ordering_overhead = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut candidate_pool_mismatches = 0_usize;
    let mut primitive_evidence_mismatches = 0_usize;
    let mut v2_allocation_growths = 0_u64;
    let mut v3_allocation_growths = 0_u64;
    let mut deterministic_failures = 0_u64;

    for case in &workload.cases {
        let documents = case
            .sessions
            .iter()
            .map(|session| {
                session
                    .turns
                    .iter()
                    .map(|turn| turn.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>();
        let index = build_index(
            documents
                .iter()
                .enumerate()
                .map(|(index, document)| (index as u64, [document.as_str()])),
            &LONGMEMEVAL_FIELDS,
        )?;
        let mut v2_scratch =
            SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
        let mut v3_scratch =
            SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
        let mut v2_evidence = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        let mut v3_evidence = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        index.search_evidence_into(&case.question, TOP_K, &mut v2_scratch, &mut v2_evidence)?;
        index.search_v3_evidence_into(
            &case.question,
            TOP_K,
            &model,
            &mut v3_scratch,
            &mut v3_evidence,
        )?;
        audit_candidate_parity(
            &v2_evidence,
            &v3_evidence,
            &mut candidate_pool_mismatches,
            &mut primitive_evidence_mismatches,
        );
        let expected_v3 = v3_evidence
            .iter()
            .take(TOP_K)
            .map(|hit| hit.external_id)
            .collect::<Vec<_>>();
        let mut v2 = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        let mut v3 = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        index.search_into(&case.question, TOP_K, &mut v2_scratch, &mut v2)?;
        index.search_v3_into(&case.question, TOP_K, &model, &mut v3_scratch, &mut v3)?;
        for _ in 0..repetitions {
            let v2_receipt = index.search_into(&case.question, TOP_K, &mut v2_scratch, &mut v2)?;
            let v3_receipt =
                index.search_v3_into(&case.question, TOP_K, &model, &mut v3_scratch, &mut v3)?;
            v2_total_samples.push(v2_receipt.stages.total);
            v3_total_samples.push(v3_receipt.stages.total);
            paired_ordering_overhead.push(
                v3_receipt
                    .stages
                    .ordering
                    .saturating_sub(v2_receipt.stages.ordering),
            );
            v2_allocation_growths += u64::from(v2_receipt.allocations_grew);
            v3_allocation_growths += u64::from(v3_receipt.allocations_grew);
            deterministic_failures += u64::from(!matches_order(&v3, &expected_v3));
        }
    }

    let v2_latency = LatencyReceipt::from_samples(&mut v2_total_samples);
    let v3_latency = LatencyReceipt::from_samples(&mut v3_total_samples);
    let overhead_latency = LatencyReceipt::from_samples(&mut paired_ordering_overhead);
    let gates = E2eGates {
        candidate_pool_mismatches_are_zero: candidate_pool_mismatches == 0,
        primitive_evidence_mismatches_are_zero: primitive_evidence_mismatches == 0,
        paired_p99_overhead_at_most_15_microseconds: overhead_latency.p99_nanos <= 15_000,
        end_to_end_p99_at_most_1_millisecond: v3_latency.p99_nanos <= 1_000_000,
        warm_query_allocation_growth_is_zero: v2_allocation_growths == 0
            && v3_allocation_growths == 0,
        deterministic_ranking_failures_are_zero: deterministic_failures == 0,
        candidate_cap_is_160: CANDIDATE_CAP == 160,
    };
    let receipt = E2eReceipt {
        contract: CONTRACT,
        workload: file_identity(workload_path)?,
        producer_binary: current_binary_identity()?,
        model_artifact,
        model_identity: model.identity(),
        model_kind,
        queries: workload.cases.len(),
        repetitions,
        v2_latency,
        v3_latency,
        paired_ordering_overhead: overhead_latency,
        candidate_pool_mismatches,
        primitive_evidence_mismatches,
        v2_allocation_growths,
        v3_allocation_growths,
        deterministic_failures,
        gates,
        readiness_verified: gates.all_pass(),
        phase_9_verified: false,
        phase_9_unverified_reason: "combined model-bound Phase 9 qualification has not run",
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(E2ePublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        v3_latency,
        paired_ordering_overhead: overhead_latency,
        gates,
        readiness_verified: receipt.readiness_verified,
        phase_9_verified: false,
    })
}

fn load_model(
    model_path: Option<&Path>,
) -> Result<(LinearRankerV3, &'static str, Option<FileIdentity>)> {
    if let Some(path) = model_path {
        let bytes = fs::read(path)
            .with_context(|| format!("read Phase 7 model artifact {}", path.display()))?;
        let artifact: LinearModelArtifactV3 = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode Phase 7 model artifact {}", path.display()))?;
        if !artifact.validate_challenger() {
            bail!("E2E benchmark requires a valid Phase 7 challenger artifact");
        }
        return Ok((
            artifact.model_parameters,
            "trained linear V3 artifact",
            Some(file_identity(path)?),
        ));
    }
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    for (index, weight) in weights.iter_mut().enumerate() {
        *weight = (index + 1) as f32 / RANK_EVIDENCE_V3_FEATURE_COUNT as f32;
    }
    let model = LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights)
        .map_err(anyhow::Error::msg)?;
    Ok((
        model,
        "compute-equivalent synthetic monotonic linear V3",
        None,
    ))
}

fn audit_candidate_parity(
    v2: &[SearchHit],
    v3: &[SearchHit],
    pool_mismatches: &mut usize,
    evidence_mismatches: &mut usize,
) {
    if v2.len() != v3.len() {
        *pool_mismatches += 1;
        return;
    }
    for v2_hit in v2 {
        let Some(v3_hit) = v3
            .iter()
            .find(|candidate| candidate.external_id == v2_hit.external_id)
        else {
            *pool_mismatches += 1;
            continue;
        };
        if v2_hit.v2_score.to_bits() != v3_hit.v2_score.to_bits()
            || v2_hit.rank_evidence_v3 != v3_hit.rank_evidence_v3
            || v2_hit.relevance_tier != v3_hit.relevance_tier
        {
            *evidence_mismatches += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct E2eGates {
    candidate_pool_mismatches_are_zero: bool,
    primitive_evidence_mismatches_are_zero: bool,
    paired_p99_overhead_at_most_15_microseconds: bool,
    end_to_end_p99_at_most_1_millisecond: bool,
    warm_query_allocation_growth_is_zero: bool,
    deterministic_ranking_failures_are_zero: bool,
    candidate_cap_is_160: bool,
}

impl E2eGates {
    fn all_pass(self) -> bool {
        self.candidate_pool_mismatches_are_zero
            && self.primitive_evidence_mismatches_are_zero
            && self.paired_p99_overhead_at_most_15_microseconds
            && self.end_to_end_p99_at_most_1_millisecond
            && self.warm_query_allocation_growth_is_zero
            && self.deterministic_ranking_failures_are_zero
            && self.candidate_cap_is_160
    }
}

#[derive(Debug, Serialize)]
struct E2eReceipt {
    contract: &'static str,
    workload: FileIdentity,
    producer_binary: FileIdentity,
    model_artifact: Option<FileIdentity>,
    model_identity: [u8; 32],
    model_kind: &'static str,
    queries: usize,
    repetitions: usize,
    v2_latency: LatencyReceipt,
    v3_latency: LatencyReceipt,
    paired_ordering_overhead: LatencyReceipt,
    candidate_pool_mismatches: usize,
    primitive_evidence_mismatches: usize,
    v2_allocation_growths: u64,
    v3_allocation_growths: u64,
    deterministic_failures: u64,
    gates: E2eGates,
    readiness_verified: bool,
    phase_9_verified: bool,
    phase_9_unverified_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct E2ePublication {
    contract: &'static str,
    output: FileIdentity,
    v3_latency: LatencyReceipt,
    paired_ordering_overhead: LatencyReceipt,
    gates: E2eGates,
    readiness_verified: bool,
    phase_9_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_gate_requires_candidate_and_evidence_parity() {
        let mut gates = E2eGates {
            candidate_pool_mismatches_are_zero: true,
            primitive_evidence_mismatches_are_zero: true,
            paired_p99_overhead_at_most_15_microseconds: true,
            end_to_end_p99_at_most_1_millisecond: true,
            warm_query_allocation_growth_is_zero: true,
            deterministic_ranking_failures_are_zero: true,
            candidate_cap_is_160: true,
        };
        assert!(gates.all_pass());
        gates.primitive_evidence_mismatches_are_zero = false;
        assert!(!gates.all_pass());
    }
}
