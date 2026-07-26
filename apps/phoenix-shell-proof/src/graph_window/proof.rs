use super::{
    GraphGpuTelemetry, GraphQueueMetrics, GraphWake, GraphWindowCommand, ParentWindowHandle,
    ViewportGeometry, ViewportMailbox,
};
use crate::lifecycle;
use anyhow::{anyhow, Context, Result};
use phoenix_app_core::{KernelCommand, PhoenixKernel};
use phoenix_scene_contract::{GraphGeneration, Manifold};
use serde::Serialize;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetParent, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    ShowWindow, GWL_STYLE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, WS_CHILD,
};
use winit::event_loop::EventLoopProxy;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct EmbeddedHostProof {
    pub resident_generation: u64,
    pub renderer_nodes: usize,
    pub renderer_edges: usize,
    pub parented_child: bool,
    pub parent_clips_child: bool,
    pub exact_viewport_bounds: bool,
    pub dpi_matches_parent: bool,
    pub stable_hwnd: bool,
    pub stable_device: bool,
    pub stable_gpu_allocations: bool,
    pub stable_graph_slots: bool,
    pub manifold_switches: u64,
    pub switch_cpu_p95_us: u128,
    pub switch_present_p95_us: u128,
    pub max_hot_page_bytes: u64,
    pub visibility_cycles: u32,
    pub resize_cycles: u32,
    pub warm_open_to_first_present_us: u128,
    pub resize_present_p95_us: u128,
    pub resize_present_max_us: u128,
    pub graph_command_queue_high_water: u64,
    pub gpu_before: GraphGpuTelemetry,
    pub gpu_after: GraphGpuTelemetry,
    pub focus: bool,
    pub resize: bool,
    pub minimize_restore: bool,
    pub maximize_restore: bool,
}

#[derive(Clone)]
pub(super) struct ProjectionIdentity {
    pub generation: GraphGeneration,
    pub node_count: usize,
    pub edge_count: usize,
    pub kernel: Arc<PhoenixKernel>,
}

#[derive(Clone)]
pub struct GraphProofHandle {
    parent: ParentWindowHandle,
    hwnd: isize,
    sender: SyncSender<GraphWindowCommand>,
    proxy: EventLoopProxy<GraphWake>,
    viewport: Arc<ViewportMailbox>,
    queue_metrics: Arc<GraphQueueMetrics>,
    projection: ProjectionIdentity,
}

impl GraphProofHandle {
    pub(super) fn new(
        parent: ParentWindowHandle,
        hwnd: isize,
        sender: SyncSender<GraphWindowCommand>,
        proxy: EventLoopProxy<GraphWake>,
        viewport: Arc<ViewportMailbox>,
        queue_metrics: Arc<GraphQueueMetrics>,
        projection: ProjectionIdentity,
    ) -> Self {
        Self {
            parent,
            hwnd,
            sender,
            proxy,
            viewport,
            queue_metrics,
            projection,
        }
    }

    pub fn run(&self) -> Result<EmbeddedHostProof> {
        eprintln!("PHOENIX_EMBEDDED_PROOF_STAGE host_start");
        let _proof_lease = self.viewport.begin_proof()?;
        let original_hwnd = self.hwnd;
        let original = self.viewport.latest()?.geometry;
        if original.width < 2 || original.height < 2 {
            return Err(anyhow!(
                "embedded viewport was not laid out before proof: {}x{}",
                original.width,
                original.height
            ));
        }
        self.set_proof_viewport(original)?;
        self.barrier()?;
        self.wait_for_visibility(true)
            .context("embedded viewport did not become visible before proof")?;
        let before = lifecycle::snapshot();
        let gpu_before = self.telemetry()?;
        for manifold in Manifold::ALL {
            if manifold != gpu_before.active_manifold {
                self.switch_manifold(manifold)?;
            }
        }
        self.send(GraphWindowCommand::ResetSwitchTelemetry)?;
        self.barrier()?;
        let gpu_after_warm = self.telemetry()?;
        for cycle in 0..200 {
            self.switch_manifold(Manifold::ALL[cycle % Manifold::ALL.len()])?;
            if cycle % 50 == 49 {
                eprintln!(
                    "PHOENIX_EMBEDDED_PROOF_STAGE manifold_switches={}",
                    cycle + 1
                );
            }
        }
        let mut hidden = original;
        hidden.visible = false;
        self.set_proof_viewport(hidden)?;
        self.barrier()?;
        let warm_started = Instant::now();
        let frame_before_open = lifecycle::snapshot().frames_presented;
        self.set_proof_viewport(original)?;
        self.barrier()?;
        self.wait_for_present(frame_before_open)?;
        let warm_open_to_first_present_us = warm_started.elapsed().as_micros();

        let mut resize_latencies = Vec::with_capacity(80);
        for cycle in 0..80_u32 {
            let mut geometry = original;
            geometry.width = original.width.saturating_sub(1 + cycle % 37).max(2);
            geometry.height = original.height.saturating_sub(1 + cycle % 29).max(2);
            let before_frame = lifecycle::snapshot().frames_presented;
            let started = Instant::now();
            self.set_proof_viewport(geometry)?;
            self.barrier()?;
            self.wait_for_present(before_frame)?;
            resize_latencies.push(started.elapsed().as_micros());
        }
        resize_latencies.sort_unstable();
        let resize_present_max_us = resize_latencies.last().copied().unwrap_or(0);
        let p95_index = resize_latencies
            .len()
            .saturating_mul(95)
            .div_ceil(100)
            .saturating_sub(1);
        let resize_present_p95_us = resize_latencies.get(p95_index).copied().unwrap_or(0);
        let mut resized = original;
        resized.width -= 1;
        resized.height -= 1;
        let focus = self.focus_and_verify()?;
        let mut completed_cycles = 0;
        for cycle in 0..200 {
            let mut hidden = resized;
            hidden.visible = false;
            self.set_proof_viewport(hidden)?;
            self.barrier()?;
            self.wait_for_visibility(false).with_context(|| {
                format!(
                    "embedded child remained visible during hide cycle {}",
                    cycle + 1
                )
            })?;
            resized.visible = true;
            self.set_proof_viewport(resized)?;
            self.barrier()?;
            self.wait_for_visibility(true).with_context(|| {
                format!(
                    "embedded child remained hidden during show cycle {}",
                    cycle + 1
                )
            })?;
            completed_cycles += 1;
            if cycle % 50 == 49 {
                eprintln!(
                    "PHOENIX_EMBEDDED_PROOF_STAGE visibility_cycles={}",
                    cycle + 1
                );
            }
        }
        self.set_proof_viewport(original)?;
        self.barrier()?;
        let gpu_after = self.telemetry()?;
        unsafe {
            let _ = ShowWindow(self.parent.hwnd(), SW_MINIMIZE);
        }
        thread::sleep(Duration::from_millis(75));
        let minimized = self.parent_is_minimized();
        unsafe {
            let _ = ShowWindow(self.parent.hwnd(), SW_RESTORE);
        }
        thread::sleep(Duration::from_millis(75));
        let restored_after_minimize = self.parent_is_restored();
        unsafe {
            let _ = ShowWindow(self.parent.hwnd(), SW_MAXIMIZE);
        }
        thread::sleep(Duration::from_millis(75));
        let maximized = self.parent_is_maximized();
        unsafe {
            let _ = ShowWindow(self.parent.hwnd(), SW_RESTORE);
        }
        thread::sleep(Duration::from_millis(75));
        let restored_after_maximize = self.parent_is_restored();
        eprintln!("PHOENIX_EMBEDDED_PROOF_STAGE parent_transitions_complete");
        self.set_proof_viewport(original)?;
        self.barrier()?;
        let after = lifecycle::snapshot();
        let proof = EmbeddedHostProof {
            resident_generation: self.projection.generation.0,
            renderer_nodes: self.projection.node_count,
            renderer_edges: self.projection.edge_count,
            parented_child: self.is_parented_child(),
            parent_clips_child: self.parent.clips_children(),
            exact_viewport_bounds: self.matches_geometry(original),
            dpi_matches_parent: self.dpi_matches(original.scale_factor),
            stable_hwnd: self.hwnd == original_hwnd
                && self.is_window()
                && after.graph_window_created == before.graph_window_created
                && after.renderer_created == before.renderer_created
                && after.surface_created == before.surface_created,
            stable_device: after.device_created == before.device_created,
            stable_gpu_allocations: gpu_after_warm.allocated_bytes == gpu_after.allocated_bytes
                && gpu_after_warm.node_buffer_generation == gpu_after.node_buffer_generation
                && gpu_after_warm.edge_buffer_generation == gpu_after.edge_buffer_generation
                && gpu_after_warm.node_product_buffer_generation
                    == gpu_after.node_product_buffer_generation
                && gpu_after_warm.edge_product_buffer_generation
                    == gpu_after.edge_product_buffer_generation,
            stable_graph_slots: gpu_after_warm.node_capacity == gpu_after.node_capacity
                && gpu_after_warm.edge_capacity == gpu_after.edge_capacity
                && gpu_after_warm.node_product_capacity == gpu_after.node_product_capacity
                && gpu_after_warm.edge_product_capacity == gpu_after.edge_product_capacity,
            manifold_switches: gpu_after.manifold_switches,
            switch_cpu_p95_us: gpu_after.switch_cpu_p95_us,
            switch_present_p95_us: gpu_after.switch_present_p95_us,
            max_hot_page_bytes: gpu_after.max_hot_page_bytes,
            visibility_cycles: completed_cycles,
            resize_cycles: resize_latencies.len() as u32,
            warm_open_to_first_present_us,
            resize_present_p95_us,
            resize_present_max_us,
            graph_command_queue_high_water: self.queue_metrics.high_water.load(Ordering::Relaxed),
            gpu_before,
            gpu_after,
            focus,
            resize: after.resize_events > before.resize_events,
            minimize_restore: minimized && restored_after_minimize,
            maximize_restore: maximized && restored_after_maximize,
        };
        eprintln!("PHOENIX_EMBEDDED_PROOF_STAGE host_complete");
        Ok(proof)
    }

    pub(super) fn set_ui_viewport(&self, geometry: ViewportGeometry) -> Result<()> {
        if self.viewport.publish_ui(geometry)? {
            self.proxy
                .send_event(GraphWake::ViewportReady)
                .map_err(|error| anyhow!("graph viewport wake rejected: {error}"))?;
        }
        Ok(())
    }

    fn set_proof_viewport(&self, geometry: ViewportGeometry) -> Result<()> {
        if self.viewport.publish_proof(geometry)? {
            self.proxy
                .send_event(GraphWake::ViewportReady)
                .map_err(|error| anyhow!("graph viewport proof wake rejected: {error}"))?;
        }
        Ok(())
    }

    pub(super) fn hide_viewport(&self) -> Result<()> {
        let mut geometry = self.viewport.latest()?.geometry;
        geometry.visible = false;
        self.set_ui_viewport(geometry)?;
        self.barrier()
    }

    fn focus_and_verify(&self) -> Result<bool> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(GraphWindowCommand::ProbeFocus(sender))?;
        receiver
            .recv_timeout(Duration::from_secs(10))
            .context("embedded graph focus probe timed out")
    }

    fn switch_manifold(&self, manifold: Manifold) -> Result<()> {
        let previous_frame = lifecycle::snapshot().frames_presented;
        self.projection
            .kernel
            .execute(KernelCommand::SetManifold(manifold))
            .with_context(|| format!("set proof manifold to {manifold:?}"))?;
        self.send(GraphWindowCommand::SyncKernelState)?;
        self.barrier()?;
        self.wait_for_present(previous_frame)?;
        let _ = self.projection.kernel.drain_events()?;
        Ok(())
    }

    pub(super) fn send(&self, command: GraphWindowCommand) -> Result<()> {
        self.queue_metrics.begin_send();
        if let Err(error) = self.sender.send(command) {
            self.queue_metrics.cancel_send();
            return Err(anyhow!("graph command rejected: {error}"));
        }
        self.proxy
            .send_event(GraphWake::CommandsReady)
            .map_err(|error| anyhow!("graph wake rejected: {error}"))
    }

    fn barrier(&self) -> Result<()> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(GraphWindowCommand::Barrier(sender))?;
        receiver
            .recv_timeout(Duration::from_secs(10))
            .context("embedded graph barrier timed out")
    }

    fn telemetry(&self) -> Result<GraphGpuTelemetry> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(GraphWindowCommand::Telemetry(sender))?;
        receiver
            .recv_timeout(Duration::from_secs(10))
            .context("embedded graph telemetry query timed out")
    }

    fn wait_for_present(&self, previous: u64) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while lifecycle::snapshot().frames_presented <= previous {
            if Instant::now() >= deadline {
                return Err(anyhow!("embedded graph present timed out"));
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn wait_for_visibility(&self, expected: bool) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(100);
        while self.is_visible() != expected {
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "child visibility did not settle to {expected} within 100ms"
                ));
            }
            thread::yield_now();
        }
        Ok(())
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }

    fn is_window(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd())) }.as_bool()
    }

    fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd()) }.as_bool()
    }

    fn parent_is_minimized(&self) -> bool {
        unsafe { IsIconic(self.parent.hwnd()) }.as_bool()
    }

    fn parent_is_maximized(&self) -> bool {
        unsafe { IsZoomed(self.parent.hwnd()) }.as_bool()
    }

    fn parent_is_restored(&self) -> bool {
        unsafe { IsWindowVisible(self.parent.hwnd()) }.as_bool()
            && !self.parent_is_minimized()
            && !self.parent_is_maximized()
    }

    fn is_parented_child(&self) -> bool {
        let parent = unsafe { GetParent(self.hwnd()) };
        let style = unsafe { GetWindowLongPtrW(self.hwnd(), GWL_STYLE) } as u32;
        parent.is_ok_and(|parent| parent == self.parent.hwnd()) && style & WS_CHILD.0 != 0
    }

    fn matches_geometry(&self, geometry: ViewportGeometry) -> bool {
        if !geometry.visible {
            return !unsafe { IsWindowVisible(self.hwnd()) }.as_bool();
        }
        let mut parent_origin = POINT::default();
        let mut child = RECT::default();
        if !unsafe { ClientToScreen(self.parent.hwnd(), &mut parent_origin) }.as_bool()
            || unsafe { GetWindowRect(self.hwnd(), &mut child) }.is_err()
        {
            return false;
        }
        child.left == parent_origin.x + geometry.x
            && child.top == parent_origin.y + geometry.y
            && child.right - child.left == geometry.width as i32
            && child.bottom - child.top == geometry.height as i32
    }

    fn dpi_matches(&self, scale_factor: f32) -> bool {
        let parent_dpi = unsafe { GetDpiForWindow(self.parent.hwnd()) };
        let child_dpi = unsafe { GetDpiForWindow(self.hwnd()) };
        parent_dpi != 0
            && parent_dpi == child_dpi
            && ((parent_dpi as f32 / 96.0) - scale_factor).abs() <= 0.01
    }
}
