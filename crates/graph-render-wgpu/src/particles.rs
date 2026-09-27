//! Additive particle stream for the guided route walk.
//!
//! Particles are pure presentation: a bounded, CPU-generated instance list
//! uploaded only while a walk animates. Geometry follows the edge's prepared
//! path when one is resident, otherwise the straight endpoint segment.

use crate::buffers::ResizableBuffer;
use crate::RenderError;
use bytemuck::{Pod, Zeroable};
use std::mem::size_of;

/// Upper bound on live particles: one traversal stream plus an arrival burst.
pub(crate) const MAX_PARTICLES: usize = 96;
const STREAM_PARTICLES: usize = 56;
const BURST_PARTICLES: usize = 18;
/// Fraction of the traversal over which particles are launched.
const LAUNCH_SPAN: f32 = 0.58;
/// Fraction of the traversal one particle takes to cross the edge.
const FLIGHT_SPAN: f32 = 1.0 - LAUNCH_SPAN;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct ParticleGpu {
    /// World position and radius in physical pixels.
    pub position_size: [f32; 4],
    pub color: [f32; 4],
    /// Screen-space offset in physical pixels; zw unused.
    pub offset: [f32; 4],
}

const _: () = assert!(size_of::<ParticleGpu>() == 48);

/// An arc-length parameterized polyline in world space.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParticleCurve {
    points: Vec<[f32; 3]>,
    cumulative: Vec<f32>,
}

impl ParticleCurve {
    pub(crate) fn new(points: Vec<[f32; 3]>) -> Self {
        let mut cumulative = Vec::with_capacity(points.len());
        let mut total = 0.0;
        for (index, point) in points.iter().enumerate() {
            if index > 0 {
                total += distance(points[index - 1], *point);
            }
            cumulative.push(total);
        }
        Self { points, cumulative }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.points.len() < 2
    }

    /// Position at normalized arc length `t` in `[0, 1]`.
    pub(crate) fn sample(&self, t: f32) -> [f32; 3] {
        let Some(&total) = self.cumulative.last() else {
            return [0.0; 3];
        };
        if self.points.len() == 1 || total <= f32::EPSILON {
            return self.points[0];
        }
        let target = t.clamp(0.0, 1.0) * total;
        let index = self
            .cumulative
            .partition_point(|&length| length < target)
            .clamp(1, self.points.len() - 1);
        let span = (self.cumulative[index] - self.cumulative[index - 1]).max(f32::EPSILON);
        let local = (target - self.cumulative[index - 1]) / span;
        lerp3(self.points[index - 1], self.points[index], local)
    }
}

/// Inputs for one frame of particles.
pub(crate) struct ParticleFrame<'a> {
    pub curve: Option<&'a ParticleCurve>,
    pub progress: f32,
    pub from_color: [f32; 4],
    pub to_color: [f32; 4],
    /// Arrival bloom: world position of the blooming node and 1..0 strength.
    pub bloom: Option<([f32; 3], [f32; 4], f32)>,
    pub scale_factor: f32,
    /// Projects world to physical screen pixels for stream-perpendicular jitter.
    pub project: &'a dyn Fn([f32; 3]) -> Option<(f32, f32)>,
}

pub(crate) fn build_particles(frame: &ParticleFrame<'_>, output: &mut Vec<ParticleGpu>) {
    output.clear();
    let scale = frame.scale_factor.max(0.5);
    if let Some(curve) = frame.curve.filter(|curve| !curve.is_empty()) {
        for index in 0..STREAM_PARTICLES {
            let seed = hash(index as u32);
            let launch = (LAUNCH_SPAN * index as f32 / STREAM_PARTICLES as f32
                + (seed[0] - 0.5) * 0.02)
                .max(0.0);
            let flight = (frame.progress - launch) / FLIGHT_SPAN;
            if flight <= 0.0 || flight >= 1.0 {
                continue;
            }
            // Slight acceleration reads as flow being drawn toward the target.
            let along = flight.powf(1.12);
            let position = curve.sample(along);
            let swell = (std::f32::consts::PI * flight).sin();
            let normal = screen_normal(curve, along, frame.project);
            let jitter = (seed[1] - 0.5) * 11.0 * scale * swell;
            let head = index % 7 == 0;
            let radius = if head {
                4.2 + seed[2] * 1.6
            } else {
                1.5 + seed[2] * 2.4
            } * scale;
            let white = if head { 0.55 } else { 0.28 };
            let tint = mix4(frame.from_color, frame.to_color, along);
            output.push(ParticleGpu {
                position_size: [position[0], position[1], position[2], radius],
                color: [
                    tint[0] + (1.0 - tint[0]) * white,
                    tint[1] + (1.0 - tint[1]) * white,
                    tint[2] + (1.0 - tint[2]) * white,
                    swell.powf(0.55) * (0.55 + 0.45 * seed[3]),
                ],
                offset: [normal.0 * jitter, normal.1 * jitter, 0.0, 0.0],
            });
        }
    }
    if let Some((position, color, strength)) = frame.bloom.filter(|bloom| bloom.2 > 0.0) {
        let age = 1.0 - strength;
        for index in 0..BURST_PARTICLES {
            let seed = hash(index as u32 + 1_000);
            let angle = std::f32::consts::TAU * (index as f32 + seed[0] * 0.6) / BURST_PARTICLES as f32;
            let reach = (10.0 + 30.0 * age * (0.7 + 0.6 * seed[1])) * scale;
            output.push(ParticleGpu {
                position_size: [position[0], position[1], position[2], (1.4 + 1.8 * seed[2]) * scale],
                color: [
                    color[0] + (1.0 - color[0]) * 0.45,
                    color[1] + (1.0 - color[1]) * 0.45,
                    color[2] + (1.0 - color[2]) * 0.45,
                    strength * strength * 0.85,
                ],
                offset: [angle.cos() * reach, angle.sin() * reach, 0.0, 0.0],
            });
        }
    }
    output.truncate(MAX_PARTICLES);
}

fn screen_normal(
    curve: &ParticleCurve,
    t: f32,
    project: &dyn Fn([f32; 3]) -> Option<(f32, f32)>,
) -> (f32, f32) {
    let (Some(a), Some(b)) = (
        project(curve.sample((t - 0.02).max(0.0))),
        project(curve.sample((t + 0.02).min(1.0))),
    ) else {
        return (0.0, 0.0);
    };
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        (0.0, 0.0)
    } else {
        (-dy / length, dx / length)
    }
}

/// Deterministic per-particle variation so a paused frame is exactly stable.
fn hash(index: u32) -> [f32; 4] {
    let mut state = index.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
    std::array::from_fn(|_| {
        state ^= state >> 15;
        state = state.wrapping_mul(0x2c1b_3c6d);
        state ^= state >> 12;
        state = state.wrapping_mul(0x297a_2d39);
        state ^= state >> 15;
        (state & 0x00ff_ffff) as f32 / 16_777_216.0
    })
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.0,
    ]
}

pub(crate) struct ParticleLayer {
    buffer: ResizableBuffer,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    count: u32,
}

impl ParticleLayer {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        surface_format: wgpu::TextureFormat,
    ) -> Result<Self, RenderError> {
        let buffer = ResizableBuffer::new::<ParticleGpu>(
            device,
            "route walk particles",
            MAX_PARTICLES,
            wgpu::BufferUsages::STORAGE,
        )?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("route walk particle layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<ParticleGpu>() as u64),
                },
                count: None,
            }],
        });
        let bind_group = crate::pipelines::bind_group(
            device,
            "route walk particle binding",
            &layout,
            &buffer.buffer,
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("route walk particle shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/particles.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("route walk particle pipeline layout"),
            bind_group_layouts: &[camera_layout, &layout],
            push_constant_ranges: &[],
        });
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("route walk particle pipeline"),
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
                    blend: Some(wgpu::BlendState {
                        color: additive,
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
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
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Ok(Self {
            buffer,
            layout,
            bind_group,
            pipeline,
            count: 0,
        })
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        particles: &[ParticleGpu],
    ) -> Result<(), RenderError> {
        self.count = particles.len() as u32;
        if particles.is_empty() {
            return Ok(());
        }
        if self.buffer.ensure_capacity(device, particles.len())? {
            self.bind_group = crate::pipelines::bind_group(
                device,
                "route walk particle binding",
                &self.layout,
                &self.buffer.buffer,
            );
        }
        self.buffer.write(queue, 0, particles);
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.count = 0;
    }

    pub(crate) fn render<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        camera: &'pass wgpu::BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.draw(0..4, 0..self.count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_samples_by_arc_length_and_particles_stay_bounded_and_stable() {
        let curve = ParticleCurve::new(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 3.0, 0.0]]);
        assert_eq!(curve.sample(0.0), [0.0, 0.0, 0.0]);
        assert_eq!(curve.sample(0.25), [1.0, 0.0, 0.0]);
        assert_eq!(curve.sample(1.0), [1.0, 3.0, 0.0]);
        let project = |p: [f32; 3]| Some((p[0] * 100.0, p[1] * 100.0));
        let frame = ParticleFrame {
            curve: Some(&curve),
            progress: 0.6,
            from_color: [0.1, 0.4, 0.9, 1.0],
            to_color: [0.9, 0.3, 0.6, 1.0],
            bloom: Some(([1.0, 3.0, 0.0], [1.0; 4], 0.5)),
            scale_factor: 1.0,
            project: &project,
        };
        let mut first = Vec::new();
        let mut second = Vec::new();
        build_particles(&frame, &mut first);
        build_particles(&frame, &mut second);
        assert!(!first.is_empty() && first.len() <= MAX_PARTICLES);
        assert_eq!(bytemuck::cast_slice::<_, u8>(&first), bytemuck::cast_slice::<_, u8>(&second));
        let idle = ParticleFrame {
            progress: 0.0,
            bloom: None,
            ..frame
        };
        build_particles(&idle, &mut first);
        assert!(first.is_empty());
    }
}
