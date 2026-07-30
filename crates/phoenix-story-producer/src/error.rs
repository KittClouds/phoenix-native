use phoenix_graph_generation_v2::GraphGenerationV2Error;
use phoenix_semantic_review::SemanticReviewError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoryProducerError {
    #[error("story producer source does not match the supplied document authority")]
    AuthorityMismatch,
    #[error("source text does not match the generation content hash")]
    SourceBindingMismatch,
    #[error("story producer cannot overwrite populated story-candidate pages")]
    StoryPagesAlreadyPopulated,
    #[error("deterministic candidate identity is zero, duplicated, or does not match its rule")]
    InvalidCandidateIdentity,
    #[error("candidate references an unknown entity, event, episode, chunk, or evidence row")]
    UnknownReference,
    #[error("candidate evidence is missing, duplicated, or does not match its endpoint")]
    InvalidEvidenceBinding,
    #[error("event or episode label is empty, synthetic, or not an exact source span")]
    InvalidSourceLabel,
    #[error("episode membership is empty, duplicated, or not evidence-bound")]
    InvalidEpisodeMembership,
    #[error("model ranking refers to an absent candidate or contains an invalid score")]
    InvalidModelRanking,
    #[error("producer registration is empty or duplicated")]
    InvalidProducerRegistration,
    #[error("the frozen Story V1 semantic lens contract is invalid: {0}")]
    InvalidLensContract(String),
    #[error("packed V2 record count or string offset exceeds its contract")]
    RecordCountOverflow,
    #[error("story generation publication failed for {path}: {source}")]
    PublicationIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Generation(#[from] GraphGenerationV2Error),
    #[error(transparent)]
    Review(#[from] SemanticReviewError),
}
