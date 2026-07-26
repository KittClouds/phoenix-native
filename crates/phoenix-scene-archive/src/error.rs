use crate::{PageKey, PageKind};
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("scene archive is missing: {0}")]
    MissingArchive(std::path::PathBuf),
    #[error("required archive page is missing: {0:?}")]
    MissingPage(PageKey),
    #[error("scene archive header is corrupt: {0}")]
    CorruptHeader(&'static str),
    #[error("scene archive directory is corrupt: {0}")]
    CorruptDirectory(&'static str),
    #[error("scene archive page hash failed for {0:?}")]
    CorruptPage(PageKey),
    #[error("scene archive is oversized: {actual} bytes exceeds {limit}")]
    OversizedArchive { actual: u64, limit: u64 },
    #[error("scene archive page {key:?} is oversized: {actual} bytes exceeds {limit}")]
    OversizedPage {
        key: PageKey,
        actual: u64,
        limit: u64,
    },
    #[error("scene archive format version {0} is unsupported")]
    UnsupportedFormatVersion(u32),
    #[error("scene archive page kind {0} is unsupported")]
    UnsupportedPageKind(u16),
    #[error("scene archive page {kind:?} version {version} is unsupported")]
    UnsupportedPageVersion { kind: PageKind, version: u32 },
    #[error("duplicate scene archive page: {0:?}")]
    DuplicatePage(PageKey),
    #[error("invalid page scope: {0:?}")]
    InvalidPageScope(PageKey),
    #[error("scene archive page {0:?} has an invalid typed layout")]
    InvalidTypedPage(PageKey),
    #[error("scene archive page {0:?} contains an invalid record range")]
    InvalidRecordRange(PageKey),
    #[error("scene archive range arithmetic overflowed")]
    ArchiveRangeOverflow,
    #[error("scene archive generation zero is reserved")]
    ZeroGeneration,
    #[error("scene archive I/O failed")]
    Io(#[from] io::Error),
}
