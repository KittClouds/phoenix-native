use crate::{AuthorityClass, PageKind};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphGenerationV2Error {
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
    #[error("page directory offset {offset} is not aligned to {alignment}")]
    DirectoryMisaligned { offset: u64, alignment: u64 },
    #[error("page tag {0} is unknown")]
    UnknownPageKind(u16),
    #[error("page {0:?} occurs more than once")]
    DuplicatePage(PageKind),
    #[error("required page {0:?} is missing")]
    MissingPage(PageKind),
    #[error("page directory entry {index} contains {actual:?}; expected {expected:?}")]
    UnexpectedPageOrder {
        index: usize,
        actual: PageKind,
        expected: PageKind,
    },
    #[error("authority tag {0} is unknown")]
    UnknownAuthority(u16),
    #[error("page {page:?} has authority {actual:?}; expected {expected:?}")]
    WrongAuthority {
        page: PageKind,
        actual: AuthorityClass,
        expected: AuthorityClass,
    },
    #[error("page {page:?} record size {actual} does not match {expected}")]
    RecordSize {
        page: PageKind,
        actual: u32,
        expected: u32,
    },
    #[error("page {page:?} record alignment {actual} does not match {expected}")]
    RecordAlignment {
        page: PageKind,
        actual: u32,
        expected: u32,
    },
    #[error("page {page:?} offset {offset} is not aligned to {alignment}")]
    PageMisaligned {
        page: PageKind,
        offset: u64,
        alignment: u64,
    },
    #[error("page {page:?} is outside the generation")]
    PageOutOfBounds { page: PageKind },
    #[error("page {left:?} overlaps page {right:?}")]
    OverlappingPages { left: PageKind, right: PageKind },
    #[error("page {page:?} count {actual} exceeds {maximum}")]
    RecordCount {
        page: PageKind,
        actual: u64,
        maximum: u64,
    },
    #[error("page {page:?} byte length {actual} does not match {expected}")]
    PageLength {
        page: PageKind,
        actual: u64,
        expected: u64,
    },
    #[error("page {page:?} hash does not match its descriptor")]
    PageHashMismatch { page: PageKind },
    #[error("generation hash does not match its header")]
    GenerationHashMismatch,
    #[error("generation is not marked complete")]
    IncompleteGeneration,
    #[error("page {page:?} schema hash does not match the frozen record layout")]
    SchemaHashMismatch { page: PageKind },
    #[error("page {page:?} cannot be viewed as the requested record type")]
    InvalidRecordLayout { page: PageKind },
    #[error("string reference is outside the string slab")]
    InvalidStringRef,
    #[error("record endpoint does not resolve")]
    InvalidEndpoint,
    #[error("record evidence binding does not resolve")]
    InvalidEvidenceBinding,
    #[error("decision does not bind to an existing candidate and evidence set")]
    InvalidDecisionBinding,
    #[error("generation belongs to a different document cohort")]
    CohortMismatch,
    #[error("generation document revision is stale")]
    StaleDocument,
    #[error("generation registry revision is stale")]
    StaleRegistry,
    #[error("page {0:?} is recognized but unsupported by this consumer")]
    UnsupportedPage(PageKind),
}

impl GraphGenerationV2Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
