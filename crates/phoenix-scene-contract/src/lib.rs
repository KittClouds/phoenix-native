//! Versioned scene authority shared by the native kernel and renderer consumer.

mod caps;
mod entities;
mod graph_palette;
mod highlights;
mod hopf;
mod topology;
mod view;
mod visual;
mod visual_v3;

use phoenix_scene_archive::{
    ArchiveError, ArchiveManifold, GuidePageView, ManifoldPageSet, PageKey, PageKind, PathPageView,
    PhoenixSceneArchiveV1, ARCHIVE_CONTRACT, FORMAT_VERSION,
};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

pub use caps::{
    CapsRole, CAPS_KLEIN_BOUND, CAPS_LAYOUT_CONTRACT, CAPS_WORLD_SCALE, CAUSAL_MIDPOINT_NODE_KIND,
    CHAPTER_NODE_KIND, CHUNK_NODE_KIND, CONTEXTUAL_MIDPOINT_NODE_KIND, DOCUMENT_NODE_KIND,
    EPISODE_NODE_KIND, EVENT_NODE_KIND, EVIDENCE_NODE_KIND, GUIDE_FLAG_CAP_BOUNDARY,
    GUIDE_FLAG_CONCENTRATION_AXIS, GUIDE_FLAG_SHELL, IDENTITY_MIDPOINT_NODE_KIND,
    MEMORY_STATE_NODE_KIND, PARAGRAPH_NODE_KIND, RELATIONSHIP_FACT_NODE_KIND, SENTENCE_NODE_KIND,
    TEMPORAL_MIDPOINT_NODE_KIND,
};
pub use entities::EntityKind;
pub use graph_palette::{GraphColorKey, GraphPalette, GRAPH_PALETTE_COLOR_COUNT};
pub use highlights::{
    AnchorCandidate, AnchorSource, DocumentAnchor, EntityFamily, FamilyPalette,
    HighlightContractError, HighlightMode, HighlightPalette, VerifiedDocumentAnchors,
    HIGHLIGHT_CONTRACT, MAX_DOCUMENT_ANCHORS,
};
pub use hopf::{
    GUIDE_FLAG_HOPF_BASE_LINK, GUIDE_FLAG_HOPF_BASE_SPHERE, GUIDE_FLAG_HOPF_FIBER,
    HOPF_VISUAL_CONTRACT,
};
pub use topology::{
    validate_topology_endpoints, TopologyInventoryCensus, TopologyInventoryError,
    TOPOLOGY_INVENTORY_CONTRACT,
};
pub use view::{
    FamilyMask, GraphAction, GraphCanvas, GraphLens, GraphReviewOverride, GraphScope, GraphSurface,
    GraphViewState, RelationFamily, RelationMask, ReviewMask, SceneAuthority, ScopeMask,
};
pub use visual::{visual_role, with_visual_role, VisualRole, VISUAL_ROLE_MASK, VISUAL_ROLE_SHIFT};
pub use visual_v3::{
    describe_edge, describe_node, primary_edge_family_mask, primary_node_family_mask,
    VisualEdgeDescriptor, VisualEdgeKind, VisualNodeDescriptor, VisualNodeKind, VisualNodeLane,
    VISUAL_GRAPH_CONTRACT_V3,
};

pub const SCENE_CONTRACT: &str = "phoenix.native.resident-scene/v1";
pub const TOPOLOGY_PROJECTION_CONTRACT: &str = "phoenix.native.topology-projection/v1";
pub const NATIVE_SCENE_COMPILER_CONTRACT: &str = "phoenix.native.active-document-scene-compiler/v2";
pub const NATIVE_SCENE_COMPILER_V2_CONTRACT: &str =
    "phoenix.native.graph-generation-scene-compiler/v2";
pub const NATIVE_SCENE_COMPILER_V3_CONTRACT: &str =
    "phoenix.native.graph-generation-scene-compiler/v3";
pub const HOT_MANIFOLD_PAGE_BUDGET_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GraphGeneration(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DocumentId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Manifold {
    Hybrid,
    Torus,
    Hopf,
    Caps,
    Transit,
    Siegel,
}

impl Manifold {
    pub const ALL: [Self; 6] = [
        Self::Hybrid,
        Self::Torus,
        Self::Hopf,
        Self::Caps,
        Self::Transit,
        Self::Siegel,
    ];
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct StyleState {
    pub revision: u64,
    pub palette_revision: u64,
    pub node_scale: f32,
    pub highlight_mode: HighlightMode,
}

impl Default for StyleState {
    fn default() -> Self {
        Self {
            revision: 1,
            palette_revision: 1,
            node_scale: 1.0,
            highlight_mode: HighlightMode::Subtle,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeCapabilities {
    pub contract: String,
    pub scene_contract: String,
    pub scene_archive_contract: String,
    pub scene_product_index_contract: String,
    pub scene_compiler_contract: String,
    pub coordinated_native_windows: bool,
    pub bounded_commands: bool,
    pub bounded_events: bool,
    pub durable_workspace: bool,
    pub native_scene_rebuild: bool,
    pub supported_manifolds: Vec<Manifold>,
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            contract: "phoenix.native.runtime-capabilities/v2".into(),
            scene_contract: SCENE_CONTRACT.into(),
            scene_archive_contract: ARCHIVE_CONTRACT.into(),
            scene_product_index_contract: phoenix_scene_product_index::PRODUCT_INDEX_CONTRACT
                .into(),
            scene_compiler_contract: NATIVE_SCENE_COMPILER_V3_CONTRACT.into(),
            coordinated_native_windows: true,
            bounded_commands: true,
            bounded_events: true,
            durable_workspace: true,
            native_scene_rebuild: true,
            supported_manifolds: Manifold::ALL.to_vec(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneSource {
    Archive,
    Backend,
    RegistryOnly,
    VerificationFixture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveIdentity {
    pub format_version: u32,
    pub generation_id: u64,
    pub page_count: u32,
    pub cohort_hash: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct ResidentScene {
    generation: GraphGeneration,
    document: Option<DocumentId>,
    source: SceneSource,
    inventory: SceneInventory,
    archive_identity: ArchiveIdentity,
    archive: Arc<PhoenixSceneArchiveV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneInventory {
    pub node_count: usize,
    pub edge_count: usize,
}

/// Verifies that optional manifold path geometry is a total, order-preserving
/// projection of the shared topology. A manifold may change positions and path
/// shape, but it may neither introduce nor omit a stable edge slot.
pub fn validate_topology_projection(
    paths: PathPageView<'_>,
    edge_count: usize,
) -> Result<(), TopologyProjectionError> {
    if paths.paths.len() != edge_count {
        return Err(TopologyProjectionError::InventoryMismatch {
            expected_edges: edge_count,
            actual_paths: paths.paths.len(),
        });
    }
    for (expected_slot, path) in paths.paths.iter().enumerate() {
        let actual_slot = path.edge_slot as usize;
        if actual_slot != expected_slot {
            return Err(TopologyProjectionError::StableSlotMismatch {
                ordinal: expected_slot,
                expected_slot,
                actual_slot,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TopologyProjectionError {
    #[error(
        "topology projection inventory mismatch: expected {expected_edges} edges, got {actual_paths} paths"
    )]
    InventoryMismatch {
        expected_edges: usize,
        actual_paths: usize,
    },
    #[error(
        "topology projection path {ordinal} references stable edge slot {actual_slot}, expected {expected_slot}"
    )]
    StableSlotMismatch {
        ordinal: usize,
        expected_slot: usize,
        actual_slot: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HotPageInventory {
    pub page_count: u8,
    pub byte_len: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ActiveManifoldPages<'a> {
    pub manifold: Manifold,
    pub pages: ManifoldPageSet<'a>,
    pub guides: Option<GuidePageView<'a>>,
    pub prepared_paths: Option<PathPageView<'a>>,
    pub hot_pages: HotPageInventory,
}

impl ResidentScene {
    pub fn from_archive(
        archive: Arc<PhoenixSceneArchiveV1>,
        document: Option<DocumentId>,
    ) -> Result<Self, SceneContractError> {
        Self::from_archive_with_source(archive, document, SceneSource::Archive)
    }

    pub fn from_archive_with_source(
        archive: Arc<PhoenixSceneArchiveV1>,
        document: Option<DocumentId>,
        source: SceneSource,
    ) -> Result<Self, SceneContractError> {
        let header = archive.header();
        if header.generation_id == 0 {
            return Err(SceneContractError::ZeroGeneration);
        }
        let first = archive.open_manifold(ArchiveManifold::Hybrid)?;
        let inventory = SceneInventory {
            node_count: first.identities.len(),
            edge_count: first.edges.len(),
        };
        let legacy_five_manifold = is_legacy_five_manifold_archive(&archive);
        for manifold in ArchiveManifold::ALL {
            let pages = match archive.open_manifold(manifold) {
                Ok(pages) => pages,
                Err(ArchiveError::MissingPage(key))
                    if manifold == ArchiveManifold::Hopf
                        && key == PageKey::manifold(PageKind::Positions, manifold)
                        && legacy_five_manifold =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if pages.identities.len() != inventory.node_count
                || pages.edges.len() != inventory.edge_count
            {
                return Err(SceneContractError::ManifoldInventoryMismatch {
                    manifold: manifold.into(),
                    expected_nodes: inventory.node_count,
                    actual_nodes: pages.identities.len(),
                    expected_edges: inventory.edge_count,
                    actual_edges: pages.edges.len(),
                });
            }
        }
        Ok(Self {
            generation: GraphGeneration(header.generation_id),
            document,
            source,
            inventory,
            archive_identity: ArchiveIdentity {
                format_version: FORMAT_VERSION,
                generation_id: header.generation_id,
                page_count: header.page_count,
                cohort_hash: header.cohort_hash,
            },
            archive,
        })
    }

    pub fn generation(&self) -> GraphGeneration {
        self.generation
    }

    pub fn document(&self) -> Option<DocumentId> {
        self.document
    }

    pub fn source(&self) -> SceneSource {
        self.source
    }

    pub fn inventory(&self) -> SceneInventory {
        self.inventory
    }

    pub fn archive_identity(&self) -> ArchiveIdentity {
        self.archive_identity
    }

    pub fn archive(&self) -> &Arc<PhoenixSceneArchiveV1> {
        &self.archive
    }

    pub fn graph_view_state(
        &self,
        index: Option<&PhoenixSceneProductIndexV1>,
    ) -> Result<GraphViewState, SceneContractError> {
        if let Some(index) = index {
            index.bind_to_archive(&self.archive)?;
            let _ = self.topology_census(index)?;
        }
        Ok(GraphViewState::for_archive(
            self.generation,
            self.archive_identity.cohort_hash,
            index.map(|index| index.header().index_hash),
        ))
    }

    /// Proves that the product index, shared topology, and all manifold
    /// projections describe the same complete graph inventory.
    pub fn topology_census(
        &self,
        index: &PhoenixSceneProductIndexV1,
    ) -> Result<TopologyInventoryCensus, SceneContractError> {
        index.bind_to_archive(&self.archive)?;
        topology::validate_topology_inventory(&self.archive, index)
    }

    pub fn activate_manifold(
        &self,
        manifold: Manifold,
    ) -> Result<ActiveManifoldPages<'_>, SceneContractError> {
        let archive_manifold = manifold.into();
        let pages = self.archive.open_manifold(archive_manifold)?;
        let mut hot_pages = HotPageInventory::default();
        for kind in [
            PageKind::Guides,
            PageKind::StraightPaths,
            PageKind::CurvedPaths,
            PageKind::BundledPaths,
        ] {
            let key = PageKey::manifold(kind, archive_manifold);
            if !self.archive.has_page(key) {
                continue;
            }
            let descriptor = self
                .archive
                .descriptors()
                .iter()
                .find(|descriptor| descriptor.key == key)
                .ok_or(SceneContractError::Archive(ArchiveError::MissingPage(key)))?;
            hot_pages.byte_len =
                checked_hot_page_total(hot_pages.byte_len, descriptor.stored_len, manifold)?;
            hot_pages.page_count = hot_pages.page_count.saturating_add(1);
            if kind == PageKind::Guides {
                let _ = self.archive.guides(archive_manifold)?;
            } else {
                let _ = self.archive.paths(kind, archive_manifold)?;
            }
        }
        let guides = self
            .archive
            .has_page(PageKey::manifold(PageKind::Guides, archive_manifold))
            .then(|| self.archive.guides(archive_manifold))
            .transpose()?;
        let path_kind = preferred_path_kind(manifold);
        let prepared_paths = self
            .archive
            .has_page(PageKey::manifold(path_kind, archive_manifold))
            .then(|| self.archive.paths(path_kind, archive_manifold))
            .transpose()?;
        if let Some(paths) = prepared_paths {
            validate_topology_projection(paths, self.inventory.edge_count)?;
        }
        Ok(ActiveManifoldPages {
            manifold,
            pages,
            guides,
            prepared_paths,
            hot_pages,
        })
    }
}

fn is_legacy_five_manifold_archive(archive: &PhoenixSceneArchiveV1) -> bool {
    const LEGACY: [ArchiveManifold; 5] = [
        ArchiveManifold::Hybrid,
        ArchiveManifold::Torus,
        ArchiveManifold::Caps,
        ArchiveManifold::Transit,
        ArchiveManifold::Siegel,
    ];
    let descriptors = archive.descriptors();
    !descriptors
        .iter()
        .any(|descriptor| descriptor.key.manifold == Some(ArchiveManifold::Hopf))
        && LEGACY.into_iter().all(|manifold| {
            descriptors.iter().any(|descriptor| {
                descriptor.key == PageKey::manifold(PageKind::Positions, manifold)
            })
        })
}

const fn preferred_path_kind(manifold: Manifold) -> PageKind {
    match manifold {
        Manifold::Hybrid | Manifold::Siegel => PageKind::BundledPaths,
        Manifold::Torus | Manifold::Hopf | Manifold::Transit => PageKind::CurvedPaths,
        Manifold::Caps => PageKind::StraightPaths,
    }
}

fn checked_hot_page_total(
    current: u64,
    additional: u64,
    manifold: Manifold,
) -> Result<u64, SceneContractError> {
    let actual = current.saturating_add(additional);
    if actual > HOT_MANIFOLD_PAGE_BUDGET_BYTES {
        return Err(SceneContractError::HotPageBudgetExceeded {
            manifold,
            actual,
            limit: HOT_MANIFOLD_PAGE_BUDGET_BYTES,
        });
    }
    Ok(actual)
}

#[derive(Debug, Error)]
pub enum SceneContractError {
    #[error("resident graph generation zero is reserved")]
    ZeroGeneration,
    #[error(
        "{manifold:?} inventory mismatch: expected {expected_nodes} nodes/{expected_edges} edges, \
         got {actual_nodes} nodes/{actual_edges} edges"
    )]
    ManifoldInventoryMismatch {
        manifold: Manifold,
        expected_nodes: usize,
        actual_nodes: usize,
        expected_edges: usize,
        actual_edges: usize,
    },
    #[error(
        "{manifold:?} guide/path pages use {actual} bytes, exceeding the {limit}-byte hot budget"
    )]
    HotPageBudgetExceeded {
        manifold: Manifold,
        actual: u64,
        limit: u64,
    },
    #[error("{manifold:?} is missing its complete prepared topology projection")]
    MissingTopologyProjection { manifold: Manifold },
    #[error(transparent)]
    TopologyInventory(#[from] TopologyInventoryError),
    #[error(transparent)]
    TopologyProjection(#[from] TopologyProjectionError),
    #[error(transparent)]
    Archive(#[from] phoenix_scene_archive::ArchiveError),
    #[error(transparent)]
    ProductIndex(#[from] phoenix_scene_product_index::ProductIndexError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ResidentSceneLoadError {
    #[error("[PHX_SCENE_MISSING] {0}")]
    Missing(String),
    #[error("[PHX_SCENE_CORRUPT] {0}")]
    Corrupt(String),
    #[error("[PHX_SCENE_OVERSIZED] {0}")]
    Oversized(String),
    #[error("[PHX_SCENE_UNSUPPORTED] {0}")]
    Unsupported(String),
}

impl ResidentSceneLoadError {
    pub fn missing(detail: impl Into<String>) -> Self {
        Self::Missing(detail.into())
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::Missing(_) => "PHX_SCENE_MISSING",
            Self::Corrupt(_) => "PHX_SCENE_CORRUPT",
            Self::Oversized(_) => "PHX_SCENE_OVERSIZED",
            Self::Unsupported(_) => "PHX_SCENE_UNSUPPORTED",
        }
    }

    pub const fn title(&self) -> &'static str {
        match self {
            Self::Missing(_) => "Resident scene unavailable",
            Self::Corrupt(_) => "Resident scene is corrupt",
            Self::Oversized(_) => "Resident scene exceeds limits",
            Self::Unsupported(_) => "Resident scene is unsupported",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Missing(detail)
            | Self::Corrupt(detail)
            | Self::Oversized(detail)
            | Self::Unsupported(detail) => detail,
        }
    }
}

impl From<ArchiveError> for ResidentSceneLoadError {
    fn from(error: ArchiveError) -> Self {
        let detail = error.to_string();
        match error {
            ArchiveError::MissingArchive(_) | ArchiveError::MissingPage(_) => Self::Missing(detail),
            ArchiveError::OversizedArchive { .. } | ArchiveError::OversizedPage { .. } => {
                Self::Oversized(detail)
            }
            ArchiveError::UnsupportedFormatVersion(_)
            | ArchiveError::UnsupportedPageKind(_)
            | ArchiveError::UnsupportedPageVersion { .. } => Self::Unsupported(detail),
            _ => Self::Corrupt(detail),
        }
    }
}

impl From<SceneContractError> for ResidentSceneLoadError {
    fn from(error: SceneContractError) -> Self {
        match error {
            SceneContractError::Archive(error) => error.into(),
            SceneContractError::ProductIndex(error) => Self::Corrupt(error.to_string()),
            SceneContractError::HotPageBudgetExceeded { .. } => Self::Oversized(error.to_string()),
            other => Self::Corrupt(other.to_string()),
        }
    }
}

impl From<Manifold> for ArchiveManifold {
    fn from(value: Manifold) -> Self {
        match value {
            Manifold::Hybrid => Self::Hybrid,
            Manifold::Torus => Self::Torus,
            Manifold::Hopf => Self::Hopf,
            Manifold::Caps => Self::Caps,
            Manifold::Transit => Self::Transit,
            Manifold::Siegel => Self::Siegel,
        }
    }
}

impl From<ArchiveManifold> for Manifold {
    fn from(value: ArchiveManifold) -> Self {
        match value {
            ArchiveManifold::Hybrid => Self::Hybrid,
            ArchiveManifold::Torus => Self::Torus,
            ArchiveManifold::Hopf => Self::Hopf,
            ArchiveManifold::Caps => Self::Caps,
            ArchiveManifold::Transit => Self::Transit,
            ArchiveManifold::Siegel => Self::Siegel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_archive::{
        EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PathRecord, PhoenixSceneArchiveBuilderV1,
        PositionRecord, TopologyRecord,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn archive_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "phoenix-scene-contract-{label}-{}-{}.phxscene",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn builder_with_shared(generation: u64) -> Result<PhoenixSceneArchiveBuilderV1, ArchiveError> {
        let mut builder = PhoenixSceneArchiveBuilderV1::new(generation)?;
        builder
            .add_records(
                PageKey::shared(PageKind::NodeIdentity),
                &[] as &[NodeIdentityRecord],
            )?
            .add_records(
                PageKey::shared(PageKind::NodeStyle),
                &[] as &[NodeStyleRecord],
            )?
            .add_records(
                PageKey::shared(PageKind::Topology),
                &[] as &[TopologyRecord],
            )?
            .add_records(PageKey::shared(PageKind::Edge), &[] as &[EdgeRecord])?;
        Ok(builder)
    }

    #[test]
    fn resident_scene_opens_shared_pages_once_and_all_six_positions() {
        let path = archive_path("six");
        let mut builder = builder_with_shared(41).unwrap_or_else(|error| panic!("{error}"));
        for manifold in ArchiveManifold::ALL {
            builder
                .add_records(
                    PageKey::manifold(PageKind::Positions, manifold),
                    &[] as &[PositionRecord],
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }
        builder
            .write_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        let archive = PhoenixSceneArchiveV1::open(&path).unwrap_or_else(|error| panic!("{error}"));
        let scene = ResidentScene::from_archive(Arc::new(archive), None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(scene.archive().verified_page_count(), 10);
        for manifold in Manifold::ALL {
            let active = scene
                .activate_manifold(manifold)
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(active.hot_pages, HotPageInventory::default());
        }
        drop(scene);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_five_manifold_archive_opens_without_inventing_hopf() {
        let path = archive_path("legacy-five");
        let mut builder = builder_with_shared(40).unwrap_or_else(|error| panic!("{error}"));
        for manifold in [
            ArchiveManifold::Hybrid,
            ArchiveManifold::Torus,
            ArchiveManifold::Caps,
            ArchiveManifold::Transit,
            ArchiveManifold::Siegel,
        ] {
            builder
                .add_records(
                    PageKey::manifold(PageKind::Positions, manifold),
                    &[] as &[PositionRecord],
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }
        builder
            .write_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        let archive = PhoenixSceneArchiveV1::open(&path).unwrap_or_else(|error| panic!("{error}"));
        let scene = ResidentScene::from_archive(Arc::new(archive), None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(scene.activate_manifold(Manifold::Torus).is_ok());
        assert!(matches!(
            scene.activate_manifold(Manifold::Hopf),
            Err(SceneContractError::Archive(ArchiveError::MissingPage(_)))
        ));
        drop(scene);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_manifold_position_fails_closed() {
        let path = archive_path("missing");
        let mut builder = builder_with_shared(42).unwrap_or_else(|error| panic!("{error}"));
        builder
            .add_records(
                PageKey::manifold(PageKind::Positions, ArchiveManifold::Hybrid),
                &[] as &[PositionRecord],
            )
            .unwrap_or_else(|error| panic!("{error}"));
        builder
            .write_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        let archive = PhoenixSceneArchiveV1::open(&path).unwrap_or_else(|error| panic!("{error}"));
        let result = ResidentScene::from_archive(Arc::new(archive), None);
        assert!(matches!(
            result,
            Err(SceneContractError::Archive(ArchiveError::MissingPage(_)))
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_hot_pages_fail_closed_before_open() {
        let result = checked_hot_page_total(HOT_MANIFOLD_PAGE_BUDGET_BYTES, 1, Manifold::Transit);
        assert!(matches!(
            result,
            Err(SceneContractError::HotPageBudgetExceeded {
                manifold: Manifold::Transit,
                ..
            })
        ));
    }

    #[test]
    fn topology_projection_requires_every_stable_edge_slot_exactly_once() {
        let complete = [path(0), path(1), path(2)];
        let view = PathPageView {
            style: 0,
            paths: &complete,
            points: &[],
        };
        assert_eq!(validate_topology_projection(view, 3), Ok(()));

        let missing = PathPageView {
            style: 0,
            paths: &complete[..2],
            points: &[],
        };
        assert_eq!(
            validate_topology_projection(missing, 3),
            Err(TopologyProjectionError::InventoryMismatch {
                expected_edges: 3,
                actual_paths: 2,
            })
        );

        let duplicate = [path(0), path(0), path(2)];
        let duplicate = PathPageView {
            style: 0,
            paths: &duplicate,
            points: &[],
        };
        assert_eq!(
            validate_topology_projection(duplicate, 3),
            Err(TopologyProjectionError::StableSlotMismatch {
                ordinal: 1,
                expected_slot: 1,
                actual_slot: 0,
            })
        );
    }

    const fn path(edge_slot: u32) -> PathRecord {
        PathRecord {
            edge_slot,
            first_point: 0,
            point_count: 0,
            flags: 0,
            rgba8: 0,
        }
    }

    #[test]
    fn scene_load_failures_have_named_fail_closed_codes() {
        let error = ResidentSceneLoadError::from(ArchiveError::UnsupportedFormatVersion(99));
        assert_eq!(error.code(), "PHX_SCENE_UNSUPPORTED");
        assert!(error.to_string().contains("[PHX_SCENE_UNSUPPORTED]"));

        let missing = ResidentSceneLoadError::missing("pass --scene-archive");
        assert_eq!(missing.code(), "PHX_SCENE_MISSING");
    }
}
