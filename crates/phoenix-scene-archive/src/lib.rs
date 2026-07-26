//! Memory-mapped, page-verified native scene archive.

#[cfg(target_endian = "big")]
compile_error!("PhoenixSceneArchiveV1 currently requires a little-endian target");

mod error;
mod format;
mod reader;
mod records;
mod writer;

pub use error::ArchiveError;
pub use format::{
    ArchiveHeader, ArchiveManifold, PageDescriptor, PageKey, PageKind, FORMAT_VERSION,
    MAX_ARCHIVE_BYTES, MAX_PAGES, MAX_PAGE_BYTES,
};
pub use reader::{GuidePageView, ManifoldPageSet, PathPageView, PhoenixSceneArchiveV1};
pub use records::{
    EdgeRecord, GuidePageHeader, GuideStrokeRecord, LabelPriorityRecord, NodeIdentityRecord,
    NodeStyleRecord, PaletteEntryRecord, PathPageHeader, PathRecord, PositionRecord,
    RelationMaskRecord, TopologyRecord,
};
pub use writer::{ArchiveBuildReceipt, PhoenixSceneArchiveBuilderV1};

pub const ARCHIVE_CONTRACT: &str = "phoenix.native.scene-archive/v1";

#[cfg(test)]
mod tests;
