//! Immutable, archive-bound product metadata for native graph scenes.

#[cfg(target_endian = "big")]
compile_error!("PhoenixSceneProductIndexV1 currently requires a little-endian target");

mod error;
mod format;
mod reader;
mod records;
mod writer;

pub use error::ProductIndexError;
pub use format::{
    ProductIndexBinding, ProductIndexHeader, FORMAT_VERSION, MAX_INDEX_BYTES, MAX_LABEL_BYTES,
    MAX_REFERENCE_RECORDS,
};
pub use reader::PhoenixSceneProductIndexV1;
pub use records::{
    EdgeProductRecord, EntityId, EntityNodeMappingRecord, NodeId, NodeProductRecord,
    ProductReferenceRecord, ReviewState,
};
pub use writer::{PhoenixSceneProductIndexBuilderV1, ProductIndexBuildReceipt};

pub const PRODUCT_INDEX_CONTRACT: &str = "phoenix.native.scene-product-index/v1";

#[cfg(test)]
mod tests;
