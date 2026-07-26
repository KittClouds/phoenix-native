use phoenix_scene_archive::PhoenixSceneArchiveV1;

pub const MAGIC: [u8; 8] = *b"PHXPRD1\0";
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 256;
pub const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_LABEL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_REFERENCE_RECORDS: u32 = 2_000_000;
pub const NO_REFERENCE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductIndexBinding {
    pub archive_generation: u64,
    pub archive_cohort_hash: [u8; 32],
}

impl ProductIndexBinding {
    #[must_use]
    pub fn from_archive(archive: &PhoenixSceneArchiveV1) -> Self {
        let header = archive.header();
        Self {
            archive_generation: header.generation_id,
            archive_cohort_hash: header.cohort_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductIndexHeader {
    pub binding: ProductIndexBinding,
    pub file_len: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub mapping_count: u32,
    pub reference_count: u32,
    pub label_bytes: u32,
    pub body_hash: [u8; 32],
    pub index_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SectionRange {
    pub offset: u64,
    pub len: u64,
}

pub(crate) fn index_hash(
    binding: ProductIndexBinding,
    node_count: u32,
    edge_count: u32,
    mapping_count: u32,
    reference_count: u32,
    label_bytes: u32,
    body_hash: [u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-scene-product-index-v1");
    hasher.update(&binding.archive_generation.to_le_bytes());
    hasher.update(&binding.archive_cohort_hash);
    hasher.update(&node_count.to_le_bytes());
    hasher.update(&edge_count.to_le_bytes());
    hasher.update(&mapping_count.to_le_bytes());
    hasher.update(&reference_count.to_le_bytes());
    hasher.update(&label_bytes.to_le_bytes());
    hasher.update(&body_hash);
    *hasher.finalize().as_bytes()
}
