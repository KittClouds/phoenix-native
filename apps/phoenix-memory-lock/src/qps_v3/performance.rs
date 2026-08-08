use super::train::LinearModelArtifactV3;
use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-performance-qualification/v1";
const KERNEL_CONTRACT: &str = "phoenix.memory.qps-v3-kernel-readiness/v1";
const E2E_CONTRACT: &str = "phoenix.memory.qps-v3-e2e-performance-readiness/v1";

pub(crate) fn qualify(
    model_path: &Path,
    kernel_path: &Path,
    e2e_path: &Path,
    output_path: &Path,
) -> Result<PerformancePublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite performance qualification {}",
            output_path.display()
        );
    }
    let model_bytes = fs::read(model_path)
        .with_context(|| format!("read Phase 7 model artifact {}", model_path.display()))?;
    let model: LinearModelArtifactV3 = serde_json::from_slice(&model_bytes)
        .with_context(|| format!("decode Phase 7 model artifact {}", model_path.display()))?;
    if !model.validate_challenger() {
        bail!("Phase 9 requires a valid Phase 7 challenger artifact");
    }
    let kernel: KernelInput = read_json(kernel_path, "kernel receipt")?;
    let e2e: E2eInput = read_json(e2e_path, "E2E receipt")?;
    let model_identity = file_identity(model_path)?;
    let kernel_bound = input_binds_model(
        &kernel.model_artifact,
        &kernel.model_identity,
        &model_identity,
        &model.model_identity,
    );
    let e2e_bound = input_binds_model(
        &e2e.model_artifact,
        &e2e.model_identity,
        &model_identity,
        &model.model_identity,
    );
    let gates = PerformanceGates {
        input_contracts_match: kernel.contract == KERNEL_CONTRACT && e2e.contract == E2E_CONTRACT,
        exact_model_bound_to_kernel: kernel_bound,
        exact_model_bound_to_e2e: e2e_bound,
        kernel_promotion_gates_pass: kernel.promotion_kernel_verified && kernel.gates.all_pass(),
        e2e_gates_pass: e2e.readiness_verified && e2e.gates.all_pass(),
        rank_160_p99_at_most_10_microseconds: kernel.latency.p99_nanos <= 10_000,
        promotion_rank_p99_at_most_8_microseconds: kernel.latency.p99_nanos <= 8_000,
        paired_p99_overhead_at_most_15_microseconds: e2e.paired_ordering_overhead.p99_nanos
            <= 15_000,
        end_to_end_p99_at_most_1_millisecond: e2e.v3_latency.p99_nanos <= 1_000_000,
        model_artifact_at_most_64_kib: model_bytes.len() <= 64 * 1_024,
        deterministic_failures_are_zero: kernel.deterministic_failures == 0
            && e2e.deterministic_failures == 0,
        candidate_and_evidence_mismatches_are_zero: e2e.candidate_pool_mismatches == 0
            && e2e.primitive_evidence_mismatches == 0,
        allocation_growth_is_zero: e2e.v2_allocation_growths == 0 && e2e.v3_allocation_growths == 0,
    };
    let receipt = PerformanceReceipt {
        contract: CONTRACT,
        model_artifact: model_identity,
        model_identity: model.model_identity,
        kernel_receipt: file_identity(kernel_path)?,
        e2e_receipt: file_identity(e2e_path)?,
        producer_binary: current_binary_identity()?,
        kernel_p99_nanos: kernel.latency.p99_nanos,
        paired_p99_overhead_nanos: e2e.paired_ordering_overhead.p99_nanos,
        end_to_end_p99_nanos: e2e.v3_latency.p99_nanos,
        gates,
        phase_9_verified: gates.all_pass(),
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(PerformancePublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        model_identity: model.model_identity,
        gates,
        phase_9_verified: receipt.phase_9_verified,
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("decode {label} {}", path.display()))
}

fn input_binds_model(
    recorded: &Option<InputFileIdentity>,
    recorded_model_identity: &[u8; 32],
    actual: &FileIdentity,
    actual_model_identity: &[u8; 32],
) -> bool {
    recorded
        .as_ref()
        .is_some_and(|identity| identity.bytes == actual.bytes && identity.sha256 == actual.sha256)
        && recorded_model_identity == actual_model_identity
}

#[derive(Clone, Copy, Debug, Serialize)]
struct PerformanceGates {
    input_contracts_match: bool,
    exact_model_bound_to_kernel: bool,
    exact_model_bound_to_e2e: bool,
    kernel_promotion_gates_pass: bool,
    e2e_gates_pass: bool,
    rank_160_p99_at_most_10_microseconds: bool,
    promotion_rank_p99_at_most_8_microseconds: bool,
    paired_p99_overhead_at_most_15_microseconds: bool,
    end_to_end_p99_at_most_1_millisecond: bool,
    model_artifact_at_most_64_kib: bool,
    deterministic_failures_are_zero: bool,
    candidate_and_evidence_mismatches_are_zero: bool,
    allocation_growth_is_zero: bool,
}

impl PerformanceGates {
    fn all_pass(self) -> bool {
        self.input_contracts_match
            && self.exact_model_bound_to_kernel
            && self.exact_model_bound_to_e2e
            && self.kernel_promotion_gates_pass
            && self.e2e_gates_pass
            && self.rank_160_p99_at_most_10_microseconds
            && self.promotion_rank_p99_at_most_8_microseconds
            && self.paired_p99_overhead_at_most_15_microseconds
            && self.end_to_end_p99_at_most_1_millisecond
            && self.model_artifact_at_most_64_kib
            && self.deterministic_failures_are_zero
            && self.candidate_and_evidence_mismatches_are_zero
            && self.allocation_growth_is_zero
    }
}

#[derive(Debug, Serialize)]
struct PerformanceReceipt {
    contract: &'static str,
    model_artifact: FileIdentity,
    model_identity: [u8; 32],
    kernel_receipt: FileIdentity,
    e2e_receipt: FileIdentity,
    producer_binary: FileIdentity,
    kernel_p99_nanos: u64,
    paired_p99_overhead_nanos: u64,
    end_to_end_p99_nanos: u64,
    gates: PerformanceGates,
    phase_9_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct PerformancePublication {
    contract: &'static str,
    output: FileIdentity,
    model_identity: [u8; 32],
    gates: PerformanceGates,
    phase_9_verified: bool,
}

#[derive(Debug, Deserialize)]
struct KernelInput {
    contract: String,
    model_artifact: Option<InputFileIdentity>,
    model_identity: [u8; 32],
    latency: InputLatency,
    deterministic_failures: usize,
    gates: KernelInputGates,
    promotion_kernel_verified: bool,
}

#[derive(Debug, Deserialize)]
struct E2eInput {
    contract: String,
    model_artifact: Option<InputFileIdentity>,
    model_identity: [u8; 32],
    v3_latency: InputLatency,
    paired_ordering_overhead: InputLatency,
    candidate_pool_mismatches: usize,
    primitive_evidence_mismatches: usize,
    v2_allocation_growths: u64,
    v3_allocation_growths: u64,
    deterministic_failures: u64,
    gates: E2eInputGates,
    readiness_verified: bool,
}

#[derive(Debug, Deserialize)]
struct KernelInputGates {
    candidate_count_is_160: bool,
    rank_p99_at_most_10_microseconds: bool,
    promotion_rank_p99_at_most_8_microseconds: bool,
    warm_allocation_growth_is_zero: bool,
    ranker_allocations_per_query_are_zero: bool,
    ranker_locks_io_and_hashing_are_zero: bool,
    linear_model_artifact_at_most_64_kib: bool,
    deterministic_ranking_failures_are_zero: bool,
}

impl KernelInputGates {
    fn all_pass(&self) -> bool {
        self.candidate_count_is_160
            && self.rank_p99_at_most_10_microseconds
            && self.promotion_rank_p99_at_most_8_microseconds
            && self.warm_allocation_growth_is_zero
            && self.ranker_allocations_per_query_are_zero
            && self.ranker_locks_io_and_hashing_are_zero
            && self.linear_model_artifact_at_most_64_kib
            && self.deterministic_ranking_failures_are_zero
    }
}

#[derive(Debug, Deserialize)]
struct E2eInputGates {
    candidate_pool_mismatches_are_zero: bool,
    primitive_evidence_mismatches_are_zero: bool,
    paired_p99_overhead_at_most_15_microseconds: bool,
    end_to_end_p99_at_most_1_millisecond: bool,
    warm_query_allocation_growth_is_zero: bool,
    deterministic_ranking_failures_are_zero: bool,
    candidate_cap_is_160: bool,
}

impl E2eInputGates {
    fn all_pass(&self) -> bool {
        self.candidate_pool_mismatches_are_zero
            && self.primitive_evidence_mismatches_are_zero
            && self.paired_p99_overhead_at_most_15_microseconds
            && self.end_to_end_p99_at_most_1_millisecond
            && self.warm_query_allocation_growth_is_zero
            && self.deterministic_ranking_failures_are_zero
            && self.candidate_cap_is_160
    }
}

#[derive(Debug, Deserialize)]
struct InputFileIdentity {
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct InputLatency {
    p99_nanos: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_nine_requires_every_model_bound_gate() {
        let mut gates = PerformanceGates {
            input_contracts_match: true,
            exact_model_bound_to_kernel: true,
            exact_model_bound_to_e2e: true,
            kernel_promotion_gates_pass: true,
            e2e_gates_pass: true,
            rank_160_p99_at_most_10_microseconds: true,
            promotion_rank_p99_at_most_8_microseconds: true,
            paired_p99_overhead_at_most_15_microseconds: true,
            end_to_end_p99_at_most_1_millisecond: true,
            model_artifact_at_most_64_kib: true,
            deterministic_failures_are_zero: true,
            candidate_and_evidence_mismatches_are_zero: true,
            allocation_growth_is_zero: true,
        };
        assert!(gates.all_pass());
        gates.exact_model_bound_to_e2e = false;
        assert!(!gates.all_pass());
    }
}
