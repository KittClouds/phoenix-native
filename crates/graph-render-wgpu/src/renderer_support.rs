use crate::RenderError;
use graph_model::GraphRevision;
use phoenix_scene_contract::{GraphViewState, SceneAuthority};

pub(crate) fn validate_view_authority(
    view: GraphViewState,
    revision: Option<GraphRevision>,
    cohort_hash: Option<[u8; 32]>,
    resident_product_hash: Option<[u8; 32]>,
) -> Result<Option<[u8; 32]>, RenderError> {
    let (generation, expected_cohort, index_hash) = match view.authority {
        SceneAuthority::Unavailable => return Err(RenderError::GraphViewGenerationMismatch),
        SceneAuthority::Archive {
            generation,
            cohort_hash,
            product_index_hash,
        } => (generation, cohort_hash, product_index_hash),
    };
    if revision != Some(GraphRevision(generation.0)) || cohort_hash != Some(expected_cohort) {
        return Err(RenderError::GraphViewGenerationMismatch);
    }
    if index_hash != resident_product_hash {
        return Err(RenderError::GraphViewProductIndexMismatch);
    }
    Ok(index_hash)
}

pub(crate) fn preferred_present_mode(modes: &[wgpu::PresentMode]) -> Option<wgpu::PresentMode> {
    [
        wgpu::PresentMode::Mailbox,
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Fifo,
    ]
    .into_iter()
    .find(|candidate| modes.contains(candidate))
}

pub(crate) fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("graph frame depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
