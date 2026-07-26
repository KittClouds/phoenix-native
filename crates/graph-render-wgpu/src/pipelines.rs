use crate::buffers::{CameraUniform, EdgeGpu, NodeGpu};
use crate::{EdgeProductGpu, GraphLensUniform, NodeProductGpu};
use bytemuck::Pod;
use std::mem::size_of;

pub struct RenderLayouts {
    pub camera: wgpu::BindGroupLayout,
    pub nodes: wgpu::BindGroupLayout,
    pub edges: wgpu::BindGroupLayout,
    pub lens: wgpu::BindGroupLayout,
}

pub struct RenderPipelines {
    pub background: wgpu::RenderPipeline,
    pub nodes: wgpu::RenderPipeline,
    pub edges: wgpu::RenderPipeline,
}

pub fn create_layouts(device: &wgpu::Device) -> RenderLayouts {
    RenderLayouts {
        camera: buffer_layout::<CameraUniform>(
            device,
            "graph camera layout",
            wgpu::BufferBindingType::Uniform,
            wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ),
        nodes: buffer_layout::<NodeGpu>(
            device,
            "graph node layout",
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ),
        edges: buffer_layout::<EdgeGpu>(
            device,
            "graph edge layout",
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ),
        lens: lens_layout(device),
    }
}

pub fn create_pipelines(
    device: &wgpu::Device,
    layouts: &RenderLayouts,
    surface_format: wgpu::TextureFormat,
) -> (RenderPipelines, wgpu::ShaderModule) {
    let node_shader = shader(
        device,
        "graph node shader",
        include_str!("../shaders/nodes.wgsl"),
    );
    let edge_shader = shader(
        device,
        "graph edge shader",
        include_str!("../shaders/edges.wgsl"),
    );
    let picking_shader = shader(
        device,
        "graph picking shader",
        include_str!("../shaders/picking.wgsl"),
    );
    let background_shader = shader(
        device,
        "graph background shader",
        include_str!("../shaders/background.wgsl"),
    );
    let background =
        background_pipeline(device, &layouts.camera, &background_shader, surface_format);
    let nodes = pipeline(
        device,
        "graph node pipeline",
        &[&layouts.camera, &layouts.nodes, &layouts.lens],
        &node_shader,
        surface_format,
        true,
    );
    let edges = pipeline(
        device,
        "graph edge pipeline",
        &[
            &layouts.camera,
            &layouts.nodes,
            &layouts.edges,
            &layouts.lens,
        ],
        &edge_shader,
        surface_format,
        false,
    );
    (
        RenderPipelines {
            background,
            nodes,
            edges,
        },
        picking_shader,
    )
}

pub fn lens_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    nodes: &wgpu::Buffer,
    edges: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("graph lens binding"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: nodes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: edges.as_entire_binding(),
            },
        ],
    })
}

fn lens_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("graph lens layout"),
        entries: &[
            lens_entry::<GraphLensUniform>(0, wgpu::BufferBindingType::Uniform),
            lens_entry::<NodeProductGpu>(1, wgpu::BufferBindingType::Storage { read_only: true }),
            lens_entry::<EdgeProductGpu>(2, wgpu::BufferBindingType::Storage { read_only: true }),
        ],
    })
}

fn lens_entry<T: Pod>(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size_of::<T>() as u64),
        },
        count: None,
    }
}

pub fn bind_group(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn buffer_layout<T: Pod>(
    device: &wgpu::Device,
    label: &'static str,
    binding_type: wgpu::BufferBindingType,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: binding_type,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<T>() as u64),
            },
            count: None,
        }],
    })
}

fn shader(device: &wgpu::Device, label: &'static str, source: &'static str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn pipeline(
    device: &wgpu::Device,
    label: &'static str,
    layouts: &[&wgpu::BindGroupLayout],
    shader: &wgpu::ShaderModule,
    surface_format: wgpu::TextureFormat,
    depth_write_enabled: bool,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: layouts,
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
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
            depth_write_enabled,
            depth_compare: if depth_write_enabled {
                wgpu::CompareFunction::Less
            } else {
                wgpu::CompareFunction::LessEqual
            },
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview: None,
        cache: None,
    })
}

fn background_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("graph background pipeline layout"),
        bind_group_layouts: &[camera_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("graph background pipeline"),
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
                format: surface_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
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
    })
}
