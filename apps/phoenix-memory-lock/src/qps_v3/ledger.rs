use phoenix_lexical_qps::{
    FrozenHoldoutV3, JudgmentIdentity, JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity,
    PairwiseJudgmentDraftV3, PairwiseJudgmentV3, RankEvidenceV3, RelevanceLedgerV3,
    SplitGroupProvenanceV3, WorkspaceIdentityKey, RANK_EVIDENCE_V3_FEATURE_NAMES,
    RANK_EVIDENCE_V3_SCHEMA_VERSION, RELEVANCE_LEDGER_V3_CONTRACT,
    RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
};

use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-ledger-qualification/v1";
const PUBLICATION_CONTRACT: &str = "phoenix.memory.qps-v3-ledger-publication/v1";

pub(crate) fn verify(
    phase_3_path: &Path,
    workspace_key_path: &Path,
    output_path: &Path,
) -> Result<Phase4Publication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite ledger receipt {}",
            output_path.display()
        );
    }
    let phase_3_bytes = fs::read(phase_3_path)
        .with_context(|| format!("read Phase 3 receipt {}", phase_3_path.display()))?;
    let phase_3: FrozenPhase3 = serde_json::from_slice(&phase_3_bytes)
        .with_context(|| format!("decode Phase 3 receipt {}", phase_3_path.display()))?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("Phase 4 requires a verified QPS V3 Phase 3 receipt");
    }
    let raw_key = read_workspace_key(workspace_key_path)?;
    let workspace_key = WorkspaceIdentityKey::new(raw_key).map_err(anyhow::Error::msg)?;
    let workspace_identity =
        KeyedIdentity::derive(&workspace_key, b"workspace", b"qps-v3-ledger-qualification");
    let workspace_key_id =
        KeyedIdentity::derive(&workspace_key, b"workspace-key-id", b"qps-v3-ledger-v3");
    let v2_model_identity = decode_hex_32(&phase_3.v2_configuration_sha256)?;
    let challenger_model_identity = decode_hex_32(&sha256_bytes(&serde_json::to_vec(&(
        RANK_EVIDENCE_V3_SCHEMA_VERSION,
        RANK_EVIDENCE_V3_FEATURE_NAMES,
    ))?))?;
    let eligible = phase_3
        .mixed_suite
        .queries
        .iter()
        .filter(|query| {
            query.candidate_pool.len() >= 2
                && query
                    .oracle_locations
                    .first()
                    .is_some_and(|location| location.candidate_pool_rank.is_some())
        })
        .collect::<Vec<_>>();
    if eligible.len() < 8 {
        bail!("Phase 4 requires at least eight mixed-suite pairwise fixtures");
    }

    let sources = [
        JudgmentSourceV3::ExplicitUserCorrection,
        JudgmentSourceV3::CuratedRegressionCase,
        JudgmentSourceV3::AcceptedOrPinnedResult,
        JudgmentSourceV3::Reformulation,
        JudgmentSourceV3::Abandonment,
        JudgmentSourceV3::OrdinaryClick,
        JudgmentSourceV3::AutomaticallyMinedNegative,
    ];
    let reasons = [
        JudgmentReasonV3::RealUserCorrection,
        JudgmentReasonV3::PartialMatchSaturation,
        JudgmentReasonV3::WeakFieldEvidence,
        JudgmentReasonV3::WrongConceptProximity,
        JudgmentReasonV3::DocumentConversationConfusion,
        JudgmentReasonV3::CommonTermDominance,
        JudgmentReasonV3::FuzzyCollision,
    ];
    let mut ledger = RelevanceLedgerV3::default();
    let mut raw_query_sha_ne_keyed = true;
    for (index, ((query, source), reason)) in eligible
        .iter()
        .take(sources.len())
        .zip(sources)
        .zip(reasons)
        .enumerate()
    {
        let judgment = judgment_for_query(
            query,
            &workspace_key,
            workspace_identity,
            v2_model_identity,
            challenger_model_identity,
            source,
            reason,
            index as u64 + 1,
            false,
            None,
            Box::new([]),
        )?;
        raw_query_sha_ne_keyed &=
            decode_hex_32(&query.query_sha256)? != judgment.query_identity.as_bytes();
        ledger.append(judgment).map_err(anyhow::Error::msg)?;
    }

    let lineage_query = eligible[7];
    let original = judgment_for_query(
        lineage_query,
        &workspace_key,
        workspace_identity,
        v2_model_identity,
        challenger_model_identity,
        JudgmentSourceV3::CuratedRegressionCase,
        JudgmentReasonV3::PhraseOrderFailure,
        8,
        false,
        None,
        Box::new([]),
    )?;
    let original_id = original.identity;
    ledger.append(original).map_err(anyhow::Error::msg)?;
    let correction = judgment_for_query(
        lineage_query,
        &workspace_key,
        workspace_identity,
        v2_model_identity,
        challenger_model_identity,
        JudgmentSourceV3::ExplicitUserCorrection,
        JudgmentReasonV3::RealUserCorrection,
        8,
        true,
        Some(original_id),
        vec![original_id].into_boxed_slice(),
    )?;
    ledger.append(correction).map_err(anyhow::Error::msg)?;

    let audit = ledger.validate().map_err(anyhow::Error::msg)?;
    let ledger_bytes = serde_json::to_vec(&ledger)?;
    let round_trip: RelevanceLedgerV3 = serde_json::from_slice(&ledger_bytes)?;
    let round_trip_audit = round_trip.validate().map_err(anyhow::Error::msg)?;
    let raw_key_hex = hex(raw_key);
    let serialized = std::str::from_utf8(&ledger_bytes).context("ledger JSON must be UTF-8")?;
    let gates = Phase4Gates {
        schema_is_v3: ledger.contract == RELEVANCE_LEDGER_V3_CONTRACT
            && ledger.schema_version == RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
        workspace_key_is_not_serialized: !serialized.contains(&raw_key_hex),
        query_identities_are_keyed_not_raw_sha256: raw_query_sha_ne_keyed,
        every_source_class_is_represented: sources.iter().all(|source| {
            ledger
                .judgments
                .iter()
                .any(|judgment| judgment.source == *source)
        }),
        ordinary_click_is_non_authoritative: JudgmentSourceV3::OrdinaryClick.authority()
            == phoenix_lexical_qps::JudgmentAuthorityV3::NonAuthoritativePositionBiased,
        candidate_pools_and_positions_are_complete: ledger.judgments.iter().all(|judgment| {
            !judgment.candidate_pool.is_empty()
                && judgment.candidate_pool.len() <= 160
                && judgment.candidate_pool[judgment.positive_position as usize]
                    == judgment.positive_document_version
                && judgment.candidate_pool[judgment.negative_position as usize]
                    == judgment.negative_document_version
        }),
        constitutional_fixtures_are_frozen_holdouts: ledger.judgments.iter().all(|judgment| {
            judgment.frozen_holdout == Some(FrozenHoldoutV3::ConstitutionalRegression)
        }),
        feature_and_provenance_completeness_is_100_percent: ledger.judgments.iter().all(
            |judgment| {
                judgment.positive_features.is_valid()
                    && judgment.negative_features.is_valid()
                    && judgment.positive_tier != phoenix_lexical_qps::RelevanceTier::Rejected
                    && judgment.negative_tier != phoenix_lexical_qps::RelevanceTier::Rejected
                    && judgment.split_groups.is_valid()
                    && judgment.index_generation > 0
                    && judgment.v2_model_identity != [0; 32]
                    && judgment.challenger_model_identity != [0; 32]
            },
        ),
        duplicate_identities_are_zero: unique_identities(&ledger),
        unresolved_authoritative_contradictions_are_zero: audit
            .unresolved_authoritative_contradictions
            == 0,
        supersession_and_contradiction_lineage_is_valid: ledger.judgments.last().is_some_and(
            |judgment| {
                judgment.supersedes == Some(original_id)
                    && judgment.contradicts.contains(&original_id)
            },
        ),
        deterministic_round_trip: ledger == round_trip && audit == round_trip_audit,
    };
    let phase_4_verified = gates.all_pass();
    let receipt = Phase4Receipt {
        contract: CONTRACT,
        phase_3_receipt: file_identity(phase_3_path)?,
        producer_binary: current_binary_identity()?,
        workspace_key_id,
        workspace_key_path_recorded: false,
        ledger,
        audit,
        gates,
        phase_4_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(Phase4Publication {
        contract: PUBLICATION_CONTRACT,
        output: file_identity(output_path)?,
        phase_3_receipt: receipt.phase_3_receipt,
        workspace_key_id,
        audit,
        gates,
        phase_4_verified,
    })
}

#[allow(clippy::too_many_arguments)]
fn judgment_for_query(
    query: &FrozenQuery,
    key: &WorkspaceIdentityKey,
    workspace_identity: KeyedIdentity,
    v2_model_identity: [u8; 32],
    challenger_model_identity: [u8; 32],
    source: JudgmentSourceV3,
    reason: JudgmentReasonV3,
    index_generation: u64,
    reverse: bool,
    supersedes: Option<JudgmentIdentity>,
    contradicts: Box<[JudgmentIdentity]>,
) -> Result<PairwiseJudgmentV3> {
    let oracle = query
        .oracle_locations
        .first()
        .context("fixture query has no oracle")?;
    let positive_index = query
        .candidate_pool
        .iter()
        .position(|candidate| candidate.document_identity == oracle.document_identity)
        .context("oracle is missing from fixture candidate pool")?;
    let negative_index = (0..query.candidate_pool.len())
        .find(|index| *index != positive_index)
        .context("fixture query has no negative")?;
    let pool = query
        .candidate_pool
        .iter()
        .map(|candidate| {
            KeyedIdentity::derive(
                key,
                b"document-version",
                candidate.document_version_sha256.as_bytes(),
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let (positive_index, negative_index) = if reverse {
        (negative_index, positive_index)
    } else {
        (positive_index, negative_index)
    };
    let positive = &query.candidate_pool[positive_index];
    let negative = &query.candidate_pool[negative_index];
    let query_family_identity =
        KeyedIdentity::derive(key, b"query-family", query.query_identity.as_bytes());
    let draft = PairwiseJudgmentDraftV3 {
        workspace_identity,
        query_identity: KeyedIdentity::derive(
            key,
            b"private-query",
            query.query_identity.as_bytes(),
        ),
        positive_document_version: pool[positive_index],
        negative_document_version: pool[negative_index],
        positive_features: positive.rank_evidence_v3,
        negative_features: negative.rank_evidence_v3,
        positive_tier: positive.relevance_tier,
        negative_tier: negative.relevance_tier,
        candidate_pool: pool,
        positive_position: u16::try_from(positive_index).context("positive position overflow")?,
        negative_position: u16::try_from(negative_index).context("negative position overflow")?,
        split_groups: SplitGroupProvenanceV3 {
            query_family_identity,
            positive_source_identity: KeyedIdentity::derive(
                key,
                b"source",
                positive.document_identity.as_bytes(),
            ),
            negative_source_identity: KeyedIdentity::derive(
                key,
                b"source",
                negative.document_identity.as_bytes(),
            ),
            positive_near_duplicate_cluster_identity: KeyedIdentity::derive(
                key,
                b"near-duplicate-cluster",
                positive.document_identity.as_bytes(),
            ),
            negative_near_duplicate_cluster_identity: KeyedIdentity::derive(
                key,
                b"near-duplicate-cluster",
                negative.document_identity.as_bytes(),
            ),
            entity_or_identifier_family_identity: KeyedIdentity::derive(
                key,
                b"entity-or-identifier-family",
                query.query_identity.as_bytes(),
            ),
            collection_cohort_identity: KeyedIdentity::derive(
                key,
                b"collection-cohort",
                query.query_identity.as_bytes(),
            ),
            collected_at_unix_seconds: 1_785_844_800 + index_generation,
        },
        frozen_holdout: Some(FrozenHoldoutV3::ConstitutionalRegression),
        v2_model_identity,
        challenger_model_identity,
        reason,
        source,
        confidence: confidence(source),
        weight: weight(source),
        index_generation,
        supersedes,
        contradicts,
    };
    Ok(PairwiseJudgmentV3::from_draft(draft))
}

fn confidence(source: JudgmentSourceV3) -> f32 {
    match source {
        JudgmentSourceV3::ExplicitUserCorrection | JudgmentSourceV3::CuratedRegressionCase => 1.0,
        JudgmentSourceV3::AcceptedOrPinnedResult => 0.9,
        JudgmentSourceV3::Reformulation | JudgmentSourceV3::Abandonment => 0.6,
        JudgmentSourceV3::OrdinaryClick => 0.25,
        JudgmentSourceV3::AutomaticallyMinedNegative => 0.5,
    }
}

fn weight(source: JudgmentSourceV3) -> f32 {
    match source {
        JudgmentSourceV3::ExplicitUserCorrection => 2.0,
        JudgmentSourceV3::CuratedRegressionCase => 1.5,
        JudgmentSourceV3::AcceptedOrPinnedResult => 1.25,
        JudgmentSourceV3::Reformulation | JudgmentSourceV3::Abandonment => 0.75,
        JudgmentSourceV3::OrdinaryClick => 0.1,
        JudgmentSourceV3::AutomaticallyMinedNegative => 0.5,
    }
}

pub(super) fn read_workspace_key(path: &Path) -> Result<[u8; 32]> {
    let bytes = fs::read(path).with_context(|| format!("read workspace key {}", path.display()))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("workspace key must contain exactly 32 raw bytes"))
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

fn unique_identities(ledger: &RelevanceLedgerV3) -> bool {
    ledger
        .judgments
        .iter()
        .map(|judgment| judgment.identity)
        .collect::<HashSet<_>>()
        .len()
        == ledger.judgments.len()
}

#[derive(Debug, Serialize)]
struct Phase4Receipt {
    contract: &'static str,
    phase_3_receipt: FileIdentity,
    producer_binary: FileIdentity,
    workspace_key_id: KeyedIdentity,
    workspace_key_path_recorded: bool,
    ledger: RelevanceLedgerV3,
    audit: phoenix_lexical_qps::LedgerAuditV3,
    gates: Phase4Gates,
    phase_4_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct Phase4Publication {
    contract: &'static str,
    output: FileIdentity,
    phase_3_receipt: FileIdentity,
    workspace_key_id: KeyedIdentity,
    audit: phoenix_lexical_qps::LedgerAuditV3,
    gates: Phase4Gates,
    phase_4_verified: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Phase4Gates {
    schema_is_v3: bool,
    workspace_key_is_not_serialized: bool,
    query_identities_are_keyed_not_raw_sha256: bool,
    every_source_class_is_represented: bool,
    ordinary_click_is_non_authoritative: bool,
    candidate_pools_and_positions_are_complete: bool,
    constitutional_fixtures_are_frozen_holdouts: bool,
    feature_and_provenance_completeness_is_100_percent: bool,
    duplicate_identities_are_zero: bool,
    unresolved_authoritative_contradictions_are_zero: bool,
    supersession_and_contradiction_lineage_is_valid: bool,
    deterministic_round_trip: bool,
}

impl Phase4Gates {
    fn all_pass(self) -> bool {
        self.schema_is_v3
            && self.workspace_key_is_not_serialized
            && self.query_identities_are_keyed_not_raw_sha256
            && self.every_source_class_is_represented
            && self.ordinary_click_is_non_authoritative
            && self.candidate_pools_and_positions_are_complete
            && self.constitutional_fixtures_are_frozen_holdouts
            && self.feature_and_provenance_completeness_is_100_percent
            && self.duplicate_identities_are_zero
            && self.unresolved_authoritative_contradictions_are_zero
            && self.supersession_and_contradiction_lineage_is_valid
            && self.deterministic_round_trip
    }
}

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    contract: String,
    v2_configuration_sha256: String,
    mixed_suite: FrozenCohort,
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
    oracle_locations: Vec<FrozenOracleLocation>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    document_version_sha256: String,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: phoenix_lexical_qps::RelevanceTier,
}

#[derive(Debug, Deserialize)]
struct FrozenOracleLocation {
    document_identity: String,
    candidate_pool_rank: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_and_weight_keep_clicks_non_authoritative() {
        assert!(weight(JudgmentSourceV3::OrdinaryClick) < weight(JudgmentSourceV3::Reformulation));
        assert!(confidence(JudgmentSourceV3::OrdinaryClick) < 0.5);
    }
}
