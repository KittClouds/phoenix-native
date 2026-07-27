use super::{
    graph_viewport, PhoenixShell, BORDER, BORDER_BRIGHT, CANVAS, SURFACE, TEXT, TEXT_MUTED,
};
use crate::lifecycle;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, Window};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::PixelsExt;
use phoenix_app_core::KernelOutcome;
use std::rc::Rc;
use std::sync::Arc;

pub(super) const DRAWER_INITIAL_HEIGHT: f32 = 420.;
pub(super) const DRAWER_MIN_HEIGHT: f32 = 280.;
pub(super) const DRAWER_MAX_HEIGHT: f32 = 720.;
pub(super) const EDITOR_MIN_HEIGHT: f32 = 260.;

const ATLAS_INITIAL_WIDTH: f32 = 304.;
const ATLAS_MIN_WIDTH: f32 = 236.;
const ATLAS_MAX_WIDTH: f32 = 460.;
const GRAPH_MIN_WIDTH: f32 = 360.;
pub(super) const ACCENT: u32 = 0x57e2bb;
pub(super) const ACCENT_DIM: u32 = 0x173b32;
const GRAPH_TABS: [&str; 5] = [
    "GRAPH",
    "PATTERNS",
    "PLOT THREADS",
    "WORLDBUILDING",
    "ATLAS CONTROL",
];

fn atlas_split_id(left_open: bool, right_open: bool) -> &'static str {
    match (left_open, right_open) {
        (true, true) => "atlas-graph-split-both",
        (true, false) => "atlas-graph-split-left",
        (false, true) => "atlas-graph-split-right",
        (false, false) => "atlas-graph-split-center",
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DrawerLayout {
    open: bool,
    height: f32,
    atlas_width: f32,
}

impl DrawerLayout {
    pub(super) const fn new(open: bool) -> Self {
        Self {
            open,
            height: DRAWER_INITIAL_HEIGHT,
            atlas_width: ATLAS_INITIAL_WIDTH,
        }
    }

    pub(super) const fn is_open(self) -> bool {
        self.open
    }

    pub(super) const fn height(self) -> f32 {
        self.height
    }

    pub(super) const fn atlas_width(self) -> f32 {
        self.atlas_width
    }

    pub(super) fn set_height(&mut self, height: f32) {
        self.height = height.clamp(DRAWER_MIN_HEIGHT, DRAWER_MAX_HEIGHT);
    }

    pub(super) fn set_atlas_width(&mut self, width: f32) {
        self.atlas_width = width.clamp(ATLAS_MIN_WIDTH, ATLAS_MAX_WIDTH);
    }

    fn toggle(&mut self) {
        self.open = !self.open;
    }
}

impl PhoenixShell {
    pub(super) fn toggle_drawer(&mut self, cx: &mut Context<Self>) {
        if self.drawer_layout.is_open() {
            if let Some(graph) = self.graph.borrow().as_ref() {
                if let Err(error) = graph.hide_viewport() {
                    lifecycle::mark_proof_failed();
                    self.status = format!("GRAPH BLOCKED / {error:#}").into();
                    cx.notify();
                    return;
                }
            }
        }
        self.drawer_layout.toggle();
        self.status = if self.drawer_layout.is_open() {
            "ATLAS / DRAWER OPEN / RESIDENT GRAPH READY".into()
        } else {
            "ATLAS / DRAWER CLOSED / RESIDENT GRAPH RETAINED".into()
        };
        cx.notify();
    }

    pub(super) fn entity_count(&self) -> usize {
        self.kernel_snapshot()
            .map(|snapshot| snapshot.atlas_registry.entities.len())
            .unwrap_or(0)
    }

    pub(super) fn start_native_scene_rebuild(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.graph_rebuild_pending {
            return;
        }
        self.graph_rebuild_pending = true;
        self.status = "GRAPH REBUILD / COMPILING VERIFIED ACTIVE DOCUMENT".into();
        cx.notify();
        let kernel = Arc::clone(&self.kernel);
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |shell, async_cx| {
            let result = background
                .spawn(async move { kernel.rebuild_active_scene() })
                .await;
            if let Err(error) = shell.update(async_cx, |this, cx| {
                this.graph_rebuild_pending = false;
                match result {
                    Ok(command) => match command.outcome {
                        KernelOutcome::GraphRebuilt(receipt) => {
                            let sync = this
                                .graph
                                .borrow()
                                .as_ref()
                                .map(|graph| graph.sync_kernel_state());
                            match sync {
                                Some(Ok(())) => {
                                    this.status = format!(
                                        "GRAPH REBUILT / G{} / {}N / {}E / {} US",
                                        receipt.publication.generation_id,
                                        receipt.publication.node_count,
                                        receipt.publication.edge_count,
                                        receipt.compile.compile_micros
                                    )
                                    .into();
                                }
                                Some(Err(error)) => {
                                    lifecycle::mark_proof_failed();
                                    this.status = format!(
                                        "GRAPH BLOCKED / G{} PUBLISHED / PRESENTATION {error:#}",
                                        receipt.publication.generation_id
                                    )
                                    .into();
                                }
                                None => {
                                    this.status = format!(
                                        "GRAPH BLOCKED / G{} PUBLISHED / HOST UNAVAILABLE",
                                        receipt.publication.generation_id
                                    )
                                    .into();
                                }
                            }
                        }
                        _ => {
                            lifecycle::mark_proof_failed();
                            this.status =
                                "GRAPH BLOCKED / REBUILD RECEIPT CONTRACT MISMATCH".into();
                        }
                    },
                    Err(error) => {
                        this.status = format!("GRAPH BLOCKED / REBUILD / {error}").into();
                    }
                }
                cx.notify();
            }) {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_NATIVE_REBUILD_DELIVERY_FAILED {error:#}");
            }
        })
        .detach();
    }

    pub(super) fn render_drawer_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let atlas_width = self.drawer_layout.atlas_width();
        let shell = cx.entity().clone();
        let split_id = atlas_split_id(self.left_open, self.right_open);
        let split = h_resizable(split_id)
            .child(
                resizable_panel()
                    .size(px(atlas_width))
                    .size_range(px(ATLAS_MIN_WIDTH)..px(ATLAS_MAX_WIDTH))
                    .child(self.render_atlas_sidebar(cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(GRAPH_MIN_WIDTH)..gpui::Pixels::MAX)
                    .child(self.render_graph_panel()),
            )
            .on_resize(move |state, _, cx| {
                let width = state.read(cx).sizes().first().map(|width| width.as_f32());
                if let Some(width) = width {
                    shell.update(cx, |shell, _| {
                        shell.drawer_layout.set_atlas_width(width);
                    });
                }
            });

        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(BORDER_BRIGHT))
            .bg(rgb(SURFACE))
            .child(self.render_drawer_tabs(cx))
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .child(split),
            )
    }

    fn render_graph_panel(&self) -> gpui::AnyElement {
        if let Some(error) = self.scene_error.as_ref() {
            return scene_error_panel(error.code(), error.title(), error.detail())
                .into_any_element();
        }
        if let Some(error) = self.graph_init_error.as_deref() {
            return scene_error_panel(
                "PHX_GRAPH_HOST_BLOCKED",
                "Native graph host unavailable",
                error,
            )
            .into_any_element();
        }
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(rgb(CANVAS))
            .child(graph_viewport::graph_viewport(
                Rc::clone(&self.graph),
                Rc::clone(&self.graph_geometry),
                true,
            ))
            .into_any_element()
    }

    fn render_drawer_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs = div()
            .h(px(42.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x171918));
        for (index, label) in GRAPH_TABS.into_iter().enumerate() {
            tabs = tabs.child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .px_3()
                    .text_xs()
                    .text_color(rgb(if index == 0 { TEXT } else { TEXT_MUTED }))
                    .when(index == 0, |tab| {
                        tab.border_b_2().border_color(rgb(ACCENT)).bg(rgb(0x1d2321))
                    })
                    .when(index != 0, |tab| tab.opacity(0.58))
                    .child(label),
            );
        }
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(tabs.overflow_hidden())
            .child(self.render_graph_controls(cx))
    }
}

fn scene_error_panel(code: &str, title: &str, detail: &str) -> impl IntoElement {
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .p_8()
        .bg(rgb(0x101716))
        .child(
            div()
                .w_full()
                .min_w_0()
                .max_w(px(560.))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x2a4a42))
                .bg(rgb(0x17201e))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ACCENT))
                        .child(code.to_owned()),
                )
                .child(
                    div()
                        .mt_2()
                        .text_lg()
                        .text_color(rgb(TEXT))
                        .child(title.to_owned()),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .mt_3()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(detail.to_owned()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawer_sizes_survive_close_and_reopen() {
        let mut layout = DrawerLayout::new(true);
        layout.set_height(512.);
        layout.set_atlas_width(372.);
        layout.toggle();
        layout.toggle();
        assert!(layout.is_open());
        assert_eq!(layout.height(), 512.);
        assert_eq!(layout.atlas_width(), 372.);
    }

    #[test]
    fn drawer_sizes_fail_closed_to_supported_ranges() {
        let mut layout = DrawerLayout::new(false);
        layout.set_height(9.);
        layout.set_atlas_width(f32::MAX);
        assert_eq!(layout.height(), DRAWER_MIN_HEIGHT);
        assert_eq!(layout.atlas_width(), ATLAS_MAX_WIDTH);
    }

    #[test]
    fn inactive_tabs_are_compile_time_markup_only() {
        assert_eq!(GRAPH_TABS[0], "GRAPH");
        assert_eq!(
            &GRAPH_TABS[1..],
            &["PATTERNS", "PLOT THREADS", "WORLDBUILDING", "ATLAS CONTROL"]
        );
    }

    #[test]
    fn sidebar_topologies_get_independent_drawer_measurements() {
        let ids = [
            atlas_split_id(true, true),
            atlas_split_id(true, false),
            atlas_split_id(false, true),
            atlas_split_id(false, false),
        ];
        for (index, id) in ids.iter().enumerate() {
            for other in &ids[index + 1..] {
                assert_ne!(id, other);
            }
        }
    }
}
