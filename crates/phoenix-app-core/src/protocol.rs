use crate::{
    AnalysisPublicationReceipt, NativeSceneCompileReceipt, NativeScenePublishCommand,
    NerEntityBatch, NliPublication, ScenePublicationReceipt,
};
use phoenix_scene_contract::{
    DocumentId, GraphAction, GraphViewState, HighlightPalette, Manifold, SceneSource, StyleState,
    VerifiedDocumentAnchors,
};
use phoenix_workspace::{
    ContentHash, DocumentLeaseToken, DocumentRevision, EntityTag, EntityTagResult, EntryId,
    EntryKind, NerPublicationResult,
};
use std::sync::Arc;

#[derive(Debug)]
pub enum KernelCommand {
    SelectEntry(EntryId),
    CreateEntry {
        kind: EntryKind,
        name: String,
    },
    RenameEntry {
        id: EntryId,
        name: String,
    },
    DeleteEntry(EntryId),
    SaveDocument {
        lease: DocumentLeaseToken,
        content: Arc<str>,
    },
    TagSelection(Box<EntityTagCommand>),
    PublishNerEntities(NerEntityBatch),
    PublishNliArtifact(NliPublication),
    PublishNativeScene(Box<NativeScenePublishCommand>),
    PublishDocumentAnchors(Arc<VerifiedDocumentAnchors>),
    SetManifold(Manifold),
    SetGraphView(Box<GraphViewState>),
    DispatchGraphAction(GraphAction),
    RequestGraphProvenance,
    SetStyle(StyleState),
    SetHighlightPalette(Box<HighlightPalette>),
    SetGraphSelection(GraphSelectionCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphSelectionCommand {
    AtlasEntity(u64),
    GraphNode(u64),
    Clear,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GraphSelectionOrigin {
    Atlas,
    Renderer,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphSelectionState {
    pub revision: u64,
    pub node_id: Option<u64>,
    pub entity_id: Option<u64>,
    pub origin: GraphSelectionOrigin,
}

#[derive(Clone, Debug)]
pub struct EntityTagCommand {
    pub lease: DocumentLeaseToken,
    pub content: Arc<str>,
    pub tag: EntityTag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphRebuildReceipt {
    pub compile: NativeSceneCompileReceipt,
    pub publication: ScenePublicationReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphProvenanceReceipt {
    pub source: SceneSource,
    pub generation_id: u64,
    pub registry_revision: u64,
    pub node_count: u64,
    pub edge_count: u64,
    pub cohort_hash: [u8; 32],
    pub product_index_hash: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelOutcome {
    StateChanged,
    EntryCreated(EntryId),
    EntriesDeleted(usize),
    DocumentSaved(DocumentRevision),
    EntityTagged(EntityTagResult),
    NerEntitiesPublished(NerPublicationResult),
    NliCandidatesPublished(AnalysisPublicationReceipt),
    GraphRebuilt(GraphRebuildReceipt),
    SceneGenerationPublished(ScenePublicationReceipt),
    GraphActionQueued(GraphAction),
    GraphProvenance(Option<GraphProvenanceReceipt>),
    DocumentAnchorsPublished(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandReceipt {
    pub sequence: u64,
    pub kernel_revision: u64,
    pub outcome: KernelOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelEvent {
    pub sequence: u64,
    pub kernel_revision: u64,
    pub kind: KernelEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelEventKind {
    ActiveDocumentChanged(Option<DocumentId>),
    WorkspaceCommitted {
        workspace_revision: u64,
    },
    DocumentCommitted {
        document: DocumentId,
        revision: DocumentRevision,
        content_hash: ContentHash,
        scene_publication: Option<ScenePublicationReceipt>,
    },
    SceneGenerationPublished {
        receipt: ScenePublicationReceipt,
    },
    GraphRebuilt {
        receipt: GraphRebuildReceipt,
    },
    DocumentAnchorsChanged {
        document: DocumentId,
        count: usize,
    },
    EntityRegistryCommitted {
        document: DocumentId,
        entity_id: u64,
        registry_revision: u64,
        scene_publication: Option<ScenePublicationReceipt>,
    },
    AtlasRegistryCommitted {
        ner_revision: u64,
        registry_revision: u64,
        canonical_entities: usize,
        scene_publication: Option<ScenePublicationReceipt>,
    },
    NliCandidatesCommitted {
        receipt: AnalysisPublicationReceipt,
    },
    ManifoldChanged(Manifold),
    GraphViewChanged(GraphViewState),
    GraphActionRequested(GraphAction),
    GraphProvenanceRequested {
        provenance: Option<GraphProvenanceReceipt>,
    },
    StyleChanged {
        revision: u64,
    },
    HighlightPaletteChanged,
    GraphSelectionChanged(GraphSelectionState),
    ShuttingDown,
}
