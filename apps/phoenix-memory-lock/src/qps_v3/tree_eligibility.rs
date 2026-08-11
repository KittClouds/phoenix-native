use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-tree-eligibility/v2";
const TREE_EVIDENCE_CONTRACT: &str = "phoenix.memory.qps-v3-tree-challenger-evidence/v1";
const PHASE_5_CONTRACT: &str = "phoenix.memory.qps-v3-corpus-readiness/v2";
const PHASE_8_CONTRACT: &str = "phoenix.memory.qps-v3-quality-qualification/v1";
const TREE_ARTIFACT_LIMIT_BYTES: u64 = 256 * 1024;
const TREE_RANK_P99_LIMIT_NANOS: u64 = 15_000;

pub(crate) fn audit(
    phase_5_path: &Path,
    phase_8_path: &Path,
    tree_evidence_path: Option<&Path>,
    output_path: &Path,
) -> Result<TreeEligibilityPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite tree eligibility receipt {}",
            output_path.display()
        );
    }

    let phase_5: FrozenPhase5 = read_receipt(phase_5_path, "Phase 5")?;
    if phase_5.contract != PHASE_5_CONTRACT || !phase_5.phase_5_verified {
        bail!("tree audit requires a verified QPS V3 Phase 5 receipt");
    }

    let phase_8: FrozenPhase8 = read_receipt(phase_8_path, "Phase 8")?;
    if phase_8.contract != PHASE_8_CONTRACT {
        bail!("tree audit requires a QPS V3 Phase 8 quality receipt");
    }

    let phase_5_receipt = file_identity(phase_5_path)?;
    let phase_8_receipt = file_identity(phase_8_path)?;
    let tree_evidence = tree_evidence_path
        .map(|path| read_tree_evidence(path, phase_5.counts.judgments))
        .transpose()?;
    let evidence_bindings_match = tree_evidence.as_ref().is_some_and(|evidence| {
        evidence.bindings_match(&phase_5_receipt, &phase_8_receipt, &phase_8.model_identity)
    });
    let gates = evaluate_gates(
        phase_5.counts.judgments,
        phase_8.phase_8_verified,
        tree_evidence.as_ref(),
        evidence_bindings_match,
    );
    let tree_eligible = gates.all_pass();
    let disposition = TreeDisposition::from_gates(gates, tree_evidence.is_some());

    // Phase 11 is an eligibility audit. It never activates a model. A later,
    // explicit promotion path must bind and atomically publish any eligible tree.
    let tree_model_enabled = false;
    let policy_verified = !tree_model_enabled || tree_eligible;
    let phase_11_verified = policy_verified;
    let active_machine_requirement = if phase_8.phase_8_verified {
        "qualified deterministic monotonic linear V3 or explicit V2 rollback"
    } else {
        "V2 active until deterministic monotonic linear V3 qualifies"
    };

    let receipt = TreeEligibilityReceipt {
        contract: CONTRACT,
        phase_5_receipt,
        phase_8_receipt,
        tree_evidence: tree_evidence_path.map(file_identity).transpose()?,
        producer_binary: current_binary_identity()?,
        reconciled_pairs: phase_5.counts.judgments,
        gates,
        disposition,
        tree_eligible,
        tree_model_enabled,
        active_machine_requirement,
        policy_verified,
        phase_11_scope: "tree eligibility decision only; QPS V3 promotion is not asserted",
        phase_11_verified,
    };
    write_json_atomic(output_path, &receipt)?;

    Ok(TreeEligibilityPublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        gates,
        disposition,
        tree_eligible,
        tree_model_enabled,
        policy_verified,
        phase_11_verified,
    })
}

fn read_receipt<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("decode {label} receipt {}", path.display()))
}

fn read_tree_evidence(path: &Path, reconciled_pairs: usize) -> Result<FrozenTreeEvidence> {
    let evidence: FrozenTreeEvidence = read_receipt(path, "tree challenger evidence")?;
    if evidence.contract != TREE_EVIDENCE_CONTRACT {
        bail!("tree audit requires QPS V3 tree challenger evidence");
    }
    if evidence.reconciled_pairs != reconciled_pairs {
        bail!(
            "tree evidence reconciled-pair count {} does not match Phase 5 count {}",
            evidence.reconciled_pairs,
            reconciled_pairs
        );
    }
    Ok(evidence)
}

fn evaluate_gates(
    reconciled_pairs: usize,
    linear_model_passed_promotion: bool,
    evidence: Option<&FrozenTreeEvidence>,
    evidence_bindings_match: bool,
) -> TreeEligibilityGates {
    TreeEligibilityGates {
        linear_model_passed_promotion,
        at_least_10_000_reconciled_pairs: reconciled_pairs >= 10_000,
        residual_interactions_proven: evidence.is_some_and(|value| {
            value.residual.repeatable_feature_interactions > 0
                && value.residual.independent_holdout_slices >= 2
        }),
        blind_quality_improves_beyond_linear: evidence.is_some_and(|value| {
            value.blind.is_finite()
                && value.blind.tree_ndcg_at_10 > value.blind.linear_ndcg_at_10
                && value.blind.tree_mrr > value.blind.linear_mrr
        }),
        rank_160_p99_below_15_microseconds: evidence.is_some_and(|value| {
            value.performance.candidate_count == 160
                && value.performance.rank_p99_nanos < TREE_RANK_P99_LIMIT_NANOS
                && value.performance.samples > 0
        }),
        artifact_below_256_kib: evidence.is_some_and(|value| {
            value.tree_model_artifact.bytes < TREE_ARTIFACT_LIMIT_BYTES
                && value.tree_model_artifact.bytes > 0
                && evidence_bindings_match
        }),
        constitutional_invariants_preserved: evidence.is_some_and(|value| {
            value.monotonic_constraints_verified
                && value.constitutional_regressions == 0
                && value.deterministic_ranking_failures == 0
        }),
        evidence_bindings_match,
    }
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
    evidence_bindings_match: bool,
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
            && self.evidence_bindings_match
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TreeDisposition {
    DeferredLinearUnqualified,
    DeferredInsufficientPairs,
    DeferredNoChallengerEvidence,
    DeferredChallengerUnqualified,
    EligibleForPromotionReview,
}

impl TreeDisposition {
    fn from_gates(gates: TreeEligibilityGates, evidence_supplied: bool) -> Self {
        if !gates.linear_model_passed_promotion {
            Self::DeferredLinearUnqualified
        } else if !gates.at_least_10_000_reconciled_pairs {
            Self::DeferredInsufficientPairs
        } else if !evidence_supplied {
            Self::DeferredNoChallengerEvidence
        } else if !gates.all_pass() {
            Self::DeferredChallengerUnqualified
        } else {
            Self::EligibleForPromotionReview
        }
    }
}

#[derive(Debug, Serialize)]
struct TreeEligibilityReceipt {
    contract: &'static str,
    phase_5_receipt: FileIdentity,
    phase_8_receipt: FileIdentity,
    tree_evidence: Option<FileIdentity>,
    producer_binary: FileIdentity,
    reconciled_pairs: usize,
    gates: TreeEligibilityGates,
    disposition: TreeDisposition,
    tree_eligible: bool,
    tree_model_enabled: bool,
    active_machine_requirement: &'static str,
    policy_verified: bool,
    phase_11_scope: &'static str,
    phase_11_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct TreeEligibilityPublication {
    contract: &'static str,
    output: FileIdentity,
    gates: TreeEligibilityGates,
    disposition: TreeDisposition,
    tree_eligible: bool,
    tree_model_enabled: bool,
    policy_verified: bool,
    phase_11_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase5 {
    contract: String,
    phase_5_verified: bool,
    counts: FrozenCounts,
}

#[derive(Debug, Deserialize)]
struct FrozenCounts {
    judgments: usize,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase8 {
    contract: String,
    model_identity: [u8; 32],
    phase_8_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenTreeEvidence {
    contract: String,
    reconciled_pairs: usize,
    phase_5_receipt_sha256: String,
    phase_8_receipt_sha256: String,
    linear_model_identity: [u8; 32],
    tree_model_identity_sha256: String,
    tree_model_artifact: FrozenFileIdentity,
    residual: FrozenResidualEvidence,
    blind: FrozenBlindMetrics,
    performance: FrozenTreePerformance,
    monotonic_constraints_verified: bool,
    constitutional_regressions: usize,
    deterministic_ranking_failures: usize,
}

impl FrozenTreeEvidence {
    fn bindings_match(
        &self,
        phase_5_receipt: &FileIdentity,
        phase_8_receipt: &FileIdentity,
        linear_model_identity: &[u8; 32],
    ) -> bool {
        self.phase_5_receipt_sha256 == phase_5_receipt.sha256
            && self.phase_8_receipt_sha256 == phase_8_receipt.sha256
            && &self.linear_model_identity == linear_model_identity
            && self.tree_model_identity_sha256 == self.tree_model_artifact.sha256
            && file_identity(Path::new(&self.tree_model_artifact.path)).is_ok_and(|actual| {
                actual.bytes == self.tree_model_artifact.bytes
                    && actual.sha256 == self.tree_model_artifact.sha256
            })
    }
}

#[derive(Debug, Deserialize)]
struct FrozenFileIdentity {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct FrozenResidualEvidence {
    repeatable_feature_interactions: usize,
    independent_holdout_slices: usize,
}

#[derive(Debug, Deserialize)]
struct FrozenBlindMetrics {
    linear_ndcg_at_10: f64,
    tree_ndcg_at_10: f64,
    linear_mrr: f64,
    tree_mrr: f64,
}

impl FrozenBlindMetrics {
    fn is_finite(&self) -> bool {
        [
            self.linear_ndcg_at_10,
            self.tree_ndcg_at_10,
            self.linear_mrr,
            self.tree_mrr,
        ]
        .into_iter()
        .all(f64::is_finite)
    }
}

#[derive(Debug, Deserialize)]
struct FrozenTreePerformance {
    candidate_count: usize,
    samples: usize,
    rank_p99_nanos: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_gates() -> TreeEligibilityGates {
        TreeEligibilityGates {
            linear_model_passed_promotion: true,
            at_least_10_000_reconciled_pairs: true,
            residual_interactions_proven: true,
            blind_quality_improves_beyond_linear: true,
            rank_160_p99_below_15_microseconds: true,
            artifact_below_256_kib: true,
            constitutional_invariants_preserved: true,
            evidence_bindings_match: true,
        }
    }

    #[test]
    fn every_tree_condition_is_mandatory() {
        let mut gates = passing_gates();
        assert!(gates.all_pass());
        gates.residual_interactions_proven = false;
        assert!(!gates.all_pass());
    }

    #[test]
    fn failed_linear_promotion_is_a_verified_defer_decision() {
        let mut gates = passing_gates();
        gates.linear_model_passed_promotion = false;
        assert_eq!(
            TreeDisposition::from_gates(gates, true),
            TreeDisposition::DeferredLinearUnqualified
        );
        assert!(!gates.all_pass());
    }

    #[test]
    fn passing_evidence_only_makes_tree_eligible_for_review() {
        let gates = passing_gates();
        assert_eq!(
            TreeDisposition::from_gates(gates, true),
            TreeDisposition::EligibleForPromotionReview
        );
        assert!(gates.all_pass());
    }

    #[test]
    fn current_unqualified_linear_receipt_produces_verified_defer_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let phase_5_path = directory.path().join("phase5.json");
        let phase_8_path = directory.path().join("phase8.json");
        let output_path = directory.path().join("phase11.json");
        fs::write(
            &phase_5_path,
            serde_json::to_vec(&serde_json::json!({
                "contract": PHASE_5_CONTRACT,
                "phase_5_verified": true,
                "counts": { "judgments": 7_828 }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &phase_8_path,
            serde_json::to_vec(&serde_json::json!({
                "contract": PHASE_8_CONTRACT,
                "model_identity": vec![0; 32],
                "phase_8_verified": false
            }))
            .unwrap(),
        )
        .unwrap();

        let publication = audit(&phase_5_path, &phase_8_path, None, &output_path).unwrap();
        assert!(publication.phase_11_verified);
        assert!(publication.policy_verified);
        assert!(!publication.tree_eligible);
        assert!(!publication.tree_model_enabled);
        assert_eq!(
            publication.disposition,
            TreeDisposition::DeferredLinearUnqualified
        );

        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
        assert_eq!(receipt["phase_11_verified"], true);
        assert_eq!(
            receipt["phase_11_scope"],
            "tree eligibility decision only; QPS V3 promotion is not asserted"
        );
    }

    #[test]
    fn concrete_tree_evidence_must_pass_every_numeric_and_binding_gate() {
        let directory = tempfile::tempdir().unwrap();
        let model_path = directory.path().join("tree-model.bin");
        fs::write(&model_path, b"monotonic-tree-model").unwrap();
        let identity = file_identity(&model_path).unwrap();
        let evidence = FrozenTreeEvidence {
            contract: TREE_EVIDENCE_CONTRACT.to_owned(),
            reconciled_pairs: 10_000,
            phase_5_receipt_sha256: "11".repeat(32),
            phase_8_receipt_sha256: "22".repeat(32),
            linear_model_identity: [3; 32],
            tree_model_identity_sha256: identity.sha256.clone(),
            tree_model_artifact: FrozenFileIdentity {
                path: identity.path,
                bytes: identity.bytes,
                sha256: identity.sha256,
            },
            residual: FrozenResidualEvidence {
                repeatable_feature_interactions: 2,
                independent_holdout_slices: 3,
            },
            blind: FrozenBlindMetrics {
                linear_ndcg_at_10: 0.72,
                tree_ndcg_at_10: 0.74,
                linear_mrr: 0.91,
                tree_mrr: 0.92,
            },
            performance: FrozenTreePerformance {
                candidate_count: 160,
                samples: 50_000,
                rank_p99_nanos: 14_999,
            },
            monotonic_constraints_verified: true,
            constitutional_regressions: 0,
            deterministic_ranking_failures: 0,
        };

        let phase_5_receipt = FileIdentity {
            path: "phase5.json".to_owned(),
            bytes: 1,
            sha256: "11".repeat(32),
        };
        let phase_8_receipt = FileIdentity {
            path: "phase8.json".to_owned(),
            bytes: 1,
            sha256: "22".repeat(32),
        };
        let bindings_match = evidence.bindings_match(&phase_5_receipt, &phase_8_receipt, &[3; 32]);
        let gates = evaluate_gates(10_000, true, Some(&evidence), bindings_match);
        assert!(gates.all_pass());

        let wrong_linear_identity = [4; 32];
        assert!(!evidence.bindings_match(
            &phase_5_receipt,
            &phase_8_receipt,
            &wrong_linear_identity
        ));
    }
}
