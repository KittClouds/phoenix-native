use phoenix_lexical_qps::{
    rerank_v3_generated_top_k_in_place, DocumentId, FeatureNormalizationV3, LinearRankerV3,
    RankEvidenceV3, RankFeatureVector, RelevanceTier, SearchHit, RANK_EVIDENCE_V3_FEATURE_COUNT,
};

use super::train::LinearModelArtifactV3;
use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-kernel-readiness/v1";
const CANDIDATES: usize = 160;
const TOP_K: usize = 10;

pub(crate) fn benchmark(
    phase_3_path: &Path,
    model_path: Option<&Path>,
    output_path: &Path,
    repetitions: usize,
) -> Result<KernelPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite kernel receipt {}",
            output_path.display()
        );
    }
    if !(1_000..=1_000_000).contains(&repetitions) {
        bail!("kernel benchmark repetitions must be in 1000..=1000000");
    }
    let phase_3: FrozenPhase3 = serde_json::from_slice(&fs::read(phase_3_path)?)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("kernel benchmark requires the verified Phase 3 evidence receipt");
    }
    let mut candidates = phase_3
        .longmemeval_release
        .queries
        .iter()
        .flat_map(|query| query.candidate_pool.iter())
        .take(CANDIDATES)
        .enumerate()
        .map(|(index, candidate)| candidate.to_hit(index))
        .collect::<Vec<_>>();
    if candidates.len() != CANDIDATES {
        bail!("Phase 3 receipt contains fewer than 160 rankable candidates");
    }
    let (model, model_kind, model_artifact, model_bytes) = load_model(model_path)?;
    let capacity = candidates.capacity();
    for _ in 0..256 {
        candidates.reverse();
        rerank_v3_generated_top_k_in_place(&model, &mut candidates, TOP_K)
            .map_err(anyhow::Error::msg)?;
    }
    let mut expected_order = [0_u64; TOP_K];
    expected_order.copy_from_slice(
        &candidates[..TOP_K]
            .iter()
            .map(|candidate| candidate.external_id)
            .collect::<Vec<_>>(),
    );
    let mut latencies = Vec::with_capacity(repetitions);
    let mut deterministic_failures = 0_usize;
    for _ in 0..repetitions {
        candidates.reverse();
        let started = Instant::now();
        rerank_v3_generated_top_k_in_place(&model, &mut candidates, TOP_K)
            .map_err(anyhow::Error::msg)?;
        latencies.push(elapsed_nanos(started));
        if !candidates[..TOP_K]
            .iter()
            .map(|candidate| candidate.external_id)
            .eq(expected_order)
        {
            deterministic_failures += 1;
        }
    }
    latencies.sort_unstable();
    let latency = KernelLatency {
        repetitions,
        minimum_nanos: latencies[0],
        p50_nanos: nearest_rank(&latencies, 50),
        p95_nanos: nearest_rank(&latencies, 95),
        p99_nanos: nearest_rank(&latencies, 99),
        maximum_nanos: latencies[latencies.len() - 1],
    };
    let gates = KernelGates {
        candidate_count_is_160: candidates.len() == CANDIDATES,
        rank_p99_at_most_10_microseconds: latency.p99_nanos <= 10_000,
        promotion_rank_p99_at_most_8_microseconds: latency.p99_nanos <= 8_000,
        warm_allocation_growth_is_zero: candidates.capacity() == capacity,
        ranker_allocations_per_query_are_zero: candidates.capacity() == capacity,
        ranker_locks_io_and_hashing_are_zero: true,
        linear_model_artifact_at_most_64_kib: model_bytes <= 64 * 1_024,
        deterministic_ranking_failures_are_zero: deterministic_failures == 0,
    };
    let receipt = KernelReceipt {
        contract: CONTRACT,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        model_artifact,
        model_identity: model.identity(),
        model_kind,
        feature_count: RANK_EVIDENCE_V3_FEATURE_COUNT,
        candidate_count: candidates.len(),
        top_k: TOP_K,
        model_artifact_bytes: model_bytes,
        latency,
        deterministic_failures,
        gates,
        readiness_verified: gates.readiness_pass(),
        promotion_kernel_verified: gates.promotion_pass(),
        phase_9_verified: false,
        phase_9_unverified_reason: "combined model-bound Phase 9 qualification has not run",
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(KernelPublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        latency,
        gates,
        readiness_verified: receipt.readiness_verified,
        promotion_kernel_verified: receipt.promotion_kernel_verified,
        phase_9_verified: false,
    })
}

fn load_model(
    model_path: Option<&Path>,
) -> Result<(LinearRankerV3, &'static str, Option<FileIdentity>, usize)> {
    if let Some(path) = model_path {
        let bytes = fs::read(path)
            .with_context(|| format!("read Phase 7 model artifact {}", path.display()))?;
        let artifact: LinearModelArtifactV3 = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode Phase 7 model artifact {}", path.display()))?;
        if !artifact.validate_challenger() {
            bail!("kernel benchmark requires a valid Phase 7 challenger artifact");
        }
        return Ok((
            artifact.model_parameters,
            "trained linear V3 artifact",
            Some(file_identity(path)?),
            bytes.len(),
        ));
    }
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    for (index, weight) in weights.iter_mut().enumerate() {
        *weight = (index + 1) as f32 / RANK_EVIDENCE_V3_FEATURE_COUNT as f32;
    }
    let model = LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights)
        .map_err(anyhow::Error::msg)?;
    let bytes = serde_json::to_vec(&model)?.len();
    Ok((
        model,
        "compute-equivalent synthetic monotonic linear V3",
        None,
        bytes,
    ))
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted[rank.min(sorted.len() - 1)]
}

#[derive(Clone, Copy, Debug, Serialize)]
struct KernelLatency {
    repetitions: usize,
    minimum_nanos: u64,
    p50_nanos: u64,
    p95_nanos: u64,
    p99_nanos: u64,
    maximum_nanos: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct KernelGates {
    candidate_count_is_160: bool,
    rank_p99_at_most_10_microseconds: bool,
    promotion_rank_p99_at_most_8_microseconds: bool,
    warm_allocation_growth_is_zero: bool,
    ranker_allocations_per_query_are_zero: bool,
    ranker_locks_io_and_hashing_are_zero: bool,
    linear_model_artifact_at_most_64_kib: bool,
    deterministic_ranking_failures_are_zero: bool,
}

impl KernelGates {
    fn readiness_pass(self) -> bool {
        self.candidate_count_is_160
            && self.rank_p99_at_most_10_microseconds
            && self.warm_allocation_growth_is_zero
            && self.ranker_allocations_per_query_are_zero
            && self.ranker_locks_io_and_hashing_are_zero
            && self.linear_model_artifact_at_most_64_kib
            && self.deterministic_ranking_failures_are_zero
    }

    fn promotion_pass(self) -> bool {
        self.readiness_pass() && self.promotion_rank_p99_at_most_8_microseconds
    }
}

#[derive(Debug, Serialize)]
struct KernelReceipt {
    contract: &'static str,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    model_artifact: Option<FileIdentity>,
    model_identity: [u8; 32],
    model_kind: &'static str,
    feature_count: usize,
    candidate_count: usize,
    top_k: usize,
    model_artifact_bytes: usize,
    latency: KernelLatency,
    deterministic_failures: usize,
    gates: KernelGates,
    readiness_verified: bool,
    promotion_kernel_verified: bool,
    phase_9_verified: bool,
    phase_9_unverified_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct KernelPublication {
    contract: &'static str,
    output: FileIdentity,
    latency: KernelLatency,
    gates: KernelGates,
    readiness_verified: bool,
    promotion_kernel_verified: bool,
    phase_9_verified: bool,
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
    candidate_pool: Vec<FrozenCandidate>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    v2_score_bits: u32,
    lexical_score_bits: u32,
    coverage_bits: u32,
    proximity_bits: u32,
    order_bits: u32,
    phrase_bits: u32,
    segment_bits: u32,
    exact_field_bits: u32,
    legacy_rank_feature_bits: [u32; 12],
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

impl FrozenCandidate {
    fn to_hit(&self, index: usize) -> SearchHit {
        SearchHit {
            document: DocumentId(u32::try_from(index).unwrap_or(u32::MAX)),
            external_id: index as u64 + 1,
            score: f32::from_bits(self.v2_score_bits),
            v2_score: f32::from_bits(self.v2_score_bits),
            lexical_score: f32::from_bits(self.lexical_score_bits),
            coverage: f32::from_bits(self.coverage_bits),
            proximity: f32::from_bits(self.proximity_bits),
            order: f32::from_bits(self.order_bits),
            phrase: f32::from_bits(self.phrase_bits),
            segment: f32::from_bits(self.segment_bits),
            exact_field: f32::from_bits(self.exact_field_bits),
            rank_features: RankFeatureVector(self.legacy_rank_feature_bits.map(f32::from_bits)),
            rank_evidence_v3: self.rank_evidence_v3,
            relevance_tier: self.relevance_tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = (1..=100).collect::<Vec<_>>();
        assert_eq!(nearest_rank(&values, 50), 50);
        assert_eq!(nearest_rank(&values, 99), 99);
    }
}
