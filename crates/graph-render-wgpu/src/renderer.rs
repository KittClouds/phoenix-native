use crate::buffers::CameraUniform;
use crate::camera::Camera;
use crate::events::PendingEvents;
use crate::gpu_scene::GpuScene;
use crate::interaction::{logical_to_physical, physical_delta_to_logical, PointerState};
use crate::labels::{LabelFocus, LabelLayer};
use crate::path_layer::PreparedPathLayer;
use crate::picking::{PickIntent, PickingPass};
use crate::pipelines::{
    bind_group, create_layouts, create_pipelines, lens_bind_group, RenderLayouts, RenderPipelines,
};
use crate::renderer_support::create_depth_texture;
pub(crate) use crate::renderer_support::{preferred_present_mode, validate_view_authority};
use crate::{
    FrameMetrics, GpuAllocationStats, GpuSceneMetrics, GraphEvent, GraphInput, GraphLensUniform,
    LensUpdateMetrics, PointerButton, PositionSwitchMetrics, ProductInstallMetrics, RenderError,
    ReviewOverlayMetrics, SceneState, SnapshotMetrics,
};
use graph_model::{GraphDiff, GraphRevision, GraphSnapshot};
use phoenix_scene_archive::{LabelPriorityRecord, ManifoldPageSet, PositionRecord};
use phoenix_scene_contract::{GraphReviewOverride, GraphViewState, Manifold};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use std::mem::size_of;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct GraphRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    scale_factor: f32,
    camera: Camera,
    camera_buffer: wgpu::Buffer,
    layouts: RenderLayouts,
    camera_bind_group: wgpu::BindGroup,
    node_bind_group: wgpu::BindGroup,
    edge_bind_group: wgpu::BindGroup,
    lens_uniform_buffer: wgpu::Buffer,
    lens_bind_group: wgpu::BindGroup,
    active_view: GraphViewState,
    loaded_cohort_hash: Option<[u8; 32]>,
    lens_uniform_writes: u64,
    pipelines: RenderPipelines,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    scene: GpuScene,
    prepared_paths: PreparedPathLayer,
    labels: LabelLayer,
    picking: PickingPass,
    pointer: PointerState,
    events: PendingEvents,
    redraw_requested: bool,
    frame_count: u64,
}

impl GraphRenderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<Self, RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::AdapterNotFound)?;
        let info = adapter.get_info();
        tracing::info!(
            adapter = info.name,
            backend = ?info.backend,
            device_type = ?info.device_type,
            "selected graph GPU"
        );
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("graph renderer device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;
        Self::on_device(
            &adapter,
            Arc::new(device),
            Arc::new(queue),
            surface,
            width,
            height,
            scale_factor,
        )
    }

    pub fn on_device(
        adapter: &wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<Self, RenderError> {
        let width = width.max(1);
        let height = height.max(1);
        let capabilities = surface.get_capabilities(adapter);
        let surface_format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RenderError::SurfaceCapabilitiesUnavailable)?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .ok_or(RenderError::SurfaceCapabilitiesUnavailable)?;
        let present_mode = preferred_present_mode(&capabilities.present_modes)
            .ok_or(RenderError::SurfaceCapabilitiesUnavailable)?;
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &surface_config);

        let camera = Camera::new(width as f32, height as f32);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera.uniform()));
        let layouts = create_layouts(&device);
        let camera_bind_group = bind_group(
            &device,
            "graph camera binding",
            &layouts.camera,
            &camera_buffer,
        );
        let scene = GpuScene::new(&device)?;
        let node_bind_group = bind_group(
            &device,
            "graph node binding",
            &layouts.nodes,
            &scene.node_buffer.buffer,
        );
        let edge_bind_group = bind_group(
            &device,
            "graph edge binding",
            &layouts.edges,
            &scene.edge_buffer.buffer,
        );
        let lens_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph lens uniform"),
            size: size_of::<GraphLensUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &lens_uniform_buffer,
            0,
            bytemuck::bytes_of(&GraphLensUniform::UNFILTERED),
        );
        let lens_bind_group = lens_bind_group(
            &device,
            &layouts.lens,
            &lens_uniform_buffer,
            &scene.node_product_buffer.buffer,
            &scene.edge_product_buffer.buffer,
        );
        let (pipelines, picking_shader) =
            create_pipelines(&device, &layouts, surface_config.format);
        let picking = PickingPass::new(
            &device,
            &layouts.camera,
            &layouts.nodes,
            &layouts.lens,
            width,
            height,
            &picking_shader,
        );
        let (depth_texture, depth_view) = create_depth_texture(&device, width, height);
        let labels = LabelLayer::new(&device, &queue, surface_config.format);
        let prepared_paths = PreparedPathLayer::new(
            &device,
            &layouts.camera,
            &layouts.lens,
            &layouts.edges,
            surface_config.format,
        )?;

        tracing::info!(
            format = ?surface_config.format,
            present_mode = ?surface_config.present_mode,
            max_buffer_size = device.limits().max_buffer_size,
            max_texture_dimension_2d = device.limits().max_texture_dimension_2d,
            "graph renderer initialized"
        );
        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            width,
            height,
            scale_factor: scale_factor.max(0.01),
            camera,
            camera_buffer,
            layouts,
            camera_bind_group,
            node_bind_group,
            edge_bind_group,
            lens_uniform_buffer,
            lens_bind_group,
            active_view: GraphViewState::default(),
            loaded_cohort_hash: None,
            lens_uniform_writes: 1,
            pipelines,
            depth_texture,
            depth_view,
            scene,
            prepared_paths,
            labels,
            picking,
            pointer: PointerState::default(),
            events: PendingEvents::new(),
            redraw_requested: true,
            frame_count: 0,
        })
    }

    pub fn set_snapshot(
        &mut self,
        snapshot: &GraphSnapshot,
    ) -> Result<SnapshotMetrics, RenderError> {
        let span = tracing::info_span!(
            "graph_snapshot",
            revision = snapshot.revision.0,
            nodes = snapshot.nodes.len(),
            edges = snapshot.edges.len()
        );
        let _guard = span.enter();
        let metrics = self
            .scene
            .set_snapshot(snapshot, &self.device, &self.queue)?;
        if metrics.bindings_changed {
            self.refresh_scene_bindings();
        }
        self.fit_active_graph();
        self.write_camera();
        self.labels.clear();
        self.prepared_paths.clear();
        self.redraw_requested = true;
        tracing::info!(
            nodes = metrics.node_count,
            edges = metrics.edge_count,
            elapsed_us = metrics.elapsed_us,
            upload_ranges = metrics.upload_ranges,
            "graph snapshot resident"
        );
        Ok(metrics)
    }

    pub fn set_archive_scene(
        &mut self,
        revision: GraphRevision,
        pages: &ManifoldPageSet<'_>,
    ) -> Result<SnapshotMetrics, RenderError> {
        self.set_archive_scene_internal(revision, None, pages)
    }

    pub fn set_archive_scene_bound(
        &mut self,
        revision: GraphRevision,
        cohort_hash: [u8; 32],
        pages: &ManifoldPageSet<'_>,
    ) -> Result<SnapshotMetrics, RenderError> {
        self.set_archive_scene_internal(revision, Some(cohort_hash), pages)
    }

    fn set_archive_scene_internal(
        &mut self,
        revision: GraphRevision,
        cohort_hash: Option<[u8; 32]>,
        pages: &ManifoldPageSet<'_>,
    ) -> Result<SnapshotMetrics, RenderError> {
        let span = tracing::info_span!(
            "packed_archive_scene",
            revision = revision.0,
            nodes = pages.identities.len(),
            edges = pages.edges.len()
        );
        let _guard = span.enter();
        let metrics = self
            .scene
            .set_archive_scene(revision, pages, &self.device, &self.queue)?;
        if metrics.bindings_changed {
            self.refresh_scene_bindings();
        }
        self.loaded_cohort_hash = cohort_hash;
        self.active_view = GraphViewState::default();
        self.write_lens_uniform(GraphLensUniform::UNFILTERED);
        self.fit_active_graph();
        self.write_camera();
        self.labels.clear();
        self.prepared_paths.clear();
        self.redraw_requested = true;
        tracing::info!(
            nodes = metrics.node_count,
            edges = metrics.edge_count,
            elapsed_us = metrics.elapsed_us,
            "packed archive scene resident"
        );
        Ok(metrics)
    }

    pub fn set_product_index(
        &mut self,
        index: &PhoenixSceneProductIndexV1,
    ) -> Result<ProductInstallMetrics, RenderError> {
        let metrics = self
            .scene
            .set_product_index(index, &self.device, &self.queue)?;
        if metrics.bindings_changed {
            self.refresh_lens_binding();
        }
        self.redraw_requested = true;
        Ok(metrics)
    }

    pub fn set_product_index_shared(
        &mut self,
        index: Arc<PhoenixSceneProductIndexV1>,
        priorities: &[LabelPriorityRecord],
    ) -> Result<ProductInstallMetrics, RenderError> {
        let metrics = self.set_product_index(&index)?;
        self.labels.install(index, priorities);
        self.redraw_requested = true;
        Ok(metrics)
    }

    pub fn set_graph_view(
        &mut self,
        view: GraphViewState,
    ) -> Result<LensUpdateMetrics, RenderError> {
        let index_hash = validate_view_authority(
            view,
            self.scene.revision(),
            self.loaded_cohort_hash,
            self.scene.bound_product_hash(),
        )?;
        let before = self.scene.allocation_stats();
        self.write_lens_uniform(GraphLensUniform::from_view(view, index_hash.is_some()));
        self.active_view = view;
        self.labels.mark_dirty();
        self.redraw_requested = true;
        let after = self.scene.allocation_stats();
        Ok(LensUpdateMetrics {
            bytes_uploaded: size_of::<GraphLensUniform>(),
            uniform_writes: self.lens_uniform_writes,
            topology_buffer_generation_before: before
                .node_buffer_generation
                .wrapping_add(before.edge_buffer_generation),
            topology_buffer_generation_after: after
                .node_buffer_generation
                .wrapping_add(after.edge_buffer_generation),
        })
    }

    pub fn switch_archive_positions(
        &mut self,
        positions: &[PositionRecord],
    ) -> Result<PositionSwitchMetrics, RenderError> {
        let metrics = self
            .scene
            .switch_archive_positions(positions, &self.queue)?;
        self.labels.mark_dirty();
        self.redraw_requested = true;
        tracing::debug!(
            nodes = metrics.node_count,
            bytes_uploaded = metrics.bytes_uploaded,
            elapsed_us = metrics.elapsed_us,
            "packed manifold positions updated in place"
        );
        Ok(metrics)
    }

    pub fn apply_diff(&mut self, diff: GraphDiff) -> Result<GpuSceneMetrics, RenderError> {
        let span = tracing::info_span!("graph_diff", revision = diff.revision.0);
        let _guard = span.enter();
        let metrics = self.scene.apply_diff(diff, &self.device, &self.queue)?;
        if metrics.bindings_changed {
            self.refresh_scene_bindings();
        }
        self.write_camera();
        self.redraw_requested = true;
        tracing::debug!(
            nodes_added = metrics.nodes_added,
            nodes_updated = metrics.nodes_updated,
            nodes_removed = metrics.nodes_removed,
            edges_added = metrics.edges_added,
            edges_updated = metrics.edges_updated,
            edges_removed = metrics.edges_removed,
            upload_ranges = metrics.buffer_ranges_updated,
            elapsed_us = metrics.diff_apply_duration_us,
            "graph diff applied"
        );
        Ok(metrics)
    }

    #[must_use]
    pub fn revision(&self) -> Option<GraphRevision> {
        self.scene.revision()
    }

    #[must_use]
    pub fn scene_state(&self) -> &SceneState {
        self.scene.state()
    }

    #[must_use]
    pub const fn graph_view(&self) -> GraphViewState {
        self.active_view
    }

    fn fit_active_graph(&mut self) {
        self.camera.fit_graph(self.scene.state().nodes());
        if self.active_view.manifold == Manifold::Caps {
            self.camera.orient(0.72, 0.34);
        }
    }

    #[must_use]
    pub const fn lens_uniform_writes(&self) -> u64 {
        self.lens_uniform_writes
    }

    #[must_use]
    pub fn gpu_allocation_stats(&self) -> GpuAllocationStats {
        self.scene.allocation_stats()
    }

    pub fn handle_input(&mut self, input: GraphInput) -> Result<Option<GraphEvent>, RenderError> {
        let event = match input {
            GraphInput::PointerMoved { x, y } => {
                let (x, y) = self.physical_point(x, y);
                let (previous_x, previous_y) = self.pointer.position;
                self.pointer.update_drag(x, y);
                self.pointer.position = (x, y);
                let delta_x = physical_delta_to_logical(x - previous_x, self.scale_factor);
                let delta_y = physical_delta_to_logical(y - previous_y, self.scale_factor);
                if self.pointer.left_down && self.pointer.dragged {
                    if self.pointer.shift_down || self.pointer.alt_down {
                        self.camera.pan(delta_x, delta_y);
                    } else {
                        self.camera.orbit(delta_x, delta_y);
                    }
                    self.camera_changed()
                } else if self.pointer.middle_down || self.pointer.right_down {
                    self.camera.pan(delta_x, delta_y);
                    self.camera_changed()
                } else if !self.pointer.left_down {
                    self.picking.request(x, y, PickIntent::Hover);
                    self.redraw_requested = true;
                    None
                } else {
                    None
                }
            }
            GraphInput::PointerPressed {
                x,
                y,
                button,
                shift,
                alt,
            } => {
                let point = self.physical_point(x, y);
                self.pointer.position = point;
                self.pointer.shift_down = shift;
                self.pointer.alt_down = alt;
                self.pointer.press_origin = Some(point);
                self.pointer.dragged = false;
                match button {
                    PointerButton::Left => self.pointer.left_down = true,
                    PointerButton::Middle => self.pointer.middle_down = true,
                    PointerButton::Right => self.pointer.right_down = true,
                }
                self.redraw_requested = true;
                None
            }
            GraphInput::PointerReleased { x, y, button } => {
                let point = self.physical_point(x, y);
                self.pointer.position = point;
                if button == PointerButton::Left {
                    self.pointer.left_down = false;
                    if !self.pointer.dragged {
                        self.picking.request(point.0, point.1, PickIntent::Select);
                        self.redraw_requested = true;
                    }
                } else if button == PointerButton::Middle {
                    self.pointer.middle_down = false;
                } else if button == PointerButton::Right {
                    self.pointer.right_down = false;
                }
                self.pointer.press_origin = None;
                self.pointer.dragged = false;
                None
            }
            GraphInput::Wheel { delta_y, .. } => {
                self.camera
                    .zoom_at(delta_y, self.pointer.position.0, self.pointer.position.1);
                self.camera_changed()
            }
            GraphInput::Resize {
                width,
                height,
                scale_factor,
            } => {
                self.resize(width, height, scale_factor);
                None
            }
            GraphInput::FitGraph => {
                self.fit_active_graph();
                self.camera_changed()
            }
            GraphInput::ResetCamera => {
                self.camera.reset();
                self.camera_changed()
            }
            GraphInput::ClearSelection => {
                if self.scene.selected_node().is_some()
                    || self.scene.secondary_selected_node().is_some()
                {
                    self.scene
                        .update_highlights(self.scene.hover_node(), None, None, &self.queue);
                    self.redraw_requested = true;
                    Some(self.events.selection(None)?)
                } else {
                    None
                }
            }
        };
        Ok(event)
    }

    pub fn update(&mut self, _elapsed: Duration) {
        if let Some(result) = self.picking.poll(&self.device, &self.scene) {
            match result.intent {
                PickIntent::Hover if self.scene.hover_node() != result.node => {
                    self.scene.update_highlights(
                        result.node,
                        self.scene.selected_node(),
                        self.scene.secondary_selected_node(),
                        &self.queue,
                    );
                    self.events.push_hover(result.node);
                    self.labels.mark_dirty();
                    self.redraw_requested = true;
                }
                PickIntent::Select if self.scene.selected_node() != result.node => {
                    self.scene.update_highlights(
                        self.scene.hover_node(),
                        result.node,
                        None,
                        &self.queue,
                    );
                    if let Err(error) = self.events.push_selection(result.node) {
                        tracing::error!(%error, "selection event rejected");
                    }
                    self.labels.mark_dirty();
                    self.redraw_requested = true;
                }
                _ => {}
            }
        }
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = GraphEvent> + '_ {
        self.events.drain()
    }

    pub fn set_external_selection(
        &mut self,
        node: Option<graph_model::NodeId>,
        focus: bool,
    ) -> Result<(), RenderError> {
        self.set_external_selection_pair(node, None, focus)
    }

    pub fn set_external_selection_pair(
        &mut self,
        primary: Option<graph_model::NodeId>,
        secondary: Option<graph_model::NodeId>,
        focus: bool,
    ) -> Result<(), RenderError> {
        if let Some(node) = primary {
            let slot = self
                .scene
                .state()
                .node_slot(node)
                .ok_or(RenderError::NodeNotFound(node))?;
            if focus {
                let position = self
                    .scene
                    .state()
                    .node_at_slot(slot)
                    .ok_or(RenderError::NodeNotFound(node))?
                    .position;
                self.camera.focus(position);
                self.write_camera();
            }
        }
        if let Some(node) = secondary {
            if self.scene.state().node_slot(node).is_none() {
                return Err(RenderError::NodeNotFound(node));
            }
        }
        self.scene
            .update_highlights(self.scene.hover_node(), primary, secondary, &self.queue);
        self.labels.mark_dirty();
        self.redraw_requested = true;
        Ok(())
    }

    pub fn set_external_hover(
        &mut self,
        node: Option<graph_model::NodeId>,
    ) -> Result<(), RenderError> {
        if let Some(node) = node {
            if self.scene.state().node_slot(node).is_none() {
                return Err(RenderError::NodeNotFound(node));
            }
        }
        self.scene.update_highlights(
            node,
            self.scene.selected_node(),
            self.scene.secondary_selected_node(),
            &self.queue,
        );
        self.labels.mark_dirty();
        self.redraw_requested = true;
        Ok(())
    }

    pub fn apply_review_overrides(
        &mut self,
        index: &PhoenixSceneProductIndexV1,
        overrides: &[GraphReviewOverride],
    ) -> Result<ReviewOverlayMetrics, RenderError> {
        let metrics = self
            .scene
            .apply_review_overrides(index, overrides, &self.queue)?;
        self.redraw_requested = true;
        Ok(metrics)
    }

    #[must_use]
    pub fn interaction_allocation_stats(&self) -> crate::InteractionAllocationStats {
        self.scene.interaction_allocation_stats()
    }

    #[must_use]
    pub fn needs_redraw(&self) -> bool {
        self.redraw_requested || self.picking.has_work()
    }

    pub fn render(&mut self) -> Result<FrameMetrics, RenderError> {
        let started = Instant::now();
        self.redraw_requested = false;
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                self.redraw_requested = true;
                return Ok(FrameMetrics::default());
            }
            Err(wgpu::SurfaceError::Timeout) => {
                self.redraw_requested = true;
                return Ok(FrameMetrics::default());
            }
            Err(error) => return Err(error.into()),
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("graph frame encoder"),
            });
        self.picking.encode_if_ready(
            &mut encoder,
            &self.camera_bind_group,
            &self.node_bind_group,
            &self.lens_bind_group,
            self.scene.node_draw_slots(),
        );
        self.labels.prepare(
            &self.device,
            &self.queue,
            &self.camera,
            self.scene.state(),
            self.active_view,
            LabelFocus {
                hover: self.scene.hover_node(),
                selected: self.scene.selected_node(),
            },
        )?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph color pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.012,
                            g: 0.016,
                            b: 0.025,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipelines.background);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.draw(0..3, 0..1);
            self.prepared_paths.render(
                &mut pass,
                &self.camera_bind_group,
                &self.lens_bind_group,
                &self.edge_bind_group,
            );
            let edge_slots = self.scene.edge_draw_slots();
            if edge_slots != 0 && !self.prepared_paths.has_paths() {
                pass.set_pipeline(&self.pipelines.edges);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, &self.node_bind_group, &[]);
                pass.set_bind_group(2, &self.edge_bind_group, &[]);
                pass.set_bind_group(3, &self.lens_bind_group, &[]);
                pass.draw(0..4, 0..edge_slots);
            }
            let node_slots = self.scene.node_draw_slots();
            if node_slots != 0 {
                pass.set_pipeline(&self.pipelines.nodes);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, &self.node_bind_group, &[]);
                pass.set_bind_group(2, &self.lens_bind_group, &[]);
                pass.draw(0..4, 0..node_slots);
            }
        }
        self.labels.render_onto(&mut encoder, &view)?;
        self.queue.submit(Some(encoder.finish()));
        self.picking.begin_map_after_submit();
        output.present();
        self.labels.trim();
        self.frame_count = self.frame_count.wrapping_add(1);
        let metrics = FrameMetrics {
            frame_number: self.frame_count,
            cpu_encode_submit_us: started.elapsed().as_micros(),
        };
        if metrics.frame_number % 300 == 0 {
            tracing::debug!(
                frame = metrics.frame_number,
                cpu_us = metrics.cpu_encode_submit_us,
                "graph frame submitted"
            );
        }
        Ok(metrics)
    }

    fn resize(&mut self, width: u32, height: u32, scale_factor: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.scale_factor = scale_factor.max(0.01);
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.camera.resize(width as f32, height as f32);
        self.write_camera();
        (self.depth_texture, self.depth_view) = create_depth_texture(&self.device, width, height);
        self.picking.resize(&self.device, width, height);
        self.labels.mark_dirty();
        self.redraw_requested = true;
    }

    fn physical_point(&self, logical_x: f32, logical_y: f32) -> (f32, f32) {
        (
            logical_to_physical(logical_x, self.scale_factor),
            logical_to_physical(logical_y, self.scale_factor),
        )
    }

    fn camera_changed(&mut self) -> Option<GraphEvent> {
        self.write_camera();
        self.labels.mark_dirty();
        self.redraw_requested = true;
        Some(GraphEvent::CameraChanged(self.camera.snapshot()))
    }

    fn write_camera(&self) {
        let mut uniform = self.camera.uniform();
        uniform.edge_opacity = crate::color::dense_edge_opacity(self.scene.state().edge_count());
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    fn refresh_scene_bindings(&mut self) {
        self.node_bind_group = bind_group(
            &self.device,
            "graph node binding",
            &self.layouts.nodes,
            &self.scene.node_buffer.buffer,
        );
        self.edge_bind_group = bind_group(
            &self.device,
            "graph edge binding",
            &self.layouts.edges,
            &self.scene.edge_buffer.buffer,
        );
        self.refresh_lens_binding();
    }

    fn refresh_lens_binding(&mut self) {
        self.lens_bind_group = lens_bind_group(
            &self.device,
            &self.layouts.lens,
            &self.lens_uniform_buffer,
            &self.scene.node_product_buffer.buffer,
            &self.scene.edge_product_buffer.buffer,
        );
    }

    fn write_lens_uniform(&mut self, uniform: GraphLensUniform) {
        self.queue
            .write_buffer(&self.lens_uniform_buffer, 0, bytemuck::bytes_of(&uniform));
        self.lens_uniform_writes = self.lens_uniform_writes.saturating_add(1);
    }
}
mod geometry;
