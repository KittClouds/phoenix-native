use crate::{AuthorityClass, PageKindV3};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryContractError {
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("generation is too small: {actual} bytes, need at least {minimum}")]
    TooSmall { actual: u64, minimum: u64 },
    #[error("generation is oversized: {actual} bytes exceeds {maximum}")]
    OversizedGeneration { actual: u64, maximum: u64 },
    #[error("generation magic is invalid")]
    BadMagic,
    #[error("unsupported generation version {actual}; expected {expected}")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("header size {actual} does not match {expected}")]
    HeaderSize { actual: u32, expected: u32 },
    #[error("declared total length {declared} does not match actual length {actual}")]
    TotalLength { declared: u64, actual: u64 },
    #[error("page count {actual} does not match required count {expected}")]
    PageCount { actual: u32, expected: u32 },
    #[error("page directory is outside the generation")]
    DirectoryOutOfBounds,
    #[error("page directory length {actual} does not match {expected}")]
    DirectoryLength { actual: u64, expected: u64 },
    #[error("offset {offset} is not aligned to {alignment}")]
    Misaligned { offset: u64, alignment: u64 },
    #[error("page tag {0} is unknown")]
    UnknownPageKind(u16),
    #[error("page directory entry {index} contains {actual:?}; expected {expected:?}")]
    UnexpectedPageOrder {
        index: usize,
        actual: PageKindV3,
        expected: PageKindV3,
    },
    #[error("authority tag {0} is unknown")]
    UnknownAuthority(u16),
    #[error("page {page:?} has authority {actual:?}; expected {expected:?}")]
    WrongAuthority {
        page: PageKindV3,
        actual: AuthorityClass,
        expected: AuthorityClass,
    },
    #[error("page {page:?} record size {actual} does not match {expected}")]
    RecordSize {
        page: PageKindV3,
        actual: u32,
        expected: u32,
    },
    #[error("page {page:?} record alignment {actual} does not match {expected}")]
    RecordAlignment {
        page: PageKindV3,
        actual: u32,
        expected: u32,
    },
    #[error("page {page:?} is outside the generation")]
    PageOutOfBounds { page: PageKindV3 },
    #[error("page {left:?} overlaps page {right:?}")]
    OverlappingPages { left: PageKindV3, right: PageKindV3 },
    #[error("page {page:?} count {actual} exceeds {maximum}")]
    RecordCount {
        page: PageKindV3,
        actual: u64,
        maximum: u64,
    },
    #[error("page {page:?} byte length {actual} does not match {expected}")]
    PageLength {
        page: PageKindV3,
        actual: u64,
        expected: u64,
    },
    #[error("page {page:?} hash does not match its descriptor")]
    PageHashMismatch { page: PageKindV3 },
    #[error("page {page:?} schema hash does not match the frozen record layout")]
    SchemaHashMismatch { page: PageKindV3 },
    #[error("generation hash does not match its header")]
    GenerationHashMismatch,
    #[error("source-set hash does not match the authoritative source pages")]
    SourceSetHashMismatch,
    #[error("generation is not marked complete")]
    IncompleteGeneration,
    #[error("page {page:?} cannot be viewed as the requested record type")]
    InvalidRecordLayout { page: PageKindV3 },
    #[error("string reference is outside the UTF-8 string slab")]
    InvalidStringRef,
    #[error("mixed-source records violate their typed authority contract: {0}")]
    InvalidSourceModel(&'static str),
    #[error("generation contains duplicate stable IDs")]
    DuplicateStableId,
    #[error("generation records are not in canonical order")]
    NonCanonicalOrder,
    #[error("generation belongs to a different namespace")]
    NamespaceMismatch,
    #[error("generation source set does not match the requested source set")]
    ExpectedSourceSetMismatch,
    #[error("generation {actual} is older than required generation {minimum}")]
    StaleGeneration { actual: u64, minimum: u64 },
    #[error("builder input contains a duplicate external source identity")]
    DuplicateSourceIdentity,
    #[error("builder input contains a duplicate conversation turn ordinal")]
    DuplicateTurnOrdinal,
    #[error("source content is too large for 32-bit byte coordinates")]
    SourceTextOversized,
    #[error("document chunk range is outside the document")]
    InvalidChunkRange,
    #[error("record count cannot be represented by the frozen format")]
    CountOverflow,
}

impl MemoryContractError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
