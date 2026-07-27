use super::{EmbeddedGraphApp, PendingManifoldSwitch};
use anyhow::{anyhow, Context, Result};
use graph_model::{GraphRevision, NodeId};
use phoenix_app_core::GraphSelectionOrigin;
use phoenix_scene_archive::{PageKey, PageKind};
use std::sync::Arc;
use std::time::Instant;

impl EmbeddedGraphApp {
    pub(super) fn sync_resident_scene(&mut self) -> Result<()> {
        let snapshot = self.kernel.snapshot().context("read kernel snapshot")?;
        let scene = snapshot.resident_scene.ok_or_else(|| {
            anyhow!("[PHX_SCENE_MISSING] resident graph generation was withdrawn")
        })?;
        if self.loaded_generation == Some(scene.generation())
            && self.loaded_graph_view == snapshot.graph_view
            && self.loaded_selection_revision == snapshot.graph_selection.revision
        {
            return Ok(());
        }
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| anyhow!("graph renderer is unavailable"))?;
        let before_verifications = scene.archive().verified_page_count();
        let started = Instant::now();
        if self.loaded_generation != Some(scene.generation()) {
            let active = scene
                .activate_manifold(snapshot.graph_view.manifold)
                .context("open new resident manifold")?;
            renderer
                .set_archive_scene_bound(
                    GraphRevision(scene.generation().0),
                    scene.archive_identity().cohort_hash,
                    &active.pages,
                )
                .context("project new resident archive generation")?;
            renderer
                .set_prepared_geometry(active.guides, active.prepared_paths)
                .context("install replacement prepared guide/path pages")?;
            if let Some(index) = snapshot.scene_product_index.as_ref() {
                install_product_index(renderer, &scene, index)?;
            }
            renderer
                .set_graph_view(snapshot.graph_view)
                .context("install resident graph view")?;
            self.loaded_generation = Some(scene.generation());
            self.loaded_manifold = snapshot.graph_view.manifold;
            self.loaded_graph_view = snapshot.graph_view;
            self.pending_switch = None;
        } else {
            if self.loaded_graph_view.authority.product_index_hash()
                != snapshot.graph_view.authority.product_index_hash()
            {
                let index = snapshot.scene_product_index.as_ref().ok_or_else(|| {
                    anyhow!("[PHX_PRODUCT_INDEX_MISSING] graph view names a missing index")
                })?;
                install_product_index(renderer, &scene, index)?;
            }
            if self.loaded_manifold != snapshot.graph_view.manifold {
                let active = scene
                    .activate_manifold(snapshot.graph_view.manifold)
                    .context("open switched resident manifold")?;
                let from = self.loaded_manifold;
                let metrics = renderer
                    .switch_archive_positions(active.pages.positions)
                    .context("switch resident manifold positions")?;
                renderer
                    .set_prepared_geometry(active.guides, active.prepared_paths)
                    .context("switch resident prepared guide/path pages")?;
                let cpu_us = started.elapsed().as_micros();
                let receipt = super::ManifoldSwitchReceipt {
                    contract: "phoenix.native.manifold-switch/v1",
                    generation: scene.generation(),
                    from,
                    to: snapshot.graph_view.manifold,
                    node_count: metrics.node_count,
                    positions_bytes: std::mem::size_of_val(active.pages.positions),
                    hot_page_count: active.hot_pages.page_count,
                    hot_page_bytes: active.hot_pages.byte_len,
                    page_verifications: scene
                        .archive()
                        .verified_page_count()
                        .saturating_sub(before_verifications),
                    cpu_us,
                    first_present_us: 0,
                };
                self.loaded_manifold = snapshot.graph_view.manifold;
                self.manifold_switches = self.manifold_switches.saturating_add(1);
                self.max_hot_page_bytes = self.max_hot_page_bytes.max(active.hot_pages.byte_len);
                self.switch_cpu_samples.push(cpu_us);
                self.pending_switch = Some(PendingManifoldSwitch { receipt, started });
            }
            renderer
                .set_graph_view(snapshot.graph_view)
                .context("update graph lens uniform")?;
            self.loaded_graph_view = snapshot.graph_view;
        }
        if self.loaded_selection_revision != snapshot.graph_selection.revision {
            renderer
                .set_external_selection(
                    snapshot.graph_selection.node_id.map(NodeId),
                    snapshot.graph_selection.origin == GraphSelectionOrigin::Atlas,
                )
                .context("synchronize kernel graph selection")?;
            self.loaded_selection_revision = snapshot.graph_selection.revision;
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        Ok(())
    }
}

fn install_product_index(
    renderer: &mut graph_render_wgpu::GraphRenderer,
    scene: &phoenix_scene_contract::ResidentScene,
    index: &Arc<phoenix_scene_product_index::PhoenixSceneProductIndexV1>,
) -> Result<()> {
    if scene
        .archive()
        .has_page(PageKey::shared(PageKind::LabelPriority))
    {
        let priorities = scene
            .archive()
            .typed_page(PageKey::shared(PageKind::LabelPriority))
            .context("open resident label-priority page")?;
        renderer
            .set_product_index_shared(Arc::clone(index), priorities)
            .context("install resident labelled product index")?;
    } else {
        tracing::warn!(
            generation = scene.generation().0,
            code = "PHX_VISUAL_PAGE_MIGRATION_REQUIRED",
            "resident pre-Cut-5 archive is unlabelled until the next native rebuild"
        );
        renderer
            .set_product_index(index)
            .context("install pre-Cut-5 product index without labels")?;
    }
    Ok(())
}
