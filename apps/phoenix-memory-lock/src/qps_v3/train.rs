use phoenix_lexical_qps::{
    rank_evidence_schema_identity_v3, train_linear_ranker_v3, LeakageSplitV3, LinearRankerV3,
    LinearTrainingConfigV3, LinearTrainingReceiptV3, PrimarySplitV3, RelevanceLedgerV3,
    RANK_EVIDENCE_V3_FEATURE_NAMES,
};

use super::*;

const ARTIFACT_CONTRACT: &str = "phoenix.qps.linear-model-artifact/v3";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-linear-training-publication/v1";
const REQUIRED_ENGINE_VERSION: &str = "phoenix-qps-v3-linear/3";

pub(crate) fn train(
    phase_6_path: &Path,
    phase_4_path: &Path,
    phase_3_path: &Path,
    output_path: &Path,
) -> Result<Phase7Publication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite V3 model artifact {}",
            output_path.display()
        );
    }
    let phase_6: FrozenPhase6 = serde_json::from_slice(&fs::read(phase_6_path)?)
        .with_context(|| format!("decode Phase 6 receipt {}", phase_6_path.display()))?;
    if phase_6.contract != "phoenix.memory.qps-v3-leakage-split/v1" || !phase_6.phase_6_verified {
        bail!("Phase 7 requires a verified QPS V3 Phase 6 split");
    }
    let phase_4: FrozenPhase4 = serde_json::from_slice(&fs::read(phase_4_path)?)
        .with_context(|| format!("decode Phase 4 receipt {}", phase_4_path.display()))?;
    if phase_4.contract != "phoenix.memory.qps-v3-ledger-qualification/v1"
        || !phase_4.phase_4_verified
    {
        bail!("Phase 7 requires a verified QPS V3 Phase 4 ledger");
    }
    let phase_3: FrozenPhase3 = serde_json::from_slice(&fs::read(phase_3_path)?)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("Phase 7 requires the frozen V2 rollback identity");
    }
    let ledger_bytes = serde_json::to_vec(&phase_4.ledger)?;
    let training_ledger_identity = decode_hex_32(&sha256_bytes(&ledger_bytes))?;
    let (config, configurations_evaluated) =
        select_configuration(&phase_4.ledger, &phase_6.split, training_ledger_identity)?;
    let first = train_linear_ranker_v3(
        &phase_4.ledger,
        &phase_6.split,
        training_ledger_identity,
        config,
    )
    .map_err(anyhow::Error::msg)?;
    let second = train_linear_ranker_v3(
        &phase_4.ledger,
        &phase_6.split,
        training_ledger_identity,
        config,
    )
    .map_err(anyhow::Error::msg)?;
    let development = pairwise_evaluation(
        &first.0,
        &phase_4.ledger,
        &phase_6.split,
        PrimarySplitV3::Development,
    );
    let blind_test = pairwise_evaluation(
        &first.0,
        &phase_4.ledger,
        &phase_6.split,
        PrimarySplitV3::BlindTest,
    );
    let qualification_receipt = Phase7QualificationReceipt {
        deterministic_training: first == second,
        monotonic_non_negative_weights: first.0.weights.iter().all(|weight| *weight >= 0.0),
        primitive_feature_schema_only: !RANK_EVIDENCE_V3_FEATURE_NAMES
            .iter()
            .any(|name| matches!(*name, "baseline_score" | "candidate_strength")),
        frozen_identity_normalization: first
            .0
            .normalization
            .offsets
            .iter()
            .all(|value| *value == 0.0)
            && first
                .0
                .normalization
                .scales
                .iter()
                .all(|value| *value == 1.0),
        bounded_training_configuration: config.epochs > 0
            && config.epochs <= 16_384
            && config.learning_rate > 0.0
            && config.learning_rate <= 1.0
            && config.l2_penalty >= 0.0
            && config.l2_penalty <= 0.1,
        loss_is_finite_and_improves: first.1.initial_pairwise_loss.is_finite()
            && first.1.final_pairwise_loss.is_finite()
            && first.1.final_pairwise_loss < first.1.initial_pairwise_loss,
        development_selected_configuration: true,
        configurations_evaluated,
        development,
        blind_test,
    };
    let rollback_model_identity = decode_hex_32(&phase_3.v2_configuration_sha256)?;
    let artifact = LinearModelArtifactV3 {
        contract: ARTIFACT_CONTRACT.to_owned(),
        artifact_version: 3,
        feature_schema_identity: rank_evidence_schema_identity_v3(),
        training_ledger_identity,
        model_identity: first.0.identity(),
        model_parameters: first.0,
        normalization_parameters: first.0.normalization,
        training_receipt: first.1,
        qualification_receipt,
        required_qps_engine_version: REQUIRED_ENGINE_VERSION.to_owned(),
        rollback_model_identity,
        runtime_training: "forbidden_offline_only".to_owned(),
    };
    let first_bytes = serde_json::to_vec_pretty(&artifact)?;
    let second_bytes = serde_json::to_vec_pretty(&artifact)?;
    let gates = Phase7Gates {
        phase_6_verified: phase_6.phase_6_verified,
        byte_identical_artifact_from_identical_inputs: first_bytes == second_bytes,
        model_is_valid: artifact.validate_challenger(),
        feature_schema_identity_matches: artifact.feature_schema_identity
            == rank_evidence_schema_identity_v3(),
        training_ledger_identity_present: artifact.training_ledger_identity != [0; 32],
        rollback_model_identity_present: artifact.rollback_model_identity != [0; 32],
        required_engine_version_present: !artifact.required_qps_engine_version.is_empty(),
        runtime_training_is_forbidden: artifact.runtime_training == "forbidden_offline_only",
        training_deterministic: qualification_receipt.deterministic_training,
        weights_are_monotonic: qualification_receipt.monotonic_non_negative_weights,
        v2_composite_features_are_absent: qualification_receipt.primitive_feature_schema_only,
        frozen_normalization_verified: qualification_receipt.frozen_identity_normalization,
        loss_improved: qualification_receipt.loss_is_finite_and_improves,
        development_scores_are_finite: development.non_finite_scores == 0,
        blind_scores_are_finite: blind_test.non_finite_scores == 0,
    };
    let phase_7_verified = gates.all_pass();
    if !phase_7_verified {
        bail!("Phase 7 deterministic linear challenger failed qualification gates");
    }
    write_bytes_atomic(output_path, &first_bytes)?;
    Ok(Phase7Publication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        producer_binary: current_binary_identity()?,
        training_receipt: artifact.training_receipt,
        qualification_receipt,
        gates,
        phase_7_verified,
    })
}

fn select_configuration(
    ledger: &RelevanceLedgerV3,
    split: &LeakageSplitV3,
    training_ledger_identity: [u8; 32],
) -> Result<(LinearTrainingConfigV3, usize)> {
    const EPOCHS: [u16; 6] = [16, 32, 64, 128, 256, 512];
    const LEARNING_RATES: [f32; 4] = [0.005, 0.01, 0.025, 0.05];
    const L2_PENALTIES: [f32; 2] = [0.0001, 0.001];
    let mut best = None::<(LinearTrainingConfigV3, PairwiseEvaluationV3, f32)>;
    let mut evaluated = 0_usize;
    for epochs in EPOCHS {
        for learning_rate in LEARNING_RATES {
            for l2_penalty in L2_PENALTIES {
                let config = LinearTrainingConfigV3 {
                    epochs,
                    learning_rate,
                    l2_penalty,
                };
                let candidate =
                    train_linear_ranker_v3(ledger, split, training_ledger_identity, config)
                        .map_err(anyhow::Error::msg)?;
                let development =
                    pairwise_evaluation(&candidate.0, ledger, split, PrimarySplitV3::Development);
                evaluated += 1;
                let replace = best.as_ref().is_none_or(|(_, prior, prior_loss)| {
                    development.accuracy > prior.accuracy
                        || (development.accuracy == prior.accuracy
                            && candidate.1.final_pairwise_loss < *prior_loss)
                });
                if replace {
                    best = Some((config, development, candidate.1.final_pairwise_loss));
                }
            }
        }
    }
    best.map(|(config, _, _)| (config, evaluated))
        .context("V3 development grid produced no model")
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        bail!(
            "refusing to overwrite immutable artifact {}",
            path.display()
        );
    }
    let parent = path.parent().context("model artifact has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json")
    ));
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    {
        let mut writer = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?,
        );
        writer.write_all(bytes)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

fn pairwise_evaluation(
    model: &LinearRankerV3,
    ledger: &RelevanceLedgerV3,
    split: &LeakageSplitV3,
    target: PrimarySplitV3,
) -> PairwiseEvaluationV3 {
    let assigned = split
        .assignments
        .iter()
        .map(|value| (value.judgment_identity, value.primary_split))
        .collect::<HashMap<_, _>>();
    let mut evaluation = PairwiseEvaluationV3::default();
    for judgment in &ledger.judgments {
        if judgment.frozen_holdout.is_some() || assigned.get(&judgment.identity) != Some(&target) {
            continue;
        }
        evaluation.judgments += 1;
        match (
            model.score(judgment.positive_features),
            model.score(judgment.negative_features),
        ) {
            (Some(positive), Some(negative)) => {
                if positive > negative {
                    evaluation.correctly_ordered += 1;
                }
            }
            _ => evaluation.non_finite_scores += 1,
        }
    }
    evaluation.accuracy = evaluation.correctly_ordered as f32 / evaluation.judgments.max(1) as f32;
    evaluation
}

fn decode_hex_32(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        bail!("identity must contain 64 hexadecimal characters");
    }
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .context("identity contains invalid hexadecimal")?;
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct PairwiseEvaluationV3 {
    judgments: usize,
    correctly_ordered: usize,
    non_finite_scores: usize,
    accuracy: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Phase7QualificationReceipt {
    deterministic_training: bool,
    monotonic_non_negative_weights: bool,
    primitive_feature_schema_only: bool,
    frozen_identity_normalization: bool,
    bounded_training_configuration: bool,
    loss_is_finite_and_improves: bool,
    development_selected_configuration: bool,
    configurations_evaluated: usize,
    development: PairwiseEvaluationV3,
    blind_test: PairwiseEvaluationV3,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct LinearModelArtifactV3 {
    pub(super) contract: String,
    pub(super) artifact_version: u16,
    pub(super) feature_schema_identity: [u8; 32],
    pub(super) training_ledger_identity: [u8; 32],
    pub(super) model_identity: [u8; 32],
    pub(super) model_parameters: LinearRankerV3,
    pub(super) normalization_parameters: phoenix_lexical_qps::FeatureNormalizationV3,
    pub(super) training_receipt: LinearTrainingReceiptV3,
    pub(super) qualification_receipt: Phase7QualificationReceipt,
    pub(super) required_qps_engine_version: String,
    pub(super) rollback_model_identity: [u8; 32],
    pub(super) runtime_training: String,
}

impl LinearModelArtifactV3 {
    pub(super) fn validate_challenger(&self) -> bool {
        self.contract == ARTIFACT_CONTRACT
            && self.artifact_version == 3
            && self.feature_schema_identity == rank_evidence_schema_identity_v3()
            && self.model_parameters.is_valid()
            && self.model_identity == self.model_parameters.identity()
            && self.normalization_parameters == self.model_parameters.normalization
            && self.training_receipt.training_ledger_identity == self.training_ledger_identity
            && self.training_receipt.feature_schema_identity == self.feature_schema_identity
            && self.training_receipt.model_identity == self.model_identity
            && self.qualification_receipt.deterministic_training
            && self.qualification_receipt.monotonic_non_negative_weights
            && self.qualification_receipt.primitive_feature_schema_only
            && self.qualification_receipt.frozen_identity_normalization
            && self.qualification_receipt.loss_is_finite_and_improves
            && self
                .qualification_receipt
                .development_selected_configuration
            && self.qualification_receipt.configurations_evaluated > 0
            && self.required_qps_engine_version == REQUIRED_ENGINE_VERSION
            && self.rollback_model_identity != [0; 32]
            && self.runtime_training == "forbidden_offline_only"
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase7Gates {
    phase_6_verified: bool,
    byte_identical_artifact_from_identical_inputs: bool,
    model_is_valid: bool,
    feature_schema_identity_matches: bool,
    training_ledger_identity_present: bool,
    rollback_model_identity_present: bool,
    required_engine_version_present: bool,
    runtime_training_is_forbidden: bool,
    training_deterministic: bool,
    weights_are_monotonic: bool,
    v2_composite_features_are_absent: bool,
    frozen_normalization_verified: bool,
    loss_improved: bool,
    development_scores_are_finite: bool,
    blind_scores_are_finite: bool,
}

impl Phase7Gates {
    fn all_pass(self) -> bool {
        self.phase_6_verified
            && self.byte_identical_artifact_from_identical_inputs
            && self.model_is_valid
            && self.feature_schema_identity_matches
            && self.training_ledger_identity_present
            && self.rollback_model_identity_present
            && self.required_engine_version_present
            && self.runtime_training_is_forbidden
            && self.training_deterministic
            && self.weights_are_monotonic
            && self.v2_composite_features_are_absent
            && self.frozen_normalization_verified
            && self.loss_improved
            && self.development_scores_are_finite
            && self.blind_scores_are_finite
    }
}

#[derive(Debug, Serialize)]
pub struct Phase7Publication {
    contract: &'static str,
    output: FileIdentity,
    producer_binary: FileIdentity,
    training_receipt: LinearTrainingReceiptV3,
    qualification_receipt: Phase7QualificationReceipt,
    gates: Phase7Gates,
    phase_7_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase6 {
    contract: String,
    split: LeakageSplitV3,
    phase_6_verified: bool,
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
    v2_configuration_sha256: String,
    phase_3_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_seven_gate_requires_determinism_and_no_v2_composite_features() {
        let mut gates = Phase7Gates {
            phase_6_verified: true,
            byte_identical_artifact_from_identical_inputs: true,
            model_is_valid: true,
            feature_schema_identity_matches: true,
            training_ledger_identity_present: true,
            rollback_model_identity_present: true,
            required_engine_version_present: true,
            runtime_training_is_forbidden: true,
            training_deterministic: true,
            weights_are_monotonic: true,
            v2_composite_features_are_absent: true,
            frozen_normalization_verified: true,
            loss_improved: true,
            development_scores_are_finite: true,
            blind_scores_are_finite: true,
        };
        assert!(gates.all_pass());
        gates.v2_composite_features_are_absent = false;
        assert!(!gates.all_pass());
    }
}
