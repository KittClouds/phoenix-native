use phoenix_graph_generation_v2::GraphGenerationV2Error;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SemanticReviewError {
    #[error("I/O failure at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid or corrupt review artifact at {0}")]
    CorruptArtifact(PathBuf),
    #[error("review artifact is oversized: {actual} bytes, maximum {maximum}")]
    OversizedArtifact { actual: u64, maximum: u64 },
    #[error("candidate is missing from the current review catalog")]
    MissingCandidate,
    #[error("candidate binding is stale for the current generation")]
    StaleCandidate,
    #[error("candidate location is invalid")]
    InvalidCandidateLocation,
    #[error("duplicate candidate ID has conflicting bindings")]
    ConflictingCandidate,
    #[error("decision action or status is invalid")]
    InvalidDecision,
    #[error("decision receipt sequence or chain is invalid")]
    InvalidDecisionChain,
    #[error("authority history is absent")]
    MissingAuthority,
    #[error("authority has no previous generation to restore")]
    NoRollbackGeneration,
    #[error("authority generation path must be a file directly under its root")]
    InvalidGenerationPath,
    #[error("generation authority does not match the verified artifact")]
    GenerationAuthorityMismatch,
    #[error("generation page count exceeds a 32-bit index")]
    RecordCountOverflow,
    #[error(transparent)]
    Generation(#[from] GraphGenerationV2Error),
}

impl SemanticReviewError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
