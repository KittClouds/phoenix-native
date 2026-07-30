use crate::{
    CoreSemanticClass, EndpointKind, LensIdentity, SemanticLensError, VerifiedSemanticLensPackV1,
};
use bytemuck::{Pod, Zeroable};
use phoenix_graph_generation_v2::CandidateId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SemanticEndpointRef {
    pub id: u64,
    pub kind: u16,
    pub flags: u16,
    pub reserved: u32,
}

impl SemanticEndpointRef {
    pub const fn new(kind: EndpointKind, id: u64) -> Self {
        Self {
            id,
            kind: kind as u16,
            flags: 0,
            reserved: 0,
        }
    }

    pub const fn kind(self) -> Option<EndpointKind> {
        EndpointKind::from_raw(self.kind)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct CandidateOrigin {
    pub candidate_id: CandidateId,
    pub lens_id: [u8; 32],
    pub vocabulary_hash: [u8; 32],
    pub semantic_code: u32,
    pub semantic_class: u16,
    pub flags_u16: u16,
    pub reserved: u32,
}

impl CandidateOrigin {
    pub fn from_pack(
        pack: &VerifiedSemanticLensPackV1,
        semantic_code: u32,
        candidate_id: CandidateId,
    ) -> Result<Self, SemanticLensError> {
        let code = pack
            .code(semantic_code)
            .ok_or(SemanticLensError::UnknownCode(semantic_code))?;
        if candidate_id.is_zero() {
            return Err(SemanticLensError::InvalidReviewBinding);
        }
        let identity = pack.identity();
        Ok(Self {
            candidate_id,
            lens_id: identity.lens_id,
            vocabulary_hash: identity.vocabulary_hash,
            semantic_code,
            semantic_class: code.class,
            flags_u16: 0,
            reserved: 0,
        })
    }

    pub fn from_identity(
        identity: LensIdentity,
        semantic_code: u32,
        semantic_class: CoreSemanticClass,
        candidate_id: CandidateId,
    ) -> Result<Self, SemanticLensError> {
        if candidate_id.is_zero() || identity.lens_id == [0; 32] {
            return Err(SemanticLensError::InvalidReviewBinding);
        }
        Ok(Self {
            candidate_id,
            lens_id: identity.lens_id,
            vocabulary_hash: identity.vocabulary_hash,
            semantic_code,
            semantic_class: semantic_class as u16,
            flags_u16: 0,
            reserved: 0,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct LensNeutralReviewBinding {
    pub origin: CandidateOrigin,
    pub origin_alignment_padding: u32,
    pub source: SemanticEndpointRef,
    pub target: SemanticEndpointRef,
    pub document_hash: [u8; 32],
    /// Hash of the exact packed candidate row.
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub producer_generation: u64,
    pub registry_revision: u64,
    pub flags: u32,
    pub reserved: u32,
}
