use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphGenerationError {
    #[error("graph generation already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("graph generation I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("graph generation is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("graph generation binding mismatch: {0}")]
    Binding(&'static str),
    #[error("graph generation exceeds its v1 bound: {0}")]
    Oversized(&'static str),
    #[error("graph generation contains an unsupported section: {0}")]
    Unsupported(u16),
}

impl GraphGenerationError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
