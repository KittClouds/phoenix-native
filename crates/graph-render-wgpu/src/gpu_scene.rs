use crate::buffers::{
    EdgeGpu, NodeGpu, ResizableBuffer, HOVERED_FLAG, NEIGHBOR_FLAG, ROUTE_FLAG, SELECTED_FLAG,
};
use crate::gpu_scene_support::{
    mark_edge, mark_node, metrics_from_changes, reserve_slots, write_dirty_ranges,
};
use crate::interaction_index::InteractionIndex;
use crate::{EdgeProductGpu, NodeProductGpu, RenderError, SceneChanges, SceneState};
use graph_model::{EdgeId, GraphDiff, GraphRevision, GraphSnapshot, NodeId};
use phoenix_scene_archive::{ManifoldPageSet, PositionRecord};
use phoenix_scene_contract::GraphReviewOverride;
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use std::mem::size_of;
use std::time::Instant;

pub(crate) const MAX_INTERACTION_NODE_SLOTS: usize = 4096;
pub(crate) const MAX_INTERACTION_EDGE_SLOTS: usize = 8192;
pub(crate) const MAX_REVIEW_OVERLAY_EDGES: usize = 65_536;
#[derive(Clone, Copy, Debug, Default)]
pub struct SnapshotMetrics {
    pub node_count: usize,
    pub edge_count: usize,
    pub upload_ranges: usize,
    pub elapsed_us: u128,
    pub bindings_changed: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PositionSwitchMetrics {
    pub node_count: usize,
    pub bytes_uploaded: usize,
    pub elapsed_us: u128,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ProductInstallMetrics {
    pub node_records: usize,
    pub edge_records: usize,
    pub bytes_uploaded: usize,
    pub bindings_changed: bool,
    pub elapsed_us: u128,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ReviewOverlayMetrics {
    pub edge_records: usize,
    pub buffer_ranges_updated: usize,
    pub bytes_uploaded: usize,
    pub elapsed_us: u128,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuSceneMetrics {
    pub nodes_added: usize,
    pub nodes_updated: usize,
    pub nodes_removed: usize,
    pub edges_added: usize,
    pub edges_updated: usize,
    pub edges_removed: usize,
    pub buffer_ranges_updated: usize,
    pub diff_apply_duration_us: u128,
    pub bindings_changed: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuAllocationStats {
    pub node_capacity: usize,
    pub edge_capacity: usize,
    pub node_buffer_generation: u64,
    pub edge_buffer_generation: u64,
    pub node_product_capacity: usize,
    pub edge_product_capacity: usize,
    pub node_product_buffer_generation: u64,
    pub edge_product_buffer_generation: u64,
    pub allocated_bytes: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionAllocationStats {
    pub node_slot_capacity: usize,
    pub edge_slot_capacity: usize,
    pub node_dirty_capacity: usize,
    pub edge_dirty_capacity: usize,
    pub queue_capacity: usize,
    pub route_node_capacity: usize,
    pub route_edge_capacity: usize,
    pub route_nodes: usize,
    pub route_edges: usize,
}
pub struct GpuScene {
    state: SceneState,
    node_gpu_data: Vec<NodeGpu>,
    edge_gpu_data: Vec<EdgeGpu>,
    node_product_data: Vec<NodeProductGpu>,
    edge_product_data: Vec<EdgeProductGpu>,
    review_override_slots: Vec<u32>,
    review_dirty_slots: Vec<u32>,
    pub node_buffer: ResizableBuffer,
    pub edge_buffer: ResizableBuffer,
    pub node_product_buffer: ResizableBuffer,
    pub edge_product_buffer: ResizableBuffer,
    bound_product_hash: Option<[u8; 32]>,
    hover_node: Option<NodeId>,
    selected_node: Option<NodeId>,
    secondary_selected_node: Option<NodeId>,
    interaction: InteractionIndex,
    interaction_node_slots: Vec<u32>,
    interaction_edge_slots: Vec<u32>,
    interaction_node_dirty: Vec<u32>,
    interaction_edge_dirty: Vec<u32>,
}

impl GpuScene {
    pub fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Ok(Self {
            state: SceneState::default(),
            node_gpu_data: Vec::new(),
            edge_gpu_data: Vec::new(),
            node_product_data: Vec::new(),
            edge_product_data: Vec::new(),
            review_override_slots: Vec::new(),
            review_dirty_slots: Vec::new(),
            node_buffer: ResizableBuffer::new::<NodeGpu>(
                device,
                "graph node storage",
                1024,
                wgpu::BufferUsages::STORAGE,
            )?,
            edge_buffer: ResizableBuffer::new::<EdgeGpu>(
                device,
                "graph edge storage",
                4096,
                wgpu::BufferUsages::STORAGE,
            )?,
            node_product_buffer: ResizableBuffer::new::<NodeProductGpu>(
                device,
                "graph node product storage",
                1024,
                wgpu::BufferUsages::STORAGE,
            )?,
            edge_product_buffer: ResizableBuffer::new::<EdgeProductGpu>(
                device,
                "graph edge product storage",
                4096,
                wgpu::BufferUsages::STORAGE,
            )?,
            bound_product_hash: None,
            hover_node: None,
            selected_node: None,
            secondary_selected_node: None,
            interaction: InteractionIndex::default(),
            interaction_node_slots: Vec::new(),
            interaction_edge_slots: Vec::new(),
            interaction_node_dirty: Vec::new(),
            interaction_edge_dirty: Vec::new(),
        })
    }

    #[must_use]
    pub fn state(&self) -> &SceneState {
        &self.state
    }

    #[must_use]
    pub fn revision(&self) -> Option<GraphRevision> {
        self.state.revision()
    }

    #[must_use]
    pub fn hover_node(&self) -> Option<NodeId> {
        self.hover_node
    }

    #[must_use]
    pub fn selected_node(&self) -> Option<NodeId> {
        self.selected_node
    }

    #[must_use]
    pub fn secondary_selected_node(&self) -> Option<NodeId> {
        self.secondary_selected_node
    }

    pub fn set_snapshot(
        &mut self,
        snapshot: &GraphSnapshot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<SnapshotMetrics, RenderError> {
        let started = Instant::now();
        self.node_buffer.validate_capacity(snapshot.nodes.len())?;
        self.edge_buffer.validate_capacity(snapshot.edges.len())?;
        self.state.set_snapshot(snapshot)?;
        let bindings_changed = self.rebuild_projection(device, queue)?;

        Ok(SnapshotMetrics {
            node_count: self.state.node_count(),
            edge_count: self.state.edge_count(),
            upload_ranges: usize::from(!self.node_gpu_data.is_empty())
                + usize::from(!self.edge_gpu_data.is_empty()),
            elapsed_us: started.elapsed().as_micros(),
            bindings_changed,
        })
    }

    pub fn set_archive_scene(
        &mut self,
        revision: GraphRevision,
        pages: &ManifoldPageSet<'_>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<SnapshotMetrics, RenderError> {
        let started = Instant::now();
        self.node_buffer.validate_capacity(pages.identities.len())?;
        self.edge_buffer.validate_capacity(pages.edges.len())?;
        self.state.set_archive_pages(revision, pages)?;
        let bindings_changed = self.rebuild_projection(device, queue)?;
        Ok(SnapshotMetrics {
            node_count: self.state.node_count(),
            edge_count: self.state.edge_count(),
            upload_ranges: usize::from(!self.node_gpu_data.is_empty())
                + usize::from(!self.edge_gpu_data.is_empty()),
            elapsed_us: started.elapsed().as_micros(),
            bindings_changed,
        })
    }

    pub fn set_product_index(
        &mut self,
        index: &PhoenixSceneProductIndexV1,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<ProductInstallMetrics, RenderError> {
        let started = Instant::now();
        if index.nodes().len() != self.state.node_count() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "node product index",
                expected: self.state.node_count(),
                actual: index.nodes().len(),
            });
        }
        if index.edges().len() != self.state.edge_count() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "edge product index",
                expected: self.state.edge_count(),
                actual: index.edges().len(),
            });
        }
        for (slot, (node, product)) in self.state.nodes().zip(index.nodes()).enumerate() {
            if node.id.0 != product.node_id {
                return Err(RenderError::ProductIdentityMismatch {
                    resource: "node",
                    slot,
                });
            }
        }
        for (slot, (edge, product)) in self.state.edges().zip(index.edges()).enumerate() {
            if edge.id.0 != product.edge_id {
                return Err(RenderError::ProductIdentityMismatch {
                    resource: "edge",
                    slot,
                });
            }
        }
        self.node_product_data.clear();
        self.node_product_data
            .extend(index.nodes().iter().map(NodeProductGpu::from));
        self.edge_product_data.clear();
        self.edge_product_data
            .extend(index.edges().iter().map(EdgeProductGpu::from));
        self.review_override_slots.clear();
        self.review_dirty_slots.clear();
        let node_changed = self
            .node_product_buffer
            .ensure_capacity(device, self.node_product_data.len())?;
        let edge_changed = self
            .edge_product_buffer
            .ensure_capacity(device, self.edge_product_data.len())?;
        self.node_product_buffer
            .write(queue, 0, &self.node_product_data);
        self.edge_product_buffer
            .write(queue, 0, &self.edge_product_data);
        self.bound_product_hash = Some(index.header().index_hash);
        Ok(ProductInstallMetrics {
            node_records: self.node_product_data.len(),
            edge_records: self.edge_product_data.len(),
            bytes_uploaded: self
                .node_product_data
                .len()
                .saturating_mul(size_of::<NodeProductGpu>())
                .saturating_add(
                    self.edge_product_data
                        .len()
                        .saturating_mul(size_of::<EdgeProductGpu>()),
                ),
            bindings_changed: node_changed || edge_changed,
            elapsed_us: started.elapsed().as_micros(),
        })
    }

    pub fn apply_review_overrides(
        &mut self,
        index: &PhoenixSceneProductIndexV1,
        overrides: &[GraphReviewOverride],
        queue: &wgpu::Queue,
    ) -> Result<ReviewOverlayMetrics, RenderError> {
        if overrides.len() > MAX_REVIEW_OVERLAY_EDGES {
            return Err(RenderError::ReviewOverlayOversized {
                actual: overrides.len(),
                limit: MAX_REVIEW_OVERLAY_EDGES,
            });
        }
        if self.bound_product_hash != Some(index.header().index_hash) {
            return Err(RenderError::GraphViewProductIndexMismatch);
        }
        let started = Instant::now();
        self.review_dirty_slots.clear();
        self.review_dirty_slots
            .extend_from_slice(&self.review_override_slots);
        for &slot in &self.review_override_slots {
            let slot = slot as usize;
            let product = index
                .edges()
                .get(slot)
                .ok_or(RenderError::ProductIdentityMismatch {
                    resource: "review overlay edge",
                    slot,
                })?;
            self.edge_product_data[slot].review_mask = product.review_mask;
        }
        self.review_override_slots.clear();
        for override_record in overrides {
            let slot = self
                .state
                .edge_slot(EdgeId(override_record.edge_id))
                .ok_or(RenderError::EdgeNotFound(EdgeId(override_record.edge_id)))?;
            let product =
                index
                    .edges()
                    .get(slot as usize)
                    .ok_or(RenderError::ProductIdentityMismatch {
                        resource: "review overlay edge",
                        slot: slot as usize,
                    })?;
            if product.edge_id != override_record.edge_id {
                return Err(RenderError::ProductIdentityMismatch {
                    resource: "review overlay edge",
                    slot: slot as usize,
                });
            }
            self.edge_product_data[slot as usize].review_mask = override_record.review_mask;
            self.review_override_slots.push(slot);
            self.review_dirty_slots.push(slot);
        }
        self.review_dirty_slots.sort_unstable();
        self.review_dirty_slots.dedup();
        let ranges = write_dirty_ranges(
            &self.edge_product_buffer,
            queue,
            &self.edge_product_data,
            &self.review_dirty_slots,
        );
        Ok(ReviewOverlayMetrics {
            edge_records: overrides.len(),
            buffer_ranges_updated: ranges,
            bytes_uploaded: self
                .review_dirty_slots
                .len()
                .saturating_mul(size_of::<EdgeProductGpu>()),
            elapsed_us: started.elapsed().as_micros(),
        })
    }

    #[must_use]
    pub const fn bound_product_hash(&self) -> Option<[u8; 32]> {
        self.bound_product_hash
    }

    pub fn switch_archive_positions(
        &mut self,
        positions: &[PositionRecord],
        queue: &wgpu::Queue,
    ) -> Result<PositionSwitchMetrics, RenderError> {
        let started = Instant::now();
        self.state.update_packed_positions(positions)?;
        if self.node_gpu_data.len() != positions.len() {
            return Err(RenderError::PackedLengthMismatch {
                resource: "GPU node projection",
                expected: positions.len(),
                actual: self.node_gpu_data.len(),
            });
        }
        for (gpu, position) in self.node_gpu_data.iter_mut().zip(positions) {
            gpu.position_radius[..3].copy_from_slice(&position.position);
        }
        self.node_buffer.write(queue, 0, &self.node_gpu_data);
        Ok(PositionSwitchMetrics {
            node_count: positions.len(),
            bytes_uploaded: positions
                .len()
                .saturating_mul(std::mem::size_of::<NodeGpu>()),
            elapsed_us: started.elapsed().as_micros(),
        })
    }

    pub fn apply_diff(
        &mut self,
        diff: GraphDiff,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<GpuSceneMetrics, RenderError> {
        let started = Instant::now();
        let node_upper_bound = self
            .state
            .node_capacity_slots()
            .checked_add(diff.added_nodes.len())
            .ok_or(RenderError::BufferSizeOverflow)?;
        let edge_upper_bound = self
            .state
            .edge_capacity_slots()
            .checked_add(diff.added_edges.len())
            .ok_or(RenderError::BufferSizeOverflow)?;
        self.node_buffer.validate_capacity(node_upper_bound)?;
        self.edge_buffer.validate_capacity(edge_upper_bound)?;

        let changes = self.state.apply_diff(diff)?;
        self.rebuild_dirty_cpu_ranges(&changes)?;
        self.node_product_data
            .resize(self.state.node_capacity_slots(), NodeProductGpu::UNFILTERED);
        self.edge_product_data
            .resize(self.state.edge_capacity_slots(), EdgeProductGpu::UNFILTERED);
        for &slot in &changes.dirty_node_slots {
            self.node_product_data[slot as usize] = NodeProductGpu::UNFILTERED;
        }
        for &slot in &changes.dirty_edge_slots {
            self.edge_product_data[slot as usize] = EdgeProductGpu::UNFILTERED;
        }
        self.bound_product_hash = None;

        let node_reallocated = self
            .node_buffer
            .ensure_capacity(device, self.node_gpu_data.len())?;
        let edge_reallocated = self
            .edge_buffer
            .ensure_capacity(device, self.edge_gpu_data.len())?;
        let node_product_reallocated = self
            .node_product_buffer
            .ensure_capacity(device, self.node_product_data.len())?;
        let edge_product_reallocated = self
            .edge_product_buffer
            .ensure_capacity(device, self.edge_product_data.len())?;
        let mut ranges = 0;
        if node_reallocated {
            self.node_buffer.write(queue, 0, &self.node_gpu_data);
            ranges += usize::from(!self.node_gpu_data.is_empty());
        } else {
            ranges += write_dirty_ranges(
                &self.node_buffer,
                queue,
                &self.node_gpu_data,
                &changes.dirty_node_slots,
            );
        }
        if edge_reallocated {
            self.edge_buffer.write(queue, 0, &self.edge_gpu_data);
            ranges += usize::from(!self.edge_gpu_data.is_empty());
        } else {
            ranges += write_dirty_ranges(
                &self.edge_buffer,
                queue,
                &self.edge_gpu_data,
                &changes.dirty_edge_slots,
            );
        }
        if node_product_reallocated {
            self.node_product_buffer
                .write(queue, 0, &self.node_product_data);
            ranges += usize::from(!self.node_product_data.is_empty());
        } else {
            ranges += write_dirty_ranges(
                &self.node_product_buffer,
                queue,
                &self.node_product_data,
                &changes.dirty_node_slots,
            );
        }
        if edge_product_reallocated {
            self.edge_product_buffer
                .write(queue, 0, &self.edge_product_data);
            ranges += usize::from(!self.edge_product_data.is_empty());
        } else {
            ranges += write_dirty_ranges(
                &self.edge_product_buffer,
                queue,
                &self.edge_product_data,
                &changes.dirty_edge_slots,
            );
        }
        self.interaction.rebuild(&self.state);
        self.interaction_node_slots.clear();
        self.interaction_edge_slots.clear();
        self.interaction_node_dirty.clear();
        self.interaction_edge_dirty.clear();
        reserve_slots(
            &mut self.interaction_node_slots,
            self.state
                .node_capacity_slots()
                .min(MAX_INTERACTION_NODE_SLOTS),
        );
        reserve_slots(
            &mut self.interaction_node_dirty,
            self.state
                .node_capacity_slots()
                .min(MAX_INTERACTION_NODE_SLOTS)
                .saturating_mul(2),
        );
        reserve_slots(
            &mut self.interaction_edge_slots,
            self.state
                .edge_capacity_slots()
                .min(MAX_INTERACTION_EDGE_SLOTS),
        );
        reserve_slots(
            &mut self.interaction_edge_dirty,
            self.state
                .edge_capacity_slots()
                .min(MAX_INTERACTION_EDGE_SLOTS)
                .saturating_mul(2),
        );

        Ok(metrics_from_changes(
            &changes,
            ranges,
            started.elapsed().as_micros(),
            node_reallocated
                || edge_reallocated
                || node_product_reallocated
                || edge_product_reallocated,
        ))
    }

    pub fn update_highlights(
        &mut self,
        hover: Option<NodeId>,
        selected: Option<NodeId>,
        secondary_selected: Option<NodeId>,
        queue: &wgpu::Queue,
    ) -> usize {
        self.interaction_node_dirty.clear();
        self.interaction_node_dirty
            .extend_from_slice(&self.interaction_node_slots);
        self.interaction_edge_dirty.clear();
        self.interaction_edge_dirty
            .extend_from_slice(&self.interaction_edge_slots);
        for &slot in &self.interaction_node_slots {
            if let Some(node) = self.state.node_at_slot(slot) {
                self.node_gpu_data[slot as usize] = NodeGpu::from_visual(node, false, false);
            }
        }
        for &slot in &self.interaction_edge_slots {
            if let Some(edge) = self.state.edge_at_slot(slot) {
                let source = self.state.node_slot(edge.source).unwrap_or_default();
                let target = self.state.node_slot(edge.target).unwrap_or_default();
                self.edge_gpu_data[slot as usize] = EdgeGpu::from_visual(edge, source, target);
            }
        }
        self.interaction_node_slots.clear();
        self.interaction_edge_slots.clear();
        self.hover_node = hover.filter(|id| self.state.node_slot(*id).is_some());
        self.selected_node = selected.filter(|id| self.state.node_slot(*id).is_some());
        self.secondary_selected_node =
            secondary_selected.filter(|id| self.state.node_slot(*id).is_some());
        let hover_slot = self.hover_node.and_then(|id| self.state.node_slot(id));
        let selected_slot = self.selected_node.and_then(|id| self.state.node_slot(id));
        let secondary_selected_slot = self
            .secondary_selected_node
            .and_then(|id| self.state.node_slot(id));

        if let Some(slot) = hover_slot {
            mark_node(
                &mut self.node_gpu_data,
                &mut self.interaction_node_slots,
                &mut self.interaction_node_dirty,
                slot,
                HOVERED_FLAG,
            );
        }
        if let Some(slot) = selected_slot {
            mark_node(
                &mut self.node_gpu_data,
                &mut self.interaction_node_slots,
                &mut self.interaction_node_dirty,
                slot,
                SELECTED_FLAG,
            );
            for adjacency in self.interaction.neighbors(slot) {
                mark_node(
                    &mut self.node_gpu_data,
                    &mut self.interaction_node_slots,
                    &mut self.interaction_node_dirty,
                    adjacency.node_slot,
                    NEIGHBOR_FLAG,
                );
                mark_edge(
                    &mut self.edge_gpu_data,
                    &mut self.interaction_edge_slots,
                    &mut self.interaction_edge_dirty,
                    adjacency.edge_slot,
                    NEIGHBOR_FLAG,
                );
            }
        }
        if let Some(slot) = secondary_selected_slot {
            mark_node(
                &mut self.node_gpu_data,
                &mut self.interaction_node_slots,
                &mut self.interaction_node_dirty,
                slot,
                SELECTED_FLAG,
            );
        }
        let route_target = secondary_selected_slot.or(hover_slot);
        if let (Some(source), Some(target)) = (selected_slot, route_target) {
            self.interaction.compute_route(source, target);
            for &slot in self.interaction.route_nodes() {
                mark_node(
                    &mut self.node_gpu_data,
                    &mut self.interaction_node_slots,
                    &mut self.interaction_node_dirty,
                    slot,
                    ROUTE_FLAG,
                );
            }
            for &slot in self.interaction.route_edges() {
                mark_edge(
                    &mut self.edge_gpu_data,
                    &mut self.interaction_edge_slots,
                    &mut self.interaction_edge_dirty,
                    slot,
                    ROUTE_FLAG,
                );
            }
        }
        self.interaction_node_slots.sort_unstable();
        self.interaction_node_slots.dedup();
        self.interaction_edge_slots.sort_unstable();
        self.interaction_edge_slots.dedup();
        self.interaction_node_dirty.sort_unstable();
        self.interaction_node_dirty.dedup();
        self.interaction_edge_dirty.sort_unstable();
        self.interaction_edge_dirty.dedup();
        write_dirty_ranges(
            &self.node_buffer,
            queue,
            &self.node_gpu_data,
            &self.interaction_node_dirty,
        ) + write_dirty_ranges(
            &self.edge_buffer,
            queue,
            &self.edge_gpu_data,
            &self.interaction_edge_dirty,
        )
    }

    #[must_use]
    pub fn node_id_by_slot(&self, slot: u32) -> Option<NodeId> {
        self.state.node_at_slot(slot).map(|node| node.id)
    }

    #[must_use]
    pub fn node_draw_slots(&self) -> u32 {
        self.node_gpu_data.len() as u32
    }

    #[must_use]
    pub fn edge_draw_slots(&self) -> u32 {
        self.edge_gpu_data.len() as u32
    }

    #[must_use]
    pub fn allocation_stats(&self) -> GpuAllocationStats {
        let node_bytes = self
            .node_buffer
            .capacity()
            .saturating_mul(std::mem::size_of::<NodeGpu>());
        let edge_bytes = self
            .edge_buffer
            .capacity()
            .saturating_mul(std::mem::size_of::<EdgeGpu>());
        let node_product_bytes = self
            .node_product_buffer
            .capacity()
            .saturating_mul(size_of::<NodeProductGpu>());
        let edge_product_bytes = self
            .edge_product_buffer
            .capacity()
            .saturating_mul(size_of::<EdgeProductGpu>());
        GpuAllocationStats {
            node_capacity: self.node_buffer.capacity(),
            edge_capacity: self.edge_buffer.capacity(),
            node_buffer_generation: self.node_buffer.generation(),
            edge_buffer_generation: self.edge_buffer.generation(),
            node_product_capacity: self.node_product_buffer.capacity(),
            edge_product_capacity: self.edge_product_buffer.capacity(),
            node_product_buffer_generation: self.node_product_buffer.generation(),
            edge_product_buffer_generation: self.edge_product_buffer.generation(),
            allocated_bytes: node_bytes
                .saturating_add(edge_bytes)
                .saturating_add(node_product_bytes)
                .saturating_add(edge_product_bytes),
        }
    }

    #[must_use]
    pub fn interaction_allocation_stats(&self) -> InteractionAllocationStats {
        let index = self.interaction.stats();
        InteractionAllocationStats {
            node_slot_capacity: self.interaction_node_slots.capacity(),
            edge_slot_capacity: self.interaction_edge_slots.capacity(),
            node_dirty_capacity: self.interaction_node_dirty.capacity(),
            edge_dirty_capacity: self.interaction_edge_dirty.capacity(),
            queue_capacity: index.queue_capacity,
            route_node_capacity: index.route_node_capacity,
            route_edge_capacity: index.route_edge_capacity,
            route_nodes: index.route_nodes,
            route_edges: index.route_edges,
        }
    }

    fn rebuild_projection(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<bool, RenderError> {
        self.hover_node = self
            .hover_node
            .filter(|id| self.state.node_slot(*id).is_some());
        self.selected_node = self
            .selected_node
            .filter(|id| self.state.node_slot(*id).is_some());
        self.secondary_selected_node = self
            .secondary_selected_node
            .filter(|id| self.state.node_slot(*id).is_some());

        self.node_gpu_data.clear();
        self.node_gpu_data.reserve(self.state.node_capacity_slots());
        for node in self.state.nodes() {
            self.node_gpu_data.push(NodeGpu::from_visual(
                node,
                self.hover_node == Some(node.id),
                self.selected_node == Some(node.id),
            ));
            if self.secondary_selected_node == Some(node.id) {
                if let Some(last) = self.node_gpu_data.last_mut() {
                    last.kind_flags |= u32::from(SELECTED_FLAG);
                }
            }
        }
        self.edge_gpu_data.clear();
        self.edge_gpu_data.reserve(self.state.edge_capacity_slots());
        for edge in self.state.edges() {
            let source = self
                .state
                .node_slot(edge.source)
                .ok_or(RenderError::NodeNotFound(edge.source))?;
            let target = self
                .state
                .node_slot(edge.target)
                .ok_or(RenderError::NodeNotFound(edge.target))?;
            self.edge_gpu_data
                .push(EdgeGpu::from_visual(edge, source, target));
        }

        let node_reallocated = self
            .node_buffer
            .ensure_capacity(device, self.node_gpu_data.len())?;
        let edge_reallocated = self
            .edge_buffer
            .ensure_capacity(device, self.edge_gpu_data.len())?;
        self.node_product_data.clear();
        self.node_product_data
            .resize(self.node_gpu_data.len(), NodeProductGpu::UNFILTERED);
        self.edge_product_data.clear();
        self.edge_product_data
            .resize(self.edge_gpu_data.len(), EdgeProductGpu::UNFILTERED);
        let node_product_reallocated = self
            .node_product_buffer
            .ensure_capacity(device, self.node_product_data.len())?;
        let edge_product_reallocated = self
            .edge_product_buffer
            .ensure_capacity(device, self.edge_product_data.len())?;
        self.node_buffer.write(queue, 0, &self.node_gpu_data);
        self.edge_buffer.write(queue, 0, &self.edge_gpu_data);
        self.node_product_buffer
            .write(queue, 0, &self.node_product_data);
        self.edge_product_buffer
            .write(queue, 0, &self.edge_product_data);
        self.bound_product_hash = None;
        self.interaction.rebuild(&self.state);
        self.interaction_node_slots.clear();
        self.interaction_edge_slots.clear();
        self.interaction_node_dirty.clear();
        self.interaction_edge_dirty.clear();
        reserve_slots(
            &mut self.interaction_node_slots,
            self.state
                .node_capacity_slots()
                .min(MAX_INTERACTION_NODE_SLOTS),
        );
        reserve_slots(
            &mut self.interaction_node_dirty,
            self.state
                .node_capacity_slots()
                .min(MAX_INTERACTION_NODE_SLOTS)
                .saturating_mul(2),
        );
        reserve_slots(
            &mut self.interaction_edge_slots,
            self.state
                .edge_capacity_slots()
                .min(MAX_INTERACTION_EDGE_SLOTS),
        );
        reserve_slots(
            &mut self.interaction_edge_dirty,
            self.state
                .edge_capacity_slots()
                .min(MAX_INTERACTION_EDGE_SLOTS)
                .saturating_mul(2),
        );
        Ok(node_reallocated
            || edge_reallocated
            || node_product_reallocated
            || edge_product_reallocated)
    }

    fn rebuild_dirty_cpu_ranges(&mut self, changes: &SceneChanges) -> Result<(), RenderError> {
        self.node_gpu_data
            .resize(self.state.node_capacity_slots(), NodeGpu::TOMBSTONE);
        for &slot in &changes.dirty_node_slots {
            self.node_gpu_data[slot as usize] = match self.state.node_at_slot(slot) {
                Some(node) => NodeGpu::from_visual(
                    node,
                    self.hover_node == Some(node.id),
                    self.selected_node == Some(node.id)
                        || self.secondary_selected_node == Some(node.id),
                ),
                None => NodeGpu::TOMBSTONE,
            };
        }

        self.edge_gpu_data
            .resize(self.state.edge_capacity_slots(), EdgeGpu::TOMBSTONE);
        for &slot in &changes.dirty_edge_slots {
            self.edge_gpu_data[slot as usize] = match self.state.edge_at_slot(slot) {
                Some(edge) => {
                    let source = self
                        .state
                        .node_slot(edge.source)
                        .ok_or(RenderError::NodeNotFound(edge.source))?;
                    let target = self
                        .state
                        .node_slot(edge.target)
                        .ok_or(RenderError::NodeNotFound(edge.target))?;
                    EdgeGpu::from_visual(edge, source, target)
                }
                None => EdgeGpu::TOMBSTONE,
            };
        }
        Ok(())
    }
}
