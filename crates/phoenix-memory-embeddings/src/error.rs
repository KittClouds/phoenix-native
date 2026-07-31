use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbeddingPageError {
    #[error("embedding page I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("embedding page is too small: {actual} bytes, minimum {minimum}")]
    TooSmall { actual: u64, minimum: u64 },
    #[error("unsupported embedding page magic or version")]
    Unsupported,
    #[error("embedding page was not atomically completed")]
    Incomplete,
    #[error("embedding page exceeds its byte budget: {actual} > {maximum}")]
    Oversized { actual: u64, maximum: u64 },
    #[error("embedding page range is out of bounds")]
    OutOfBounds,
    #[error("embedding page layout is invalid")]
    InvalidLayout,
    #[error("embedding rows are not canonical")]
    NonCanonicalRows,
    #[error("embedding row {row} is invalid: {reason}")]
    InvalidRow { row: usize, reason: &'static str },
    #[error("embedding page hash mismatch for {page}")]
    HashMismatch { page: &'static str },
    #[error("embedding page authority does not match {field}")]
    AuthorityMismatch { field: &'static str },
}

impl EmbeddingPageError {
    pub(crate) fn io(path: PathBuf, source: std::io::Error) -> Self {
        Self::Io { path, source }
    }
}
