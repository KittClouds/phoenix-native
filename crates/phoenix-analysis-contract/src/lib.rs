//! Sealed process boundary between the legacy analysis producer and Phoenix Native.

mod codec;
mod types;

pub use codec::{
    open_analysis_artifact, open_message, open_nli_artifact, write_analysis_artifact_new,
    write_message_new, write_nli_artifact_new, AnalysisContractError, VerifiedAnalysisArtifact,
    VerifiedNliArtifact, ANALYSIS_ARTIFACT_EXTENSION,
};
pub use types::*;

pub const ANALYSIS_CONTRACT: &str = "phoenix.native.document-analysis/v1";
pub const ANALYSIS_FORMAT_VERSION: u32 = 1;
