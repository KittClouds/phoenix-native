use super::GraphRenderer;
use crate::{PreparedGeometryMetrics, RenderError};
use phoenix_scene_archive::{GuidePageView, PathPageView};

impl GraphRenderer {
    pub fn set_prepared_geometry(
        &mut self,
        guides: Option<GuidePageView<'_>>,
        paths: Option<PathPageView<'_>>,
    ) -> Result<PreparedGeometryMetrics, RenderError> {
        let metrics = self.prepared_paths.install(
            guides,
            paths,
            self.scene.edge_draw_slots() as usize,
            &self.device,
            &self.queue,
        )?;
        self.redraw_requested = true;
        Ok(metrics)
    }
}
