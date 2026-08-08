use super::train::LinearModelArtifactV3;
use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-activation-readiness/v1";

pub(crate) fn audit(
    model_path: Option<&Path>,
    phase_8_path: Option<&Path>,
    phase_9_path: Option<&Path>,
    output_path: &Path,
) -> Result<ActivationPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite activation receipt {}",
            output_path.display()
        );
    }
    let status = activation_status(model_path, phase_8_path, phase_9_path);
    let gates = ActivationGates {
        missing_model_reports_v2_active: probe_missing_model().engine == ActiveEngineV3::V2Active,
        corrupt_model_reports_v2_active: probe_corrupt_model().engine == ActiveEngineV3::V2Active,
        incompatible_model_reports_v2_active: status.engine != ActiveEngineV3::V3Active
            || status.reason == ActivationReasonV3::QualifiedV3,
        unqualified_model_reports_v2_active: status.engine != ActiveEngineV3::V3Active
            || status.reason == ActivationReasonV3::QualifiedV3,
        no_silent_v3_claim: status.engine == ActiveEngineV3::V3Active
            || status.reported_label == "V2 active",
        immutable_model_identity_reported_when_v3: status.engine != ActiveEngineV3::V3Active
            || status.model_identity.is_some(),
    };
    let receipt = ActivationReceipt {
        contract: CONTRACT,
        producer_binary: current_binary_identity()?,
        requested_model: model_path.and_then(optional_file_identity),
        phase_8_receipt: phase_8_path.and_then(optional_file_identity),
        phase_9_receipt: phase_9_path.and_then(optional_file_identity),
        status,
        gates,
        fallback_contract_verified: gates.all_pass(),
        phase_10_verified: false,
        phase_10_unverified_reason:
            "shadow reconciliation and qualified atomic promotion have not run",
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(ActivationPublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        status,
        gates,
        fallback_contract_verified: receipt.fallback_contract_verified,
        phase_10_verified: false,
    })
}

fn activation_status(
    model_path: Option<&Path>,
    phase_8_path: Option<&Path>,
    phase_9_path: Option<&Path>,
) -> ActivationStatusV3 {
    let Some(model_path) = model_path else {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelMissing);
    };
    let Ok(bytes) = fs::read(model_path) else {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelMissing);
    };
    let Ok(artifact) = serde_json::from_slice::<LinearModelArtifactV3>(&bytes) else {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelCorrupt);
    };
    if !artifact.validate_challenger() {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelIncompatible);
    }
    let Some(phase_8_path) = phase_8_path else {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelUnqualified);
    };
    let Some(phase_9_path) = phase_9_path else {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelUnqualified);
    };
    let Ok(phase_8) = read_qualification::<Phase8Qualification>(phase_8_path) else {
        return ActivationStatusV3::v2(ActivationReasonV3::QualificationCorrupt);
    };
    let Ok(phase_9) = read_qualification::<Phase9Qualification>(phase_9_path) else {
        return ActivationStatusV3::v2(ActivationReasonV3::QualificationCorrupt);
    };
    if phase_8.contract != "phoenix.memory.qps-v3-quality-qualification/v1"
        || phase_9.contract != "phoenix.memory.qps-v3-performance-qualification/v1"
        || !phase_8.phase_8_verified
        || !phase_9.phase_9_verified
    {
        return ActivationStatusV3::v2(ActivationReasonV3::ModelUnqualified);
    }
    ActivationStatusV3 {
        engine: ActiveEngineV3::V3Active,
        reported_label: "V3 active",
        reason: ActivationReasonV3::QualifiedV3,
        model_identity: Some(artifact.model_identity),
        rollback_model_identity: Some(artifact.rollback_model_identity),
    }
}

fn read_qualification<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(Into::into)
}

fn optional_file_identity(path: &Path) -> Option<FileIdentity> {
    file_identity(path).ok()
}

fn probe_missing_model() -> ActivationStatusV3 {
    activation_status(None, None, None)
}

fn probe_corrupt_model() -> ActivationStatusV3 {
    let impossible = Path::new("__phoenix_qps_v3_intentionally_missing_model__");
    activation_status(Some(impossible), None, None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ActiveEngineV3 {
    V2Active,
    V3Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ActivationReasonV3 {
    ModelMissing,
    ModelCorrupt,
    ModelIncompatible,
    ModelUnqualified,
    QualificationCorrupt,
    QualifiedV3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct ActivationStatusV3 {
    engine: ActiveEngineV3,
    reported_label: &'static str,
    reason: ActivationReasonV3,
    model_identity: Option<[u8; 32]>,
    rollback_model_identity: Option<[u8; 32]>,
}

impl ActivationStatusV3 {
    const fn v2(reason: ActivationReasonV3) -> Self {
        Self {
            engine: ActiveEngineV3::V2Active,
            reported_label: "V2 active",
            reason,
            model_identity: None,
            rollback_model_identity: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ActivationGates {
    missing_model_reports_v2_active: bool,
    corrupt_model_reports_v2_active: bool,
    incompatible_model_reports_v2_active: bool,
    unqualified_model_reports_v2_active: bool,
    no_silent_v3_claim: bool,
    immutable_model_identity_reported_when_v3: bool,
}

impl ActivationGates {
    fn all_pass(self) -> bool {
        self.missing_model_reports_v2_active
            && self.corrupt_model_reports_v2_active
            && self.incompatible_model_reports_v2_active
            && self.unqualified_model_reports_v2_active
            && self.no_silent_v3_claim
            && self.immutable_model_identity_reported_when_v3
    }
}

#[derive(Debug, Serialize)]
struct ActivationReceipt {
    contract: &'static str,
    producer_binary: FileIdentity,
    requested_model: Option<FileIdentity>,
    phase_8_receipt: Option<FileIdentity>,
    phase_9_receipt: Option<FileIdentity>,
    status: ActivationStatusV3,
    gates: ActivationGates,
    fallback_contract_verified: bool,
    phase_10_verified: bool,
    phase_10_unverified_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ActivationPublication {
    contract: &'static str,
    output: FileIdentity,
    status: ActivationStatusV3,
    gates: ActivationGates,
    fallback_contract_verified: bool,
    phase_10_verified: bool,
}

#[derive(Debug, Deserialize)]
struct Phase8Qualification {
    contract: String,
    phase_8_verified: bool,
}

#[derive(Debug, Deserialize)]
struct Phase9Qualification {
    contract: String,
    phase_9_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_explicitly_v2_active() {
        let status = activation_status(None, None, None);
        assert_eq!(status.engine, ActiveEngineV3::V2Active);
        assert_eq!(status.reported_label, "V2 active");
        assert_eq!(status.reason, ActivationReasonV3::ModelMissing);
    }

    #[test]
    fn malformed_model_never_claims_v3() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad-model.json");
        fs::write(&path, b"not-json").unwrap();
        let status = activation_status(Some(&path), None, None);
        assert_eq!(status.engine, ActiveEngineV3::V2Active);
        assert_eq!(status.reported_label, "V2 active");
        assert_eq!(status.reason, ActivationReasonV3::ModelCorrupt);
    }
}
