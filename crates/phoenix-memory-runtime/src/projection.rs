use crate::{DecisionDispositionV1, MemoryCatalogV1, MemoryRuntimeError, PolicyDecisionLedgerV1};
use bytemuck::{bytes_of, try_cast_slice, try_from_bytes, Pod, Zeroable};
use hashbrown::HashMap;
use memmap2::{Mmap, MmapOptions};
use phoenix_memory_semantics::MemoryActionV1;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};

pub const CURRENT_MEMORY_PROJECTION_EXTENSION: &str = "phxmemory";
const CURRENT_MEMORY_PROJECTION_MAGIC: [u8; 8] = *b"PHXMEMV1";
const CURRENT_MEMORY_PROJECTION_VERSION: u32 = 1;
const MAX_CURRENT_MEMORY_PROJECTION_BYTES: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum CurrentMemoryStateV1 {
    Active = 1,
    Superseded = 2,
    Disputed = 3,
    Historical = 4,
    CandidateOnly = 5,
    EvidenceOnly = 6,
}

impl CurrentMemoryStateV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Active),
            2 => Some(Self::Superseded),
            3 => Some(Self::Disputed),
            4 => Some(Self::Historical),
            5 => Some(Self::CandidateOnly),
            6 => Some(Self::EvidenceOnly),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CurrentMemoryRecordV1 {
    pub candidate_id: [u8; 32],
    pub related_candidate_id: [u8; 32],
    pub decision_receipt_id: [u8; 32],
    pub source_id: u64,
    pub decision_sequence: u64,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub original_valid_time_to_millis: i64,
    pub system_sequence_from: u64,
    pub system_sequence_to: u64,
    pub candidate_row_index: u32,
    pub state: u16,
    pub flags: u16,
}

const _: [(); 160] = [(); core::mem::size_of::<CurrentMemoryRecordV1>()];

impl CurrentMemoryRecordV1 {
    pub fn memory_state(&self) -> CurrentMemoryStateV1 {
        CurrentMemoryStateV1::from_raw(self.state).unwrap_or(CurrentMemoryStateV1::CandidateOnly)
    }

    pub fn active_at(&self, valid_at_millis: i64, ledger_sequence: u64) -> bool {
        let state_was_active = self.memory_state() == CurrentMemoryStateV1::Active
            || (self.memory_state() == CurrentMemoryStateV1::Superseded
                && ledger_sequence < self.system_sequence_to);
        let valid_to = if ledger_sequence < self.system_sequence_to {
            self.original_valid_time_to_millis
        } else {
            self.valid_time_to_millis
        };
        state_was_active
            && self.system_sequence_from <= ledger_sequence
            && ledger_sequence < self.system_sequence_to
            && self.valid_time_from_millis <= valid_at_millis
            && valid_at_millis < valid_to
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionReceiptV1 {
    pub source_generation_hash: [u8; 32],
    pub projection_hash: [u8; 32],
    pub ledger_sequence: u64,
    pub decision_count: u32,
    pub projected_count: u32,
    pub active_count: u32,
    pub superseded_count: u32,
    pub disputed_count: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct CurrentMemoryProjectionHeaderV1 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub total_len: u64,
    pub record_count: u64,
    pub ledger_sequence: u64,
    pub source_generation_hash: [u8; 32],
    pub projection_hash: [u8; 32],
    pub artifact_hash: [u8; 32],
    pub active_count: u32,
    pub superseded_count: u32,
    pub disputed_count: u32,
    pub reserved_u32: u32,
    pub reserved: [u64; 3],
}

const _: [(); 176] = [(); core::mem::size_of::<CurrentMemoryProjectionHeaderV1>()];

#[derive(Debug)]
pub struct VerifiedCurrentMemoryProjectionV1 {
    mmap: Mmap,
    header: CurrentMemoryProjectionHeaderV1,
    path: PathBuf,
}

impl VerifiedCurrentMemoryProjectionV1 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MemoryRuntimeError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| MemoryRuntimeError::io(path, source))?;
        let len = file
            .metadata()
            .map_err(|source| MemoryRuntimeError::io(path, source))?
            .len();
        if len > MAX_CURRENT_MEMORY_PROJECTION_BYTES {
            return Err(MemoryRuntimeError::OversizedProjection {
                actual: len,
                maximum: MAX_CURRENT_MEMORY_PROJECTION_BYTES,
            });
        }
        // SAFETY: the verified owner retains this read-only mapping.
        let mmap = unsafe {
            MmapOptions::new()
                .map(&file)
                .map_err(|source| MemoryRuntimeError::io(path, source))?
        };
        let header = mmap
            .get(..size_of::<CurrentMemoryProjectionHeaderV1>())
            .and_then(|bytes| try_from_bytes::<CurrentMemoryProjectionHeaderV1>(bytes).ok())
            .copied()
            .ok_or_else(|| MemoryRuntimeError::CorruptProjection(path.to_path_buf()))?;
        let records_bytes = mmap
            .get(size_of::<CurrentMemoryProjectionHeaderV1>()..)
            .ok_or_else(|| MemoryRuntimeError::CorruptProjection(path.to_path_buf()))?;
        let records = try_cast_slice::<u8, CurrentMemoryRecordV1>(records_bytes)
            .map_err(|_| MemoryRuntimeError::CorruptProjection(path.to_path_buf()))?;
        let active_count = records
            .iter()
            .filter(|record| record.memory_state() == CurrentMemoryStateV1::Active)
            .count() as u32;
        let superseded_count = records
            .iter()
            .filter(|record| record.memory_state() == CurrentMemoryStateV1::Superseded)
            .count() as u32;
        let disputed_count = records
            .iter()
            .filter(|record| record.memory_state() == CurrentMemoryStateV1::Disputed)
            .count() as u32;
        if header.magic != CURRENT_MEMORY_PROJECTION_MAGIC
            || header.version != CURRENT_MEMORY_PROJECTION_VERSION
            || header.header_size as usize != size_of::<CurrentMemoryProjectionHeaderV1>()
            || header.total_len != len
            || header.record_count != records.len() as u64
            || header.artifact_hash != projection_artifact_hash(&header, records)
            || projection_hash(
                header.source_generation_hash,
                header.ledger_sequence,
                records,
            ) != header.projection_hash
            || header.active_count != active_count
            || header.superseded_count != superseded_count
            || header.disputed_count != disputed_count
            || !validate_projected_records(records)
        {
            return Err(MemoryRuntimeError::CorruptProjection(path.to_path_buf()));
        }
        Ok(Self {
            mmap,
            header,
            path: path.to_path_buf(),
        })
    }

    pub fn header(&self) -> &CurrentMemoryProjectionHeaderV1 {
        &self.header
    }

    pub fn records(&self) -> &[CurrentMemoryRecordV1] {
        let bytes = &self.mmap[size_of::<CurrentMemoryProjectionHeaderV1>()..];
        try_cast_slice(bytes).expect("verified projection record layout")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug)]
pub struct CurrentMemoryProjectionV1 {
    records: Box<[CurrentMemoryRecordV1]>,
    receipt: ProjectionReceiptV1,
}

impl CurrentMemoryProjectionV1 {
    pub fn materialize(
        catalog: &MemoryCatalogV1,
        ledger: &PolicyDecisionLedgerV1,
    ) -> Result<Self, MemoryRuntimeError> {
        let effective = effective_receipts(ledger)?;
        let mut records =
            HashMap::<[u8; 32], CurrentMemoryRecordV1>::with_capacity(effective.len());
        let mut replacements = Vec::new();
        for header in effective {
            if header.disposition() != Some(DecisionDispositionV1::Commit) {
                continue;
            }
            let action = header
                .memory_action()
                .ok_or(MemoryRuntimeError::InvalidProjection(
                    "decision action is invalid",
                ))?;
            let binding = catalog.exact(
                header.candidate_id,
                header.source_generation_hash,
                header.candidate_hash,
                header.evidence_hash,
            )?;
            let state = state_for_action(action);
            let mut valid_from = binding.valid_time_from_millis;
            if action.requires_replacement_target() {
                valid_from = valid_from.max(header.effective_at_unix_millis);
                replacements.push((
                    header.sequence,
                    header.replacement_target_id,
                    header.candidate_id,
                    header.effective_at_unix_millis,
                ));
            }
            records.insert(
                header.candidate_id,
                CurrentMemoryRecordV1 {
                    candidate_id: header.candidate_id,
                    related_candidate_id: if action.requires_replacement_target() {
                        header.replacement_target_id
                    } else {
                        [0; 32]
                    },
                    decision_receipt_id: header.receipt_id,
                    source_id: binding.source_id,
                    decision_sequence: header.sequence,
                    valid_time_from_millis: valid_from,
                    valid_time_to_millis: binding.valid_time_to_millis,
                    original_valid_time_to_millis: binding.valid_time_to_millis,
                    system_sequence_from: header.sequence,
                    system_sequence_to: u64::MAX,
                    candidate_row_index: binding.row_index,
                    state: state as u16,
                    flags: 0,
                },
            );
        }
        replacements.sort_unstable_by_key(|item| item.0);
        for (sequence, target, replacement, effective_at) in replacements {
            let target_record =
                records
                    .get_mut(&target)
                    .ok_or(MemoryRuntimeError::InvalidProjection(
                        "replacement target has no effective committed memory",
                    ))?;
            if target_record.memory_state() != CurrentMemoryStateV1::Active
                || target_record.system_sequence_from >= sequence
                || effective_at < target_record.valid_time_from_millis
                || effective_at > target_record.original_valid_time_to_millis
            {
                return Err(MemoryRuntimeError::InvalidProjection(
                    "replacement target is not an earlier active memory",
                ));
            }
            target_record.state = CurrentMemoryStateV1::Superseded as u16;
            target_record.related_candidate_id = replacement;
            target_record.valid_time_to_millis = target_record
                .original_valid_time_to_millis
                .min(effective_at);
            target_record.system_sequence_to = sequence;
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.sort_unstable_by_key(|record| record.candidate_id);
        let receipt = projection_receipt(catalog, ledger, &records)?;
        Ok(Self {
            records: records.into_boxed_slice(),
            receipt,
        })
    }

    pub fn records(&self) -> &[CurrentMemoryRecordV1] {
        &self.records
    }

    pub fn receipt(&self) -> ProjectionReceiptV1 {
        self.receipt
    }

    pub fn get(&self, candidate_id: [u8; 32]) -> Option<&CurrentMemoryRecordV1> {
        self.records
            .binary_search_by_key(&candidate_id, |record| record.candidate_id)
            .ok()
            .map(|index| &self.records[index])
    }

    pub fn active_at(
        &self,
        valid_at_millis: i64,
        ledger_sequence: u64,
    ) -> impl Iterator<Item = &CurrentMemoryRecordV1> {
        self.records
            .iter()
            .filter(move |record| record.active_at(valid_at_millis, ledger_sequence))
    }

    pub fn publish(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<VerifiedCurrentMemoryProjectionV1, MemoryRuntimeError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| MemoryRuntimeError::io(parent, source))?;
        }
        if path.exists() {
            let existing = VerifiedCurrentMemoryProjectionV1::open(path)?;
            if existing.header().projection_hash == self.receipt.projection_hash {
                return Ok(existing);
            }
            return Err(MemoryRuntimeError::InvalidProjection(
                "projection path already contains a different immutable artifact",
            ));
        }
        let total_len = size_of::<CurrentMemoryProjectionHeaderV1>()
            .checked_add(size_of::<CurrentMemoryRecordV1>() * self.records.len())
            .ok_or(MemoryRuntimeError::InvalidProjection(
                "projection artifact length overflowed",
            ))?;
        let mut header = CurrentMemoryProjectionHeaderV1 {
            magic: CURRENT_MEMORY_PROJECTION_MAGIC,
            version: CURRENT_MEMORY_PROJECTION_VERSION,
            header_size: size_of::<CurrentMemoryProjectionHeaderV1>() as u32,
            total_len: total_len as u64,
            record_count: self.records.len() as u64,
            ledger_sequence: self.receipt.ledger_sequence,
            source_generation_hash: self.receipt.source_generation_hash,
            projection_hash: self.receipt.projection_hash,
            artifact_hash: [0; 32],
            active_count: self.receipt.active_count,
            superseded_count: self.receipt.superseded_count,
            disputed_count: self.receipt.disputed_count,
            reserved_u32: 0,
            reserved: [0; 3],
        };
        header.artifact_hash = projection_artifact_hash(&header, &self.records);
        let pending = path.with_extension("pending");
        if pending.exists() {
            fs::remove_file(&pending).map_err(|source| MemoryRuntimeError::io(&pending, source))?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|source| MemoryRuntimeError::io(&pending, source))?;
        let write_result = file
            .write_all(bytes_of(&header))
            .and_then(|_| file.write_all(bytemuck::cast_slice(&self.records)))
            .and_then(|_| file.sync_all());
        if let Err(source) = write_result {
            let _ = fs::remove_file(&pending);
            return Err(MemoryRuntimeError::io(&pending, source));
        }
        if let Err(source) = fs::rename(&pending, path) {
            let _ = fs::remove_file(&pending);
            return Err(MemoryRuntimeError::io(path, source));
        }
        VerifiedCurrentMemoryProjectionV1::open(path)
    }
}

fn effective_receipts(
    ledger: &PolicyDecisionLedgerV1,
) -> Result<Vec<&crate::PolicyDecisionReceiptHeaderV1>, MemoryRuntimeError> {
    let mut stacks = HashMap::<[u8; 32], Vec<&crate::PolicyDecisionReceiptHeaderV1>>::new();
    for receipt in ledger.receipts() {
        let header = receipt.header();
        match header.disposition() {
            Some(DecisionDispositionV1::Undo) => {
                if stacks
                    .entry(header.candidate_id)
                    .or_default()
                    .pop()
                    .is_none()
                {
                    return Err(MemoryRuntimeError::InvalidDecisionChain);
                }
            }
            Some(_) => stacks.entry(header.candidate_id).or_default().push(header),
            None => return Err(MemoryRuntimeError::InvalidDecisionChain),
        }
    }
    let mut effective = stacks
        .into_values()
        .filter_map(|mut stack| stack.pop())
        .collect::<Vec<_>>();
    effective.sort_unstable_by_key(|header| header.sequence);
    Ok(effective)
}

fn state_for_action(action: MemoryActionV1) -> CurrentMemoryStateV1 {
    match action {
        MemoryActionV1::AddCandidate
        | MemoryActionV1::Elaborate
        | MemoryActionV1::CloseAndReplace
        | MemoryActionV1::Supersede
        | MemoryActionV1::RetainBoth => CurrentMemoryStateV1::Active,
        MemoryActionV1::OpenDispute => CurrentMemoryStateV1::Disputed,
        MemoryActionV1::PreserveHistorical => CurrentMemoryStateV1::Historical,
        MemoryActionV1::CandidateOnly | MemoryActionV1::Ignore | MemoryActionV1::Defer => {
            CurrentMemoryStateV1::CandidateOnly
        }
        MemoryActionV1::AddEvidence => CurrentMemoryStateV1::EvidenceOnly,
    }
}

fn projection_receipt(
    catalog: &MemoryCatalogV1,
    ledger: &PolicyDecisionLedgerV1,
    records: &[CurrentMemoryRecordV1],
) -> Result<ProjectionReceiptV1, MemoryRuntimeError> {
    let active_count = records
        .iter()
        .filter(|record| record.memory_state() == CurrentMemoryStateV1::Active)
        .count();
    let superseded_count = records
        .iter()
        .filter(|record| record.memory_state() == CurrentMemoryStateV1::Superseded)
        .count();
    let disputed_count = records
        .iter()
        .filter(|record| record.memory_state() == CurrentMemoryStateV1::Disputed)
        .count();
    Ok(ProjectionReceiptV1 {
        source_generation_hash: catalog.source_generation_hash(),
        projection_hash: projection_hash(
            catalog.source_generation_hash(),
            ledger.sequence(),
            records,
        ),
        ledger_sequence: ledger.sequence(),
        decision_count: u32::try_from(ledger.receipts().len())
            .map_err(|_| MemoryRuntimeError::InvalidProjection("too many decisions"))?,
        projected_count: u32::try_from(records.len())
            .map_err(|_| MemoryRuntimeError::InvalidProjection("too many memories"))?,
        active_count: u32::try_from(active_count)
            .map_err(|_| MemoryRuntimeError::InvalidProjection("too many active memories"))?,
        superseded_count: u32::try_from(superseded_count)
            .map_err(|_| MemoryRuntimeError::InvalidProjection("too many supersessions"))?,
        disputed_count: u32::try_from(disputed_count)
            .map_err(|_| MemoryRuntimeError::InvalidProjection("too many disputes"))?,
    })
}

fn projection_hash(
    source_generation_hash: [u8; 32],
    ledger_sequence: u64,
    records: &[CurrentMemoryRecordV1],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.current-memory-projection/v1\0");
    hasher.update(&source_generation_hash);
    hasher.update(&ledger_sequence.to_le_bytes());
    for record in records {
        hasher.update(bytes_of(record));
    }
    *hasher.finalize().as_bytes()
}

fn projection_artifact_hash(
    header: &CurrentMemoryProjectionHeaderV1,
    records: &[CurrentMemoryRecordV1],
) -> [u8; 32] {
    let mut unhashed = *header;
    unhashed.artifact_hash = [0; 32];
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes_of(&unhashed));
    hasher.update(bytemuck::cast_slice(records));
    *hasher.finalize().as_bytes()
}

fn validate_projected_records(records: &[CurrentMemoryRecordV1]) -> bool {
    let mut previous = None;
    for record in records {
        if previous.is_some_and(|id| id >= record.candidate_id)
            || record.candidate_id == [0; 32]
            || record.decision_receipt_id == [0; 32]
            || CurrentMemoryStateV1::from_raw(record.state).is_none()
            || record.valid_time_from_millis > record.valid_time_to_millis
            || record.valid_time_to_millis > record.original_valid_time_to_millis
            || record.system_sequence_from > record.system_sequence_to
        {
            return false;
        }
        previous = Some(record.candidate_id);
    }
    true
}
