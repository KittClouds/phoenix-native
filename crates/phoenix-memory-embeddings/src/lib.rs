//! Verified, mmap-backed embedding pages derived from a Phoenix graph generation.
//!
//! Embeddings are deliberately a sidecar rather than source authority. Every
//! sidecar is bound to one immutable generation, source set, model asset and
//! embedding configuration.

mod error;
mod format;
mod open;
mod write;

pub use error::EmbeddingPageError;
pub use format::{
    compute_artifact_hash, EmbeddingPageHeaderV1, EmbeddingRowV1, EmbeddingScalarFormat,
    EMBEDDING_PAGE_ALIGNMENT, EMBEDDING_PAGE_EXTENSION, EMBEDDING_PAGE_FLAG_COMPLETE,
    EMBEDDING_PAGE_MAGIC, EMBEDDING_PAGE_VERSION, MAX_EMBEDDING_DIMENSION,
    MAX_EMBEDDING_PAGE_BYTES, MAX_EMBEDDING_ROWS, ROW_FLAG_NORMALIZED,
};
pub use open::{EmbeddingPageExpectation, VerifiedEmbeddingPagesV1};
pub use write::{write_embedding_pages_new, EmbeddingPageWriteAuthority};

#[cfg(test)]
mod tests;
