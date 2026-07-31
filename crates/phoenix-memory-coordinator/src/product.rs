use crate::{CommittedTurn, CoordinatorError, IngestDocumentRevision};
use phoenix_analysis_contract::PhoenixStructuralSubstrateV1;
use phoenix_memory_contract::{
    CandidateEndpointRoleV3, CandidateStatus, CanonicalBindingKind, ParticipantRole,
    ProducerProductV3, SemanticCandidateFamilyV3, VocabularyPackKindV3,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const MAX_ENTITIES_PER_SOURCE: usize = 250_000;
pub const MAX_MENTIONS_PER_SOURCE: usize = 2_000_000;
pub const MAX_CANDIDATES_PER_SOURCE: usize = 1_000_000;
pub const MAX_EVIDENCE_PER_CANDIDATE: usize = 64;
pub const MAX_ENDPOINTS_PER_CANDIDATE: usize = 32;
pub const MAX_VOCABULARY_PACKS: usize = 64;
pub const NO_MODEL_IDENTITY: u32 = u32::MAX;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelIdentityInputV3 {
    pub name: Arc<str>,
    pub runtime: Arc<str>,
    pub artifact_uri: Arc<str>,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationSupport {
    Supported,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProducerRegistrationV3 {
    pub product: ProducerProductV3,
    pub producer: Arc<str>,
    pub producer_binary_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub model_identity_index: Option<u32>,
    pub support: RegistrationSupport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityDraft {
    pub stable_id: u64,
    pub label: Arc<str>,
    pub custom_kind: Option<Arc<str>>,
    pub mention_count: u32,
    pub kind: u16,
    pub source_mask: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MentionDraft {
    pub stable_id: u64,
    pub entity_id: u64,
    pub evidence_id: u64,
    pub start: u32,
    pub end: u32,
    pub confidence: f32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalBindingDraft {
    pub source_entity_id: u64,
    pub canonical_entity_id: u64,
    pub decision_id: u64,
    pub source_mask: u16,
    pub kind: CanonicalBindingKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticCandidateDraft {
    pub candidate_id: [u8; 32],
    pub vocabulary_pack_id: u64,
    pub relation_kind: Arc<str>,
    pub value: Arc<str>,
    pub endpoints: Arc<[CandidateEndpointDraft]>,
    pub evidence_ids: Arc<[u64]>,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub family: SemanticCandidateFamilyV3,
    pub confidence: f32,
    pub model_identity_index: Option<u32>,
    pub producer_identity_hash: [u8; 32],
    pub status: CandidateStatus,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateEndpointDraft {
    pub endpoint_id: u64,
    pub role: CandidateEndpointRoleV3,
    pub flags: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VocabularyPackDraft {
    pub id: u64,
    pub name: Arc<str>,
    pub version: Arc<str>,
    pub schema_hash: [u8; 32],
    pub producer_identity_hash: [u8; 32],
    pub kind: VocabularyPackKindV3,
    pub flags: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommonProducts {
    pub entities: Vec<EntityDraft>,
    pub mentions: Vec<MentionDraft>,
    pub canonical_bindings: Vec<CanonicalBindingDraft>,
    pub vocabulary_packs: Vec<VocabularyPackDraft>,
    pub candidates: Vec<SemanticCandidateDraft>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentProduction {
    pub structural: PhoenixStructuralSubstrateV1,
    pub common: CommonProducts,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnProduction {
    pub common: CommonProducts,
}

#[derive(Clone)]
pub struct CancellationProbe {
    epoch: Arc<AtomicU64>,
    expected: u64,
}

impl CancellationProbe {
    pub(crate) fn new(epoch: Arc<AtomicU64>, expected: u64) -> Self {
        Self { epoch, expected }
    }

    pub fn is_cancelled(&self) -> bool {
        self.epoch.load(Ordering::Acquire) != self.expected
    }

    pub fn check(&self) -> Result<(), CoordinatorError> {
        if self.is_cancelled() {
            return Err(CoordinatorError::Cancelled);
        }
        Ok(())
    }
}

pub trait DualFaceProducer: Send + Sync + 'static {
    fn analyze_document(
        &self,
        request: &IngestDocumentRevision,
        cancellation: &CancellationProbe,
    ) -> Result<DocumentProduction, CoordinatorError>;

    fn analyze_turn(
        &self,
        conversation_external_id: &[u8],
        turn: &CommittedTurn,
        cancellation: &CancellationProbe,
    ) -> Result<TurnProduction, CoordinatorError>;
}

pub fn authoritative_product(product: ProducerProductV3) -> bool {
    matches!(
        product,
        ProducerProductV3::SourceStructure
            | ProducerProductV3::ContentUnitsAndChunks
            | ProducerProductV3::MentionsAndEvidence
            | ProducerProductV3::CanonicalEntityBindings
    )
}

pub(crate) fn validate_common_products(
    products: &CommonProducts,
    source_len: u32,
) -> Result<(), CoordinatorError> {
    if products.entities.len() > MAX_ENTITIES_PER_SOURCE
        || products.mentions.len() > MAX_MENTIONS_PER_SOURCE
        || products.candidates.len() > MAX_CANDIDATES_PER_SOURCE
        || products.vocabulary_packs.len() > MAX_VOCABULARY_PACKS
    {
        return Err(CoordinatorError::Oversized);
    }
    for entity in &products.entities {
        if entity.stable_id == 0
            || entity.label.trim().is_empty()
            || entity.mention_count == 0
            || entity.source_mask == 0
        {
            return Err(CoordinatorError::ProducerAuthority(
                "entity identity or provenance is invalid",
            ));
        }
    }
    for mention in &products.mentions {
        if mention.stable_id == 0
            || mention.entity_id == 0
            || mention.evidence_id == 0
            || mention.start >= mention.end
            || mention.end > source_len
            || !mention.confidence.is_finite()
        {
            return Err(CoordinatorError::ProducerAuthority(
                "mention evidence range is invalid",
            ));
        }
    }
    for binding in &products.canonical_bindings {
        let merge = binding.source_entity_id != binding.canonical_entity_id;
        if binding.source_entity_id == 0
            || binding.canonical_entity_id == 0
            || (merge
                && (binding.kind != CanonicalBindingKind::CoordinatorDecision
                    || binding.decision_id == 0))
        {
            return Err(CoordinatorError::InvalidIdentityMerge);
        }
    }
    for pack in &products.vocabulary_packs {
        if pack.id == 0
            || pack.name.trim().is_empty()
            || pack.version.trim().is_empty()
            || pack.schema_hash == [0; 32]
            || pack.producer_identity_hash == [0; 32]
        {
            return Err(CoordinatorError::InvalidCandidate);
        }
    }
    let packs = products
        .vocabulary_packs
        .iter()
        .map(|pack| pack.id)
        .collect::<hashbrown::HashSet<_>>();
    for candidate in &products.candidates {
        if candidate.candidate_id == [0; 32]
            || !packs.contains(&candidate.vocabulary_pack_id)
            || candidate.relation_kind.trim().is_empty()
            || candidate.endpoints.is_empty()
            || candidate.endpoints.len() > MAX_ENDPOINTS_PER_CANDIDATE
            || candidate
                .endpoints
                .iter()
                .any(|endpoint| endpoint.endpoint_id == 0)
            || candidate.status != CandidateStatus::Proposed
            || candidate.evidence_ids.is_empty()
            || candidate.evidence_ids.len() > MAX_EVIDENCE_PER_CANDIDATE
            || candidate.valid_time_from_millis > candidate.valid_time_to_millis
            || !candidate.confidence.is_finite()
            || candidate.producer_identity_hash == [0; 32]
        {
            return Err(CoordinatorError::InvalidCandidate);
        }
    }
    Ok(())
}

pub(crate) const fn role_is_source(role: ParticipantRole) -> bool {
    matches!(
        role,
        ParticipantRole::System
            | ParticipantRole::User
            | ParticipantRole::Assistant
            | ParticipantRole::Tool
            | ParticipantRole::Other
    )
}

pub(crate) const fn candidate_product(family: SemanticCandidateFamilyV3) -> ProducerProductV3 {
    match family {
        SemanticCandidateFamilyV3::Identity | SemanticCandidateFamilyV3::Coreference => {
            ProducerProductV3::IdentityCoreference
        }
        SemanticCandidateFamilyV3::Claim | SemanticCandidateFamilyV3::Attribute => {
            ProducerProductV3::ClaimsAttributes
        }
        SemanticCandidateFamilyV3::Relationship => ProducerProductV3::Relationships,
        SemanticCandidateFamilyV3::Event | SemanticCandidateFamilyV3::Temporal => {
            ProducerProductV3::EventsTemporal
        }
        SemanticCandidateFamilyV3::Causal => ProducerProductV3::Causality,
        SemanticCandidateFamilyV3::State | SemanticCandidateFamilyV3::Belief => {
            ProducerProductV3::StateBelief
        }
        SemanticCandidateFamilyV3::Correction | SemanticCandidateFamilyV3::Supersession => {
            ProducerProductV3::CorrectionsSupersession
        }
        SemanticCandidateFamilyV3::Goal | SemanticCandidateFamilyV3::Procedure => {
            ProducerProductV3::GoalsProcedures
        }
        SemanticCandidateFamilyV3::ContextualEvidence => ProducerProductV3::ContextualEvidence,
    }
}
