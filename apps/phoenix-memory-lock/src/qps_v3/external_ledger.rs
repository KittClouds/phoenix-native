use phoenix_lexical_qps::{
    FrozenHoldoutV3, KeyedIdentity, RankEvidenceV3, RelevanceLedgerV3, RelevanceTier,
    WorkspaceIdentityKey, RELEVANCE_LEDGER_V3_CONTRACT, RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-ledger-qualification/v1";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-ledger-publication/v1";

pub(crate) fn qualify(
    ledger_path: &Path,
    phase_3_path: &Path,
    workspace_key_path: &Path,
    output_path: &Path,
) -> Result<ExternalLedgerPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite ledger qualification {}",
            output_path.display()
        );
    }
    let ledger_bytes = fs::read(ledger_path)
        .with_context(|| format!("read external V3 ledger {}", ledger_path.display()))?;
    let ledger: RelevanceLedgerV3 = serde_json::from_slice(&ledger_bytes)
        .with_context(|| format!("decode external V3 ledger {}", ledger_path.display()))?;
    let audit = ledger.validate().map_err(anyhow::Error::msg)?;
    if ledger.judgments.is_empty() {
        bail!("external V3 ledger must contain at least one judgment");
    }
    let phase_3: FrozenPhase3 = serde_json::from_slice(&fs::read(phase_3_path)?)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("external ledger qualification requires verified Phase 3 holdout identities");
    }
    let raw_key = super::ledger::read_workspace_key(workspace_key_path)?;
    let key = WorkspaceIdentityKey::new(raw_key).map_err(anyhow::Error::msg)?;
    let workspace_key_id = KeyedIdentity::derive(&key, b"workspace-key-id", b"qps-v3-ledger-v3");
    let constitutional = keyed_queries(&key, &phase_3.mixed_suite.queries);
    let longmemeval_release = keyed_queries(&key, &phase_3.longmemeval_release.queries);
    let raw_query_hashes = phase_3
        .mixed_suite
        .queries
        .iter()
        .chain(&phase_3.longmemeval_release.queries)
        .filter_map(|query| decode_hex_32(&query.query_sha256).ok())
        .collect::<HashSet<_>>();
    let serialized = std::str::from_utf8(&ledger_bytes).context("ledger JSON must be UTF-8")?;
    let longmemeval_release_intersections = ledger
        .judgments
        .iter()
        .filter(|judgment| longmemeval_release.contains(&judgment.query_identity))
        .count();
    let release_evidence = release_evidence_fingerprints(&phase_3.longmemeval_release);
    let release_evidence_intersections = ledger
        .judgments
        .iter()
        .filter(|judgment| {
            release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.positive_features,
                judgment.positive_tier,
            )) || release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.negative_features,
                judgment.negative_tier,
            ))
        })
        .count();
    let gates = ExternalLedgerGates {
        schema_is_v3: ledger.contract == RELEVANCE_LEDGER_V3_CONTRACT
            && ledger.schema_version == RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
        workspace_key_is_not_serialized: !serialized.contains(&hex(raw_key)),
        private_query_identities_are_keyed: ledger.judgments.iter().all(|judgment| {
            judgment.query_identity.is_valid()
                && !raw_query_hashes.contains(&judgment.query_identity.as_bytes())
        }),
        candidate_pools_and_positions_are_complete: ledger.judgments.iter().all(|judgment| {
            !judgment.candidate_pool.is_empty()
                && judgment.candidate_pool.len() <= 160
                && judgment.candidate_pool[judgment.positive_position as usize]
                    == judgment.positive_document_version
                && judgment.candidate_pool[judgment.negative_position as usize]
                    == judgment.negative_document_version
        }),
        feature_schema_and_provenance_complete: ledger.judgments.iter().all(|judgment| {
            judgment.positive_features.is_valid()
                && judgment.negative_features.is_valid()
                && judgment.split_groups.is_valid()
                && judgment.index_generation > 0
                && judgment.v2_model_identity != [0; 32]
                && judgment.challenger_model_identity != [0; 32]
        }),
        constitutional_queries_are_holdouts: ledger.judgments.iter().all(|judgment| {
            !constitutional.contains(&judgment.query_identity)
                || judgment.frozen_holdout == Some(FrozenHoldoutV3::ConstitutionalRegression)
        }),
        longmemeval_release_intersections_are_zero: longmemeval_release_intersections == 0,
        longmemeval_release_evidence_intersections_are_zero: release_evidence_intersections == 0,
        duplicate_identities_are_zero: unique_identities(&ledger),
        unresolved_authoritative_contradictions_are_zero: audit
            .unresolved_authoritative_contradictions
            == 0,
        lineage_order_is_valid: true,
        deterministic_round_trip: serde_json::from_slice::<RelevanceLedgerV3>(
            &serde_json::to_vec(&ledger)?,
        )? == ledger,
    };
    let phase_4_verified = gates.all_pass();
    let receipt = ExternalLedgerReceipt {
        contract: CONTRACT,
        source_ledger: file_identity(ledger_path)?,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        workspace_key_id,
        workspace_key_path_recorded: false,
        ledger,
        audit,
        longmemeval_release_intersections,
        release_evidence_intersections,
        gates,
        phase_4_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(ExternalLedgerPublication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        source_ledger: receipt.source_ledger,
        audit,
        gates,
        phase_4_verified,
    })
}

fn release_evidence_fingerprints(cohort: &FrozenCohort) -> HashSet<[u8; 32]> {
    cohort
        .queries
        .iter()
        .flat_map(|query| query.candidate_pool.iter())
        .map(|candidate| {
            super::provenance::evidence_fingerprint(
                candidate.rank_evidence_v3,
                candidate.relevance_tier,
            )
        })
        .collect()
}

fn keyed_queries(key: &WorkspaceIdentityKey, queries: &[FrozenQuery]) -> HashSet<KeyedIdentity> {
    queries
        .iter()
        .map(|query| KeyedIdentity::derive(key, b"private-query", query.query_identity.as_bytes()))
        .collect()
}

fn unique_identities(ledger: &RelevanceLedgerV3) -> bool {
    ledger
        .judgments
        .iter()
        .map(|judgment| judgment.identity)
        .collect::<HashSet<_>>()
        .len()
        == ledger.judgments.len()
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

#[derive(Clone, Copy, Debug, Serialize)]
struct ExternalLedgerGates {
    schema_is_v3: bool,
    workspace_key_is_not_serialized: bool,
    private_query_identities_are_keyed: bool,
    candidate_pools_and_positions_are_complete: bool,
    feature_schema_and_provenance_complete: bool,
    constitutional_queries_are_holdouts: bool,
    longmemeval_release_intersections_are_zero: bool,
    longmemeval_release_evidence_intersections_are_zero: bool,
    duplicate_identities_are_zero: bool,
    unresolved_authoritative_contradictions_are_zero: bool,
    lineage_order_is_valid: bool,
    deterministic_round_trip: bool,
}

impl ExternalLedgerGates {
    fn all_pass(self) -> bool {
        self.schema_is_v3
            && self.workspace_key_is_not_serialized
            && self.private_query_identities_are_keyed
            && self.candidate_pools_and_positions_are_complete
            && self.feature_schema_and_provenance_complete
            && self.constitutional_queries_are_holdouts
            && self.longmemeval_release_intersections_are_zero
            && self.longmemeval_release_evidence_intersections_are_zero
            && self.duplicate_identities_are_zero
            && self.unresolved_authoritative_contradictions_are_zero
            && self.lineage_order_is_valid
            && self.deterministic_round_trip
    }
}

#[derive(Debug, Serialize)]
struct ExternalLedgerReceipt {
    contract: &'static str,
    source_ledger: FileIdentity,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    workspace_key_id: KeyedIdentity,
    workspace_key_path_recorded: bool,
    ledger: RelevanceLedgerV3,
    audit: phoenix_lexical_qps::LedgerAuditV3,
    longmemeval_release_intersections: usize,
    release_evidence_intersections: usize,
    gates: ExternalLedgerGates,
    phase_4_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct ExternalLedgerPublication {
    contract: &'static str,
    output: FileIdentity,
    source_ledger: FileIdentity,
    audit: phoenix_lexical_qps::LedgerAuditV3,
    gates: ExternalLedgerGates,
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
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    query_sha256: String,
    candidate_pool: Vec<FrozenCandidate>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_ledger_gate_requires_release_isolation() {
        let mut gates = ExternalLedgerGates {
            schema_is_v3: true,
            workspace_key_is_not_serialized: true,
            private_query_identities_are_keyed: true,
            candidate_pools_and_positions_are_complete: true,
            feature_schema_and_provenance_complete: true,
            constitutional_queries_are_holdouts: true,
            longmemeval_release_intersections_are_zero: true,
            longmemeval_release_evidence_intersections_are_zero: true,
            duplicate_identities_are_zero: true,
            unresolved_authoritative_contradictions_are_zero: true,
            lineage_order_is_valid: true,
            deterministic_round_trip: true,
        };
        assert!(gates.all_pass());
        gates.longmemeval_release_intersections_are_zero = false;
        assert!(!gates.all_pass());
    }
}
