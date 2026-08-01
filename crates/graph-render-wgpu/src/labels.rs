use crate::{Camera, RenderError, SceneState};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};
use graph_model::NodeId;
use phoenix_scene_archive::LabelPriorityRecord;
use phoenix_scene_contract::{FamilyMask, GraphViewState};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use std::sync::Arc;

pub const MAX_RESIDENT_LABELS: usize = 256;
const MAX_VISIBLE_LABELS: usize = 96;
const LABEL_WIDTH: f32 = 176.0;
const LABEL_HEIGHT: f32 = 22.0;
const LABEL_FONT_SIZE: f32 = 13.0;
const LABEL_LINE_HEIGHT: f32 = 18.0;
const COLLISION_CELL_WIDTH: f32 = 92.0;
const COLLISION_CELL_HEIGHT: f32 = 24.0;

#[derive(Clone, Copy)]
pub(crate) struct LabelFocus {
    pub(crate) hover: Option<NodeId>,
    pub(crate) selected: Option<NodeId>,
}

struct LabelEntry {
    node_slot: u32,
    family_mask: u64,
    scope_mask: u64,
    review_mask: u32,
    width: f32,
    buffer: Buffer,
}

#[derive(Clone, Copy)]
struct PlacedLabel {
    entry: u16,
    left: f32,
    top: f32,
}

pub(crate) struct LabelLayer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    index: Option<Arc<PhoenixSceneProductIndexV1>>,
    entries: Vec<LabelEntry>,
    placed: Vec<PlacedLabel>,
    collision_stamps: Vec<u32>,
    collision_generation: u32,
    dirty: bool,
}

impl LabelLayer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            font_system,
            swash_cache,
            viewport,
            atlas,
            renderer,
            index: None,
            entries: Vec::with_capacity(MAX_RESIDENT_LABELS),
            placed: Vec::with_capacity(MAX_VISIBLE_LABELS),
            collision_stamps: Vec::new(),
            collision_generation: 0,
            dirty: true,
        }
    }

    pub(crate) fn install(
        &mut self,
        index: Arc<PhoenixSceneProductIndexV1>,
        priorities: &[LabelPriorityRecord],
    ) {
        self.entries.clear();
        let mut ordered = priorities
            .iter()
            .copied()
            .filter(|priority| (priority.node_slot as usize) < index.nodes().len())
            .collect::<Vec<_>>();
        if ordered.is_empty() {
            ordered.extend(
                (0..index.nodes().len().min(MAX_RESIDENT_LABELS)).map(|node_slot| {
                    LabelPriorityRecord {
                        node_slot: node_slot as u32,
                        rank: node_slot as u32,
                    }
                }),
            );
        } else {
            ordered.sort_unstable_by_key(|priority| (priority.rank, priority.node_slot));
            ordered.truncate(MAX_RESIDENT_LABELS);
        }

        for priority in ordered {
            let slot = priority.node_slot as usize;
            let Some(product) = index.nodes().get(slot) else {
                continue;
            };
            let Some(label) = index.label(slot) else {
                continue;
            };
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(LABEL_FONT_SIZE, LABEL_LINE_HEIGHT),
            );
            buffer.set_size(&mut self.font_system, Some(LABEL_WIDTH), Some(LABEL_HEIGHT));
            buffer.set_wrap(&mut self.font_system, Wrap::None);
            buffer.set_text(
                &mut self.font_system,
                label,
                Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            let width = buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0_f32, f32::max)
                .clamp(1.0, LABEL_WIDTH);
            self.entries.push(LabelEntry {
                node_slot: priority.node_slot,
                family_mask: product.family_mask,
                scope_mask: product.scope_mask,
                review_mask: product.review_mask,
                width,
                buffer,
            });
        }
        self.index = Some(index);
        self.dirty = true;
        tracing::info!(
            resident_labels = self.entries.len(),
            max_resident_labels = MAX_RESIDENT_LABELS,
            "bounded graph label atlas installed"
        );
    }

    pub(crate) fn clear(&mut self) {
        self.index = None;
        self.entries.clear();
        self.placed.clear();
        self.atlas.trim();
        self.dirty = true;
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        scene: &SceneState,
        view: GraphViewState,
        focus: LabelFocus,
    ) -> Result<(), RenderError> {
        let (width, height) = camera.viewport_size();
        if !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        self.placed.clear();
        self.viewport.update(queue, Resolution { width, height });
        if self.index.is_none() || width == 0 || height == 0 {
            return Ok(());
        }

        let columns = ((width as f32 / COLLISION_CELL_WIDTH).ceil() as usize).max(1);
        let rows = ((height as f32 / COLLISION_CELL_HEIGHT).ceil() as usize).max(1);
        let cells = columns.saturating_mul(rows);
        if self.collision_stamps.len() < cells {
            self.collision_stamps.resize(cells, 0);
        }
        self.collision_generation = self.collision_generation.wrapping_add(1).max(1);
        let generation = self.collision_generation;
        let matrix = camera.view_projection_matrix();
        let family_mask = view.family_mask().0;
        let entity_family_mask = view.entity_families.0;
        let topology_family_mask = view.topology_families.0;
        let scope_mask = view.scope_mask().0;
        let hover_slot = focus.hover.and_then(|id| scene.node_slot(id));
        let selected_slot = focus.selected.and_then(|id| scene.node_slot(id));
        for (entry_index, entry) in self.entries.iter().enumerate() {
            if Some(entry.node_slot) != hover_slot && Some(entry.node_slot) != selected_slot {
                continue;
            }
            let entity_lanes = entry.family_mask & FamilyMask::ENTITY_LANES.0;
            let topology_lanes = entry.family_mask & FamilyMask::TOPOLOGY_LANES.0;
            if entry.family_mask & family_mask == 0
                || (entity_lanes != 0 && entity_lanes & entity_family_mask == 0)
                || (topology_lanes != 0 && topology_lanes & topology_family_mask == 0)
                || entry.scope_mask & scope_mask == 0
                || entry.review_mask & view.reviews.0 == 0
            {
                continue;
            }
            let Some(node) = scene.node_at_slot(entry.node_slot) else {
                continue;
            };
            let clip = matrix * glam::Vec3::from_array(node.position).extend(1.0);
            if clip.w <= 0.0 {
                continue;
            }
            let ndc = clip.truncate() / clip.w;
            if ndc.z < 0.0 || ndc.z > 1.0 || ndc.x.abs() > 1.05 || ndc.y.abs() > 1.05 {
                continue;
            }
            let left = (ndc.x * 0.5 + 0.5) * width as f32 + 8.0;
            let top = (0.5 - ndc.y * 0.5) * height as f32 - LABEL_HEIGHT * 0.5;
            if left + entry.width > width as f32 || top < 0.0 || top + LABEL_HEIGHT > height as f32
            {
                continue;
            }
            if collides(
                &mut self.collision_stamps,
                generation,
                columns,
                rows,
                left,
                top,
                entry.width,
            ) {
                continue;
            }
            self.placed.push(PlacedLabel {
                entry: entry_index as u16,
                left,
                top,
            });
            if self.placed.len() == MAX_VISIBLE_LABELS {
                break;
            }
        }

        let entries = &self.entries;
        let areas = self.placed.iter().map(|placed| {
            let entry = &entries[placed.entry as usize];
            TextArea {
                buffer: &entry.buffer,
                left: placed.left,
                top: placed.top,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: Color::rgba(221, 235, 231, 225),
                custom_glyphs: &[],
            }
        });
        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )
            .map_err(|error| RenderError::LabelPrepare(error.to_string()))?;
        tracing::debug!(
            visible_labels = self.placed.len(),
            max_visible_labels = MAX_VISIBLE_LABELS,
            "graph label collision pass prepared"
        );
        Ok(())
    }

    pub(crate) fn render_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        if self.placed.is_empty() {
            return Ok(());
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("graph label overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        self.renderer
            .render(&self.atlas, &self.viewport, &mut pass)
            .map_err(|error| RenderError::LabelRender(error.to_string()))
    }

    pub(crate) fn trim(&mut self) {
        self.atlas.trim();
    }
}

fn collides(
    stamps: &mut [u32],
    generation: u32,
    columns: usize,
    rows: usize,
    left: f32,
    top: f32,
    width: f32,
) -> bool {
    let first_x = (left / COLLISION_CELL_WIDTH).floor().max(0.0) as usize;
    let last_x = ((left + width) / COLLISION_CELL_WIDTH).floor().max(0.0) as usize;
    let first_y = (top / COLLISION_CELL_HEIGHT).floor().max(0.0) as usize;
    let last_y = ((top + LABEL_HEIGHT) / COLLISION_CELL_HEIGHT)
        .floor()
        .max(0.0) as usize;
    let first_x = first_x.min(columns - 1);
    let last_x = last_x.min(columns - 1);
    let first_y = first_y.min(rows - 1);
    let last_y = last_y.min(rows - 1);
    for y in first_y..=last_y {
        for x in first_x..=last_x {
            if stamps[y * columns + x] == generation {
                return true;
            }
        }
    }
    for y in first_y..=last_y {
        for x in first_x..=last_x {
            stamps[y * columns + x] = generation;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_grid_rejects_overlap_without_allocating() {
        let mut stamps = vec![0; 16];
        assert!(!collides(&mut stamps, 1, 4, 4, 2.0, 2.0, 70.0));
        assert!(collides(&mut stamps, 1, 4, 4, 20.0, 4.0, 70.0));
        assert!(!collides(&mut stamps, 2, 4, 4, 20.0, 4.0, 70.0));
    }
}
