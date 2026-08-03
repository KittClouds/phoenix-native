use crate::gpu_scene::GpuScene;
use graph_model::NodeId;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAP_IDLE: u8 = 0;
const MAP_PENDING: u8 = 1;
const MAP_READY: u8 = 2;
const MAP_FAILED: u8 = 3;
const COPY_ENCODED: u8 = 4;
const COPY_BYTES_PER_ROW: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickIntent {
    Hover,
    Select,
}

#[derive(Clone, Copy, Debug)]
struct PickRequest {
    x: u32,
    y: u32,
    intent: PickIntent,
    epoch: u64,
    viewport_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickResult {
    pub intent: PickIntent,
    pub node: Option<NodeId>,
}

pub struct PickingPass {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    staging_buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    pending: Option<PickRequest>,
    inflight_intent: Option<PickIntent>,
    inflight_epoch: Option<u64>,
    inflight_viewport_revision: Option<u64>,
    epoch: u64,
    viewport_revision: u64,
    map_state: Arc<AtomicU8>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    next_hover_at: Instant,
    hover_interval: Duration,
}

impl PickingPass {
    pub fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        node_layout: &wgpu::BindGroupLayout,
        lens_layout: &wgpu::BindGroupLayout,
        width: u32,
        height: u32,
        shader: &wgpu::ShaderModule,
    ) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let (texture, view) = create_pick_texture(device, width, height);
        let (depth_texture, depth_view) = create_depth_texture(device, width, height);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph picking pipeline layout"),
            bind_group_layouts: &[camera_layout, node_layout, lens_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph picking pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R32Uint,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph picking readback"),
            size: u64::from(COPY_BYTES_PER_ROW),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            depth_texture,
            depth_view,
            pipeline,
            staging_buffer,
            width,
            height,
            pending: None,
            inflight_intent: None,
            inflight_epoch: None,
            inflight_viewport_revision: None,
            epoch: 1,
            viewport_revision: 1,
            map_state: Arc::new(AtomicU8::new(MAP_IDLE)),
            wake: None,
            next_hover_at: Instant::now(),
            hover_interval: Duration::from_millis(32),
        }
    }

    /// Install a host-neutral wake callback for asynchronous readback
    /// completion. The renderer does not know about winit or any particular
    /// event loop; the host supplies the smallest possible wake operation.
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = Some(wake);
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        viewport_revision: u64,
    ) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.viewport_revision = viewport_revision.max(1);
        (self.texture, self.view) = create_pick_texture(device, self.width, self.height);
        (self.depth_texture, self.depth_view) =
            create_depth_texture(device, self.width, self.height);
        self.invalidate();
    }

    /// Discard requests that were encoded against an older camera, scene, or
    /// lens view. GPU readback is asynchronous, so without an epoch a result
    /// can be applied after the pointer has moved to a different projection.
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        self.pending = None;
    }

    pub fn request(&mut self, x: f32, y: f32, intent: PickIntent) {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= self.width as f32
            || y >= self.height as f32
        {
            if intent == PickIntent::Hover {
                self.pending = None;
            }
            return;
        }
        let request = PickRequest {
            x: x as u32,
            y: y as u32,
            intent,
            epoch: self.epoch,
            viewport_revision: self.viewport_revision,
        };
        match (self.pending, intent) {
            (Some(pending), PickIntent::Hover) if pending.intent == PickIntent::Select => {}
            _ => self.pending = Some(request),
        }
    }

    #[must_use]
    pub fn has_work(&self) -> bool {
        self.pending.is_some()
            || matches!(
                self.map_state.load(Ordering::Acquire),
                COPY_ENCODED | MAP_READY | MAP_FAILED
            )
    }

    #[must_use]
    pub fn map_in_flight(&self) -> bool {
        matches!(
            self.map_state.load(Ordering::Acquire),
            COPY_ENCODED | MAP_PENDING
        )
    }

    pub fn encode_if_ready(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        camera_bind_group: &wgpu::BindGroup,
        node_bind_group: &wgpu::BindGroup,
        lens_bind_group: &wgpu::BindGroup,
        node_slots: u32,
    ) {
        if self.map_state.load(Ordering::Acquire) != MAP_IDLE {
            return;
        }
        let Some(request) = self.pending else {
            return;
        };
        if request.intent == PickIntent::Hover && Instant::now() < self.next_hover_at {
            return;
        }
        self.pending = None;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph picking pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, camera_bind_group, &[]);
            pass.set_bind_group(1, node_bind_group, &[]);
            pass.set_bind_group(2, lens_bind_group, &[]);
            pass.draw(0..4, 0..node_slots);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: request.x.min(self.width - 1),
                    y: request.y.min(self.height - 1),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(COPY_BYTES_PER_ROW),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        self.inflight_intent = Some(request.intent);
        self.inflight_epoch = Some(request.epoch);
        self.inflight_viewport_revision = Some(request.viewport_revision);
        self.next_hover_at = Instant::now() + self.hover_interval;
        self.map_state.store(COPY_ENCODED, Ordering::Release);
    }

    pub fn begin_map_after_submit(&mut self) {
        if self
            .map_state
            .compare_exchange(
                COPY_ENCODED,
                MAP_PENDING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        let state = Arc::clone(&self.map_state);
        let wake = self.wake.clone();
        self.staging_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(
                    if result.is_ok() {
                        MAP_READY
                    } else {
                        MAP_FAILED
                    },
                    Ordering::Release,
                );
                if let Some(wake) = wake {
                    wake();
                }
            });
    }

    pub fn poll(&mut self, device: &wgpu::Device, scene: &GpuScene) -> Option<PickResult> {
        device.poll(wgpu::MaintainBase::Poll);
        match self.map_state.load(Ordering::Acquire) {
            MAP_READY => {
                let mapped = self.staging_buffer.slice(..).get_mapped_range();
                let encoded_slot = mapped
                    .get(..4)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes);
                drop(mapped);
                self.staging_buffer.unmap();
                self.map_state.store(MAP_IDLE, Ordering::Release);
                let Some(intent) = self.inflight_intent.take() else {
                    self.inflight_epoch = None;
                    self.inflight_viewport_revision = None;
                    return None;
                };
                let Some(request_epoch) = self.inflight_epoch.take() else {
                    self.inflight_viewport_revision = None;
                    return None;
                };
                let request_viewport_revision = self.inflight_viewport_revision.take()?;
                if !pick_is_current(
                    request_epoch,
                    request_viewport_revision,
                    self.epoch,
                    self.viewport_revision,
                ) {
                    return None;
                }
                let node = encoded_slot?
                    .checked_sub(1)
                    .and_then(|slot| scene.node_id_by_slot(slot));
                Some(PickResult { intent, node })
            }
            MAP_FAILED => {
                tracing::warn!("GPU picking readback failed");
                self.map_state.store(MAP_IDLE, Ordering::Release);
                self.inflight_intent = None;
                self.inflight_epoch = None;
                self.inflight_viewport_revision = None;
                None
            }
            _ => None,
        }
    }
}

#[inline]
fn pick_is_current(
    request_epoch: u64,
    request_viewport_revision: u64,
    epoch: u64,
    viewport_revision: u64,
) -> bool {
    request_epoch == epoch && request_viewport_revision == viewport_revision
}

fn create_pick_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_texture(
        device,
        "graph picking IDs",
        width,
        height,
        wgpu::TextureFormat::R32Uint,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    )
}

fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_texture(
        device,
        "graph picking depth",
        width,
        height,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    )
}

fn create_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stale_pick_is_rejected_after_viewport_revision_changes() {
        assert!(!super::pick_is_current(4, 2, 5, 2));
        assert!(!super::pick_is_current(5, 1, 5, 2));
        assert!(super::pick_is_current(5, 2, 5, 2));
    }
}
