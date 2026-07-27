use graph_model::{EdgeId, ModelError, NodeId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("renderer has no graph snapshot")]
    SceneUninitialized,
    #[error("packed scene length mismatch: {resource} has {actual} records, expected {expected}")]
    PackedLengthMismatch {
        resource: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("packed manifold position {slot} contains non-finite coordinates")]
    InvalidPackedPosition { slot: usize },
    #[error("packed manifold switching requires a dense, uncompacted scene")]
    PackedSceneFragmented,
    #[error("scene product index {resource} identity mismatch at slot {slot}")]
    ProductIdentityMismatch { resource: &'static str, slot: usize },
    #[error("graph view authority generation does not match the resident GPU scene")]
    GraphViewGenerationMismatch,
    #[error("graph view product-index authority does not match the resident GPU metadata")]
    GraphViewProductIndexMismatch,
    #[error("filtered graph view requires resident product-index metadata")]
    GraphViewProductIndexRequired,
    #[error("graph has more than {limit} addressable {resource} slots")]
    SlotLimit { resource: &'static str, limit: u64 },
    #[error("GPU buffer byte size overflow")]
    BufferSizeOverflow,
    #[error("requested GPU buffer size {requested} exceeds device limit {limit}")]
    DeviceBufferLimit { requested: u64, limit: u64 },
    #[error("GPU adapter not found")]
    AdapterNotFound,
    #[error("surface reports no compatible formats or alpha modes")]
    SurfaceCapabilitiesUnavailable,
    #[error("GPU device request failed: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),
    #[error("surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),
    #[error("node not found: {0}")]
    NodeNotFound(NodeId),
    #[error("edge not found: {0}")]
    EdgeNotFound(EdgeId),
    #[error("selection event sequence exhausted")]
    SelectionSequenceExhausted,
    #[error("GPU label preparation failed: {0}")]
    LabelPrepare(String),
    #[error("GPU label rendering failed: {0}")]
    LabelRender(String),
    #[error("prepared {resource} point range is outside its verified page")]
    PreparedGeometryRange { resource: &'static str },
    #[error("prepared path references edge slot {slot}, but the scene has {edge_count} edges")]
    PreparedGeometryEdgeSlot { slot: u32, edge_count: usize },
    #[error("prepared geometry has {actual} segments, exceeding the fixed limit of {limit}")]
    PreparedGeometryOversized { actual: usize, limit: usize },
}
