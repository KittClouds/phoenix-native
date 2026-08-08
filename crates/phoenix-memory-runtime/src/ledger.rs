use crate::contract::{
    DecisionDispositionV1, PolicyDecisionCommandV1, PolicyDecisionOutcomeV1,
    PolicyDecisionReceiptHeaderV1, VerifiedPolicyDecisionReceiptV1, MAX_POLICY_DECISION_BYTES,
    MAX_POLICY_REASON_BYTES, POLICY_DECISION_EXTENSION, POLICY_DECISION_MAGIC,
    POLICY_DECISION_VERSION,
};
use crate::{MemoryCatalogV1, MemoryRuntimeError};
use bytemuck::{bytes_of, try_from_bytes};
use hashbrown::HashMap;
use memmap2::MmapOptions;
use phoenix_memory_semantics::{MemoryActionV1, PolicyReasonV1};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};

const FLAG_CLOSE_EXISTING_VALIDITY: u32 = 1 << 0;
const FLAG_PRESERVE_HISTORY: u32 = 1 << 1;
const FLAG_REQUIRES_EXPLICIT_DECISION: u32 = 1 << 2;

pub struct PolicyDecisionLedgerV1 {
    root: PathBuf,
    receipts: Vec<VerifiedPolicyDecisionReceiptV1>,
    by_command: HashMap<[u8; 32], usize>,
    by_receipt: HashMap<[u8; 32], usize>,
    heads: HashMap<[u8; 32], usize>,
}

impl PolicyDecisionLedgerV1 {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, MemoryRuntimeError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| MemoryRuntimeError::io(&root, source))?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&root).map_err(|source| MemoryRuntimeError::io(&root, source))? {
            let path = entry
                .map_err(|source| MemoryRuntimeError::io(&root, source))?
                .path();
            if path.extension().and_then(|value| value.to_str()) == Some(POLICY_DECISION_EXTENSION)
            {
                paths.push(path);
            }
        }
        paths.sort_unstable();
        let mut ledger = Self {
            root,
            receipts: Vec::with_capacity(paths.len()),
            by_command: HashMap::with_capacity(paths.len()),
            by_receipt: HashMap::with_capacity(paths.len()),
            heads: HashMap::with_capacity(paths.len()),
        };
        for path in paths {
            let receipt = open_receipt(&path)?;
            ledger.push_verified(receipt)?;
        }
        Ok(ledger)
    }

    pub fn receipts(&self) -> &[VerifiedPolicyDecisionReceiptV1] {
        &self.receipts
    }

    pub fn sequence(&self) -> u64 {
        self.receipts
            .last()
            .map_or(0, |receipt| receipt.header().sequence)
    }

    pub fn head(&self, candidate_id: [u8; 32]) -> Option<&VerifiedPolicyDecisionReceiptV1> {
        self.heads
            .get(&candidate_id)
            .map(|index| &self.receipts[*index])
    }

    pub fn head_receipts(&self) -> impl Iterator<Item = &VerifiedPolicyDecisionReceiptV1> {
        self.heads.values().map(|index| &self.receipts[*index])
    }

    pub fn decide(
        &mut self,
        catalog: &MemoryCatalogV1,
        command: &PolicyDecisionCommandV1,
    ) -> Result<PolicyDecisionOutcomeV1, MemoryRuntimeError> {
        validate_command(catalog, command)?;
        if command.disposition == DecisionDispositionV1::Undo {
            let previous = self
                .head(command.candidate_id)
                .ok_or(MemoryRuntimeError::InvalidDecision)?;
            if previous.header().memory_action() != Some(command.proposal.action)
                || previous.header().policy_reason() != Some(command.proposal.reason)
            {
                return Err(MemoryRuntimeError::InvalidDecision);
            }
        }
        let command_id = command_hash(command);
        if let Some(index) = self.by_command.get(&command_id) {
            let header = self.receipts[*index].header();
            return Ok(PolicyDecisionOutcomeV1 {
                receipt_id: header.receipt_id,
                sequence: header.sequence,
                reused: true,
            });
        }
        let sequence = self
            .sequence()
            .checked_add(1)
            .ok_or(MemoryRuntimeError::InvalidDecisionChain)?;
        let previous_global_receipt_id = self
            .receipts
            .last()
            .map_or([0; 32], |receipt| receipt.header().receipt_id);
        let previous_candidate_receipt_id = self
            .head(command.candidate_id)
            .map_or([0; 32], |receipt| receipt.header().receipt_id);
        let receipt_id = receipt_hash(
            command_id,
            previous_global_receipt_id,
            previous_candidate_receipt_id,
            sequence,
            command.decided_at_unix_millis,
        );
        let mut header = PolicyDecisionReceiptHeaderV1 {
            magic: POLICY_DECISION_MAGIC,
            version: POLICY_DECISION_VERSION,
            header_size: size_of::<PolicyDecisionReceiptHeaderV1>() as u32,
            total_len: (size_of::<PolicyDecisionReceiptHeaderV1>() + command.reason.len()) as u64,
            sequence,
            decided_at_unix_millis: command.decided_at_unix_millis,
            effective_at_unix_millis: command.effective_at_unix_millis,
            receipt_id,
            command_id,
            previous_global_receipt_id,
            previous_candidate_receipt_id,
            source_generation_hash: command.expected_source_generation_hash,
            candidate_id: command.candidate_id,
            candidate_hash: command.expected_candidate_hash,
            evidence_hash: command.expected_evidence_hash,
            replacement_target_id: command.replacement_target_id,
            policy_identity: command.policy_identity,
            semantic_observation_hash: command.semantic_observation_hash,
            artifact_hash: [0; 32],
            memory_action: command.proposal.action as u16,
            policy_reason: command.proposal.reason as u16,
            disposition: command.disposition as u16,
            source_authority: command.source_authority as u16,
            reason_len: command.reason.len() as u32,
            flags: proposal_flags(command),
            reserved: [0; 2],
        };
        header.artifact_hash = artifact_hash(&header, command.reason.as_bytes());
        let path = receipt_path(&self.root, sequence);
        write_receipt_new(&path, &header, command.reason.as_bytes())?;
        let receipt = open_receipt(&path)?;
        self.push_verified(receipt)?;
        Ok(PolicyDecisionOutcomeV1 {
            receipt_id,
            sequence,
            reused: false,
        })
    }

    fn push_verified(
        &mut self,
        receipt: VerifiedPolicyDecisionReceiptV1,
    ) -> Result<(), MemoryRuntimeError> {
        let header = receipt.header();
        let expected_sequence = self
            .sequence()
            .checked_add(1)
            .ok_or(MemoryRuntimeError::InvalidDecisionChain)?;
        let expected_global = self
            .receipts
            .last()
            .map_or([0; 32], |prior| prior.header().receipt_id);
        let candidate_chain_matches = self.heads.get(&header.candidate_id).map_or(
            header.previous_candidate_receipt_id == [0; 32],
            |index| {
                self.receipts[*index].header().receipt_id == header.previous_candidate_receipt_id
            },
        );
        if header.sequence != expected_sequence
            || header.previous_global_receipt_id != expected_global
            || !candidate_chain_matches
            || self.by_command.contains_key(&header.command_id)
            || self.by_receipt.contains_key(&header.receipt_id)
        {
            return Err(MemoryRuntimeError::InvalidDecisionChain);
        }
        let index = self.receipts.len();
        self.by_command.insert(header.command_id, index);
        self.by_receipt.insert(header.receipt_id, index);
        self.heads.insert(header.candidate_id, index);
        self.receipts.push(receipt);
        Ok(())
    }
}

fn validate_command(
    catalog: &MemoryCatalogV1,
    command: &PolicyDecisionCommandV1,
) -> Result<(), MemoryRuntimeError> {
    catalog.exact(
        command.candidate_id,
        command.expected_source_generation_hash,
        command.expected_candidate_hash,
        command.expected_evidence_hash,
    )?;
    let action = command.proposal.action;
    let target_required = action.requires_replacement_target()
        && command.disposition == DecisionDispositionV1::Commit;
    let target_present = command.replacement_target_id != [0; 32];
    if command.reason.len() > MAX_POLICY_REASON_BYTES
        || command.reason.as_bytes().contains(&0)
        || command.policy_identity == [0; 32]
        || command.semantic_observation_hash == [0; 32]
        || command.candidate_id == [0; 32]
        || command.replacement_target_id == command.candidate_id
        || !command.proposal.is_constitutional()
        || target_required != target_present
        || (!target_required && target_present)
        || !command.proposal.preserve_history
        || !command.proposal.requires_explicit_decision
        || (target_present && catalog.get(command.replacement_target_id).is_none())
    {
        return Err(MemoryRuntimeError::InvalidDecision);
    }
    Ok(())
}

fn proposal_flags(command: &PolicyDecisionCommandV1) -> u32 {
    let mut flags = 0;
    if command.proposal.close_existing_validity {
        flags |= FLAG_CLOSE_EXISTING_VALIDITY;
    }
    if command.proposal.preserve_history {
        flags |= FLAG_PRESERVE_HISTORY;
    }
    if command.proposal.requires_explicit_decision {
        flags |= FLAG_REQUIRES_EXPLICIT_DECISION;
    }
    flags
}

fn command_hash(command: &PolicyDecisionCommandV1) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.policy-decision-command/v1\0");
    hasher.update(&command.candidate_id);
    hasher.update(&command.expected_source_generation_hash);
    hasher.update(&command.expected_candidate_hash);
    hasher.update(&command.expected_evidence_hash);
    hasher.update(&command.replacement_target_id);
    hasher.update(&command.policy_identity);
    hasher.update(&command.semantic_observation_hash);
    hasher.update(&(command.source_authority as u16).to_le_bytes());
    hasher.update(&(command.proposal.action as u16).to_le_bytes());
    hasher.update(&(command.proposal.reason as u16).to_le_bytes());
    hasher.update(&(command.disposition as u16).to_le_bytes());
    hasher.update(&command.effective_at_unix_millis.to_le_bytes());
    hasher.update(&(command.reason.len() as u64).to_le_bytes());
    hasher.update(command.reason.as_bytes());
    *hasher.finalize().as_bytes()
}

fn receipt_hash(
    command_id: [u8; 32],
    previous_global: [u8; 32],
    previous_candidate: [u8; 32],
    sequence: u64,
    decided_at: i64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.policy-decision-receipt/v1\0");
    hasher.update(&command_id);
    hasher.update(&previous_global);
    hasher.update(&previous_candidate);
    hasher.update(&sequence.to_le_bytes());
    hasher.update(&decided_at.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn artifact_hash(header: &PolicyDecisionReceiptHeaderV1, reason: &[u8]) -> [u8; 32] {
    let mut unhashed = *header;
    unhashed.artifact_hash = [0; 32];
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes_of(&unhashed));
    hasher.update(reason);
    *hasher.finalize().as_bytes()
}

fn receipt_path(root: &Path, sequence: u64) -> PathBuf {
    root.join(format!("{sequence:020}.{POLICY_DECISION_EXTENSION}"))
}

fn write_receipt_new(
    path: &Path,
    header: &PolicyDecisionReceiptHeaderV1,
    reason: &[u8],
) -> Result<(), MemoryRuntimeError> {
    let pending = path.with_extension("pending");
    if pending.exists() {
        fs::remove_file(&pending).map_err(|source| MemoryRuntimeError::io(&pending, source))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|source| MemoryRuntimeError::io(&pending, source))?;
    let result = file
        .write_all(bytes_of(header))
        .and_then(|_| file.write_all(reason))
        .and_then(|_| file.sync_all());
    if let Err(source) = result {
        let _ = fs::remove_file(&pending);
        return Err(MemoryRuntimeError::io(&pending, source));
    }
    if let Err(source) = fs::rename(&pending, path) {
        let _ = fs::remove_file(&pending);
        return Err(MemoryRuntimeError::io(path, source));
    }
    Ok(())
}

fn open_receipt(path: &Path) -> Result<VerifiedPolicyDecisionReceiptV1, MemoryRuntimeError> {
    let file = File::open(path).map_err(|source| MemoryRuntimeError::io(path, source))?;
    let len = file
        .metadata()
        .map_err(|source| MemoryRuntimeError::io(path, source))?
        .len();
    if len > MAX_POLICY_DECISION_BYTES {
        return Err(MemoryRuntimeError::OversizedDecision {
            actual: len,
            maximum: MAX_POLICY_DECISION_BYTES,
        });
    }
    // SAFETY: the verified owner retains the read-only mapping for its lifetime.
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .map_err(|source| MemoryRuntimeError::io(path, source))?
    };
    let header_bytes = mmap
        .get(..size_of::<PolicyDecisionReceiptHeaderV1>())
        .ok_or_else(|| MemoryRuntimeError::CorruptDecision(path.to_path_buf()))?;
    let header = *try_from_bytes::<PolicyDecisionReceiptHeaderV1>(header_bytes)
        .map_err(|_| MemoryRuntimeError::CorruptDecision(path.to_path_buf()))?;
    let reason_end = (header.header_size as usize)
        .checked_add(header.reason_len as usize)
        .ok_or_else(|| MemoryRuntimeError::CorruptDecision(path.to_path_buf()))?;
    let reason = mmap
        .get(header.header_size as usize..reason_end)
        .ok_or_else(|| MemoryRuntimeError::CorruptDecision(path.to_path_buf()))?;
    let action = MemoryActionV1::from_raw(header.memory_action);
    let disposition = DecisionDispositionV1::from_raw(header.disposition);
    let target_present = header.replacement_target_id != [0; 32];
    let target_required = disposition == Some(DecisionDispositionV1::Commit)
        && action.is_some_and(MemoryActionV1::requires_replacement_target);
    let proposal = action
        .zip(PolicyReasonV1::from_raw(header.policy_reason))
        .map(
            |(action, reason)| phoenix_memory_semantics::PolicyProposalV1 {
                action,
                reason,
                close_existing_validity: header.flags & FLAG_CLOSE_EXISTING_VALIDITY != 0,
                preserve_history: header.flags & FLAG_PRESERVE_HISTORY != 0,
                requires_explicit_decision: header.flags & FLAG_REQUIRES_EXPLICIT_DECISION != 0,
            },
        );
    if header.magic != POLICY_DECISION_MAGIC
        || header.version != POLICY_DECISION_VERSION
        || header.header_size as usize != size_of::<PolicyDecisionReceiptHeaderV1>()
        || header.total_len != len
        || reason.len() > MAX_POLICY_REASON_BYTES
        || std::str::from_utf8(reason).is_err()
        || action.is_none()
        || PolicyReasonV1::from_raw(header.policy_reason).is_none()
        || disposition.is_none()
        || header.receipt_id == [0; 32]
        || header.command_id == [0; 32]
        || header.candidate_id == [0; 32]
        || header.policy_identity == [0; 32]
        || header.semantic_observation_hash == [0; 32]
        || header.source_authority().is_none()
        || proposal.is_none_or(|proposal| !proposal.is_constitutional())
        || (disposition == Some(DecisionDispositionV1::Undo)
            && header.previous_candidate_receipt_id == [0; 32])
        || reason.contains(&0)
        || target_present != target_required
        || header.artifact_hash != artifact_hash(&header, reason)
    {
        return Err(MemoryRuntimeError::CorruptDecision(path.to_path_buf()));
    }
    Ok(VerifiedPolicyDecisionReceiptV1 {
        mmap,
        header,
        path: path.to_path_buf(),
    })
}
