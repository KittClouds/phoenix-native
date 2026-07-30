use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;
use phoenix_graph_generation_v2::{CandidateId, DecisionAction};
use phoenix_semantic_lens::LensNeutralReviewBinding;
use std::path::PathBuf;

pub const DECISION_MAGIC: [u8; 8] = *b"PHXDECV1";
pub const AUTHORITY_MAGIC: [u8; 8] = *b"PHXAUTV1";
pub const REVIEW_VERSION: u32 = 1;
pub const DECISION_EXTENSION: &str = "phxdecision";
pub const AUTHORITY_EXTENSION: &str = "phxauthority";
pub const MAX_REASON_BYTES: usize = 16 * 1024;
pub const MAX_DECISION_BYTES: u64 = 64 * 1024;
pub const MAX_AUTHORITY_BYTES: u64 = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ReviewPage {
    TypedRelationship = 1,
    Identity = 2,
    Event = 3,
    Episode = 4,
    EpisodeMembership = 5,
    Temporal = 6,
    Causal = 7,
    MemoryState = 8,
}

impl ReviewPage {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::TypedRelationship),
            2 => Some(Self::Identity),
            3 => Some(Self::Event),
            4 => Some(Self::Episode),
            5 => Some(Self::EpisodeMembership),
            6 => Some(Self::Temporal),
            7 => Some(Self::Causal),
            8 => Some(Self::MemoryState),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ReviewCandidateLocation {
    pub page: u16,
    pub flags_u16: u16,
    pub row_index: u32,
}

impl ReviewCandidateLocation {
    pub const fn new(page: ReviewPage, row_index: u32) -> Self {
        Self {
            page: page as u16,
            flags_u16: 0,
            row_index,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ReviewCandidate {
    pub binding: LensNeutralReviewBinding,
    pub location: ReviewCandidateLocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewAuthority {
    pub source_generation_hash: [u8; 32],
    pub document_hash: [u8; 32],
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionCommand {
    pub candidate_id: CandidateId,
    pub expected_source_generation_hash: [u8; 32],
    pub expected_candidate_hash: [u8; 32],
    pub expected_evidence_hash: [u8; 32],
    pub action: DecisionAction,
    pub reason: String,
    pub decided_at_unix_millis: u64,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct DecisionReceiptHeaderV1 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub total_len: u64,
    pub sequence: u64,
    pub receipt_id: [u8; 32],
    pub command_id: [u8; 32],
    pub previous_receipt_id: [u8; 32],
    pub source_generation_hash: [u8; 32],
    pub document_hash: [u8; 32],
    pub candidate_id: CandidateId,
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub lens_id: [u8; 32],
    pub vocabulary_hash: [u8; 32],
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub decided_at_unix_millis: u64,
    pub action: u16,
    pub status: u16,
    pub reason_len: u32,
    pub flags: u32,
    pub artifact_hash: [u8; 32],
    pub reserved: u32,
}

#[derive(Debug)]
pub struct VerifiedDecisionReceipt {
    pub(crate) mmap: Mmap,
    pub(crate) header: DecisionReceiptHeaderV1,
    pub(crate) path: PathBuf,
}

impl VerifiedDecisionReceipt {
    pub fn header(&self) -> &DecisionReceiptHeaderV1 {
        &self.header
    }

    pub fn reason(&self) -> &str {
        let start = self.header.header_size as usize;
        let end = start + self.header.reason_len as usize;
        std::str::from_utf8(&self.mmap[start..end]).unwrap_or("")
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct GenerationAuthorityHeaderV1 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub total_len: u64,
    pub sequence: u64,
    pub active_generation_hash: [u8; 32],
    pub previous_generation_hash: [u8; 32],
    pub parent_authority_hash: [u8; 32],
    pub authority_hash: [u8; 32],
    pub active_name_len: u32,
    pub previous_name_len: u32,
    pub flags: u32,
    pub reserved: u32,
}
