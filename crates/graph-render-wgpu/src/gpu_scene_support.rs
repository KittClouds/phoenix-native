use crate::buffers::{EdgeGpu, NodeGpu, ResizableBuffer};
use crate::gpu_scene::{GpuSceneMetrics, MAX_INTERACTION_EDGE_SLOTS, MAX_INTERACTION_NODE_SLOTS};
use crate::SceneChanges;
use std::ops::Range;

pub(crate) fn mark_node(
    nodes: &mut [NodeGpu],
    active: &mut Vec<u32>,
    dirty: &mut Vec<u32>,
    slot: u32,
    flag: u16,
) {
    if active.len() < MAX_INTERACTION_NODE_SLOTS {
        if let Some(node) = nodes.get_mut(slot as usize) {
            node.kind_flags |= u32::from(flag);
            active.push(slot);
            dirty.push(slot);
        }
    }
}

pub(crate) fn mark_edge(
    edges: &mut [EdgeGpu],
    active: &mut Vec<u32>,
    dirty: &mut Vec<u32>,
    slot: u32,
    flag: u16,
) {
    if active.len() < MAX_INTERACTION_EDGE_SLOTS {
        if let Some(edge) = edges.get_mut(slot as usize) {
            edge.kind_flags |= u32::from(flag);
            active.push(slot);
            dirty.push(slot);
        }
    }
}

pub(crate) fn reserve_slots(slots: &mut Vec<u32>, capacity: usize) {
    if slots.capacity() < capacity {
        slots.reserve_exact(capacity - slots.capacity());
    }
}

pub(crate) fn write_dirty_ranges<T: bytemuck::Pod>(
    buffer: &ResizableBuffer,
    queue: &wgpu::Queue,
    data: &[T],
    slots: &[u32],
) -> usize {
    let mut count = 0;
    for range in coalesced_ranges(slots) {
        buffer.write(queue, range.start, &data[range]);
        count += 1;
    }
    count
}

fn coalesced_ranges(slots: &[u32]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        let first = *slots.get(cursor)? as usize;
        let mut end = first + 1;
        cursor += 1;
        while let Some(&slot) = slots.get(cursor) {
            if slot as usize != end {
                break;
            }
            end += 1;
            cursor += 1;
        }
        Some(first..end)
    })
}

pub(crate) fn metrics_from_changes(
    changes: &SceneChanges,
    ranges: usize,
    elapsed_us: u128,
    bindings_changed: bool,
) -> GpuSceneMetrics {
    GpuSceneMetrics {
        nodes_added: changes.nodes_added,
        nodes_updated: changes.nodes_updated,
        nodes_removed: changes.nodes_removed,
        edges_added: changes.edges_added,
        edges_updated: changes.edges_updated,
        edges_removed: changes.edges_removed,
        buffer_ranges_updated: ranges,
        diff_apply_duration_us: elapsed_us,
        bindings_changed,
    }
}
