mod commands;
mod manifold;
mod proof;
mod sync;
mod viewport;

use crate::lifecycle;
use anyhow::{anyhow, Context, Result};
use graph_model::{GraphRevision, NodeId};
use graph_render_wgpu::{GraphEvent, GraphInput, GraphRenderer, PointerButton};
use phoenix_app_core::{GraphSelectionCommand, GraphSelectionOrigin, KernelCommand, PhoenixKernel};
use phoenix_scene_archive::{PageKey, PageKind};
use phoenix_scene_contract::{GraphGeneration, GraphViewState, Manifold};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, ShowWindow, HWND_TOP, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

use commands::{GraphQueueMetrics, GraphWake, GraphWindowCommand};
use manifold::{FixedSamples, PendingManifoldSwitch};
pub use manifold::{GraphGpuTelemetry, ManifoldSwitchReceipt};
use proof::ProjectionIdentity;
pub use proof::{EmbeddedHostProof, GraphProofHandle};
use viewport::{hwnd_for_window, prepare_child_window, ViewportMailbox};
pub use viewport::{ParentWindowHandle, ViewportGeometry};

const COMMAND_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct InteractionStressProof {
    pub updates: u32,
    pub cpu_p95_us: u128,
    pub cpu_max_us: u128,
    pub stable_capacities: bool,
    pub route_node_capacity: usize,
    pub route_edge_capacity: usize,
}

struct Ready {
    hwnd: isize,
    proxy: EventLoopProxy<GraphWake>,
    node_count: usize,
    edge_count: usize,
    generation: GraphGeneration,
}

struct GraphRuntimeSignals {
    ready_sender: SyncSender<std::result::Result<Ready, String>>,
    proxy: EventLoopProxy<GraphWake>,
    ui_notifications: async_channel::Sender<()>,
}

pub struct GraphWindow {
    parent: ParentWindowHandle,
    hwnd: isize,
    sender: SyncSender<GraphWindowCommand>,
    proxy: EventLoopProxy<GraphWake>,
    viewport: Arc<ViewportMailbox>,
    queue_metrics: Arc<GraphQueueMetrics>,
    join: Option<JoinHandle<Result<()>>>,
    node_count: usize,
    edge_count: usize,
    generation: GraphGeneration,
    kernel: Arc<PhoenixKernel>,
}

impl GraphWindow {
    pub fn start(
        parent: ParentWindowHandle,
        kernel: Arc<PhoenixKernel>,
        ui_notifications: async_channel::Sender<()>,
    ) -> Result<Self> {
        parent.prepare_for_child_hosting()?;
        let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let viewport = Arc::new(ViewportMailbox::new());
        let queue_metrics = Arc::new(GraphQueueMetrics::default());
        let thread_viewport = Arc::clone(&viewport);
        let thread_queue_metrics = Arc::clone(&queue_metrics);
        let thread_kernel = Arc::clone(&kernel);
        let join = thread::Builder::new()
            .name("phoenix-child-graph".into())
            .spawn(move || {
                run_graph_window(
                    parent,
                    thread_kernel,
                    receiver,
                    thread_viewport,
                    thread_queue_metrics,
                    ready_sender,
                    ui_notifications,
                )
            })
            .context("spawn embedded graph event loop")?;
        match ready_receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(ready)) => {
                tracing::info!(
                    parent_hwnd = parent.hwnd().0 as isize,
                    child_hwnd = ready.hwnd,
                    "embedded graph child attached"
                );
                Ok(Self {
                    parent,
                    hwnd: ready.hwnd,
                    sender,
                    proxy: ready.proxy,
                    viewport,
                    queue_metrics,
                    join: Some(join),
                    node_count: ready.node_count,
                    edge_count: ready.edge_count,
                    generation: ready.generation,
                    kernel,
                })
            }
            Ok(Err(message)) => {
                let _ = join.join();
                Err(anyhow!(message))
            }
            Err(error) => {
                let _ = join.join();
                Err(anyhow!("embedded graph initialization timed out: {error}"))
            }
        }
    }

    pub fn set_viewport(&self, geometry: ViewportGeometry) -> Result<()> {
        self.proof_handle().set_ui_viewport(geometry)
    }

    pub fn hide_viewport(&self) -> Result<()> {
        self.proof_handle().hide_viewport()
    }

    pub fn inventory(&self) -> (usize, usize) {
        (self.node_count, self.edge_count)
    }

    pub fn sync_kernel_state(&self) -> Result<()> {
        self.proof_handle()
            .send(GraphWindowCommand::SyncKernelState)
    }

    pub fn fit_graph(&self) -> Result<()> {
        self.proof_handle().send(GraphWindowCommand::FitGraph)
    }

    pub fn reset_camera(&self) -> Result<()> {
        self.proof_handle().send(GraphWindowCommand::ResetCamera)
    }

    pub fn proof_handle(&self) -> GraphProofHandle {
        GraphProofHandle::new(
            self.parent,
            self.hwnd,
            self.sender.clone(),
            self.proxy.clone(),
            Arc::clone(&self.viewport),
            Arc::clone(&self.queue_metrics),
            ProjectionIdentity {
                generation: self.generation,
                node_count: self.node_count,
                edge_count: self.edge_count,
                kernel: Arc::clone(&self.kernel),
            },
        )
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if self.join.is_none() {
            return Ok(());
        }
        let _ = self.proof_handle().send(GraphWindowCommand::Shutdown);
        let join = self
            .join
            .take()
            .ok_or_else(|| anyhow!("missing graph join handle"))?;
        join.join()
            .map_err(|_| anyhow!("embedded graph thread panicked"))?
    }
}

impl Drop for GraphWindow {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::error!(%error, "embedded graph shutdown failed");
            lifecycle::mark_proof_failed();
        }
    }
}

fn run_graph_window(
    parent: ParentWindowHandle,
    kernel: Arc<PhoenixKernel>,
    receiver: Receiver<GraphWindowCommand>,
    viewport: Arc<ViewportMailbox>,
    queue_metrics: Arc<GraphQueueMetrics>,
    ready_sender: SyncSender<std::result::Result<Ready, String>>,
    ui_notifications: async_channel::Sender<()>,
) -> Result<()> {
    let mut builder = EventLoop::<GraphWake>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build().context("build graph event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = EmbeddedGraphApp::new(
        parent,
        kernel,
        receiver,
        viewport,
        queue_metrics,
        GraphRuntimeSignals {
            ready_sender,
            proxy,
            ui_notifications,
        },
    );
    event_loop
        .run_app(&mut app)
        .context("run embedded graph event loop")
}

struct EmbeddedGraphApp {
    parent: ParentWindowHandle,
    kernel: Arc<PhoenixKernel>,
    receiver: Receiver<GraphWindowCommand>,
    viewport: Arc<ViewportMailbox>,
    queue_metrics: Arc<GraphQueueMetrics>,
    ready_sender: Option<SyncSender<std::result::Result<Ready, String>>>,
    ui_notifications: async_channel::Sender<()>,
    proxy: EventLoopProxy<GraphWake>,
    window: Option<Arc<Window>>,
    renderer: Option<GraphRenderer>,
    logical_pointer: (f32, f32),
    shift_down: bool,
    alt_down: bool,
    last_update: Instant,
    lifetime_registered: bool,
    loaded_generation: Option<GraphGeneration>,
    loaded_manifold: Manifold,
    loaded_graph_view: GraphViewState,
    loaded_selection_revision: u64,
    loaded_review_overlay_revision: u64,
    pending_switch: Option<PendingManifoldSwitch>,
    switch_cpu_samples: FixedSamples,
    switch_present_samples: FixedSamples,
    manifold_switches: u64,
    max_hot_page_bytes: u64,
    latest_switch: Option<ManifoldSwitchReceipt>,
    applied_viewport_revision: u64,
    applied_geometry: ViewportGeometry,
}

impl EmbeddedGraphApp {
    fn new(
        parent: ParentWindowHandle,
        kernel: Arc<PhoenixKernel>,
        receiver: Receiver<GraphWindowCommand>,
        viewport: Arc<ViewportMailbox>,
        queue_metrics: Arc<GraphQueueMetrics>,
        signals: GraphRuntimeSignals,
    ) -> Self {
        Self {
            parent,
            kernel,
            receiver,
            viewport,
            queue_metrics,
            ready_sender: Some(signals.ready_sender),
            ui_notifications: signals.ui_notifications,
            proxy: signals.proxy,
            window: None,
            renderer: None,
            logical_pointer: (0.0, 0.0),
            shift_down: false,
            alt_down: false,
            last_update: Instant::now(),
            lifetime_registered: false,
            loaded_generation: None,
            loaded_manifold: Manifold::Hybrid,
            loaded_graph_view: GraphViewState::default(),
            loaded_selection_revision: 0,
            loaded_review_overlay_revision: 0,
            pending_switch: None,
            switch_cpu_samples: FixedSamples::new(),
            switch_present_samples: FixedSamples::new(),
            manifold_switches: 0,
            max_hot_page_bytes: 0,
            latest_switch: None,
            applied_viewport_revision: 0,
            applied_geometry: ViewportGeometry::hidden(),
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let kernel_snapshot = self
            .kernel
            .snapshot()
            .context("read initial resident scene")?;
        let scene = kernel_snapshot.resident_scene.ok_or_else(|| {
            anyhow!("[PHX_SCENE_MISSING] kernel has no resident graph generation for the renderer")
        })?;
        let active = scene
            .activate_manifold(kernel_snapshot.graph_view.manifold)
            .context("open initial resident manifold")?;
        let attributes = Window::default_attributes()
            .with_title("Phoenix Graph / Embedded Native Viewport")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(1, 1))
            .with_position(PhysicalPosition::new(0, 0));
        // SAFETY: `parent` was captured from the live GPUI window that owns this
        // graph host. PhoenixShell drops the child graph before GPUI destroys
        // that parent, so the Win32 handle remains valid for the child lifetime.
        let attributes = unsafe { attributes.with_parent_window(Some(self.parent.raw())) };
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create embedded child graph window")?,
        );
        let size = window.inner_size();
        let hwnd = prepare_child_window(window.as_ref())?;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: graph_render_wgpu::native_backends(),
            ..Default::default()
        });
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("create embedded graph surface")?;
        let mut renderer = pollster::block_on(GraphRenderer::new(
            &instance,
            surface,
            size.width,
            size.height,
            window.scale_factor() as f32,
        ))
        .context("initialize embedded graph renderer")?;
        renderer
            .set_archive_scene_bound(
                GraphRevision(scene.generation().0),
                scene.archive_identity().cohort_hash,
                &active.pages,
            )
            .context("project initial resident scene")?;
        renderer
            .set_prepared_geometry(active.guides, active.prepared_paths)
            .context("install initial prepared guide/path pages")?;
        if let Some(index) = kernel_snapshot.scene_product_index.as_ref() {
            if scene
                .archive()
                .has_page(PageKey::shared(PageKind::LabelPriority))
            {
                let priorities = scene
                    .archive()
                    .typed_page(PageKey::shared(PageKind::LabelPriority))
                    .context("open resident label-priority page")?;
                renderer
                    .set_product_index_shared(Arc::clone(index), priorities)
                    .context("install initial scene product index")?;
            } else {
                tracing::warn!(
                    generation = scene.generation().0,
                    code = "PHX_VISUAL_PAGE_MIGRATION_REQUIRED",
                    "resident pre-Cut-5 archive is unlabelled until the next native rebuild"
                );
                renderer
                    .set_product_index(index)
                    .context("install pre-Cut-5 product index without labels")?;
            }
            renderer
                .apply_review_overrides(index, &kernel_snapshot.graph_review_overlay.entries)
                .context("install initial decision-ledger review overlay")?;
        }
        renderer
            .set_graph_view(kernel_snapshot.graph_view)
            .context("install initial native graph view")?;
        renderer
            .set_external_selection_pair(
                kernel_snapshot.graph_selection.node_id.map(NodeId),
                kernel_snapshot
                    .graph_selection
                    .secondary_node_id
                    .map(NodeId),
                matches!(
                    kernel_snapshot.graph_selection.origin,
                    GraphSelectionOrigin::Atlas | GraphSelectionOrigin::AtlasCandidate
                ),
            )
            .context("install initial graph selection")?;
        let projected_generation = renderer
            .revision()
            .map(|revision| GraphGeneration(revision.0))
            .ok_or_else(|| anyhow!("renderer did not retain the resident generation"))?;
        if projected_generation != scene.generation() {
            return Err(anyhow!(
                "renderer generation {} differs from resident generation {}",
                projected_generation.0,
                scene.generation().0
            ));
        }
        self.loaded_generation = Some(scene.generation());
        self.loaded_manifold = kernel_snapshot.graph_view.manifold;
        self.loaded_graph_view = kernel_snapshot.graph_view;
        self.loaded_selection_revision = kernel_snapshot.graph_selection.revision;
        self.loaded_review_overlay_revision = kernel_snapshot.graph_review_overlay.revision;
        self.max_hot_page_bytes = active.hot_pages.byte_len;
        self.renderer = Some(renderer);
        self.window = Some(Arc::clone(&window));
        self.lifetime_registered = true;
        lifecycle::graph_window_created();
        window.request_redraw();
        if let Some(sender) = self.ready_sender.take() {
            let renderer = self
                .renderer
                .as_ref()
                .ok_or_else(|| anyhow!("embedded graph renderer vanished during startup"))?;
            let _ = sender.send(Ok(Ready {
                hwnd: hwnd.0 as isize,
                proxy: self.proxy.clone(),
                node_count: renderer.scene_state().node_count(),
                edge_count: renderer.scene_state().edge_count(),
                generation: projected_generation,
            }));
        }
        Ok(())
    }

    fn send_input(&mut self, input: GraphInput) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Err(error) = renderer.handle_input(input) {
            tracing::error!(%error, "embedded graph input rejected");
            lifecycle::mark_proof_failed();
        }
    }

    fn recover_renderer(&mut self, window: &Arc<Window>) -> Result<()> {
        let kernel_snapshot = self
            .kernel
            .snapshot()
            .context("read resident scene for renderer recovery")?;
        let scene = kernel_snapshot.resident_scene.ok_or_else(|| {
            anyhow!("[PHX_SCENE_MISSING] renderer recovery has no resident generation")
        })?;
        let active = scene
            .activate_manifold(kernel_snapshot.graph_view.manifold)
            .context("open resident manifold for renderer recovery")?;
        let size = window.inner_size();
        // A Win32 HWND may have only one configured wgpu surface at a time.
        // Release the old renderer, surface, device, and queue before creating
        // their replacements against the same child window.
        drop(self.renderer.take());
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: graph_render_wgpu::native_backends(),
            ..Default::default()
        });
        let surface = instance
            .create_surface(Arc::clone(window))
            .context("recreate embedded graph surface")?;
        let mut renderer = pollster::block_on(GraphRenderer::new(
            &instance,
            surface,
            size.width.max(1),
            size.height.max(1),
            window.scale_factor() as f32,
        ))
        .context("recreate embedded graph renderer and device")?;
        renderer
            .set_archive_scene_bound(
                GraphRevision(scene.generation().0),
                scene.archive_identity().cohort_hash,
                &active.pages,
            )
            .context("restore resident scene after renderer recovery")?;
        renderer
            .set_prepared_geometry(active.guides, active.prepared_paths)
            .context("restore prepared geometry after renderer recovery")?;
        if let Some(index) = kernel_snapshot.scene_product_index.as_ref() {
            if scene
                .archive()
                .has_page(PageKey::shared(PageKind::LabelPriority))
            {
                let priorities = scene
                    .archive()
                    .typed_page(PageKey::shared(PageKind::LabelPriority))
                    .context("open label priorities after renderer recovery")?;
                renderer
                    .set_product_index_shared(Arc::clone(index), priorities)
                    .context("restore product index after renderer recovery")?;
            } else {
                renderer
                    .set_product_index(index)
                    .context("restore pre-label product index after renderer recovery")?;
            }
            renderer
                .apply_review_overrides(index, &kernel_snapshot.graph_review_overlay.entries)
                .context("restore review overlay after renderer recovery")?;
        }
        renderer
            .set_graph_view(kernel_snapshot.graph_view)
            .context("restore graph view after renderer recovery")?;
        renderer
            .set_external_selection_pair(
                kernel_snapshot.graph_selection.node_id.map(NodeId),
                kernel_snapshot
                    .graph_selection
                    .secondary_node_id
                    .map(NodeId),
                matches!(
                    kernel_snapshot.graph_selection.origin,
                    GraphSelectionOrigin::Atlas | GraphSelectionOrigin::AtlasCandidate
                ),
            )
            .context("restore graph selection after renderer recovery")?;
        let projected = renderer
            .revision()
            .map(|revision| GraphGeneration(revision.0))
            .ok_or_else(|| anyhow!("recovered renderer has no resident generation"))?;
        if projected != scene.generation() {
            return Err(anyhow!(
                "recovered renderer generation {} differs from authority {}",
                projected.0,
                scene.generation().0
            ));
        }
        self.renderer = Some(renderer);
        self.loaded_generation = Some(projected);
        self.loaded_manifold = kernel_snapshot.graph_view.manifold;
        self.loaded_graph_view = kernel_snapshot.graph_view;
        self.loaded_selection_revision = kernel_snapshot.graph_selection.revision;
        self.loaded_review_overlay_revision = kernel_snapshot.graph_review_overlay.revision;
        self.max_hot_page_bytes = self.max_hot_page_bytes.max(active.hot_pages.byte_len);
        lifecycle::graph_resources_recreated();
        window.request_redraw();
        Ok(())
    }

    fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(command) = self.receiver.try_recv() {
            self.queue_metrics.received();
            let Some(window) = self.window.as_ref().cloned() else {
                continue;
            };
            match command {
                GraphWindowCommand::SyncKernelState => {}
                GraphWindowCommand::FitGraph => self.send_input(GraphInput::FitGraph),
                GraphWindowCommand::ResetCamera => self.send_input(GraphInput::ResetCamera),
                GraphWindowCommand::ResetSwitchTelemetry => {
                    self.switch_cpu_samples = FixedSamples::new();
                    self.switch_present_samples = FixedSamples::new();
                    self.manifold_switches = 0;
                    self.max_hot_page_bytes = 0;
                    self.latest_switch = None;
                    self.pending_switch = None;
                }
                GraphWindowCommand::StressInteraction(sender) => {
                    let result = self
                        .stress_interaction()
                        .map_err(|error| format!("{error:#}"));
                    let _ = sender.send(result);
                    window.request_redraw();
                }
                GraphWindowCommand::PickProbePoint(sender) => {
                    let point = self
                        .renderer
                        .as_ref()
                        .and_then(GraphRenderer::visible_node_pick_point);
                    let _ = sender.send(point);
                }
                GraphWindowCommand::RecoverRenderer(sender) => {
                    let result = self
                        .recover_renderer(&window)
                        .map_err(|error| format!("{error:#}"));
                    let _ = sender.send(result);
                }
                GraphWindowCommand::ProbeFocus(sender) => {
                    let _ = sender.send(focus_child(window.as_ref()).unwrap_or(false));
                }
                GraphWindowCommand::Barrier(sender) => {
                    let _ = sender.send(());
                }
                GraphWindowCommand::Telemetry(sender) => {
                    if let Some(renderer) = self.renderer.as_ref() {
                        let stats = renderer.gpu_allocation_stats();
                        let _ = sender.send(GraphGpuTelemetry {
                            node_capacity: stats.node_capacity,
                            edge_capacity: stats.edge_capacity,
                            node_buffer_generation: stats.node_buffer_generation,
                            edge_buffer_generation: stats.edge_buffer_generation,
                            node_product_capacity: stats.node_product_capacity,
                            edge_product_capacity: stats.edge_product_capacity,
                            node_product_buffer_generation: stats.node_product_buffer_generation,
                            edge_product_buffer_generation: stats.edge_product_buffer_generation,
                            product_index_bound: renderer
                                .graph_view()
                                .authority
                                .product_index_hash()
                                .is_some(),
                            lens_uniform_writes: renderer.lens_uniform_writes(),
                            allocated_bytes: stats.allocated_bytes,
                            active_manifold: self.loaded_manifold,
                            manifold_switches: self.manifold_switches,
                            switch_cpu_p95_us: self.switch_cpu_samples.p95(),
                            switch_present_p95_us: self.switch_present_samples.p95(),
                            max_hot_page_bytes: self.max_hot_page_bytes,
                            latest_switch: self.latest_switch,
                        });
                    }
                }
                GraphWindowCommand::Shutdown => {
                    event_loop.exit();
                    return;
                }
            }
        }
    }

    fn process_viewport(&mut self) -> Result<()> {
        while let Some(stamped) = self.viewport.next_after(self.applied_viewport_revision)? {
            self.apply_viewport(stamped.geometry)?;
            self.applied_viewport_revision = stamped.revision;
        }
        Ok(())
    }

    fn apply_viewport(&mut self, geometry: ViewportGeometry) -> Result<()> {
        let window = self
            .window
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("embedded graph window is unavailable"))?;
        let hwnd = hwnd_for_window(window.as_ref())?;
        let should_show = geometry.visible && geometry.width > 0 && geometry.height > 0;
        if !should_show {
            if self.applied_geometry.visible {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                lifecycle::visibility_transition();
            }
            self.applied_geometry = geometry;
            return Ok(());
        }
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                geometry.x,
                geometry.y,
                geometry.width as i32,
                geometry.height as i32,
                SWP_NOACTIVATE,
            )
            .context("position embedded graph child")?;
        }
        self.send_input(GraphInput::Resize {
            width: geometry.width,
            height: geometry.height,
            scale_factor: geometry.scale_factor,
        });
        if !self.applied_geometry.visible {
            self.send_input(GraphInput::FitGraph);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
            lifecycle::visibility_transition();
        }
        self.applied_geometry = geometry;
        window.request_redraw();
        Ok(())
    }

    fn cleanup(&mut self) {
        self.renderer.take();
        self.window.take();
        if self.lifetime_registered {
            lifecycle::graph_window_dropped();
            self.lifetime_registered = false;
        }
    }
}

fn focus_child(window: &Window) -> Result<bool> {
    let hwnd = hwnd_for_window(window)?;
    unsafe {
        let _ = SetFocus(Some(hwnd));
        Ok(GetFocus() == hwnd)
    }
}

impl ApplicationHandler<GraphWake> for EmbeddedGraphApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.initialize(event_loop) {
            if let Some(sender) = self.ready_sender.take() {
                let _ = sender.send(Err(format!("{error:#}")));
            }
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: GraphWake) {
        if let Err(error) = self.process_viewport() {
            tracing::error!(%error, "embedded viewport synchronization failed");
            lifecycle::mark_proof_failed();
            event_loop.exit();
            return;
        }
        self.process_commands(event_loop);
        if let Err(error) = self.sync_resident_scene() {
            tracing::error!(%error, "resident scene synchronization failed");
            lifecycle::mark_proof_failed();
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
            return;
        }
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        match event {
            WindowEvent::Focused(true) => lifecycle::focus_event(),
            WindowEvent::Resized(size) => {
                lifecycle::resize_event();
                self.send_input(GraphInput::Resize {
                    width: size.width,
                    height: size.height,
                    scale_factor: self.applied_geometry.scale_factor,
                });
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                lifecycle::dpi_event();
                let size = window.inner_size();
                self.send_input(GraphInput::Resize {
                    width: size.width,
                    height: size.height,
                    scale_factor: scale_factor as f32,
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                lifecycle::pointer_event();
                let point = position.to_logical::<f32>(window.scale_factor());
                self.logical_pointer = (point.x, point.y);
                self.send_input(GraphInput::PointerMoved {
                    x: point.x,
                    y: point.y,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                lifecycle::pointer_event();
                let button = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Middle => PointerButton::Middle,
                    MouseButton::Right => PointerButton::Right,
                    _ => return,
                };
                let input = match state {
                    ElementState::Pressed => GraphInput::PointerPressed {
                        x: self.logical_pointer.0,
                        y: self.logical_pointer.1,
                        button,
                        shift: self.shift_down,
                        alt: self.alt_down,
                    },
                    ElementState::Released => GraphInput::PointerReleased {
                        x: self.logical_pointer.0,
                        y: self.logical_pointer.1,
                        button,
                    },
                };
                self.send_input(input);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                lifecycle::wheel_event();
                let (delta_x, delta_y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(point) => {
                        (point.x as f32 / 120.0, point.y as f32 / 120.0)
                    }
                };
                self.send_input(GraphInput::Wheel { delta_x, delta_y });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift_down = modifiers.state().shift_key();
                self.alt_down = modifiers.state().alt_key();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.update(now.saturating_duration_since(self.last_update));
                    for event in renderer.drain_events() {
                        match event {
                            GraphEvent::SelectionChanged { node, .. } => {
                                let command = node.map_or(GraphSelectionCommand::Clear, |node| {
                                    GraphSelectionCommand::GraphNode(node.0)
                                });
                                if let Err(error) = self
                                    .kernel
                                    .execute(KernelCommand::SetGraphSelection(command))
                                {
                                    tracing::error!(%error, "renderer selection was rejected");
                                } else {
                                    let _ = self.ui_notifications.try_send(());
                                }
                            }
                            GraphEvent::HoverChanged(node) => {
                                lifecycle::hover_pick_event();
                                tracing::trace!(
                                    node_id = node.map(|node| node.0),
                                    "renderer hover changed"
                                );
                            }
                            GraphEvent::CameraChanged(_) => {}
                        }
                    }
                    match renderer.render() {
                        Ok(metrics) if metrics.frame_number != 0 => {
                            lifecycle::frame_presented();
                            if let Some(mut pending) = self.pending_switch.take() {
                                pending.receipt.first_present_us =
                                    pending.started.elapsed().as_micros();
                                self.switch_present_samples
                                    .push(pending.receipt.first_present_us);
                                tracing::info!(
                                    contract = pending.receipt.contract,
                                    generation = pending.receipt.generation.0,
                                    from = ?pending.receipt.from,
                                    to = ?pending.receipt.to,
                                    nodes = pending.receipt.node_count,
                                    positions_bytes = pending.receipt.positions_bytes,
                                    hot_page_bytes = pending.receipt.hot_page_bytes,
                                    page_verifications = pending.receipt.page_verifications,
                                    cpu_us = pending.receipt.cpu_us,
                                    first_present_us = pending.receipt.first_present_us,
                                    "resident manifold switch presented"
                                );
                                self.latest_switch = Some(pending.receipt);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::error!(%error, "embedded graph render failed");
                            lifecycle::mark_proof_failed();
                            event_loop.exit();
                        }
                    }
                }
                self.last_update = now;
            }
            _ => {}
        }
        if self
            .renderer
            .as_ref()
            .is_some_and(GraphRenderer::needs_redraw)
        {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.process_viewport() {
            tracing::error!(%error, "embedded viewport synchronization failed");
            lifecycle::mark_proof_failed();
            event_loop.exit();
            return;
        }
        if let Err(error) = self.sync_resident_scene() {
            tracing::error!(%error, "resident scene synchronization failed");
            lifecycle::mark_proof_failed();
            event_loop.exit();
            return;
        }
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            if renderer.needs_redraw() {
                window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.cleanup();
    }
}

impl Drop for EmbeddedGraphApp {
    fn drop(&mut self) {
        self.cleanup();
    }
}
