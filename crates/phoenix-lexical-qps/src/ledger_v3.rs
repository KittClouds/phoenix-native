use std::fmt;

use hashbrown::HashSet;
use serde::{Deserialize, Serialize};

use crate::{RankEvidenceV3, RelevanceTier};

pub const RELEVANCE_LEDGER_V3_SCHEMA_VERSION: u16 = 3;
pub const RELEVANCE_LEDGER_V3_CONTRACT: &str = "phoenix.qps.relevance-ledger/v3";

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct KeyedIdentity([u8; 32]);

impl KeyedIdentity {
    pub fn derive(key: &WorkspaceIdentityKey, domain: &[u8], value: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new_keyed(&key.0);
        hasher.update(b"phoenix-qps-keyed-identity-v3\0");
        hasher.update(&(domain.len() as u64).to_le_bytes());
        hasher.update(domain);
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value);
        Self(*hasher.finalize().as_bytes())
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn is_valid(self) -> bool {
        self.0 != [0; 32]
    }
}

impl fmt::Debug for KeyedIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "KeyedIdentity({:02x}{:02x}..)",
            self.0[0], self.0[1]
        )
    }
}

/// Secret workspace key used only while deriving unlinkable identities. It is
/// intentionally not serializable and its debug form is redacted.
#[derive(Clone, Copy)]
pub struct WorkspaceIdentityKey([u8; 32]);

impl WorkspaceIdentityKey {
    pub fn new(bytes: [u8; 32]) -> Result<Self, &'static str> {
        if bytes == [0; 32] {
            return Err("workspace identity key must not be zero");
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for WorkspaceIdentityKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceIdentityKey([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct JudgmentIdentity([u8; 32]);

impl JudgmentIdentity {
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentSourceV3 {
    ExplicitUserCorrection,
    CuratedRegressionCase,
    AcceptedOrPinnedResult,
    Reformulation,
    Abandonment,
    OrdinaryClick,
    AutomaticallyMinedNegative,
}

impl JudgmentSourceV3 {
    pub const fn authority(self) -> JudgmentAuthorityV3 {
        match self {
            Self::ExplicitUserCorrection | Self::CuratedRegressionCase => {
                JudgmentAuthorityV3::Authoritative
            }
            Self::AcceptedOrPinnedResult => JudgmentAuthorityV3::StrongEvidence,
            Self::Reformulation | Self::Abandonment => JudgmentAuthorityV3::MiningSignal,
            Self::OrdinaryClick => JudgmentAuthorityV3::NonAuthoritativePositionBiased,
            Self::AutomaticallyMinedNegative => JudgmentAuthorityV3::ReviewCandidate,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentAuthorityV3 {
    Authoritative,
    StrongEvidence,
    MiningSignal,
    NonAuthoritativePositionBiased,
    ReviewCandidate,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentReasonV3 {
    PartialMatchSaturation,
    ScatteredTerms,
    PhraseOrderFailure,
    IdentifierCollision,
    FuzzyCollision,
    WeakFieldEvidence,
    CommonTermDominance,
    LengthPriorFailure,
    WrongConceptProximity,
    DocumentConversationConfusion,
    LongQueryFailure,
    RealUserCorrection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrozenHoldoutV3 {
    ConstitutionalRegression,
    LongMemEvalRelease,
}

/// Privacy-preserving grouping provenance required to construct leakage-safe
/// train/development/test partitions. Every identity is keyed by the owning
/// workspace; the timestamp is coarse collection metadata rather than query
/// or document content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SplitGroupProvenanceV3 {
    pub query_family_identity: KeyedIdentity,
    pub positive_source_identity: KeyedIdentity,
    pub negative_source_identity: KeyedIdentity,
    pub positive_near_duplicate_cluster_identity: KeyedIdentity,
    pub negative_near_duplicate_cluster_identity: KeyedIdentity,
    pub entity_or_identifier_family_identity: KeyedIdentity,
    pub collection_cohort_identity: KeyedIdentity,
    pub collected_at_unix_seconds: u64,
}

impl SplitGroupProvenanceV3 {
    pub fn is_valid(self) -> bool {
        self.query_family_identity.is_valid()
            && self.positive_source_identity.is_valid()
            && self.negative_source_identity.is_valid()
            && self.positive_near_duplicate_cluster_identity.is_valid()
            && self.negative_near_duplicate_cluster_identity.is_valid()
            && self.entity_or_identifier_family_identity.is_valid()
            && self.collection_cohort_identity.is_valid()
            && self.collected_at_unix_seconds > 0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairwiseJudgmentV3 {
    pub identity: JudgmentIdentity,
    pub workspace_identity: KeyedIdentity,
    pub query_identity: KeyedIdentity,
    pub positive_document_version: KeyedIdentity,
    pub negative_document_version: KeyedIdentity,
    pub positive_features: RankEvidenceV3,
    pub negative_features: RankEvidenceV3,
    pub positive_tier: RelevanceTier,
    pub negative_tier: RelevanceTier,
    pub candidate_pool: Box<[KeyedIdentity]>,
    pub positive_position: u16,
    pub negative_position: u16,
    pub split_groups: SplitGroupProvenanceV3,
    pub frozen_holdout: Option<FrozenHoldoutV3>,
    pub v2_model_identity: [u8; 32],
    pub challenger_model_identity: [u8; 32],
    pub reason: JudgmentReasonV3,
    pub source: JudgmentSourceV3,
    pub confidence: f32,
    pub weight: f32,
    pub index_generation: u64,
    pub supersedes: Option<JudgmentIdentity>,
    pub contradicts: Box<[JudgmentIdentity]>,
}

#[derive(Clone, Debug)]
pub struct PairwiseJudgmentDraftV3 {
    pub workspace_identity: KeyedIdentity,
    pub query_identity: KeyedIdentity,
    pub positive_document_version: KeyedIdentity,
    pub negative_document_version: KeyedIdentity,
    pub positive_features: RankEvidenceV3,
    pub negative_features: RankEvidenceV3,
    pub positive_tier: RelevanceTier,
    pub negative_tier: RelevanceTier,
    pub candidate_pool: Box<[KeyedIdentity]>,
    pub positive_position: u16,
    pub negative_position: u16,
    pub split_groups: SplitGroupProvenanceV3,
    pub frozen_holdout: Option<FrozenHoldoutV3>,
    pub v2_model_identity: [u8; 32],
    pub challenger_model_identity: [u8; 32],
    pub reason: JudgmentReasonV3,
    pub source: JudgmentSourceV3,
    pub confidence: f32,
    pub weight: f32,
    pub index_generation: u64,
    pub supersedes: Option<JudgmentIdentity>,
    pub contradicts: Box<[JudgmentIdentity]>,
}

impl PairwiseJudgmentV3 {
    pub fn from_draft(draft: PairwiseJudgmentDraftV3) -> Self {
        let identity = identity(&draft);
        Self {
            identity,
            workspace_identity: draft.workspace_identity,
            query_identity: draft.query_identity,
            positive_document_version: draft.positive_document_version,
            negative_document_version: draft.negative_document_version,
            positive_features: draft.positive_features,
            negative_features: draft.negative_features,
            positive_tier: draft.positive_tier,
            negative_tier: draft.negative_tier,
            candidate_pool: draft.candidate_pool,
            positive_position: draft.positive_position,
            negative_position: draft.negative_position,
            split_groups: draft.split_groups,
            frozen_holdout: draft.frozen_holdout,
            v2_model_identity: draft.v2_model_identity,
            challenger_model_identity: draft.challenger_model_identity,
            reason: draft.reason,
            source: draft.source,
            confidence: draft.confidence,
            weight: draft.weight,
            index_generation: draft.index_generation,
            supersedes: draft.supersedes,
            contradicts: draft.contradicts,
        }
    }

    pub fn validate_shape(&self) -> Result<(), &'static str> {
        if !self.workspace_identity.is_valid()
            || !self.query_identity.is_valid()
            || !self.positive_document_version.is_valid()
            || !self.negative_document_version.is_valid()
            || self.positive_document_version == self.negative_document_version
            || !self.positive_features.is_valid()
            || !self.negative_features.is_valid()
            || self.positive_tier == RelevanceTier::Rejected
            || self.negative_tier == RelevanceTier::Rejected
            || self.candidate_pool.is_empty()
            || self.candidate_pool.len() > 160
            || self.positive_position as usize >= self.candidate_pool.len()
            || self.negative_position as usize >= self.candidate_pool.len()
            || self.positive_position == self.negative_position
            || !self.split_groups.is_valid()
            || self.candidate_pool[self.positive_position as usize]
                != self.positive_document_version
            || self.candidate_pool[self.negative_position as usize]
                != self.negative_document_version
            || self.v2_model_identity == [0; 32]
            || self.challenger_model_identity == [0; 32]
            || !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || self.confidence == 0.0
            || !self.weight.is_finite()
            || self.weight <= 0.0
            || self.index_generation == 0
            || self.identity != identity_from_judgment(self)
            || self.contradicts.contains(&self.identity)
            || self.supersedes == Some(self.identity)
        {
            return Err("invalid V3 pairwise judgment");
        }
        Ok(())
    }

    pub fn is_model_training_eligible(&self) -> bool {
        self.frozen_holdout.is_none()
            && self.positive_tier == self.negative_tier
            && self.positive_tier != RelevanceTier::Rejected
            && !matches!(
                self.source,
                JudgmentSourceV3::OrdinaryClick | JudgmentSourceV3::AutomaticallyMinedNegative
            )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelevanceLedgerV3 {
    pub contract: String,
    pub schema_version: u16,
    pub judgments: Vec<PairwiseJudgmentV3>,
}

impl Default for RelevanceLedgerV3 {
    fn default() -> Self {
        Self {
            contract: RELEVANCE_LEDGER_V3_CONTRACT.to_owned(),
            schema_version: RELEVANCE_LEDGER_V3_SCHEMA_VERSION,
            judgments: Vec::new(),
        }
    }
}

impl RelevanceLedgerV3 {
    pub fn append(&mut self, judgment: PairwiseJudgmentV3) -> Result<(), &'static str> {
        self.append_batch(std::iter::once(judgment))
    }

    /// Transactionally appends a preordered batch while building identity and
    /// lineage state once. No row is committed when any row is invalid.
    pub fn append_batch(
        &mut self,
        judgments: impl IntoIterator<Item = PairwiseJudgmentV3>,
    ) -> Result<(), &'static str> {
        let mut identities = self
            .judgments
            .iter()
            .map(|existing| existing.identity)
            .collect::<HashSet<_>>();
        let mut pending = Vec::new();
        for judgment in judgments {
            judgment.validate_shape()?;
            if !identities.insert(judgment.identity) {
                return Err("duplicate V3 judgment identity");
            }
            if judgment
                .supersedes
                .into_iter()
                .chain(judgment.contradicts.iter().copied())
                .any(|identity| !identities.contains(&identity))
            {
                return Err("judgment lineage must reference an earlier ledger entry");
            }
            pending.push(judgment);
        }
        self.judgments.extend(pending);
        Ok(())
    }

    pub fn validate(&self) -> Result<LedgerAuditV3, &'static str> {
        if self.contract != RELEVANCE_LEDGER_V3_CONTRACT
            || self.schema_version != RELEVANCE_LEDGER_V3_SCHEMA_VERSION
        {
            return Err("unsupported V3 relevance ledger schema");
        }
        let mut identities = HashSet::with_capacity(self.judgments.len());
        let mut prior = HashSet::with_capacity(self.judgments.len());
        for judgment in &self.judgments {
            judgment.validate_shape()?;
            if !identities.insert(judgment.identity) {
                return Err("duplicate V3 judgment identity");
            }
            if judgment
                .supersedes
                .into_iter()
                .chain(judgment.contradicts.iter().copied())
                .any(|identity| !prior.contains(&identity))
            {
                return Err("invalid V3 judgment lineage order");
            }
            prior.insert(judgment.identity);
        }
        Ok(self.audit())
    }

    pub fn audit(&self) -> LedgerAuditV3 {
        let mut audit = LedgerAuditV3 {
            judgments: self.judgments.len(),
            ..LedgerAuditV3::default()
        };
        for (index, judgment) in self.judgments.iter().enumerate() {
            match judgment.source.authority() {
                JudgmentAuthorityV3::Authoritative => audit.authoritative += 1,
                JudgmentAuthorityV3::StrongEvidence => audit.strong_evidence += 1,
                JudgmentAuthorityV3::MiningSignal => audit.mining_signals += 1,
                JudgmentAuthorityV3::NonAuthoritativePositionBiased => {
                    audit.position_biased_clicks += 1;
                }
                JudgmentAuthorityV3::ReviewCandidate => audit.review_candidates += 1,
            }
            if judgment.source.authority() != JudgmentAuthorityV3::Authoritative {
                continue;
            }
            for prior in &self.judgments[..index] {
                let reversed = judgment.query_identity == prior.query_identity
                    && judgment.positive_document_version == prior.negative_document_version
                    && judgment.negative_document_version == prior.positive_document_version;
                if reversed
                    && prior.source.authority() == JudgmentAuthorityV3::Authoritative
                    && !(judgment.supersedes == Some(prior.identity)
                        && judgment.contradicts.contains(&prior.identity))
                {
                    audit.unresolved_authoritative_contradictions += 1;
                }
            }
        }
        audit
    }

    /// Returns the append-only ledger rows which currently own their pairwise
    /// judgment. A later `supersedes` edge retires the referenced row from
    /// splitting, training, and evaluation without deleting provenance.
    pub fn active_model_training_indices(&self) -> Vec<usize> {
        let superseded = self
            .judgments
            .iter()
            .filter_map(|judgment| judgment.supersedes)
            .collect::<HashSet<_>>();
        self.judgments
            .iter()
            .enumerate()
            .filter(|(_, judgment)| {
                judgment.is_model_training_eligible() && !superseded.contains(&judgment.identity)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn active_model_training_judgments(&self) -> Vec<&PairwiseJudgmentV3> {
        self.active_model_training_indices()
            .into_iter()
            .map(|index| &self.judgments[index])
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LedgerAuditV3 {
    pub judgments: usize,
    pub authoritative: usize,
    pub strong_evidence: usize,
    pub mining_signals: usize,
    pub position_biased_clicks: usize,
    pub review_candidates: usize,
    pub unresolved_authoritative_contradictions: usize,
}

fn identity(draft: &PairwiseJudgmentDraftV3) -> JudgmentIdentity {
    identity_parts(
        draft.workspace_identity,
        draft.query_identity,
        draft.positive_document_version,
        draft.negative_document_version,
        draft.index_generation,
    )
}

fn identity_from_judgment(judgment: &PairwiseJudgmentV3) -> JudgmentIdentity {
    identity_parts(
        judgment.workspace_identity,
        judgment.query_identity,
        judgment.positive_document_version,
        judgment.negative_document_version,
        judgment.index_generation,
    )
}

fn identity_parts(
    workspace: KeyedIdentity,
    query: KeyedIdentity,
    positive: KeyedIdentity,
    negative: KeyedIdentity,
    generation: u64,
) -> JudgmentIdentity {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-qps-v3-judgment-identity\0");
    hasher.update(&workspace.0);
    hasher.update(&query.0);
    hasher.update(&positive.0);
    hasher.update(&negative.0);
    hasher.update(&generation.to_le_bytes());
    JudgmentIdentity(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RankEvidenceV3, RANK_EVIDENCE_V3_SCHEMA_VERSION};

    fn evidence() -> RankEvidenceV3 {
        RankEvidenceV3 {
            schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
            query_groups: 1,
            matched_groups: 1,
            missing_groups: 0,
            query_flags: 0,
            field_count: 1,
            values: [0.5; 30],
        }
    }

    fn draft(
        positive: KeyedIdentity,
        negative: KeyedIdentity,
        supersedes: Option<JudgmentIdentity>,
        contradicts: Box<[JudgmentIdentity]>,
    ) -> PairwiseJudgmentDraftV3 {
        PairwiseJudgmentDraftV3 {
            workspace_identity: KeyedIdentity::from_bytes([1; 32]),
            query_identity: KeyedIdentity::from_bytes([2; 32]),
            positive_document_version: positive,
            negative_document_version: negative,
            positive_features: evidence(),
            negative_features: evidence(),
            positive_tier: RelevanceTier::CompleteExactGroups,
            negative_tier: RelevanceTier::CompleteExactGroups,
            candidate_pool: vec![positive, negative].into_boxed_slice(),
            positive_position: 0,
            negative_position: 1,
            split_groups: SplitGroupProvenanceV3 {
                query_family_identity: KeyedIdentity::from_bytes([10; 32]),
                positive_source_identity: KeyedIdentity::from_bytes([11; 32]),
                negative_source_identity: KeyedIdentity::from_bytes([12; 32]),
                positive_near_duplicate_cluster_identity: KeyedIdentity::from_bytes([13; 32]),
                negative_near_duplicate_cluster_identity: KeyedIdentity::from_bytes([14; 32]),
                entity_or_identifier_family_identity: KeyedIdentity::from_bytes([15; 32]),
                collection_cohort_identity: KeyedIdentity::from_bytes([16; 32]),
                collected_at_unix_seconds: 1_700_000_000,
            },
            frozen_holdout: None,
            v2_model_identity: [3; 32],
            challenger_model_identity: [4; 32],
            reason: JudgmentReasonV3::RealUserCorrection,
            source: JudgmentSourceV3::ExplicitUserCorrection,
            confidence: 1.0,
            weight: 1.0,
            index_generation: 7,
            supersedes,
            contradicts,
        }
    }

    #[test]
    fn keyed_identity_is_domain_separated_and_key_never_serializes() {
        let key = WorkspaceIdentityKey::new([9; 32]).unwrap();
        assert_ne!(
            KeyedIdentity::derive(&key, b"query", b"private text"),
            KeyedIdentity::derive(&key, b"document", b"private text")
        );
        assert_eq!(format!("{key:?}"), "WorkspaceIdentityKey([REDACTED])");
    }

    #[test]
    fn ledger_rejects_duplicates_and_tracks_resolved_contradictions() {
        let a = KeyedIdentity::from_bytes([5; 32]);
        let b = KeyedIdentity::from_bytes([6; 32]);
        let first = PairwiseJudgmentV3::from_draft(draft(a, b, None, Box::new([])));
        let first_id = first.identity;
        let mut ledger = RelevanceLedgerV3::default();
        ledger.append(first.clone()).unwrap();
        assert!(ledger.append(first).is_err());
        let correction = PairwiseJudgmentV3::from_draft(draft(
            b,
            a,
            Some(first_id),
            vec![first_id].into_boxed_slice(),
        ));
        let correction_id = correction.identity;
        ledger.append(correction).unwrap();
        let audit = ledger.validate().unwrap();
        assert_eq!(audit.authoritative, 2);
        assert_eq!(audit.unresolved_authoritative_contradictions, 0);
        let active = ledger.active_model_training_judgments();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].identity, correction_id);
    }

    #[test]
    fn ordinary_clicks_are_explicitly_non_authoritative() {
        assert_eq!(
            JudgmentSourceV3::OrdinaryClick.authority(),
            JudgmentAuthorityV3::NonAuthoritativePositionBiased
        );
    }

    #[test]
    fn mined_negatives_require_review_before_training() {
        let positive = KeyedIdentity::from_bytes([5; 32]);
        let negative = KeyedIdentity::from_bytes([6; 32]);
        let mut candidate = draft(positive, negative, None, Box::new([]));
        candidate.source = JudgmentSourceV3::AutomaticallyMinedNegative;
        assert!(!PairwiseJudgmentV3::from_draft(candidate).is_model_training_eligible());
    }

    #[test]
    fn batch_append_is_transactional_on_duplicate_identity() {
        let positive = KeyedIdentity::from_bytes([5; 32]);
        let negative = KeyedIdentity::from_bytes([6; 32]);
        let judgment =
            PairwiseJudgmentV3::from_draft(draft(positive, negative, None, Box::new([])));
        let mut ledger = RelevanceLedgerV3::default();
        assert!(ledger.append_batch([judgment.clone(), judgment]).is_err());
        assert!(ledger.judgments.is_empty());
    }
}
