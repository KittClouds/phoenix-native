use crate::fixtures::{self, FixtureTopology};
use graph_model::{GraphRevision, NodeId};
use graph_render_wgpu::{GraphEvent, GraphInput, GraphRenderer, PointerButton};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub struct AppConfig {
    pub topology: FixtureTopology,
    pub nodes: usize,
    pub edges: usize,
    pub seed: u64,
    pub json_path: Option<std::path::PathBuf>,
}

pub struct GraphDemoApp {
    config: AppConfig,
    window: Option<Arc<Window>>,
    renderer: Option<GraphRenderer>,
    rng: ChaCha8Rng,
    animating: bool,
    shift_down: bool,
    logical_pointer: (f32, f32),
    last_update: Instant,
    title_window_start: Instant,
    title_frames: u64,
    selected_node: Option<NodeId>,
}

impl GraphDemoApp {
    pub fn new(config: AppConfig) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(config.seed),
            config,
            window: None,
            renderer: None,
            animating: false,
            shift_down: false,
            logical_pointer: (0.0, 0.0),
            last_update: Instant::now(),
            title_window_start: Instant::now(),
            title_frames: 0,
            selected_node: None,
        }
    }

    fn apply_update(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let Some(revision) = renderer.revision().and_then(GraphRevision::checked_next) else {
            tracing::error!("graph revision exhausted; stopping animation");
            self.animating = false;
            return;
        };
        let diff = fixtures::animated_diff(renderer.scene_state(), revision, 0.01, &mut self.rng);
        match renderer.apply_diff(diff) {
            Ok(metrics) => tracing::debug!(
                updated_nodes = metrics.nodes_updated,
                upload_ranges = metrics.buffer_ranges_updated,
                elapsed_us = metrics.diff_apply_duration_us,
                "demo incremental update"
            ),
            Err(error) => {
                tracing::error!(%error, "incremental update rejected");
                self.animating = false;
            }
        }
    }

    fn log_events(&mut self) -> bool {
        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        let events: Vec<_> = renderer.drain_events().collect();
        let mut selection_changed = false;
        for event in events {
            match event {
                GraphEvent::HoverChanged(node) => tracing::trace!(?node, "hover changed"),
                GraphEvent::SelectionChanged { sequence, node } => {
                    self.selected_node = node;
                    selection_changed = true;
                    tracing::info!(sequence, ?node, "selection changed");
                }
                GraphEvent::CameraChanged(_) => {}
            }
        }
        selection_changed
    }
}

impl ApplicationHandler for GraphDemoApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Phoenix Native Graph")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                tracing::error!(%error, "window creation failed");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: graph_render_wgpu::native_backends(),
            ..Default::default()
        });
        let surface = match instance.create_surface(Arc::clone(&window)) {
            Ok(surface) => surface,
            Err(error) => {
                tracing::error!(%error, "surface creation failed");
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match pollster::block_on(GraphRenderer::new(
            &instance,
            surface,
            size.width,
            size.height,
            scale_factor,
        )) {
            Ok(renderer) => renderer,
            Err(error) => {
                tracing::error!(%error, "renderer initialization failed");
                event_loop.exit();
                return;
            }
        };
        let snapshot = match self.config.json_path.as_deref() {
            Some(path) => fixtures::load_json(path),
            None => Ok(fixtures::generate(
                self.config.topology,
                self.config.nodes,
                self.config.edges,
                self.config.seed,
            )),
        };
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::error!(%error, "graph fixture rejected");
                event_loop.exit();
                return;
            }
        };
        if let Err(error) = renderer.set_snapshot(&snapshot) {
            tracing::error!(%error, "initial graph rejected");
            event_loop.exit();
            return;
        }
        tracing::info!(
            "controls: click select; drag orbit; shift-drag/middle-drag pan; wheel zoom; F fit; R reset; U update 1%; Space animate"
        );
        self.renderer = Some(renderer);
        self.set_idle_title(&window);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
            return;
        }
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };

        match event {
            WindowEvent::Resized(size) => {
                self.send_input(GraphInput::Resize {
                    width: size.width,
                    height: size.height,
                    scale_factor: window.scale_factor() as f32,
                });
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = window.inner_size();
                self.send_input(GraphInput::Resize {
                    width: size.width,
                    height: size.height,
                    scale_factor: scale_factor as f32,
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f32>(window.scale_factor());
                self.logical_pointer = (logical.x, logical.y);
                self.send_input(GraphInput::PointerMoved {
                    x: logical.x,
                    y: logical.y,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
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
                let (delta_x, delta_y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32 / 120.0, position.y as f32 / 120.0)
                    }
                };
                self.send_input(GraphInput::Wheel { delta_x, delta_y });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift_down = modifiers.state().shift_key();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.logical_key.as_ref() {
                    Key::Character("f" | "F") => self.send_input(GraphInput::FitGraph),
                    Key::Character("r" | "R") => self.send_input(GraphInput::ResetCamera),
                    Key::Character("u" | "U") => self.apply_update(),
                    Key::Named(NamedKey::Space) | Key::Character(" ") => {
                        self.animating = !self.animating;
                        self.title_window_start = Instant::now();
                        self.title_frames = 0;
                        if !self.animating {
                            self.set_idle_title(&window);
                        }
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.send_input(GraphInput::ClearSelection);
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(self.last_update);
                self.last_update = now;
                if self.animating {
                    self.apply_update();
                }
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.update(elapsed);
                    match renderer.render() {
                        Ok(_) => self.title_frames = self.title_frames.wrapping_add(1),
                        Err(error) => {
                            tracing::error!(%error, "render failed");
                            event_loop.exit();
                        }
                    }
                }
                if self.log_events() && !self.animating {
                    self.set_idle_title(&window);
                }
                self.update_title(&window, now);
            }
            _ => {}
        }
        if self
            .renderer
            .as_ref()
            .is_some_and(GraphRenderer::needs_redraw)
            || self.animating
        {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            if renderer.needs_redraw() || self.animating {
                window.request_redraw();
            }
        }
    }
}

impl GraphDemoApp {
    fn send_input(&mut self, input: GraphInput) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match renderer.handle_input(input) {
            Ok(Some(GraphEvent::SelectionChanged { sequence, node })) => {
                tracing::info!(sequence, ?node, "selection changed")
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "input rejected"),
        }
    }

    fn update_title(&mut self, window: &Window, now: Instant) {
        if !self.animating {
            return;
        }
        let elapsed = now.saturating_duration_since(self.title_window_start);
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let fps = self.title_frames as f64 / elapsed.as_secs_f64();
        if let Some(renderer) = &self.renderer {
            let selection = self
                .selected_node
                .map_or_else(|| "none".to_owned(), |node| node.0.to_string());
            window.set_title(&format!(
                "Phoenix Native Graph — {} nodes / {} edges — {fps:.0} FPS — selected {selection}",
                renderer.scene_state().node_count(),
                renderer.scene_state().edge_count()
            ));
        }
        self.title_window_start = now;
        self.title_frames = 0;
    }

    fn set_idle_title(&self, window: &Window) {
        if let Some(renderer) = &self.renderer {
            let selection = self
                .selected_node
                .map_or_else(|| "none".to_owned(), |node| node.0.to_string());
            window.set_title(&format!(
                "Phoenix Native Graph — {} nodes / {} edges — event-driven idle — selected {selection}",
                renderer.scene_state().node_count(),
                renderer.scene_state().edge_count()
            ));
        }
    }
}
