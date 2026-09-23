//! Revision-pinned Reader plans, sessions and immutable completed audio cache.
mod cache;
mod cast;
pub use cast::*;
mod mapping;
mod plain;
mod plan;
mod planner;
mod runtime;
mod session;
mod storage;
pub use cache::*;
pub use mapping::*;
pub use phoenix_tts_contract::{Digest, Error as ContractError};
pub use plain::plan_plain_chapter;
pub use plan::*;
pub use planner::{plan_markdown, PlannedNarration, PlannerConfig, PlannerReceipt};
pub use runtime::*;
pub use session::*;
pub use storage::{SnapshotStore, VerifiedBytes};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error("reader: {0}")]
    Invalid(&'static str),
    #[error("reader planner {code} at source byte {source_offset}")]
    Planning {
        code: &'static str,
        source_offset: u32,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Encoding(#[from] postcard::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
