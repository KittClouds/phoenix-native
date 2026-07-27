//! Single-process authority and bounded coordinator for Phoenix Native.

mod atlas;
mod entity_tags;
mod graph_selection;
mod graph_view;
mod metrics;
mod protocol;
mod scene_publication;
mod scene_rebuild;
mod state;

pub use atlas::{AtlasEntity, AtlasRegistry, NerEntityBatch};
pub use metrics::KernelMetrics;
pub use phoenix_scene_compiler::{
    NativeSceneCompileReceipt, NativeSceneCompilerError, NATIVE_SCENE_COMPILER_CONTRACT,
};
pub use phoenix_scene_publisher::{
    NativeScenePublication, SceneEdgeProduct, SceneNodeProduct, ScenePublicationKind,
    ScenePublicationReceipt, SCENE_PUBLISHER_CONTRACT,
};
pub use phoenix_workspace::{EntitySourceMask, NerEntityRecord};
pub use protocol::*;
pub use scene_rebuild::NativeScenePublishCommand;
use state::*;

use phoenix_scene_contract::{
    DocumentId, GraphAction, GraphGeneration, GraphViewState, HighlightContractError,
    HighlightPalette, Manifold, ResidentScene, RuntimeCapabilities, SceneContractError, StyleState,
    VerifiedDocumentAnchors,
};
use phoenix_scene_product_index::{PhoenixSceneProductIndexV1, ProductIndexError};
use phoenix_scene_publisher::{ScenePublicationError, ScenePublicationStore};
use phoenix_workspace::{
    load_highlight_palette_or_default, open_document, ContentHash, DocumentLease,
    DocumentLeaseToken, DocumentRevision, EntityRegistry, EntryId, EntryKind, WorkspaceDocument,
    WorkspaceError, ROOT_ID,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use thiserror::Error;

pub const COMMAND_CAPACITY: usize = 64;
pub const EVENT_CAPACITY: usize = 256;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct KernelSnapshot {
    pub revision: u64,
    pub workspace: Arc<WorkspaceDocument>,
    pub active_entry: EntryId,
    pub active_document: Option<DocumentId>,
    pub active_document_lease: Option<Arc<DocumentLease>>,
    pub entity_registry: Arc<EntityRegistry>,
    pub atlas_registry: Arc<AtlasRegistry>,
    pub resident_scene: Option<Arc<ResidentScene>>,
    pub scene_product_index: Option<Arc<PhoenixSceneProductIndexV1>>,
    pub scene_publication: Option<ScenePublicationReceipt>,
    pub document_anchors: Option<Arc<VerifiedDocumentAnchors>>,
    pub graph_view: GraphViewState,
    pub graph_selection: GraphSelectionState,
    pub style: StyleState,
    pub highlight_palette: Arc<HighlightPalette>,
    pub capabilities: Arc<RuntimeCapabilities>,
    pub shutting_down: bool,
}

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("kernel command queue is full at capacity {COMMAND_CAPACITY}")]
    CommandQueueFull,
    #[error("kernel coordinator is unavailable")]
    CoordinatorUnavailable,
    #[error("kernel command {0} timed out")]
    CommandTimedOut(u64),
    #[error("kernel is shutting down")]
    ShuttingDown,
    #[error("kernel event queue is full at capacity {EVENT_CAPACITY}")]
    EventQueueFull,
    #[error("kernel lock is poisoned: {0}")]
    Poisoned(&'static str),
    #[error("workspace entry {0:?} is not a note")]
    ActiveEntryNotDocument(EntryId),
    #[error("document lease does not belong to the active kernel document")]
    DocumentLeaseNotActive,
    #[error("resident generation {incoming:?} is not newer than {current:?}")]
    StaleGeneration {
        current: GraphGeneration,
        incoming: GraphGeneration,
    },
    #[error("style state is invalid")]
    InvalidStyle,
    #[error("document anchor snapshot does not match the active document lease")]
    DocumentAnchorsNotActive,
    #[error("scene product index requires a resident archive")]
    ProductIndexWithoutScene,
    #[error("native scene publication is unavailable in fixture/recovery mode")]
    ProductionPublisherUnavailable,
    #[error("native scene rebuild requires an active document lease")]
    ActiveSceneDocumentUnavailable,
    #[error("backend scene publication must carry full graph authority")]
    BackendPublicationMustBeFull,
    #[error("compiled scene publication metadata does not match its archive generation")]
    CompiledPublicationMismatch,
    #[error(
        "scene publication registry revision {publication} does not match current revision {current}"
    )]
    PublicationRegistryMismatch { publication: u64, current: u64 },
    #[error(
        "published registry revision {published} is newer than workspace revision {workspace}"
    )]
    PublishedRegistryAhead { published: u64, workspace: u64 },
    #[error("canonical Atlas entity identity zero is reserved")]
    InvalidAtlasEntityIdentity,
    #[error("filtered graph view requires a verified scene product index")]
    ProductIndexRequiredForFilteredView,
    #[error("graph view must retain at least one visible review and relation family")]
    InvalidGraphView,
    #[error("graph view authority does not match the resident scene authority")]
    StaleGraphViewAuthority,
    #[error("Atlas entity {0} has no verified node mapping in the resident scene")]
    AtlasEntityNotMapped(u64),
    #[error("graph node {0} is absent from the resident scene product index")]
    GraphNodeNotFound(u64),
    #[error(transparent)]
    SceneContract(#[from] SceneContractError),
    #[error(transparent)]
    ProductIndex(#[from] ProductIndexError),
    #[error(transparent)]
    ScenePublication(#[from] ScenePublicationError),
    #[error(transparent)]
    SceneCompiler(#[from] NativeSceneCompilerError),
    #[error(transparent)]
    Highlight(#[from] HighlightContractError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("kernel worker thread panicked")]
    WorkerPanicked,
}

struct KernelState {
    revision: u64,
    workspace: Arc<WorkspaceDocument>,
    active_entry: EntryId,
    active_document: Option<DocumentId>,
    active_document_lease: Option<Arc<DocumentLease>>,
    entity_registry: Arc<EntityRegistry>,
    atlas_registry: Arc<AtlasRegistry>,
    resident_scene: Option<Arc<ResidentScene>>,
    scene_product_index: Option<Arc<PhoenixSceneProductIndexV1>>,
    scene_publication: Option<ScenePublicationReceipt>,
    document_anchors: Option<Arc<VerifiedDocumentAnchors>>,
    graph_view: GraphViewState,
    graph_selection: GraphSelectionState,
    style: StyleState,
    highlight_palette: Arc<HighlightPalette>,
    capabilities: Arc<RuntimeCapabilities>,
    shutting_down: bool,
}

struct KernelShared {
    state: RwLock<KernelState>,
    events: Mutex<VecDeque<KernelEvent>>,
    workspace_path: PathBuf,
    publisher: Option<Arc<ScenePublicationStore>>,
    metrics: KernelMetricAtoms,
}

use metrics::KernelMetricAtoms;

struct Envelope {
    sequence: u64,
    command: KernelCommand,
    reply: SyncSender<Result<CommandReceipt, KernelError>>,
}

enum CoordinatorMessage {
    Command(Envelope),
    Shutdown(SyncSender<Result<(), KernelError>>),
}

pub struct PhoenixKernel {
    shared: Arc<KernelShared>,
    sender: SyncSender<CoordinatorMessage>,
    next_sequence: AtomicU64,
    shutdown_started: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl PhoenixKernel {
    pub fn start_production(workspace_path: PathBuf) -> Result<Arc<Self>, KernelError> {
        let publisher = Arc::new(ScenePublicationStore::for_workspace(&workspace_path)?);
        Self::start_internal(workspace_path, None, None, Some(publisher))
    }

    /// Starts the explicit fixture/recovery harness. Production startup uses
    /// [`Self::start_production`] and its atomic publication manifest.
    pub fn start(
        workspace_path: PathBuf,
        initial_scene: Option<Arc<ResidentScene>>,
    ) -> Result<Arc<Self>, KernelError> {
        Self::start_with_product_index(workspace_path, initial_scene, None)
    }

    pub fn start_with_product_index(
        workspace_path: PathBuf,
        initial_scene: Option<Arc<ResidentScene>>,
        initial_product_index: Option<Arc<PhoenixSceneProductIndexV1>>,
    ) -> Result<Arc<Self>, KernelError> {
        Self::start_internal(workspace_path, initial_scene, initial_product_index, None)
    }

    fn start_internal(
        workspace_path: PathBuf,
        mut initial_scene: Option<Arc<ResidentScene>>,
        mut initial_product_index: Option<Arc<PhoenixSceneProductIndexV1>>,
        publisher: Option<Arc<ScenePublicationStore>>,
    ) -> Result<Arc<Self>, KernelError> {
        if initial_product_index.is_some() && initial_scene.is_none() {
            return Err(KernelError::ProductIndexWithoutScene);
        }
        let workspace = Arc::new(WorkspaceDocument::load_or_seed(&workspace_path)?);
        let active_entry = workspace.first_note().unwrap_or(ROOT_ID);
        let active_document = active_document(&workspace, active_entry);
        let active_document_lease = active_document
            .map(|_| open_document(&workspace_path, &workspace, active_entry))
            .transpose()?
            .map(Arc::new);
        let entity_registry = Arc::new(EntityRegistry::load_or_empty(&workspace_path)?);
        let atlas_registry = Arc::new(AtlasRegistry::from_registry(&entity_registry));
        let highlight_palette = Arc::new(load_highlight_palette_or_default(&workspace_path)?);
        let mut scene_publication = None;
        if let Some(publisher) = publisher.as_ref() {
            let published = scene_publication::initial_production_scene(
                publisher,
                &atlas_registry,
                *highlight_palette,
            )?;
            scene_publication = Some(published.receipt);
            initial_scene = Some(published.scene);
            initial_product_index = Some(published.product_index);
        }
        let graph_view = initial_scene
            .as_ref()
            .map(|scene| scene.graph_view_state(initial_product_index.as_deref()))
            .transpose()?
            .unwrap_or_default();
        let document_anchors =
            entity_tags::registry_anchors(&entity_registry, active_document_lease.as_deref())?;
        let state = KernelState {
            revision: 1,
            workspace,
            active_entry,
            active_document,
            active_document_lease,
            entity_registry,
            atlas_registry,
            resident_scene: initial_scene,
            scene_product_index: initial_product_index,
            scene_publication,
            document_anchors,
            graph_view,
            graph_selection: GraphSelectionState::default(),
            style: StyleState::default(),
            highlight_palette,
            capabilities: Arc::new(RuntimeCapabilities::default()),
            shutting_down: false,
        };
        let shared = Arc::new(KernelShared {
            state: RwLock::new(state),
            events: Mutex::new(VecDeque::with_capacity(32)),
            workspace_path,
            publisher,
            metrics: KernelMetricAtoms::default(),
        });
        let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("phoenix-native-kernel".into())
            .spawn(move || coordinator_loop(worker_shared, receiver))
            .map_err(|_| KernelError::CoordinatorUnavailable)?;
        Ok(Arc::new(Self {
            shared,
            sender,
            next_sequence: AtomicU64::new(1),
            shutdown_started: AtomicBool::new(false),
            worker: Mutex::new(Some(worker)),
        }))
    }

    pub fn snapshot(&self) -> Result<KernelSnapshot, KernelError> {
        let state = self
            .shared
            .state
            .read()
            .map_err(|_| KernelError::Poisoned("state read"))?;
        Ok(KernelSnapshot {
            revision: state.revision,
            workspace: Arc::clone(&state.workspace),
            active_entry: state.active_entry,
            active_document: state.active_document,
            active_document_lease: state.active_document_lease.as_ref().map(Arc::clone),
            entity_registry: Arc::clone(&state.entity_registry),
            atlas_registry: Arc::clone(&state.atlas_registry),
            resident_scene: state.resident_scene.as_ref().map(Arc::clone),
            scene_product_index: state.scene_product_index.as_ref().map(Arc::clone),
            scene_publication: state.scene_publication,
            document_anchors: state.document_anchors.as_ref().map(Arc::clone),
            graph_view: state.graph_view,
            graph_selection: state.graph_selection,
            style: state.style,
            highlight_palette: Arc::clone(&state.highlight_palette),
            capabilities: Arc::clone(&state.capabilities),
            shutting_down: state.shutting_down,
        })
    }

    pub fn execute(&self, command: KernelCommand) -> Result<CommandReceipt, KernelError> {
        if self.shutdown_started.load(Ordering::Acquire) {
            return Err(KernelError::ShuttingDown);
        }
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let envelope = Envelope {
            sequence,
            command,
            reply: reply_sender,
        };
        let pending = self
            .shared
            .metrics
            .commands_pending
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.shared
            .metrics
            .command_queue_high_water
            .fetch_max(pending, Ordering::Relaxed);
        match self.sender.try_send(CoordinatorMessage::Command(envelope)) {
            Ok(()) => {
                self.shared
                    .metrics
                    .commands_submitted
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_)) => {
                self.shared
                    .metrics
                    .commands_pending
                    .fetch_sub(1, Ordering::Relaxed);
                self.shared
                    .metrics
                    .commands_rejected
                    .fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::CommandQueueFull);
            }
            Err(TrySendError::Disconnected(_)) => {
                self.shared
                    .metrics
                    .commands_pending
                    .fetch_sub(1, Ordering::Relaxed);
                self.shared
                    .metrics
                    .commands_rejected
                    .fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::CoordinatorUnavailable);
            }
        }
        reply_receiver
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| KernelError::CommandTimedOut(sequence))?
    }

    pub fn events_after(&self, sequence: u64) -> Result<Vec<KernelEvent>, KernelError> {
        let events = self
            .shared
            .events
            .lock()
            .map_err(|_| KernelError::Poisoned("event read"))?;
        Ok(events
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect())
    }

    pub fn drain_events(&self) -> Result<Vec<KernelEvent>, KernelError> {
        let mut events = self
            .shared
            .events
            .lock()
            .map_err(|_| KernelError::Poisoned("event drain"))?;
        Ok(events.drain(..).collect())
    }

    pub fn workspace_path(&self) -> &Path {
        &self.shared.workspace_path
    }

    pub fn metrics(&self) -> KernelMetrics {
        self.shared.metrics.snapshot()
    }

    pub fn shutdown(&self) -> Result<(), KernelError> {
        if self.shutdown_started.swap(true, Ordering::AcqRel) {
            return self.join_worker();
        }
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(CoordinatorMessage::Shutdown(reply_sender))
            .map_err(|_| KernelError::CoordinatorUnavailable)?;
        reply_receiver
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| KernelError::CommandTimedOut(0))??;
        self.join_worker()
    }

    fn join_worker(&self) -> Result<(), KernelError> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| KernelError::Poisoned("worker join"))?;
        if let Some(handle) = worker.take() {
            handle.join().map_err(|_| KernelError::WorkerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for PhoenixKernel {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::error!(%error, "native kernel shutdown failed");
        }
    }
}

fn coordinator_loop(shared: Arc<KernelShared>, receiver: Receiver<CoordinatorMessage>) {
    while let Ok(message) = receiver.recv() {
        match message {
            CoordinatorMessage::Command(envelope) => {
                shared
                    .metrics
                    .commands_pending
                    .fetch_sub(1, Ordering::Relaxed);
                let result = apply_command(&shared, envelope.sequence, envelope.command);
                if result.is_ok() {
                    shared
                        .metrics
                        .commands_completed
                        .fetch_add(1, Ordering::Relaxed);
                    shared
                        .metrics
                        .last_sequence
                        .store(envelope.sequence, Ordering::Release);
                } else {
                    shared
                        .metrics
                        .commands_rejected
                        .fetch_add(1, Ordering::Relaxed);
                }
                let _ = envelope.reply.send(result);
            }
            CoordinatorMessage::Shutdown(reply) => {
                let result = mark_shutting_down(&shared);
                let _ = reply.send(result);
                break;
            }
        }
    }
    shared.metrics.worker_exited.store(true, Ordering::Release);
}

fn apply_command(
    shared: &KernelShared,
    sequence: u64,
    command: KernelCommand,
) -> Result<CommandReceipt, KernelError> {
    ensure_event_space(shared)?;
    match command {
        KernelCommand::SelectEntry(id) => select_entry(shared, sequence, id),
        KernelCommand::CreateEntry { kind, name } => create_entry(shared, sequence, kind, &name),
        KernelCommand::RenameEntry { id, name } => {
            mutate_workspace(shared, sequence, |workspace| {
                workspace.rename(id, &name)?;
                Ok(KernelOutcome::StateChanged)
            })
        }
        KernelCommand::DeleteEntry(id) => mutate_workspace(shared, sequence, |workspace| {
            workspace.delete(id).map(KernelOutcome::EntriesDeleted)
        }),
        KernelCommand::SaveDocument { lease, content } => {
            entity_tags::save_document(shared, sequence, lease, content)
        }
        KernelCommand::TagSelection(command) => entity_tags::tag_selection(
            shared,
            sequence,
            command.lease,
            command.content,
            command.tag,
        ),
        KernelCommand::PublishNerEntities(batch) => {
            atlas::publish_ner_batch(shared, sequence, batch)
        }
        KernelCommand::PublishNativeScene(publication) => {
            scene_publication::publish_full_scene(shared, sequence, *publication)
        }
        KernelCommand::PublishDocumentAnchors(anchors) => {
            publish_document_anchors(shared, sequence, anchors)
        }
        KernelCommand::SetManifold(manifold) => {
            graph_view::set_manifold(shared, sequence, manifold)
        }
        KernelCommand::SetGraphView(view) => graph_view::set_graph_view(shared, sequence, *view),
        KernelCommand::DispatchGraphAction(action) => {
            graph_view::dispatch_graph_action(shared, sequence, action)
        }
        KernelCommand::RequestGraphProvenance => {
            graph_view::request_graph_provenance(shared, sequence)
        }
        KernelCommand::SetStyle(style) => graph_view::set_style(shared, sequence, style),
        KernelCommand::SetHighlightPalette(palette) => {
            graph_view::set_highlight_palette(shared, sequence, *palette)
        }
        KernelCommand::SetGraphSelection(selection) => {
            graph_selection::set_selection(shared, sequence, selection)
        }
    }
}

fn select_entry(
    shared: &KernelShared,
    sequence: u64,
    id: EntryId,
) -> Result<CommandReceipt, KernelError> {
    let workspace = Arc::clone(&read_state(shared)?.workspace);
    let kind = workspace
        .entry(id)
        .ok_or(WorkspaceError::MissingEntry(id))?
        .kind;
    let active_document_lease = if kind == EntryKind::Note {
        Some(Arc::new(open_document(
            &shared.workspace_path,
            &workspace,
            id,
        )?))
    } else {
        None
    };
    let mut state = write_state(shared)?;
    let document_anchors =
        entity_tags::registry_anchors(&state.entity_registry, active_document_lease.as_deref())?;
    state.active_entry = id;
    state.active_document = if kind == EntryKind::Note {
        Some(DocumentId(id.0))
    } else {
        None
    };
    state.active_document_lease = active_document_lease;
    state.document_anchors = document_anchors;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    let document = state.active_document;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::ActiveDocumentChanged(document),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

fn create_entry(
    shared: &KernelShared,
    sequence: u64,
    kind: EntryKind,
    name: &str,
) -> Result<CommandReceipt, KernelError> {
    let active = read_state(shared)?.active_entry;
    mutate_workspace(shared, sequence, |workspace| {
        let parent = workspace.parent_for_create(active)?;
        workspace
            .create(parent, kind, name)
            .map(KernelOutcome::EntryCreated)
    })
}

fn mutate_workspace(
    shared: &KernelShared,
    sequence: u64,
    mutation: impl FnOnce(&mut WorkspaceDocument) -> Result<KernelOutcome, WorkspaceError>,
) -> Result<CommandReceipt, KernelError> {
    let current = Arc::clone(&read_state(shared)?.workspace);
    let mut candidate = (*current).clone();
    let outcome = mutation(&mut candidate)?;
    candidate.save_atomic(&shared.workspace_path)?;
    let workspace_revision = candidate.revision();
    let candidate = Arc::new(candidate);
    let mut state = write_state(shared)?;
    state.workspace = candidate;
    if state.workspace.entry(state.active_entry).is_none() {
        state.active_entry = ROOT_ID;
        state.active_document = None;
        state.active_document_lease = None;
        state.document_anchors = None;
    }
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::WorkspaceCommitted { workspace_revision },
        },
    )?;
    Ok(receipt(sequence, revision, outcome))
}

fn publish_document_anchors(
    shared: &KernelShared,
    sequence: u64,
    anchors: Arc<VerifiedDocumentAnchors>,
) -> Result<CommandReceipt, KernelError> {
    let mut state = write_state(shared)?;
    let active_document = state
        .active_document
        .ok_or(KernelError::DocumentAnchorsNotActive)?;
    let active_lease = state
        .active_document_lease
        .as_ref()
        .ok_or(KernelError::DocumentAnchorsNotActive)?;
    if anchors.document() != active_document
        || anchors.document_revision() != active_lease.revision.0
        || anchors.content_hash() != active_lease.content_hash.0
    {
        return Err(KernelError::DocumentAnchorsNotActive);
    }
    if let Some(generation) = anchors.graph_generation() {
        if state
            .resident_scene
            .as_ref()
            .is_none_or(|scene| scene.generation() != generation)
        {
            return Err(KernelError::DocumentAnchorsNotActive);
        }
    }
    let document = anchors.document();
    let count = anchors.anchors().len();
    state.document_anchors = Some(anchors);
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::DocumentAnchorsChanged { document, count },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::DocumentAnchorsPublished(count),
    ))
}

fn mark_shutting_down(shared: &KernelShared) -> Result<(), KernelError> {
    ensure_event_space(shared)?;
    let mut state = write_state(shared)?;
    state.shutting_down = true;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence: u64::MAX,
            kernel_revision: revision,
            kind: KernelEventKind::ShuttingDown,
        },
    )
}

fn push_event(shared: &KernelShared, event: KernelEvent) -> Result<(), KernelError> {
    let mut events = shared
        .events
        .lock()
        .map_err(|_| KernelError::Poisoned("event write"))?;
    if events.len() >= EVENT_CAPACITY {
        return Err(KernelError::EventQueueFull);
    }
    events.push_back(event);
    shared
        .metrics
        .events_published
        .fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn ensure_event_space(shared: &KernelShared) -> Result<(), KernelError> {
    let events = shared
        .events
        .lock()
        .map_err(|_| KernelError::Poisoned("event capacity"))?;
    if events.len() >= EVENT_CAPACITY {
        return Err(KernelError::EventQueueFull);
    }
    Ok(())
}

fn active_document(workspace: &WorkspaceDocument, id: EntryId) -> Option<DocumentId> {
    workspace
        .entry(id)
        .filter(|entry| entry.kind == EntryKind::Note)
        .map(|entry| DocumentId(entry.id.0))
}

#[cfg(test)]
mod tests;
