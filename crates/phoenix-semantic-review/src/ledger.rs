use crate::contract::{
    DecisionCommand, DecisionReceiptHeaderV1, VerifiedDecisionReceipt, DECISION_EXTENSION,
    DECISION_MAGIC, MAX_DECISION_BYTES, MAX_REASON_BYTES, REVIEW_VERSION,
};
use crate::{ReviewCatalog, SemanticReviewError};
use bytemuck::{bytes_of, try_from_bytes};
use hashbrown::HashMap;
use memmap2::MmapOptions;
use phoenix_graph_generation_v2::{CandidateId, CandidateStatus, DecisionAction};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionOutcome {
    pub receipt_id: [u8; 32],
    pub sequence: u64,
    pub reused: bool,
}

pub struct DecisionLedger {
    root: PathBuf,
    receipts: Vec<VerifiedDecisionReceipt>,
    by_command: HashMap<[u8; 32], usize>,
    heads: HashMap<CandidateId, usize>,
}

impl DecisionLedger {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SemanticReviewError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| SemanticReviewError::io(&root, source))?;
        let mut paths = fs::read_dir(&root)
            .map_err(|source| SemanticReviewError::io(&root, source))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|value| value.to_str()) == Some(DECISION_EXTENSION)
            })
            .collect::<Vec<_>>();
        paths.sort_unstable();

        let mut ledger = Self {
            root,
            receipts: Vec::with_capacity(paths.len()),
            by_command: HashMap::with_capacity(paths.len()),
            heads: HashMap::with_capacity(paths.len()),
        };
        for path in paths {
            let receipt = open_receipt(&path)?;
            ledger.push_verified(receipt)?;
        }
        Ok(ledger)
    }

    pub fn receipts(&self) -> &[VerifiedDecisionReceipt] {
        &self.receipts
    }

    pub fn head(&self, candidate_id: CandidateId) -> Option<&VerifiedDecisionReceipt> {
        self.heads
            .get(&candidate_id)
            .map(|index| &self.receipts[*index])
    }

    pub fn head_receipts(&self) -> impl Iterator<Item = &VerifiedDecisionReceipt> {
        self.heads.values().map(|index| &self.receipts[*index])
    }

    pub fn decide(
        &mut self,
        catalog: &ReviewCatalog,
        command: &DecisionCommand,
    ) -> Result<DecisionOutcome, SemanticReviewError> {
        if command.reason.len() > MAX_REASON_BYTES
            || command.reason.as_bytes().contains(&0)
            || DecisionAction::from_raw(command.action as u16).is_none()
        {
            return Err(SemanticReviewError::InvalidDecision);
        }
        let candidate = catalog.exact(
            command.candidate_id,
            command.expected_source_generation_hash,
            command.expected_candidate_hash,
            command.expected_evidence_hash,
        )?;
        let authority = catalog.authority();
        let command_id = command_hash(command, candidate.binding.origin.lens_id);
        if let Some(index) = self.by_command.get(&command_id) {
            let header = self.receipts[*index].header();
            return Ok(DecisionOutcome {
                receipt_id: header.receipt_id,
                sequence: header.sequence,
                reused: true,
            });
        }

        let sequence = match self.receipts.last() {
            Some(receipt) => receipt
                .header()
                .sequence
                .checked_add(1)
                .ok_or(SemanticReviewError::InvalidDecisionChain)?,
            None => 1,
        };
        let previous_receipt_id = self
            .head(command.candidate_id)
            .map_or([0; 32], |receipt| receipt.header().receipt_id);
        let status = status_for_action(command.action);
        let receipt_id = receipt_hash(
            command_id,
            previous_receipt_id,
            sequence,
            command.decided_at_unix_millis,
        );
        let mut header = DecisionReceiptHeaderV1 {
            magic: DECISION_MAGIC,
            version: REVIEW_VERSION,
            header_size: size_of::<DecisionReceiptHeaderV1>() as u32,
            total_len: (size_of::<DecisionReceiptHeaderV1>() + command.reason.len()) as u64,
            sequence,
            receipt_id,
            command_id,
            previous_receipt_id,
            source_generation_hash: authority.source_generation_hash,
            document_hash: authority.document_hash,
            candidate_id: command.candidate_id,
            candidate_hash: candidate.binding.candidate_hash,
            evidence_hash: candidate.binding.evidence_hash,
            lens_id: candidate.binding.origin.lens_id,
            vocabulary_hash: candidate.binding.origin.vocabulary_hash,
            native_document_id: authority.native_document_id,
            document_revision: authority.document_revision,
            registry_revision: authority.registry_revision,
            producer_generation: authority.producer_generation,
            decided_at_unix_millis: command.decided_at_unix_millis,
            action: command.action as u16,
            status: status as u16,
            reason_len: command.reason.len() as u32,
            flags: 0,
            artifact_hash: [0; 32],
            reserved: 0,
        };
        header.artifact_hash = artifact_hash(&header, command.reason.as_bytes());
        let path = receipt_path(&self.root, sequence, receipt_id);
        write_receipt_new(&path, &header, command.reason.as_bytes())?;
        let receipt = open_receipt(&path)?;
        self.push_verified(receipt)?;
        Ok(DecisionOutcome {
            receipt_id,
            sequence,
            reused: false,
        })
    }

    fn push_verified(
        &mut self,
        receipt: VerifiedDecisionReceipt,
    ) -> Result<(), SemanticReviewError> {
        let header = receipt.header();
        let expected_sequence = match self.receipts.last() {
            Some(previous) => previous
                .header()
                .sequence
                .checked_add(1)
                .ok_or(SemanticReviewError::InvalidDecisionChain)?,
            None => 1,
        };
        if header.sequence != expected_sequence
            || self.by_command.contains_key(&header.command_id)
            || self
                .heads
                .get(&header.candidate_id)
                .map_or(header.previous_receipt_id != [0; 32], |index| {
                    self.receipts[*index].header().receipt_id != header.previous_receipt_id
                })
        {
            return Err(SemanticReviewError::InvalidDecisionChain);
        }
        let index = self.receipts.len();
        self.by_command.insert(header.command_id, index);
        self.heads.insert(header.candidate_id, index);
        self.receipts.push(receipt);
        Ok(())
    }
}

fn status_for_action(action: DecisionAction) -> CandidateStatus {
    match action {
        DecisionAction::Accept => CandidateStatus::Accepted,
        DecisionAction::Reject => CandidateStatus::Rejected,
        DecisionAction::Defer => CandidateStatus::Deferred,
        DecisionAction::Undo => CandidateStatus::Proposed,
    }
}

fn command_hash(command: &DecisionCommand, lens_id: [u8; 32]) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"phoenix-decision-command/v1");
    hash.update(&command.candidate_id.0);
    hash.update(&command.expected_source_generation_hash);
    hash.update(&command.expected_candidate_hash);
    hash.update(&command.expected_evidence_hash);
    hash.update(&lens_id);
    hash.update(&(command.action as u16).to_le_bytes());
    hash.update(&(command.reason.len() as u64).to_le_bytes());
    hash.update(command.reason.as_bytes());
    *hash.finalize().as_bytes()
}

fn receipt_hash(
    command_id: [u8; 32],
    previous: [u8; 32],
    sequence: u64,
    decided_at: u64,
) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"phoenix-decision-receipt/v1");
    hash.update(&command_id);
    hash.update(&previous);
    hash.update(&sequence.to_le_bytes());
    hash.update(&decided_at.to_le_bytes());
    *hash.finalize().as_bytes()
}

fn artifact_hash(header: &DecisionReceiptHeaderV1, reason: &[u8]) -> [u8; 32] {
    let mut unhashed = *header;
    unhashed.artifact_hash = [0; 32];
    let mut hash = blake3::Hasher::new();
    hash.update(bytes_of(&unhashed));
    hash.update(reason);
    *hash.finalize().as_bytes()
}

fn receipt_path(root: &Path, sequence: u64, id: [u8; 32]) -> PathBuf {
    root.join(format!(
        "{sequence:020}-{}.{}",
        short_hex(id),
        DECISION_EXTENSION
    ))
}

fn short_hex(value: [u8; 32]) -> String {
    let mut output = String::with_capacity(16);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in &value[..8] {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn write_receipt_new(
    path: &Path,
    header: &DecisionReceiptHeaderV1,
    reason: &[u8],
) -> Result<(), SemanticReviewError> {
    let pending = path.with_extension("pending");
    if pending.exists() {
        fs::remove_file(&pending).map_err(|source| SemanticReviewError::io(&pending, source))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|source| SemanticReviewError::io(&pending, source))?;
    let write_result = file
        .write_all(bytes_of(header))
        .and_then(|_| file.write_all(reason))
        .and_then(|_| file.sync_all());
    if let Err(source) = write_result {
        let _ = fs::remove_file(&pending);
        return Err(SemanticReviewError::io(&pending, source));
    }
    if let Err(source) = fs::rename(&pending, path) {
        let _ = fs::remove_file(&pending);
        return Err(SemanticReviewError::io(path, source));
    }
    Ok(())
}

fn open_receipt(path: &Path) -> Result<VerifiedDecisionReceipt, SemanticReviewError> {
    let file = File::open(path).map_err(|source| SemanticReviewError::io(path, source))?;
    let len = file
        .metadata()
        .map_err(|source| SemanticReviewError::io(path, source))?
        .len();
    if len > MAX_DECISION_BYTES {
        return Err(SemanticReviewError::OversizedArtifact {
            actual: len,
            maximum: MAX_DECISION_BYTES,
        });
    }
    // SAFETY: the file is mapped read-only and retained by the verified owner.
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .map_err(|source| SemanticReviewError::io(path, source))?
    };
    let header_bytes = mmap
        .get(..size_of::<DecisionReceiptHeaderV1>())
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let header = *try_from_bytes::<DecisionReceiptHeaderV1>(header_bytes)
        .map_err(|_| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let reason_end = (header.header_size as usize)
        .checked_add(header.reason_len as usize)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let reason = mmap
        .get(header.header_size as usize..reason_end)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    if header.magic != DECISION_MAGIC
        || header.version != REVIEW_VERSION
        || header.header_size as usize != size_of::<DecisionReceiptHeaderV1>()
        || header.total_len != len
        || reason.len() > MAX_REASON_BYTES
        || std::str::from_utf8(reason).is_err()
        || DecisionAction::from_raw(header.action).is_none()
        || CandidateStatus::from_raw(header.status).is_none()
        || header.artifact_hash != artifact_hash(&header, reason)
    {
        return Err(SemanticReviewError::CorruptArtifact(path.to_path_buf()));
    }
    Ok(VerifiedDecisionReceipt {
        mmap,
        header,
        path: path.to_path_buf(),
    })
}
