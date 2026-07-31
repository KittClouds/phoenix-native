use bytemuck::{Pod, Zeroable};

pub const EMBEDDING_PAGE_MAGIC: [u8; 8] = *b"PHXEM001";
pub const EMBEDDING_PAGE_VERSION: u32 = 1;
pub const EMBEDDING_PAGE_EXTENSION: &str = "phxe1";
pub const EMBEDDING_PAGE_ALIGNMENT: u64 = 64;
pub const EMBEDDING_PAGE_FLAG_COMPLETE: u32 = 1;
pub const ROW_FLAG_NORMALIZED: u32 = 1;
pub const MAX_EMBEDDING_PAGE_BYTES: u64 = 16 << 30;
pub const MAX_EMBEDDING_ROWS: u64 = 64_000_000;
pub const MAX_EMBEDDING_DIMENSION: u32 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum EmbeddingScalarFormat {
    F32 = 1,
}

impl EmbeddingScalarFormat {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::F32),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EmbeddingPageHeaderV1 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub flags: u32,
    pub scalar_format: u16,
    pub reserved_u16: u16,
    pub dimension: u32,
    pub row_record_size: u32,
    pub total_len: u64,
    pub rows_offset: u64,
    pub rows_len: u64,
    pub vectors_offset: u64,
    pub vectors_len: u64,
    pub row_count: u64,
    pub generation_hash: [u8; 32],
    pub source_set_hash: [u8; 32],
    pub model_identity_hash: [u8; 32],
    pub model_asset_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub rows_hash: [u8; 32],
    pub vectors_hash: [u8; 32],
    pub artifact_hash: [u8; 32],
    pub reserved: [u64; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct EmbeddingRowV1 {
    pub subject_id: u64,
    pub source_id: u64,
    pub content_hash: [u8; 32],
    pub vector_start: u64,
    pub source_start: u32,
    pub source_end: u32,
    pub ordinal: u32,
    pub dimension: u32,
    pub source_kind: u16,
    pub content_kind: u16,
    pub flags: u32,
    pub reserved: [u64; 2],
}

pub fn compute_artifact_hash(header: &EmbeddingPageHeaderV1) -> [u8; 32] {
    let mut canonical = *header;
    canonical.artifact_hash = [0; 32];
    *blake3::hash(bytemuck::bytes_of(&canonical)).as_bytes()
}

pub(crate) const fn align_up(value: u64, alignment: u64) -> u64 {
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + (alignment - remainder)
    }
}
