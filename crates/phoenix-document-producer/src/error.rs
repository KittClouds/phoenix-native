use phoenix_graph_generation_v2::GraphGenerationV2Error;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocumentProducerError {
    #[error("verified structural input is invalid: {0}")]
    InvalidStructuralInput(&'static str),
    #[error("source text does not match the verified structural binding")]
    SourceBindingMismatch,
    #[error("structural record count exceeds V2 bounds")]
    RecordCountOverflow,
    #[error("structural coordinate does not resolve to its declared parent")]
    InvalidStructuralParent,
    #[error("stable structural identity collision")]
    IdentityCollision,
    #[error("existing generation at {path} does not match the requested authority")]
    ExistingAuthorityMismatch { path: PathBuf },
    #[error("generation publication failed for {path}: {source}")]
    PublicationIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Generation(#[from] GraphGenerationV2Error),
}
