use crate::ScenePublicationKind;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScenePublicationError {
    #[error("scene publication workspace path has no parent: {0}")]
    WorkspacePathWithoutParent(PathBuf),
    #[error("scene publication generation zero is reserved")]
    ZeroGeneration,
    #[error("scene publication generation {incoming} is not newer than {current}")]
    StaleGeneration { current: u64, incoming: u64 },
    #[error("registry-only generation {incoming} cannot replace full generation {current}")]
    FullGenerationProtected { current: u64, incoming: u64 },
    #[error("scene publication manifest is corrupt: {0}")]
    CorruptManifest(&'static str),
    #[error("scene publication manifest format version {0} is unsupported")]
    UnsupportedManifestVersion(u32),
    #[error("scene publication manifest hash failed")]
    CorruptManifestHash,
    #[error("scene publication artifact is missing: {0}")]
    MissingArtifact(PathBuf),
    #[error("scene publication artifact already exists: {0}")]
    ArtifactAlreadyExists(PathBuf),
    #[error("scene publication inventory mismatch: {0}")]
    InventoryMismatch(&'static str),
    #[error("scene publication identity mismatch for {resource} slot {slot}")]
    IdentityMismatch { resource: &'static str, slot: usize },
    #[error("registry-only publication contains graph topology")]
    RegistryContainsTopology,
    #[error("registry-only publication mapping is not one-to-one")]
    RegistryMappingMismatch,
    #[error("scene publication kind {0:?} is unsupported here")]
    UnsupportedKind(ScenePublicationKind),
    #[error("scene publication range arithmetic overflowed")]
    RangeOverflow,
    #[error("scene publication I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("scene publication atomic replacement failed at {path}: {source}")]
    AtomicReplace {
        path: PathBuf,
        #[source]
        source: windows::core::Error,
    },
    #[error(transparent)]
    Archive(#[from] phoenix_scene_archive::ArchiveError),
    #[error(transparent)]
    ProductIndex(#[from] phoenix_scene_product_index::ProductIndexError),
    #[error(transparent)]
    Scene(#[from] phoenix_scene_contract::SceneContractError),
    #[error(transparent)]
    Topology(#[from] phoenix_scene_contract::TopologyInventoryError),
}
