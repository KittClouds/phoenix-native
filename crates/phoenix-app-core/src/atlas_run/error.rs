use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AtlasRunReceiptError {
    #[error("Atlas run receipt I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Atlas run receipt header is invalid")]
    InvalidHeader,
    #[error("Atlas run receipt version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("Atlas run receipt payload is oversized: {0} bytes")]
    Oversized(usize),
    #[error("Atlas run receipt payload hash mismatch")]
    HashMismatch,
    #[error("Atlas run receipt codec failed: {0}")]
    Codec(String),
    #[error("Atlas run receipt contract failed: {0}")]
    Invalid(&'static str),
    #[error("Atlas run receipt does not match the current source or publication authority")]
    AuthorityMismatch,
}
