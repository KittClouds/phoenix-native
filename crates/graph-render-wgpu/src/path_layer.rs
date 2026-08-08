use crate::buffers::ResizableBuffer;
use crate::color::rich_edge_rgba;
use crate::RenderError;
use bytemuck::{Pod, Zeroable};
use phoenix_scene_archive::{GuidePageView, PathPageView, PositionRecord};
use phoenix_scene_contract::{
    validate_topology_projection, GUIDE_FLAG_CAP_BOUNDARY, GUIDE_FLAG_CONCENTRATION_AXIS,
    GUIDE_FLAG_HOPF_BASE_LINK, GUIDE_FLAG_HOPF_BASE_SPHERE, GUIDE_FLAG_HOPF_FIBER,
    GUIDE_FLAG_SHELL,
};
use std::mem::size_of;

const MAX_PREPARED_SEGMENTS: usize = 500_000;
const GUIDE_FLAG: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PreparedSegmentGpu {
    start: [f32; 4],
    end: [f32; 4],
    color: [f32; 4],
    edge_slot: u32,
    flags: u32,
    _padding: [u32; 2],
}

const _: () = assert!(size_of::<PreparedSegmentGpu>() == 64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreparedGeometryMetrics {
    pub guide_segments: usize,
    pub path_segments: usize,
    pub bytes_uploaded: usize,
    pub buffer_generation: u64,
}

pub(crate) struct PreparedPathLayer {
    segments: Vec<PreparedSegmentGpu>,
    buffer: ResizableBuffer,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    guide_segments: usize,
}

impl PreparedPathLayer {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        lens_layout: &wgpu::BindGroupLayout,
        edge_layout: &wgpu::BindGroupLayout,
        surface_format: wgpu::TextureFormat,
    ) -> Result<Self, RenderError> {
        let buffer = ResizableBuffer::new::<PreparedSegmentGpu>(
            device,
            "prepared graph paths",
            4096,
            wgpu::BufferUsages::STORAGE,
        )?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("prepared graph path layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<PreparedSegmentGpu>() as u64),
                },
                count: None,
            }],
        });
        let bind_group = create_bind_group(device, &layout, &buffer.buffer);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("prepared graph path shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/paths.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("prepared graph path pipeline layout"),
            bind_group_layouts: &[camera_layout, &layout, lens_layout, edge_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("prepared graph path pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Ok(Self {
            segments: Vec::new(),
            buffer,
            layout,
            bind_group,
            pipeline,
            guide_segments: 0,
        })
    }

    pub(crate) fn install(
        &mut self,
        guides: Option<GuidePageView<'_>>,
        paths: Option<PathPageView<'_>>,
        edge_count: usize,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<PreparedGeometryMetrics, RenderError> {
        self.segments.clear();
        if let Some(guides) = guides {
            for stroke in guides.strokes {
                let points = page_points(
                    guides.points,
                    stroke.first_point,
                    stroke.point_count,
                    "guide",
                )?;
                append_segments(
                    &mut self.segments,
                    points,
                    rgba8(stroke.rgba8),
                    guide_width(stroke.flags),
                    u32::MAX,
                    GUIDE_FLAG | (stroke.flags << 8),
                )?;
            }
        }
        self.guide_segments = self.segments.len();
        if let Some(paths) = paths {
            validate_topology_projection(paths, edge_count)?;
            for path in paths.paths {
                let points = page_points(
                    paths.points,
                    path.first_point,
                    u32::from(path.point_count),
                    "path",
                )?;
                append_segments(
                    &mut self.segments,
                    points,
                    rgba8(path.rgba8),
                    1.15,
                    path.edge_slot,
                    u32::from(path.flags) << 8,
                )?;
            }
        }
        let changed = self
            .buffer
            .ensure_capacity(device, self.segments.len().max(1))?;
        if changed {
            self.bind_group = create_bind_group(device, &self.layout, &self.buffer.buffer);
        }
        self.buffer.write(queue, 0, &self.segments);
        Ok(PreparedGeometryMetrics {
            guide_segments: self.guide_segments,
            path_segments: self.segments.len().saturating_sub(self.guide_segments),
            bytes_uploaded: self
                .segments
                .len()
                .saturating_mul(size_of::<PreparedSegmentGpu>()),
            buffer_generation: self.buffer.generation(),
        })
    }

    pub(crate) fn clear(&mut self) {
        self.segments.clear();
        self.guide_segments = 0;
    }

    pub(crate) fn has_paths(&self) -> bool {
        self.segments.len() > self.guide_segments
    }

    pub(crate) fn render<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        camera: &'pass wgpu::BindGroup,
        lens: &'pass wgpu::BindGroup,
        edges: &'pass wgpu::BindGroup,
    ) {
        if self.segments.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_bind_group(2, lens, &[]);
        pass.set_bind_group(3, edges, &[]);
        pass.draw(0..4, 0..self.segments.len() as u32);
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("prepared graph path binding"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn page_points<'a>(
    points: &'a [PositionRecord],
    first: u32,
    count: u32,
    resource: &'static str,
) -> Result<&'a [PositionRecord], RenderError> {
    let start = first as usize;
    let end = start
        .checked_add(count as usize)
        .ok_or(RenderError::BufferSizeOverflow)?;
    points
        .get(start..end)
        .ok_or(RenderError::PreparedGeometryRange { resource })
}

fn append_segments(
    output: &mut Vec<PreparedSegmentGpu>,
    points: &[PositionRecord],
    color: [f32; 4],
    width: f32,
    edge_slot: u32,
    flags: u32,
) -> Result<(), RenderError> {
    let added = points.len().saturating_sub(1);
    let total = output
        .len()
        .checked_add(added)
        .ok_or(RenderError::BufferSizeOverflow)?;
    if total > MAX_PREPARED_SEGMENTS {
        return Err(RenderError::PreparedGeometryOversized {
            actual: total,
            limit: MAX_PREPARED_SEGMENTS,
        });
    }
    output.reserve(added);
    output.extend(points.windows(2).map(|pair| PreparedSegmentGpu {
        start: [
            pair[0].position[0],
            pair[0].position[1],
            pair[0].position[2],
            width,
        ],
        end: [
            pair[1].position[0],
            pair[1].position[1],
            pair[1].position[2],
            width,
        ],
        color,
        edge_slot,
        flags,
        _padding: [0; 2],
    }));
    Ok(())
}

fn rgba8(packed: u32) -> [f32; 4] {
    const SCALE: f32 = 1.0 / 255.0;
    rich_edge_rgba([
        (packed & 0xff) as f32 * SCALE,
        ((packed >> 8) & 0xff) as f32 * SCALE,
        ((packed >> 16) & 0xff) as f32 * SCALE,
        ((packed >> 24) & 0xff) as f32 * SCALE,
    ])
}

const fn guide_width(flags: u32) -> f32 {
    match flags {
        GUIDE_FLAG_SHELL => 0.64,
        GUIDE_FLAG_CAP_BOUNDARY => 0.78,
        GUIDE_FLAG_CONCENTRATION_AXIS => 0.92,
        GUIDE_FLAG_HOPF_FIBER => 0.82,
        GUIDE_FLAG_HOPF_BASE_SPHERE => 0.52,
        GUIDE_FLAG_HOPF_BASE_LINK => 0.44,
        _ => 0.70,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_color_decodes_rgba_order_into_rich_linear_space() {
        let decoded = rgba8(0x8040_2010);
        assert!(decoded[2] > decoded[1]);
        assert!(decoded[1] > decoded[0]);
        assert!(decoded[..3].iter().all(|channel| *channel <= 0.48));
        assert_eq!(decoded[3], 128.0 / 255.0);
    }

    #[test]
    fn caps_guide_semantics_have_bounded_distinct_widths() {
        assert!(guide_width(GUIDE_FLAG_SHELL) < guide_width(GUIDE_FLAG_CAP_BOUNDARY));
        assert!(guide_width(GUIDE_FLAG_CAP_BOUNDARY) < guide_width(GUIDE_FLAG_CONCENTRATION_AXIS));
        assert!(guide_width(GUIDE_FLAG_CONCENTRATION_AXIS) < 1.0);
    }
}
