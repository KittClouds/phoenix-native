use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-tree-eligibility/v1";

pub(crate) fn audit(
    phase_5_path: &Path,
    phase_8_path: Option<&Path>,
    output_path: &Path,
) -> Result<TreeEligibilityPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite tree eligibility receipt {}",
            output_path.display()
        );
    }
    let phase_5: FrozenPhase5 = serde_json::from_slice(&fs::read(phase_5_path)?)
        .with_context(|| format!("decode Phase 5 receipt {}", phase_5_path.display()))?;
    if phase_5.contract != "phoenix.memory.qps-v3-corpus-readiness/v2" {
        bail!("tree audit requires a QPS V3 Phase 5 receipt");
    }
    let linear_promotion_passed = phase_8_path
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<FrozenPhase8>(&bytes).ok())
        .is_some_and(|receipt| {
            receipt.contract == "phoenix.memory.qps-v3-quality-qualification/v1"
                && receipt.phase_8_verified
        });
    let gates = TreeEligibilityGates {
        linear_model_passed_promotion: linear_promotion_passed,
        at_least_10_000_reconciled_pairs: phase_5.counts.judgments >= 10_000,
        residual_interactions_proven: false,
        blind_quality_improves_beyond_linear: false,
        rank_160_p99_below_15_microseconds: false,
        artifact_below_256_kib: false,
        constitutional_invariants_preserved: false,
    };
    let tree_eligible = gates.all_pass();
    let receipt = TreeEligibilityReceipt {
        contract: CONTRACT,
        phase_5_receipt: file_identity(phase_5_path)?,
        phase_8_receipt: phase_8_path.and_then(|path| file_identity(path).ok()),
        producer_binary: current_binary_identity()?,
        reconciled_pairs: phase_5.counts.judgments,
        gates,
        tree_eligible,
        tree_model_enabled: false,
        required_machine: "deterministic monotonic linear V3",
        policy_verified: !tree_eligible,
        phase_11_verified: false,
        phase_11_unverified_reason: "linear promotion must complete before final tree eligibility",
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(TreeEligibilityPublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        gates,
        tree_eligible,
        tree_model_enabled: false,
        policy_verified: receipt.policy_verified,
        phase_11_verified: false,
    })
}

#[derive(Clone, Copy, Debug, Serialize)]
struct TreeEligibilityGates {
    linear_model_passed_promotion: bool,
    at_least_10_000_reconciled_pairs: bool,
    residual_interactions_proven: bool,
    blind_quality_improves_beyond_linear: bool,
    rank_160_p99_below_15_microseconds: bool,
    artifact_below_256_kib: bool,
    constitutional_invariants_preserved: bool,
}

impl TreeEligibilityGates {
    fn all_pass(self) -> bool {
        self.linear_model_passed_promotion
            && self.at_least_10_000_reconciled_pairs
            && self.residual_interactions_proven
            && self.blind_quality_improves_beyond_linear
            && self.rank_160_p99_below_15_microseconds
            && self.artifact_below_256_kib
            && self.constitutional_invariants_preserved
    }
}

#[derive(Debug, Serialize)]
struct TreeEligibilityReceipt {
    contract: &'static str,
    phase_5_receipt: FileIdentity,
    phase_8_receipt: Option<FileIdentity>,
    producer_binary: FileIdentity,
    reconciled_pairs: usize,
    gates: TreeEligibilityGates,
    tree_eligible: bool,
    tree_model_enabled: bool,
    required_machine: &'static str,
    policy_verified: bool,
    phase_11_verified: bool,
    phase_11_unverified_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TreeEligibilityPublication {
    contract: &'static str,
    output: FileIdentity,
    gates: TreeEligibilityGates,
    tree_eligible: bool,
    tree_model_enabled: bool,
    policy_verified: bool,
    phase_11_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase5 {
    contract: String,
    counts: FrozenCounts,
}

#[derive(Debug, Deserialize)]
struct FrozenCounts {
    judgments: usize,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase8 {
    contract: String,
    phase_8_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tree_condition_is_mandatory() {
        let mut gates = TreeEligibilityGates {
            linear_model_passed_promotion: true,
            at_least_10_000_reconciled_pairs: true,
            residual_interactions_proven: true,
            blind_quality_improves_beyond_linear: true,
            rank_160_p99_below_15_microseconds: true,
            artifact_below_256_kib: true,
            constitutional_invariants_preserved: true,
        };
        assert!(gates.all_pass());
        gates.residual_interactions_proven = false;
        assert!(!gates.all_pass());
    }
}
