use crate::{PackDescriptor, VocabularyRelation};
use phoenix_memory_contract::{
    CandidateEndpointRoleV3, CandidateStatus, SemanticCandidateFamilyV3, TIME_UNBOUNDED,
};
use phoenix_memory_coordinator::{CandidateEndpointDraft, SemanticCandidateDraft};
use smallvec::SmallVec;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SemanticError {
    #[error("semantic candidates require at least one endpoint")]
    MissingEndpoint,
    #[error("semantic candidates require exact evidence")]
    MissingEvidence,
    #[error("semantic candidate confidence must be finite and within [0, 1]")]
    InvalidConfidence,
}

pub struct CandidateBuilder {
    pack: PackDescriptor,
    family: SemanticCandidateFamilyV3,
    relation_kind: Arc<str>,
    value: Arc<str>,
    endpoints: SmallVec<[CandidateEndpointDraft; 4]>,
    evidence_ids: SmallVec<[u64; 4]>,
    confidence: f32,
    valid_time_from_millis: i64,
    valid_time_to_millis: i64,
    model_identity_index: Option<u32>,
}

impl CandidateBuilder {
    pub fn new(
        pack: PackDescriptor,
        family: SemanticCandidateFamilyV3,
        relation: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            pack,
            family,
            relation_kind: relation.into(),
            value: Arc::from(""),
            endpoints: SmallVec::new(),
            evidence_ids: SmallVec::new(),
            confidence: 1.0,
            valid_time_from_millis: i64::MIN,
            valid_time_to_millis: TIME_UNBOUNDED,
            model_identity_index: None,
        }
    }

    pub fn relation<R: VocabularyRelation>(
        pack: PackDescriptor,
        family: SemanticCandidateFamilyV3,
        relation: R,
    ) -> Self {
        Self::new(pack, family, relation.stable_name())
    }

    pub fn endpoint(mut self, id: u64, role: CandidateEndpointRoleV3) -> Self {
        self.endpoints.push(CandidateEndpointDraft {
            endpoint_id: id,
            role,
            flags: 0,
        });
        self
    }

    pub fn evidence(mut self, id: u64) -> Self {
        self.evidence_ids.push(id);
        self
    }

    pub fn value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.value = value.into();
        self
    }

    pub fn confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn valid_time(mut self, from_millis: i64, to_millis: i64) -> Self {
        self.valid_time_from_millis = from_millis;
        self.valid_time_to_millis = to_millis;
        self
    }

    pub fn model_identity(mut self, index: u32) -> Self {
        self.model_identity_index = Some(index);
        self
    }

    pub fn build(mut self) -> Result<SemanticCandidateDraft, SemanticError> {
        if self.endpoints.is_empty()
            || self
                .endpoints
                .iter()
                .any(|endpoint| endpoint.endpoint_id == 0)
        {
            return Err(SemanticError::MissingEndpoint);
        }
        if self.evidence_ids.is_empty() || self.evidence_ids.contains(&0) {
            return Err(SemanticError::MissingEvidence);
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(SemanticError::InvalidConfidence);
        }
        self.endpoints.sort_unstable_by_key(|endpoint| {
            (endpoint.role as u16, endpoint.endpoint_id, endpoint.flags)
        });
        self.endpoints.dedup();
        self.evidence_ids.sort_unstable();
        self.evidence_ids.dedup();
        let candidate_id = candidate_id(&self);
        Ok(SemanticCandidateDraft {
            candidate_id,
            vocabulary_pack_id: self.pack.id,
            relation_kind: self.relation_kind,
            value: self.value,
            endpoints: Arc::from(self.endpoints.into_vec()),
            evidence_ids: Arc::from(self.evidence_ids.into_vec()),
            valid_time_from_millis: self.valid_time_from_millis,
            valid_time_to_millis: self.valid_time_to_millis,
            family: self.family,
            confidence: self.confidence,
            model_identity_index: self.model_identity_index,
            producer_identity_hash: self.pack.producer_identity_hash,
            status: CandidateStatus::Proposed,
            flags: 0,
        })
    }
}

fn candidate_id(candidate: &CandidateBuilder) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.semantic-candidate/v3\0");
    hasher.update(&candidate.pack.id.to_le_bytes());
    hasher.update(&(candidate.family as u16).to_le_bytes());
    hasher.update(candidate.relation_kind.as_bytes());
    hasher.update(&[0]);
    hasher.update(candidate.value.as_bytes());
    for endpoint in &candidate.endpoints {
        hasher.update(&endpoint.endpoint_id.to_le_bytes());
        hasher.update(&(endpoint.role as u16).to_le_bytes());
        hasher.update(&endpoint.flags.to_le_bytes());
    }
    for evidence_id in &candidate.evidence_ids {
        hasher.update(&evidence_id.to_le_bytes());
    }
    hasher.update(&candidate.valid_time_from_millis.to_le_bytes());
    hasher.update(&candidate.valid_time_to_millis.to_le_bytes());
    hasher.update(
        &candidate
            .model_identity_index
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    hasher.update(&candidate.pack.producer_identity_hash);
    *hasher.finalize().as_bytes()
}
