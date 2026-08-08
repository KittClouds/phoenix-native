use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;
use phoenix_memory_semantics::{
    MemoryActionV1, PolicyProposalV1, PolicyReasonV1, SourceAuthorityV1,
};
use std::path::{Path, PathBuf};

pub const POLICY_DECISION_MAGIC: [u8; 8] = *b"PHXPDCV1";
pub const POLICY_DECISION_VERSION: u32 = 1;
pub const POLICY_DECISION_EXTENSION: &str = "phxpdecision";
pub const MAX_POLICY_REASON_BYTES: usize = 16 * 1024;
pub const MAX_POLICY_DECISION_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum DecisionDispositionV1 {
    Commit = 1,
    Reject = 2,
    Defer = 3,
    Undo = 4,
}

impl DecisionDispositionV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Commit),
            2 => Some(Self::Reject),
            3 => Some(Self::Defer),
            4 => Some(Self::Undo),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyDecisionCommandV1 {
    pub candidate_id: [u8; 32],
    pub expected_source_generation_hash: [u8; 32],
    pub expected_candidate_hash: [u8; 32],
    pub expected_evidence_hash: [u8; 32],
    pub replacement_target_id: [u8; 32],
    pub policy_identity: [u8; 32],
    pub semantic_observation_hash: [u8; 32],
    pub source_authority: SourceAuthorityV1,
    pub proposal: PolicyProposalV1,
    pub disposition: DecisionDispositionV1,
    pub decided_at_unix_millis: i64,
    pub effective_at_unix_millis: i64,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyDecisionOutcomeV1 {
    pub receipt_id: [u8; 32],
    pub sequence: u64,
    pub reused: bool,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct PolicyDecisionReceiptHeaderV1 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub total_len: u64,
    pub sequence: u64,
    pub decided_at_unix_millis: i64,
    pub effective_at_unix_millis: i64,
    pub receipt_id: [u8; 32],
    pub command_id: [u8; 32],
    pub previous_global_receipt_id: [u8; 32],
    pub previous_candidate_receipt_id: [u8; 32],
    pub source_generation_hash: [u8; 32],
    pub candidate_id: [u8; 32],
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub replacement_target_id: [u8; 32],
    pub policy_identity: [u8; 32],
    pub semantic_observation_hash: [u8; 32],
    pub artifact_hash: [u8; 32],
    pub memory_action: u16,
    pub policy_reason: u16,
    pub disposition: u16,
    pub source_authority: u16,
    pub reason_len: u32,
    pub flags: u32,
    pub reserved: [u64; 2],
}

impl PolicyDecisionReceiptHeaderV1 {
    pub fn memory_action(&self) -> Option<MemoryActionV1> {
        MemoryActionV1::from_raw(self.memory_action)
    }

    pub fn policy_reason(&self) -> Option<PolicyReasonV1> {
        PolicyReasonV1::from_raw(self.policy_reason)
    }

    pub fn disposition(&self) -> Option<DecisionDispositionV1> {
        DecisionDispositionV1::from_raw(self.disposition)
    }

    pub fn source_authority(&self) -> Option<SourceAuthorityV1> {
        SourceAuthorityV1::from_raw(self.source_authority)
    }
}

const _: [(); 464] = [(); core::mem::size_of::<PolicyDecisionReceiptHeaderV1>()];

#[derive(Debug)]
pub struct VerifiedPolicyDecisionReceiptV1 {
    pub(crate) mmap: Mmap,
    pub(crate) header: PolicyDecisionReceiptHeaderV1,
    pub(crate) path: PathBuf,
}

impl VerifiedPolicyDecisionReceiptV1 {
    pub fn header(&self) -> &PolicyDecisionReceiptHeaderV1 {
        &self.header
    }

    pub fn reason(&self) -> &str {
        let start = self.header.header_size as usize;
        let end = start + self.header.reason_len as usize;
        std::str::from_utf8(&self.mmap[start..end]).unwrap_or("")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
