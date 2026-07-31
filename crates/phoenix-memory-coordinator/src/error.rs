use phoenix_memory_contract::MemoryContractError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("ingestion queue is full")]
    QueueFull,
    #[error("ingestion coordinator is shut down")]
    Shutdown,
    #[error("operation was cancelled")]
    Cancelled,
    #[error("document lease revision or hash does not match the command")]
    LeaseMismatch,
    #[error("producer output does not bind to the exact source authority: {0}")]
    ProducerAuthority(&'static str),
    #[error("conversation turn conflicts with an already committed turn")]
    ConflictingTurn,
    #[error("conversation turn ordinal is not the next committed ordinal")]
    NonContiguousTurn,
    #[error("pending recall turn is already committed")]
    PendingTurnCommitted,
    #[error("LongMemEval gold answers and gold sessions cannot enter authority")]
    GoldDataRejected,
    #[error("producer capability registration is incomplete or invalid")]
    InvalidCapabilityMatrix,
    #[error("semantic candidate is not evidence-bound or is not proposed")]
    InvalidCandidate,
    #[error("canonical identity merge lacks an explicit coordinator decision")]
    InvalidIdentityMerge,
    #[error("source text, count, or context packet exceeds its bound")]
    Oversized,
    #[error("bounded lexical recall failed closed: {0}")]
    LexicalRecall(String),
    #[error("artifact directory I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Contract(#[from] MemoryContractError),
}

impl CoordinatorError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
