use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProductIndexError {
    #[error("scene product index is missing: {0}")]
    Missing(std::path::PathBuf),
    #[error("scene product index header is corrupt: {0}")]
    CorruptHeader(&'static str),
    #[error("scene product index body hash failed")]
    CorruptBody,
    #[error("scene product index hash failed")]
    CorruptIndexHash,
    #[error("scene product index format version {0} is unsupported")]
    UnsupportedFormatVersion(u32),
    #[error("scene product index is oversized: {actual} bytes exceeds {limit}")]
    Oversized { actual: u64, limit: u64 },
    #[error("scene product index label slab is oversized: {actual} bytes exceeds {limit}")]
    OversizedLabels { actual: u64, limit: u64 },
    #[error("scene product index range arithmetic overflowed")]
    RangeOverflow,
    #[error("scene product index typed section {0} is invalid")]
    InvalidSection(&'static str),
    #[error("scene product index label slab is not valid UTF-8")]
    InvalidLabelSlab,
    #[error("scene product index label range is invalid for node slot {0}")]
    InvalidLabelRange(usize),
    #[error("scene product index reference {reference} is invalid for {resource} slot {slot}")]
    InvalidReference {
        resource: &'static str,
        slot: usize,
        reference: u32,
    },
    #[error("scene product index archive generation {index} does not match {archive}")]
    StaleGeneration { index: u64, archive: u64 },
    #[error("scene product index archive cohort hash does not match")]
    CohortMismatch,
    #[error("scene product index {resource} count {index} does not match archive count {archive}")]
    InventoryMismatch {
        resource: &'static str,
        index: usize,
        archive: usize,
    },
    #[error(
        "scene product index {resource} identity {index_id} does not match archive identity \
         {archive_id} at slot {slot}"
    )]
    IdentityMismatch {
        resource: &'static str,
        slot: usize,
        index_id: u64,
        archive_id: u64,
    },
    #[error("scene product index entity identity zero is reserved")]
    ZeroEntityIdentity,
    #[error("scene product index contains duplicate entity identity {0}")]
    DuplicateEntity(u64),
    #[error("scene product index contains duplicate mapped node identity {0}")]
    DuplicateMappedNode(u64),
    #[error("scene product index entity {entity} maps to missing node {node}")]
    MissingMappedNode { entity: u64, node: u64 },
    #[error("scene product index target already exists: {0}")]
    AlreadyExists(std::path::PathBuf),
    #[error("scene product index I/O failed")]
    Io(#[from] std::io::Error),
}
