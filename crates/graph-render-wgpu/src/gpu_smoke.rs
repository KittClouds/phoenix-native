use crate::buffers::CameraUniform;
use crate::gpu_scene::GpuScene;
use crate::labels::{LabelFocus, LabelLayer};
use crate::picking::PickingPass;
use crate::pipelines::{bind_group, create_layouts, create_pipelines};
use crate::Camera;
use bytemuck::Zeroable;
use graph_model::{
    EdgeId, EdgeVisual, GraphDiff, GraphRevision, GraphSnapshot, NodeId, NodeVisual,
};
use phoenix_scene_archive::LabelPriorityRecord;
use phoenix_scene_contract::{GraphGeneration, GraphViewState};
use phoenix_scene_product_index::{
    EdgeProductRecord, NodeProductRecord, PhoenixSceneProductIndexBuilderV1,
    PhoenixSceneProductIndexV1, ProductIndexBinding, ReviewState,
};
use std::sync::Arc;

#[test]
fn headless_gpu_resources_accept_snapshot_diff_and_shaders() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: crate::native_backends(),
        ..Default::default()
    });
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
    else {
        eprintln!("no compatible GPU adapter; headless smoke test skipped");
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap_or_else(|error| panic!("GPU device request failed: {error}"));

    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layouts = create_layouts(&device);
    let (pipelines, picking_shader) =
        create_pipelines(&device, &layouts, wgpu::TextureFormat::Bgra8UnormSrgb);
    encode_background_pass(&device, &queue, &layouts.camera, &pipelines.background);
    let _picking = PickingPass::new(
        &device,
        &layouts.camera,
        &layouts.nodes,
        &layouts.lens,
        640,
        480,
        &picking_shader,
    );
    let mut scene = GpuScene::new(&device).unwrap_or_else(|error| panic!("{error}"));
    let snapshot = snapshot();
    scene
        .set_snapshot(&snapshot, &device, &queue)
        .unwrap_or_else(|error| panic!("{error}"));
    let before_product = scene.allocation_stats();
    let index_path = std::env::temp_dir().join(format!(
        "graph-render-product-smoke-{}.pspi",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&index_path);
    let mut builder = PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding {
        archive_generation: 1,
        archive_cohort_hash: [7; 32],
    });
    for node in &snapshot.nodes {
        builder
            .push_node(product_node(node.id.0), &format!("Node {}", node.id.0))
            .unwrap_or_else(|error| panic!("{error}"));
    }
    for edge in &snapshot.edges {
        builder.push_edge(product_edge(edge.id.0));
    }
    builder
        .write_to_path(&index_path)
        .unwrap_or_else(|error| panic!("{error}"));
    let index = Arc::new(
        PhoenixSceneProductIndexV1::open(&index_path).unwrap_or_else(|error| panic!("{error}")),
    );
    let product_metrics = scene
        .set_product_index(&index, &device, &queue)
        .unwrap_or_else(|error| panic!("{error}"));
    let after_product = scene.allocation_stats();
    assert_eq!(product_metrics.node_records, 3);
    assert_eq!(product_metrics.edge_records, 2);
    assert_eq!(
        before_product.node_buffer_generation,
        after_product.node_buffer_generation
    );
    assert_eq!(
        before_product.edge_buffer_generation,
        after_product.edge_buffer_generation
    );
    assert_eq!(scene.bound_product_hash(), Some(index.header().index_hash));
    encode_nonempty_label_overlay(&device, &queue, &scene, Arc::clone(&index));
    let mut diff = GraphDiff::new(GraphRevision(2));
    let mut updated = node(2);
    updated.position[2] = 5.0;
    diff.updated_nodes.push(updated);
    let metrics = scene
        .apply_diff(diff, &device, &queue)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(metrics.nodes_updated, 1);
    assert_eq!(
        metrics.buffer_ranges_updated, 2,
        "legacy diffs update the node plus its unfiltered product metadata"
    );
    device.poll(wgpu::MaintainBase::Wait);
    let validation_error = pollster::block_on(device.pop_error_scope());
    assert!(validation_error.is_none(), "{validation_error:?}");
    drop(index);
    let _ = std::fs::remove_file(index_path);
}

fn encode_nonempty_label_overlay(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &GpuScene,
    index: Arc<PhoenixSceneProductIndexV1>,
) {
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut labels = LabelLayer::new(device, queue, format);
    labels.install(
        Arc::clone(&index),
        &[
            LabelPriorityRecord {
                node_slot: 0,
                rank: 0,
            },
            LabelPriorityRecord {
                node_slot: 1,
                rank: 1,
            },
        ],
    );
    let camera = Camera::new(640.0, 480.0);
    labels
        .prepare(
            device,
            queue,
            &camera,
            scene.state(),
            GraphViewState::for_archive(
                GraphGeneration(1),
                [7; 32],
                Some(index.header().index_hash),
            ),
            LabelFocus {
                hover: Some(NodeId(1)),
                selected: None,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let color = attachment(device, "label smoke color", format);
    let view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("label overlay smoke encoder"),
    });
    labels
        .render_onto(&mut encoder, &view)
        .unwrap_or_else(|error| panic!("{error}"));
    queue.submit(Some(encoder.finish()));
}

fn encode_background_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    pipeline: &wgpu::RenderPipeline,
) {
    let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("background smoke camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut camera = CameraUniform::zeroed();
    camera.viewport_size = [16.0, 16.0];
    camera.edge_opacity = 0.18;
    queue.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera));
    let camera_bind_group = bind_group(
        device,
        "background smoke camera binding",
        camera_layout,
        &camera_buffer,
    );
    let color = attachment(
        device,
        "background smoke color",
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let depth = attachment(
        device,
        "background smoke depth",
        wgpu::TextureFormat::Depth32Float,
    );
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("background smoke encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("background smoke pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &camera_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit(Some(encoder.finish()));
}

fn attachment(
    device: &wgpu::Device,
    label: &'static str,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn snapshot() -> GraphSnapshot {
    GraphSnapshot::new(
        GraphRevision(1),
        vec![node(1), node(2), node(3)],
        vec![
            EdgeVisual {
                id: EdgeId(1),
                source: NodeId(1),
                target: NodeId(2),
                width: 1.0,
                color: [0.4, 0.6, 0.8, 0.4],
                kind: 0,
                flags: 0,
            },
            EdgeVisual {
                id: EdgeId(2),
                source: NodeId(2),
                target: NodeId(3),
                width: 1.0,
                color: [0.4, 0.6, 0.8, 0.4],
                kind: 0,
                flags: 0,
            },
        ],
    )
}

fn node(id: u64) -> NodeVisual {
    NodeVisual {
        id: NodeId(id),
        position: [id as f32 * 2.0, 0.0, 0.0],
        radius: 1.5,
        color: [0.2, 0.8, 0.9, 1.0],
        kind: 0,
        flags: 0,
    }
}

fn product_node(node_id: u64) -> NodeProductRecord {
    NodeProductRecord {
        node_id,
        family_mask: 1,
        scope_mask: 1,
        review_mask: ReviewState::Accepted as u32,
        label_offset: 0,
        label_len: 0,
        inspector_ref: u32::MAX,
        provenance_ref: u32::MAX,
        reserved: 0,
    }
}

fn product_edge(edge_id: u64) -> EdgeProductRecord {
    EdgeProductRecord {
        edge_id,
        family_mask: 1,
        scope_mask: 1,
        relation_mask: 1,
        review_mask: ReviewState::Accepted as u32,
        inspector_ref: u32::MAX,
        provenance_ref: u32::MAX,
        reserved: 0,
    }
}
