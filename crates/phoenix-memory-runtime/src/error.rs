use phoenix_memory_contract::MemoryContractError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryRuntimeError {
    #[error(transparent)]
    Contract(#[from] MemoryContractError),
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("policy decision artifact is corrupt: {0}")]
    CorruptDecision(PathBuf),
    #[error("policy decision artifact exceeds {maximum} bytes: {actual}")]
    OversizedDecision { actual: u64, maximum: u64 },
    #[error("policy decision command does not bind the current candidate generation")]
    StaleCandidateBinding,
    #[error("policy decision command is invalid")]
    InvalidDecision,
    #[error("policy decision receipt chain is invalid")]
    InvalidDecisionChain,
    #[error("current-memory projection is inconsistent: {0}")]
    InvalidProjection(&'static str),
    #[error("current-memory projection artifact is corrupt: {0}")]
    CorruptProjection(PathBuf),
    #[error("current-memory projection artifact exceeds {maximum} bytes: {actual}")]
    OversizedProjection { actual: u64, maximum: u64 },
    #[error("recursive working-set request exceeds its constitutional bounds")]
    RecursiveBounds,
    #[error("recursive working-set graph is too large")]
    OversizedWorkingSet,
}

impl MemoryRuntimeError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
