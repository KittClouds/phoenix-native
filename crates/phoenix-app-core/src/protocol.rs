use crate::{
    AnalysisPublicationReceipt, AtlasCandidateId, AtlasDecisionCommand, AtlasDecisionReceiptV1,
    AtlasDecisionStatus, NativeSceneCompileReceipt, NativeScenePublishCommand, NerEntityBatch,
    NliPublication, ScenePublicationReceipt,
};
use phoenix_memory_coordinator::{IngestTurn, MemoryScope, MemorySourceLocator, RecallTurn};
use phoenix_scene_contract::{
    DocumentId, GraphAction, GraphReviewOverride, GraphViewState, HighlightPalette, Manifold,
    SceneSource, StyleState, VerifiedDocumentAnchors,
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
    PublishNliArtifact(Box<NliPublication>),
    CancelAtlasRun,
    ReviewAtlasCandidate(Box<AtlasDecisionCommand>),
    PublishReviewedDecisions,
    PublishNativeScene(Box<NativeScenePublishCommand>),
    PublishDocumentAnchors(Arc<VerifiedDocumentAnchors>),
    SetMemoryScope(MemoryScope),
    RecallMemory(Box<RecallTurn>),
    IngestMemoryTurn(Box<IngestTurn>),
    SelectMemoryContext {
        source_id: u64,
        content_id: u64,
    },
    SetManifold(Manifold),
    SetGraphView(Box<GraphViewState>),
    DispatchGraphAction(GraphAction),
    RequestGraphProvenance,
    SetStyle(StyleState),
    SetHighlightPalette(Box<HighlightPalette>),
    SetGraphSelection(GraphSelectionCommand),
    SelectAtlasCandidate(AtlasCandidateId),
    #[cfg(test)]
    TestHoldCoordinator(Box<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>),
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
    AtlasCandidate,
    Renderer,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphSelectionState {
    pub revision: u64,
    pub node_id: Option<u64>,
    pub secondary_node_id: Option<u64>,
    pub entity_id: Option<u64>,
    pub candidate_id: Option<AtlasCandidateId>,
    pub evidence: Option<EditorEvidenceSelection>,
    pub origin: GraphSelectionOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorEvidenceSelection {
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub start: u32,
    pub end: u32,
    pub evidence_hash: [u8; 32],
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphReviewOverlay {
    pub revision: u64,
    pub generation_id: Option<u64>,
    pub entries: Arc<[GraphReviewOverride]>,
}

#[derive(Clone, Debug)]
pub struct EntityTagCommand {
    pub lease: DocumentLeaseToken,
    pub content: Arc<str>,
    pub tag: EntityTag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphRebuildReceipt {
    pub run_id: u64,
    pub compile: NativeSceneCompileReceipt,
    pub publication: ScenePublicationReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtlasDecisionCommandReceipt {
    pub receipt_id: [u8; 32],
    pub candidate_id: [u8; 32],
    pub status: Option<AtlasDecisionStatus>,
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
pub struct MemoryGenerationReceipt {
    pub generation_hash: [u8; 32],
    pub source_count: u64,
    pub document_count: u64,
    pub conversation_count: u64,
    pub turn_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRecallReceipt {
    pub pending_turn_hash: [u8; 32],
    pub scope_hash: [u8; 32],
    pub generation_hash: Option<[u8; 32]>,
    pub returned_items: u16,
    pub returned_candidates: u16,
    pub returned_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryContextSelection {
    pub resident_generation_hash: [u8; 32],
    pub source_id: u64,
    pub content_id: u64,
    pub locator: MemorySourceLocator,
    pub source_start: u32,
    pub source_end: u32,
    pub content_hash: [u8; 32],
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
    AtlasCandidateReviewed(AtlasDecisionCommandReceipt),
    GraphRebuilt(GraphRebuildReceipt),
    SceneGenerationPublished(ScenePublicationReceipt),
    GraphActionQueued(GraphAction),
    GraphProvenance(Option<GraphProvenanceReceipt>),
    DocumentAnchorsPublished(usize),
    MemoryScopeChanged,
    MemoryRecalled(MemoryRecallReceipt),
    MemoryGenerationPublished(MemoryGenerationReceipt),
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
    MemoryScopeChanged {
        scope_hash: [u8; 32],
    },
    MemoryRecalled {
        receipt: MemoryRecallReceipt,
    },
    MemoryContextSelected(MemoryContextSelection),
    MemoryGenerationPublished {
        generation_hash: [u8; 32],
        source_count: u64,
    },
    AtlasRunCancellationRequested,
    AtlasCandidateReviewed {
        receipt: AtlasDecisionReceiptV1,
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
