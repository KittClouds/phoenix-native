use phoenix_lexical_qps::{
    FrozenHoldoutV3, KeyedIdentity, LeakageSplitAuditV3, LeakageSplitV3, RelevanceLedgerV3,
    LEAKAGE_SPLIT_V3_CONTRACT, LEAKAGE_SPLIT_V3_SCHEMA_VERSION,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-leakage-split/v1";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-leakage-split-publication/v1";

pub(crate) fn split(
    phase_5_path: &Path,
    phase_4_path: &Path,
    phase_3_path: &Path,
    output_path: &Path,
) -> Result<Phase6Publication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite split receipt {}",
            output_path.display()
        );
    }
    let phase_5: FrozenPhase5 = serde_json::from_slice(&fs::read(phase_5_path)?)
        .with_context(|| format!("decode Phase 5 receipt {}", phase_5_path.display()))?;
    if phase_5.contract != "phoenix.memory.qps-v3-corpus-readiness/v2" || !phase_5.phase_5_verified
    {
        bail!("Phase 6 requires a verified QPS V3 Phase 5 training corpus");
    }
    let phase_4: FrozenPhase4 = serde_json::from_slice(&fs::read(phase_4_path)?)
        .with_context(|| format!("decode Phase 4 receipt {}", phase_4_path.display()))?;
    if phase_4.contract != "phoenix.memory.qps-v3-ledger-qualification/v1"
        || !phase_4.phase_4_verified
    {
        bail!("Phase 6 requires a verified QPS V3 Phase 4 ledger");
    }
    let phase_3: FrozenPhase3 = serde_json::from_slice(&fs::read(phase_3_path)?)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("Phase 6 requires the frozen constitutional and release cohorts");
    }

    let split = LeakageSplitV3::build(&phase_4.ledger).map_err(anyhow::Error::msg)?;
    let repeated = LeakageSplitV3::build(&phase_4.ledger).map_err(anyhow::Error::msg)?;
    let frozen_constitutional = phase_4
        .ledger
        .judgments
        .iter()
        .filter(|judgment| {
            judgment.frozen_holdout == Some(FrozenHoldoutV3::ConstitutionalRegression)
        })
        .count();
    let frozen_longmemeval = phase_4
        .ledger
        .judgments
        .iter()
        .filter(|judgment| judgment.frozen_holdout == Some(FrozenHoldoutV3::LongMemEvalRelease))
        .count();
    let gates = Phase6Gates {
        phase_5_training_corpus_verified: phase_5.phase_5_verified,
        split_schema_is_v3: split.contract == LEAKAGE_SPLIT_V3_CONTRACT
            && split.schema_version == LEAKAGE_SPLIT_V3_SCHEMA_VERSION,
        deterministic_assignment: split == repeated,
        grouped_ratio_is_60_20_20: split.audit.maximum_ratio_deviation_bps
            <= split.audit.nearest_feasible_ratio_tolerance_bps,
        query_family_leaks_are_zero: split.audit.query_family_leaks == 0,
        source_leaks_are_zero: split.audit.source_leaks == 0,
        near_duplicate_cluster_leaks_are_zero: split.audit.near_duplicate_cluster_leaks == 0,
        entity_or_identifier_family_leaks_are_zero: split.audit.entity_or_identifier_family_leaks
            == 0,
        collection_cohort_leaks_are_zero: split.audit.collection_cohort_leaks == 0,
        future_time_holdout_is_strict: split.audit.future_time_ordering_violations == 0
            && split.audit.future_time_holdout_judgments > 0,
        unseen_source_holdout_is_strict: split.audit.source_leaks == 0
            && split.audit.unseen_source_holdout_judgments > 0,
        every_major_class_is_represented: split.audit.training_major_classes_missing == 0
            && split.audit.development_major_classes_missing == 0
            && split.audit.blind_test_major_classes_missing == 0,
        constitutional_regression_suite_is_frozen: !phase_3.mixed_suite.queries.is_empty()
            && frozen_constitutional > 0,
        longmemeval_release_cohort_is_frozen: !phase_3.longmemeval_release.queries.is_empty()
            && phase_5.integrity.release_cohort_intersections_are_zero,
        frozen_holdouts_are_never_training: split.audit.frozen_holdout_training_assignments == 0,
        every_eligible_judgment_is_assigned_once: split.audit.duplicate_or_missing_assignments == 0,
        split_audit_qualified: split.audit.is_qualified(),
    };
    let connectivity = connectivity_diagnostic(&phase_4.ledger);
    let phase_6_verified = gates.all_pass();
    let receipt = Phase6Receipt {
        contract: CONTRACT,
        phase_5_receipt: file_identity(phase_5_path)?,
        phase_4_receipt: file_identity(phase_4_path)?,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        frozen_constitutional_judgments: frozen_constitutional,
        frozen_longmemeval_judgments: frozen_longmemeval,
        frozen_constitutional_queries: phase_3.mixed_suite.queries.len(),
        frozen_longmemeval_queries: phase_3.longmemeval_release.queries.len(),
        split,
        connectivity,
        gates,
        phase_6_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(Phase6Publication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        audit: receipt.split.audit,
        gates,
        phase_6_verified,
    })
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase6Gates {
    phase_5_training_corpus_verified: bool,
    split_schema_is_v3: bool,
    deterministic_assignment: bool,
    grouped_ratio_is_60_20_20: bool,
    query_family_leaks_are_zero: bool,
    source_leaks_are_zero: bool,
    near_duplicate_cluster_leaks_are_zero: bool,
    entity_or_identifier_family_leaks_are_zero: bool,
    collection_cohort_leaks_are_zero: bool,
    future_time_holdout_is_strict: bool,
    unseen_source_holdout_is_strict: bool,
    every_major_class_is_represented: bool,
    constitutional_regression_suite_is_frozen: bool,
    longmemeval_release_cohort_is_frozen: bool,
    frozen_holdouts_are_never_training: bool,
    every_eligible_judgment_is_assigned_once: bool,
    split_audit_qualified: bool,
}

impl Phase6Gates {
    fn all_pass(self) -> bool {
        self.phase_5_training_corpus_verified
            && self.split_schema_is_v3
            && self.deterministic_assignment
            && self.grouped_ratio_is_60_20_20
            && self.query_family_leaks_are_zero
            && self.source_leaks_are_zero
            && self.near_duplicate_cluster_leaks_are_zero
            && self.entity_or_identifier_family_leaks_are_zero
            && self.collection_cohort_leaks_are_zero
            && self.future_time_holdout_is_strict
            && self.unseen_source_holdout_is_strict
            && self.every_major_class_is_represented
            && self.constitutional_regression_suite_is_frozen
            && self.longmemeval_release_cohort_is_frozen
            && self.frozen_holdouts_are_never_training
            && self.every_eligible_judgment_is_assigned_once
            && self.split_audit_qualified
    }
}

#[derive(Debug, Serialize)]
struct Phase6Receipt {
    contract: &'static str,
    phase_5_receipt: FileIdentity,
    phase_4_receipt: FileIdentity,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    frozen_constitutional_judgments: usize,
    frozen_longmemeval_judgments: usize,
    frozen_constitutional_queries: usize,
    frozen_longmemeval_queries: usize,
    split: LeakageSplitV3,
    connectivity: ConnectivityDiagnostic,
    gates: Phase6Gates,
    phase_6_verified: bool,
}

#[derive(Debug, Serialize)]
struct ConnectivityDiagnostic {
    all_axes: ComponentShape,
    without_query_family: ComponentShape,
    without_source: ComponentShape,
    without_near_duplicate: ComponentShape,
    without_entity_family: ComponentShape,
    without_collection_cohort: ComponentShape,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ComponentShape {
    components: usize,
    largest_judgments: usize,
}

fn connectivity_diagnostic(ledger: &RelevanceLedgerV3) -> ConnectivityDiagnostic {
    ConnectivityDiagnostic {
        all_axes: component_shape(ledger, None),
        without_query_family: component_shape(ledger, Some(0)),
        without_source: component_shape(ledger, Some(1)),
        without_near_duplicate: component_shape(ledger, Some(2)),
        without_entity_family: component_shape(ledger, Some(3)),
        without_collection_cohort: component_shape(ledger, Some(4)),
    }
}

fn component_shape(ledger: &RelevanceLedgerV3, omitted_axis: Option<u8>) -> ComponentShape {
    let eligible = ledger.active_model_training_indices();
    let mut sets = DiagnosticDisjointSets::new(eligible.len());
    let mut groups = [
        HashMap::<KeyedIdentity, usize>::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
    ];
    for (local, ledger_index) in eligible.iter().copied().enumerate() {
        let split = ledger.judgments[ledger_index].split_groups;
        if omitted_axis != Some(0) {
            diagnostic_union(
                &mut sets,
                &mut groups[0],
                split.query_family_identity,
                local,
            );
        }
        if omitted_axis != Some(1) {
            diagnostic_union(
                &mut sets,
                &mut groups[1],
                split.positive_source_identity,
                local,
            );
            diagnostic_union(
                &mut sets,
                &mut groups[1],
                split.negative_source_identity,
                local,
            );
        }
        if omitted_axis != Some(2) {
            diagnostic_union(
                &mut sets,
                &mut groups[2],
                split.positive_near_duplicate_cluster_identity,
                local,
            );
            diagnostic_union(
                &mut sets,
                &mut groups[2],
                split.negative_near_duplicate_cluster_identity,
                local,
            );
        }
        if omitted_axis != Some(3) {
            diagnostic_union(
                &mut sets,
                &mut groups[3],
                split.entity_or_identifier_family_identity,
                local,
            );
        }
        if omitted_axis != Some(4) {
            diagnostic_union(
                &mut sets,
                &mut groups[4],
                split.collection_cohort_identity,
                local,
            );
        }
    }
    let mut sizes = HashMap::<usize, usize>::new();
    for local in 0..eligible.len() {
        *sizes.entry(sets.find(local)).or_default() += 1;
    }
    ComponentShape {
        components: sizes.len(),
        largest_judgments: sizes.values().copied().max().unwrap_or(0),
    }
}

fn diagnostic_union(
    sets: &mut DiagnosticDisjointSets,
    groups: &mut HashMap<KeyedIdentity, usize>,
    identity: KeyedIdentity,
    local: usize,
) {
    if let Some(other) = groups.insert(identity, local) {
        sets.union(local, other);
    }
}

struct DiagnosticDisjointSets {
    parents: Vec<usize>,
}

impl DiagnosticDisjointSets {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
        }
    }

    fn find(&mut self, value: usize) -> usize {
        let parent = self.parents[value];
        if parent != value {
            self.parents[value] = self.find(parent);
        }
        self.parents[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Phase6Publication {
    contract: &'static str,
    output: FileIdentity,
    audit: LeakageSplitAuditV3,
    gates: Phase6Gates,
    phase_6_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase5 {
    contract: String,
    integrity: FrozenIntegrity,
    phase_5_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenIntegrity {
    release_cohort_intersections_are_zero: bool,
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
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_six_gate_requires_every_holdout_and_leakage_check() {
        let mut gates = Phase6Gates {
            phase_5_training_corpus_verified: true,
            split_schema_is_v3: true,
            deterministic_assignment: true,
            grouped_ratio_is_60_20_20: true,
            query_family_leaks_are_zero: true,
            source_leaks_are_zero: true,
            near_duplicate_cluster_leaks_are_zero: true,
            entity_or_identifier_family_leaks_are_zero: true,
            collection_cohort_leaks_are_zero: true,
            future_time_holdout_is_strict: true,
            unseen_source_holdout_is_strict: true,
            every_major_class_is_represented: true,
            constitutional_regression_suite_is_frozen: true,
            longmemeval_release_cohort_is_frozen: true,
            frozen_holdouts_are_never_training: true,
            every_eligible_judgment_is_assigned_once: true,
            split_audit_qualified: true,
        };
        assert!(gates.all_pass());
        gates.source_leaks_are_zero = false;
        assert!(!gates.all_pass());
    }
}
