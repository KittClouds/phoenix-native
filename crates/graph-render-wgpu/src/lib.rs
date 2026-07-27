mod buffers;
mod camera;
mod color;
mod error;
mod events;
mod gpu_scene;
mod gpu_scene_support;
mod interaction;
mod interaction_index;
mod labels;
mod lens;
mod path_layer;
mod picking;
mod pipelines;
mod renderer;
mod renderer_metrics;
mod renderer_support;
#[cfg(test)]
mod renderer_tests;
mod scene_state;

#[cfg(test)]
mod gpu_smoke;

pub use camera::{Camera, CameraSnapshot};
pub use error::RenderError;
pub use gpu_scene::{
    GpuAllocationStats, GpuSceneMetrics, InteractionAllocationStats, PositionSwitchMetrics,
    ProductInstallMetrics, SnapshotMetrics,
};
pub use interaction::{logical_to_physical, GraphEvent, GraphInput, PointerButton};
pub use lens::{EdgeProductGpu, GraphLensUniform, NodeProductGpu};
pub use path_layer::PreparedGeometryMetrics;
pub use renderer::GraphRenderer;
pub use renderer_metrics::{FrameMetrics, LensUpdateMetrics};
pub use scene_state::{SceneChanges, SceneState};

/// Backends allowed for Phoenix's native renderer.
///
/// Windows is deliberately Vulkan-only. Asking wgpu to probe every backend
/// creates redundant graphics stacks, while wgpu 24's DX12 path reports a
/// blanket downlevel limitation for indirect draws that Phoenix does not use.
pub const fn native_backends() -> wgpu::Backends {
    #[cfg(target_os = "windows")]
    {
        wgpu::Backends::VULKAN
    }
    #[cfg(not(target_os = "windows"))]
    {
        wgpu::Backends::PRIMARY
    }
}

#[cfg(test)]
mod backend_tests {
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_native_backend_is_vulkan_only() {
        assert_eq!(super::native_backends(), wgpu::Backends::VULKAN);
    }
}
