mod atlas_control;
mod atlas_entities;
mod drawer;
mod entity_tags;
mod footer;
mod graph_controls;
mod graph_viewport;
mod highlights;
mod view;

use crate::graph_window::{GraphWindow, ViewportGeometry};
use crate::lifecycle;
use crate::proof;
use gpui::{AppContext as _, Context, Entity, FocusHandle, SharedString, Timer, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::resizable::ResizableState;
use hashbrown::HashSet;
use phoenix_app_core::GraphProvenanceReceipt;
use phoenix_app_core::{KernelCommand, KernelOutcome, KernelSnapshot, PhoenixKernel};
use phoenix_scene_contract::ResidentSceneLoadError;
use phoenix_workspace::{DocumentLease, EntryId, EntryKind, WorkspaceEntry, ROOT_ID};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const CANVAS: u32 = 0x151718;
const SURFACE: u32 = 0x1a1c1d;
const BORDER: u32 = 0x303534;
const BORDER_BRIGHT: u32 = 0x3c4442;
const TEXT: u32 = 0xdde2e0;
const TEXT_MUTED: u32 = 0x858f8c;
const DANGER: u32 = 0xe46f69;
const LEFT_SIDEBAR_INITIAL_WIDTH: f32 = 344.;
const LEFT_SIDEBAR_MIN_WIDTH: f32 = 240.;
const LEFT_SIDEBAR_MAX_WIDTH: f32 = 520.;
const RIGHT_SIDEBAR_INITIAL_WIDTH: f32 = 320.;
const RIGHT_SIDEBAR_MIN_WIDTH: f32 = 260.;
const RIGHT_SIDEBAR_MAX_WIDTH: f32 = 480.;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditMode {
    CreateNote,
    CreateFolder,
    Rename,
}

impl EditMode {
    fn label(self) -> &'static str {
        match self {
            Self::CreateNote => "Create note",
            Self::CreateFolder => "Create folder",
            Self::Rename => "Rename",
        }
    }
}

pub struct PhoenixShell {
    kernel: Arc<PhoenixKernel>,
    editor: Entity<velotype::Editor>,
    editor_lease: Option<Arc<DocumentLease>>,
    graph: Rc<RefCell<Option<GraphWindow>>>,
    graph_geometry: Rc<Cell<Option<ViewportGeometry>>>,
    scene_error: Option<ResidentSceneLoadError>,
    graph_init_error: Option<String>,
    graph_rebuild_pending: bool,
    graph_provenance: Option<GraphProvenanceReceipt>,
    proof_pending: bool,
    soak_mode: bool,
    expanded: HashSet<EntryId>,
    name_input: Entity<InputState>,
    atlas_search: Entity<InputState>,
    edit_mode: EditMode,
    delete_armed: Option<EntryId>,
    left_open: bool,
    right_open: bool,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    drawer_layout: drawer::DrawerLayout,
    drawer_resize_state: Entity<ResizableState>,
    drawer_tab: drawer::DrawerTab,
    atlas_control_section: atlas_control::AtlasControlSection,
    atlas_control_focus: FocusHandle,
    atlas_selected_candidate: Option<phoenix_app_core::AtlasCandidateId>,
    document_metrics: footer::DocumentMetrics,
    status: SharedString,
}

impl PhoenixShell {
    pub fn new(
        proof_mode: bool,
        soak_mode: bool,
        design_preview: bool,
        kernel: Arc<PhoenixKernel>,
        scene_error: Option<ResidentSceneLoadError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let parent = match graph_viewport::parent_window_handle(window) {
            Ok(parent) => Some(parent),
            Err(error) => {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_EMBEDDED_GRAPH_PARENT_FAILED {error:#}");
                None
            }
        };
        let mut expanded = HashSet::new();
        expanded.insert(ROOT_ID);
        expanded.insert(EntryId(2));
        expanded.insert(EntryId(4));
        let status = scene_error.as_ref().map_or_else(
            || "READY / SHARED NATIVE KERNEL ONLINE".into(),
            |error| format!("GRAPH BLOCKED / {error}").into(),
        );
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name this item..."));
        let atlas_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search canonical entities..."));
        let atlas_control_focus = atlas_control::focus_handle(cx);
        cx.subscribe(
            &atlas_search,
            |_shell: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        )
        .detach();
        let editor_lease = kernel
            .snapshot()
            .ok()
            .and_then(|snapshot| snapshot.active_document_lease);
        let initial_markdown = editor_lease
            .as_ref()
            .map(|lease| lease.content.to_string())
            .unwrap_or_default();
        let document_metrics = footer::DocumentMetrics::from_text(&initial_markdown);
        let editor = cx.new(|cx| velotype::Editor::embedded_from_markdown(cx, initial_markdown));
        cx.subscribe(&editor, Self::on_editor_event).detach();
        let mut shell = Self {
            kernel,
            editor,
            editor_lease,
            graph: Rc::new(RefCell::new(None)),
            graph_geometry: Rc::new(Cell::new(None)),
            scene_error,
            graph_init_error: parent
                .is_none()
                .then(|| "GPUI parent window handle is unavailable".into()),
            graph_rebuild_pending: false,
            graph_provenance: None,
            proof_pending: proof_mode || soak_mode,
            soak_mode,
            expanded,
            name_input,
            atlas_search,
            edit_mode: EditMode::CreateNote,
            delete_armed: None,
            left_open: true,
            right_open: true,
            left_sidebar_width: LEFT_SIDEBAR_INITIAL_WIDTH,
            right_sidebar_width: RIGHT_SIDEBAR_INITIAL_WIDTH,
            drawer_layout: drawer::DrawerLayout::new(proof_mode || soak_mode || design_preview),
            drawer_resize_state: cx.new(|_| ResizableState::default()),
            drawer_tab: drawer::DrawerTab::Graph,
            atlas_control_section: atlas_control::AtlasControlSection::Overview,
            atlas_control_focus,
            atlas_selected_candidate: None,
            document_metrics,
            status,
        };
        shell.initialize_highlights(cx);
        if shell.scene_error.is_none() {
            if let Some(parent) = parent {
                shell.start_graph_host(parent, window, cx);
            }
        }
        cx.on_app_quit(|this, cx| {
            let graph = this.graph.borrow_mut().take();
            let background = cx.background_executor().clone();
            async move {
                if let Some(mut graph) = graph {
                    if let Err(error) = background.spawn(async move { graph.shutdown() }).await {
                        lifecycle::mark_proof_failed();
                        eprintln!("PHOENIX_EMBEDDED_GRAPH_SHUTDOWN_FAILED {error:#}");
                    }
                }
            }
        })
        .detach();
        shell
    }

    fn start_graph_host(
        &mut self,
        parent: crate::graph_window::ParentWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kernel = Arc::clone(&self.kernel);
        let (ui_sender, ui_receiver) = async_channel::bounded(1);
        cx.spawn(async move |shell, async_cx| {
            while ui_receiver.recv().await.is_ok() {
                if shell
                    .update(async_cx, |_this, cx| {
                        cx.notify();
                        cx.refresh_windows();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |shell, async_cx| {
            let result = background
                .spawn(async move {
                    GraphWindow::start(parent, kernel, ui_sender)
                        .map_err(|error| format!("{error:#}"))
                })
                .await;
            if let Err(error) = shell.update(async_cx, |this, cx| {
                match result {
                    Ok(graph) => {
                        let (node_count, edge_count) = graph.inventory();
                        if let Some(geometry) = this.graph_geometry.get() {
                            if let Err(error) = graph.set_viewport(geometry) {
                                lifecycle::mark_proof_failed();
                                this.graph_init_error = Some(format!("{error:#}"));
                                this.status = format!("GRAPH BLOCKED / {error:#}").into();
                                *this.graph.borrow_mut() = Some(graph);
                                cx.notify();
                                return;
                            }
                        }
                        *this.graph.borrow_mut() = Some(graph);
                        this.graph_init_error = None;
                        this.status =
                            format!("READY / EMBEDDED GRAPH {node_count}N / {edge_count}E").into();
                    }
                    Err(error) => {
                        lifecycle::mark_proof_failed();
                        this.graph_init_error = Some(error.clone());
                        this.status = format!("GRAPH BLOCKED / {error}").into();
                    }
                }
                cx.notify();
            }) {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_EMBEDDED_GRAPH_DELIVERY_FAILED {error:#}");
            }
        })
        .detach();
    }

    fn schedule_proof(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.proof_pending || self.graph.borrow().is_none() {
            return;
        }
        self.proof_pending = false;
        let graph = Rc::clone(&self.graph);
        let proof_handle = graph.borrow().as_ref().map(|native| native.proof_handle());
        let kernel = Arc::clone(&self.kernel);
        let editor = self.editor.clone();
        let init_error = self.graph_init_error.clone();
        let soak_mode = self.soak_mode;
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |shell, async_cx| {
            Timer::after(Duration::from_millis(250)).await;
            let mut shell_soak = None;
            if soak_mode {
                let mut result = proof::ShellSoakProof::default();
                for _ in 0..200 {
                    if shell
                        .update(async_cx, |this, cx| this.toggle_drawer(cx))
                        .is_err()
                    {
                        lifecycle::mark_proof_failed();
                        break;
                    }
                    result.drawer_toggles += 1;
                    Timer::after(Duration::from_millis(2)).await;
                }
                for cycle in 0..80_u32 {
                    if shell
                        .update(async_cx, |this, cx| {
                            let height = drawer::DRAWER_MIN_HEIGHT + (cycle % 31) as f32 * 8.0;
                            let width = 236.0 + (cycle % 29) as f32 * 7.0;
                            this.drawer_layout.set_height(height);
                            this.drawer_layout.set_atlas_width(width);
                            if cycle % 10 == 0 {
                                this.left_open = false;
                                result.left_sidebar_collapses += 1;
                            } else if cycle % 10 == 1 {
                                this.left_open = true;
                            }
                            if cycle % 12 == 0 {
                                this.right_open = false;
                                result.right_sidebar_collapses += 1;
                            } else if cycle % 12 == 1 {
                                this.right_open = true;
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        lifecycle::mark_proof_failed();
                        break;
                    }
                    result.layout_resize_cycles += 1;
                    Timer::after(Duration::from_millis(3)).await;
                }
                let _ = shell.update(async_cx, |this, cx| {
                    this.left_open = true;
                    this.right_open = true;
                    this.drawer_layout.set_height(drawer::DRAWER_INITIAL_HEIGHT);
                    cx.notify();
                });
                Timer::after(Duration::from_millis(100)).await;
                shell_soak = Some(result);
            }
            let proof_result = match proof_handle {
                Some(handle) => Some(
                    background
                        .spawn(async move { handle.run().map_err(|error| format!("{error:#}")) })
                        .await,
                ),
                None => None,
            };
            let graph_to_shutdown = graph.borrow_mut().take();
            let shutdown_error = match graph_to_shutdown {
                Some(mut graph) => background
                    .spawn(async move { graph.shutdown().map_err(|error| format!("{error:#}")) })
                    .await
                    .err(),
                None => Some("embedded graph host vanished before shutdown".into()),
            };
            if let Err(error) = async_cx.update(|_, cx| {
                proof::execute(
                    &kernel,
                    &editor,
                    cx,
                    proof::ExecutionReport {
                        init_error,
                        proof_result,
                        shutdown_error,
                        soak_mode,
                        shell_soak,
                    },
                );
                cx.quit();
            }) {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_SHELL_CUT1_PROOF_QUIT_FAILED {error:#}");
            }
        })
        .detach();
    }

    fn set_edit_mode(&mut self, mode: EditMode, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_mode = mode;
        self.delete_armed = None;
        let value = if mode == EditMode::Rename {
            self.selected_entry()
                .map(|entry| entry.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.name_input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        cx.notify();
    }

    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().to_string();
        let active = self.active_entry();
        let command = match self.edit_mode {
            EditMode::CreateNote | EditMode::CreateFolder => {
                let kind = if self.edit_mode == EditMode::CreateNote {
                    EntryKind::Note
                } else {
                    EntryKind::Folder
                };
                KernelCommand::CreateEntry { kind, name }
            }
            EditMode::Rename => KernelCommand::RenameEntry { id: active, name },
        };
        let verb = self.edit_mode.label();
        match self.kernel.execute(command) {
            Ok(receipt) => {
                if let KernelOutcome::EntryCreated(created) = receipt.outcome {
                    if let Ok(snapshot) = self.kernel.snapshot() {
                        if let Ok(parent) = snapshot.workspace.parent_for_create(created) {
                            self.expanded.insert(parent);
                        }
                    }
                    self.select_entry(created, cx);
                }
                let _ = self.kernel.drain_events();
                self.status = format!(
                    "COMMITTED / {verb} / KERNEL REVISION {} / SEQUENCE {}",
                    receipt.kernel_revision, receipt.sequence
                )
                .into();
                self.name_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.edit_mode = EditMode::CreateNote;
            }
            Err(error) => self.status = format!("BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let selected = self.active_entry();
        if self.editor_lease.is_some() && self.editor.read_with(cx, |editor, _| editor.is_dirty()) {
            self.status = "BLOCKED / SAVE THE ACTIVE NOTE BEFORE DELETING IT".into();
            cx.notify();
            return;
        }
        if selected == ROOT_ID {
            self.status = "BLOCKED / THE WORKSPACE ROOT IS IMMUTABLE".into();
            cx.notify();
            return;
        }
        if self.delete_armed != Some(selected) {
            self.delete_armed = Some(selected);
            self.status = "CONFIRM / PRESS DELETE AGAIN TO REMOVE THE ENTIRE BRANCH".into();
            cx.notify();
            return;
        }
        let parent = self
            .kernel
            .snapshot()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .workspace
                    .entry(selected)
                    .and_then(|entry| entry.parent)
            })
            .unwrap_or(ROOT_ID);
        match self.kernel.execute(KernelCommand::DeleteEntry(selected)) {
            Ok(receipt) => {
                let removed = match receipt.outcome {
                    KernelOutcome::EntriesDeleted(count) => count,
                    _ => 0,
                };
                let _ = self.kernel.execute(KernelCommand::SelectEntry(parent));
                self.reload_editor_from_kernel(cx);
                self.initialize_highlights(cx);
                let _ = self.kernel.drain_events();
                self.delete_armed = None;
                self.status = format!(
                    "COMMITTED / REMOVED {removed} ITEM(S) / SEQUENCE {}",
                    receipt.sequence
                )
                .into();
            }
            Err(error) => self.status = format!("BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn select_entry(&mut self, id: EntryId, cx: &mut Context<Self>) {
        self.delete_armed = None;
        if id != self.active_entry()
            && self.editor_lease.is_some()
            && self.editor.read_with(cx, |editor, _| editor.is_dirty())
        {
            self.status = "BLOCKED / SAVE THE ACTIVE NOTE BEFORE CHANGING SELECTION".into();
            cx.notify();
            return;
        }
        if self
            .kernel
            .snapshot()
            .ok()
            .and_then(|snapshot| snapshot.workspace.entry(id).cloned())
            .is_some_and(|entry| entry.kind == EntryKind::Folder)
            && !self.expanded.remove(&id)
        {
            self.expanded.insert(id);
        }
        match self.kernel.execute(KernelCommand::SelectEntry(id)) {
            Ok(receipt) => {
                let _ = self.kernel.drain_events();
                self.reload_editor_from_kernel(cx);
                self.initialize_highlights(cx);
                self.status =
                    format!("READY / DOCUMENT LEASE / SEQUENCE {}", receipt.sequence).into();
            }
            Err(error) => self.status = format!("BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn on_editor_event(
        &mut self,
        editor: Entity<velotype::Editor>,
        event: &velotype::EditorEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, velotype::EditorEvent::DocumentChanged { .. }) {
            self.document_metrics = editor.read_with(cx, |editor, cx| {
                footer::DocumentMetrics::from_text(&editor.host_document_text(cx))
            });
            cx.notify();
            return;
        }
        if let velotype::EditorEvent::EntityTagRequested(request) = event {
            self.tag_entity_selection(editor, request, cx);
            return;
        }
        if !matches!(event, velotype::EditorEvent::SaveRequested) {
            return;
        }
        let Some(lease) = self.editor_lease.as_ref().map(Arc::clone) else {
            self.status = "BLOCKED / NO ACTIVE KERNEL DOCUMENT LEASE".into();
            cx.notify();
            return;
        };
        let content: Arc<str> =
            Arc::from(editor.read_with(cx, |editor, cx| editor.host_document_text(cx)));
        match self.kernel.execute(KernelCommand::SaveDocument {
            lease: lease.token(),
            content,
        }) {
            Ok(receipt) => {
                if let Ok(snapshot) = self.kernel.snapshot() {
                    self.editor_lease = snapshot.active_document_lease;
                }
                editor.update(cx, |editor, cx| editor.mark_embedded_saved(cx));
                self.initialize_highlights(cx);
                let document_revision = self
                    .editor_lease
                    .as_ref()
                    .map(|lease| lease.revision.0)
                    .unwrap_or(0);
                self.status = format!(
                    "SAVED / DOCUMENT REVISION {document_revision} / SEQUENCE {}",
                    receipt.sequence
                )
                .into();
                let _ = self.kernel.drain_events();
            }
            Err(error) => {
                if let Ok(snapshot) = self.kernel.snapshot() {
                    self.editor_lease = snapshot.active_document_lease;
                }
                self.status = format!("SAVE BLOCKED / {error}").into();
            }
        }
        cx.notify();
    }

    fn reload_editor_from_kernel(&mut self, cx: &mut Context<Self>) {
        let next = self
            .kernel
            .snapshot()
            .ok()
            .and_then(|snapshot| snapshot.active_document_lease);
        let unchanged = self
            .editor_lease
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(current, next)| current.token() == next.token());
        if unchanged {
            return;
        }
        let markdown = next
            .as_ref()
            .map(|lease| lease.content.to_string())
            .unwrap_or_default();
        self.document_metrics = footer::DocumentMetrics::from_text(&markdown);
        self.editor.update(cx, |editor, cx| {
            editor.replace_embedded_document(markdown, cx)
        });
        self.editor_lease = next;
    }

    fn kernel_snapshot(&self) -> Option<KernelSnapshot> {
        self.kernel.snapshot().ok()
    }

    fn active_entry(&self) -> EntryId {
        self.kernel_snapshot()
            .map(|snapshot| snapshot.active_entry)
            .unwrap_or(ROOT_ID)
    }

    fn selected_entry(&self) -> Option<WorkspaceEntry> {
        let snapshot = self.kernel_snapshot()?;
        snapshot.workspace.entry(snapshot.active_entry).cloned()
    }
}
