use phoenix_graph_generation_v2::GraphGenerationV2Error;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EntityProducerError {
    #[error("verified NER artifact is invalid: {0}")]
    InvalidNer(&'static str),
    #[error("NER and structural generations do not share one authority binding")]
    AuthorityMismatch,
    #[error("source text does not match the generation authority")]
    SourceBindingMismatch,
    #[error("entity producer cannot overwrite an existing entity generation")]
    EntityPagesAlreadyPopulated,
    #[error("entity or decision identity is zero, duplicated, or collides")]
    IdentityCollision,
    #[error("explicit identity merge decision is invalid or ambiguous")]
    InvalidMergeDecision,
    #[error("same stable identity carries incompatible entity metadata")]
    ConflictingEntityIdentity,
    #[error("mention range is invalid or does not match its exact source surface")]
    InvalidMentionRange,
    #[error("mention does not resolve to exactly one source chunk and sentence")]
    MissingStructuralBinding,
    #[error("identity candidate is invalid or lacks exact mention evidence")]
    InvalidIdentityCandidate,
    #[error("packed V2 record count exceeds its u32 contract")]
    RecordCountOverflow,
    #[error("generation publication failed for {path}: {source}")]
    PublicationIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Generation(#[from] GraphGenerationV2Error),
    #[error(transparent)]
    Structural(#[from] phoenix_scene_compiler::StructuralSourceError),
}
