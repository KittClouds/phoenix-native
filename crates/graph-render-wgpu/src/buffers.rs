use crate::color::{rich_edge_rgba, srgb_rgba_to_linear};
use crate::RenderError;
use bytemuck::{Pod, Zeroable};
use graph_model::{EdgeVisual, NodeVisual};

pub const HOVERED_FLAG: u16 = 1 << 12;
pub const SELECTED_FLAG: u16 = 1 << 13;
pub const NEIGHBOR_FLAG: u16 = 1 << 14;
pub const ROUTE_FLAG: u16 = 1 << 15;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct NodeGpu {
    pub position_radius: [f32; 4],
    pub color: [f32; 4],
    pub id_low: u32,
    pub id_high: u32,
    pub kind_flags: u32,
    pub _padding: u32,
}

const _: () = assert!(std::mem::size_of::<NodeGpu>() == 48);
const _: () = assert!(std::mem::align_of::<NodeGpu>() == 4);

impl NodeGpu {
    pub const TOMBSTONE: Self = Self {
        position_radius: [0.0; 4],
        color: [0.0; 4],
        id_low: 0,
        id_high: 0,
        kind_flags: 0,
        _padding: 0,
    };

    #[must_use]
    pub fn from_visual(node: &NodeVisual, hovered: bool, selected: bool) -> Self {
        let flags = node.flags
            | if hovered { HOVERED_FLAG } else { 0 }
            | if selected { SELECTED_FLAG } else { 0 };
        Self {
            position_radius: [
                node.position[0],
                node.position[1],
                node.position[2],
                node.radius,
            ],
            color: srgb_rgba_to_linear(node.color),
            id_low: node.id.0 as u32,
            id_high: (node.id.0 >> 32) as u32,
            kind_flags: (u32::from(node.kind) << 16) | u32::from(flags),
            _padding: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct EdgeGpu {
    pub source_slot: u32,
    pub target_slot: u32,
    pub kind_flags: u32,
    pub _padding0: u32,
    pub color: [f32; 4],
    pub width: f32,
    pub id_low: u32,
    pub id_high: u32,
    pub _padding1: u32,
}

const _: () = assert!(std::mem::size_of::<EdgeGpu>() == 48);
const _: () = assert!(std::mem::align_of::<EdgeGpu>() == 4);

impl EdgeGpu {
    pub const TOMBSTONE: Self = Self {
        source_slot: 0,
        target_slot: 0,
        kind_flags: 0,
        _padding0: 0,
        color: [0.0; 4],
        width: 0.0,
        id_low: 0,
        id_high: 0,
        _padding1: 0,
    };

    #[must_use]
    pub fn from_visual(edge: &EdgeVisual, source_slot: u32, target_slot: u32) -> Self {
        Self {
            source_slot,
            target_slot,
            kind_flags: (u32::from(edge.kind) << 16) | u32::from(edge.flags),
            _padding0: 0,
            color: rich_edge_rgba(edge.color),
            width: edge.width,
            id_low: edge.id.0 as u32,
            id_high: (edge.id.0 >> 32) as u32,
            _padding1: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye_position: [f32; 4],
    pub view_right: [f32; 4],
    pub view_up: [f32; 4],
    pub viewport_size: [f32; 2],
    pub edge_opacity: f32,
    pub _padding: f32,
}

const _: () = assert!(std::mem::size_of::<CameraUniform>() == 128);

pub struct ResizableBuffer {
    pub buffer: wgpu::Buffer,
    capacity: usize,
    element_size: usize,
    max_size: u64,
    generation: u64,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl ResizableBuffer {
    pub fn new<T: Pod>(
        device: &wgpu::Device,
        label: &'static str,
        initial_capacity: usize,
        usage: wgpu::BufferUsages,
    ) -> Result<Self, RenderError> {
        let element_size = std::mem::size_of::<T>();
        let capacity = initial_capacity.max(16);
        let max_size = device.limits().max_buffer_size;
        let size = checked_buffer_size(capacity, element_size, max_size)?;
        let buffer = create_buffer(device, label, size, usage);
        Ok(Self {
            buffer,
            capacity,
            element_size,
            max_size,
            generation: 0,
            label,
            usage,
        })
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn validate_capacity(&self, required: usize) -> Result<(), RenderError> {
        checked_buffer_size(required.max(16), self.element_size, self.max_size).map(|_| ())
    }

    pub fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        required: usize,
    ) -> Result<bool, RenderError> {
        if required <= self.capacity {
            return Ok(false);
        }
        let new_capacity = required
            .checked_next_power_of_two()
            .ok_or(RenderError::BufferSizeOverflow)?
            .max(16);
        let size = checked_buffer_size(new_capacity, self.element_size, self.max_size)?;
        tracing::debug!(
            buffer = self.label,
            old_capacity = self.capacity,
            new_capacity,
            "growing GPU buffer"
        );
        self.buffer = create_buffer(device, self.label, size, self.usage);
        self.capacity = new_capacity;
        self.generation = self.generation.wrapping_add(1);
        Ok(true)
    }

    pub fn write<T: Pod>(&self, queue: &wgpu::Queue, start: usize, data: &[T]) {
        if data.is_empty() {
            return;
        }
        debug_assert_eq!(std::mem::size_of::<T>(), self.element_size);
        let offset = (start * self.element_size) as u64;
        queue.write_buffer(&self.buffer, offset, bytemuck::cast_slice(data));
    }
}

fn checked_buffer_size(
    capacity: usize,
    element_size: usize,
    limit: u64,
) -> Result<u64, RenderError> {
    let bytes = capacity
        .checked_mul(element_size)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(RenderError::BufferSizeOverflow)?;
    if bytes > limit {
        return Err(RenderError::DeviceBufferLimit {
            requested: bytes,
            limit,
        });
    }
    Ok(bytes)
}

fn create_buffer(
    device: &wgpu::Device,
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
