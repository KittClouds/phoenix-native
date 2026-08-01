use super::{
    graph_viewport, PhoenixShell, BORDER, BORDER_BRIGHT, CANVAS, SURFACE, TEXT, TEXT_MUTED,
};
use crate::lifecycle;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, Window};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::PixelsExt;
use phoenix_app_core::{KernelCommand, KernelError, KernelOutcome};
use phoenix_scene_contract::Manifold;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::sync::Arc;

pub(super) const DRAWER_INITIAL_HEIGHT: f32 = 420.;
pub(super) const DRAWER_MIN_HEIGHT: f32 = 280.;

const ATLAS_INITIAL_WIDTH: f32 = 336.;
const ATLAS_MIN_WIDTH: f32 = 236.;
const ATLAS_MAX_WIDTH: f32 = 520.;
const GRAPH_MIN_WIDTH: f32 = 360.;
pub(super) const ACCENT: u32 = 0x57e2bb;
pub(super) const ACCENT_DIM: u32 = 0x173b32;
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DrawerTab {
    Graph,
    Patterns,
    PlotThreads,
    Worldbuilding,
    AtlasControl,
}

impl DrawerTab {
    const ALL: [Self; 5] = [
        Self::Graph,
        Self::Patterns,
        Self::PlotThreads,
        Self::Worldbuilding,
        Self::AtlasControl,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Graph => "GRAPH",
            Self::Patterns => "PATTERNS",
            Self::PlotThreads => "PLOT THREADS",
            Self::Worldbuilding => "WORLDBUILDING",
            Self::AtlasControl => "ATLAS CONTROL",
        }
    }

    const fn is_active_product(self) -> bool {
        matches!(self, Self::Graph | Self::AtlasControl)
    }
}

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
    full_page: bool,
    height: f32,
    atlas_width: f32,
}

impl DrawerLayout {
    pub(super) const fn new(open: bool) -> Self {
        Self {
            open,
            full_page: false,
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

    pub(super) const fn is_full_page(self) -> bool {
        self.full_page
    }

    pub(super) const fn atlas_width(self) -> f32 {
        self.atlas_width
    }

    pub(super) const fn snapshot(self) -> (bool, bool, f32, f32) {
        (self.open, self.full_page, self.height, self.atlas_width)
    }

    pub(super) fn restore(open: bool, full_page: bool, height: f32, atlas_width: f32) -> Self {
        let mut layout = Self::new(open);
        layout.full_page = open && full_page;
        layout.set_height(height);
        layout.set_atlas_width(atlas_width);
        layout
    }

    pub(super) fn set_height(&mut self, height: f32) {
        if height.is_finite() {
            self.height = height.max(DRAWER_MIN_HEIGHT);
        }
    }

    pub(super) fn set_atlas_width(&mut self, width: f32) {
        self.atlas_width = width.clamp(ATLAS_MIN_WIDTH, ATLAS_MAX_WIDTH);
    }

    fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.full_page = false;
        }
    }

    fn toggle_full_page(&mut self) {
        self.open = true;
        self.full_page = !self.full_page;
    }
}

impl PhoenixShell {
    pub(super) fn select_drawer_tab(
        &mut self,
        tab: DrawerTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !tab.is_active_product() || self.drawer_tab == tab {
            return;
        }
        if tab != DrawerTab::Graph {
            if let Some(graph) = self.graph.borrow().as_ref() {
                if let Err(error) = graph.hide_viewport() {
                    lifecycle::mark_proof_failed();
                    self.status = format!("GRAPH BLOCKED / {error:#}").into();
                    cx.notify();
                    return;
                }
            }
        }
        self.drawer_tab = tab;
        self.status = match tab {
            DrawerTab::Graph => "GRAPH / RESIDENT GENERATION".into(),
            DrawerTab::AtlasControl => "ATLAS CONTROL / NATIVE AUTHORITY".into(),
            _ => unreachable!("inactive tabs cannot be selected"),
        };
        if tab == DrawerTab::AtlasControl {
            self.focus_atlas_control(window);
        }
        cx.notify();
    }

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

    pub(super) fn toggle_drawer_full_page(&mut self, cx: &mut Context<Self>) {
        self.drawer_layout.toggle_full_page();
        self.status = if self.drawer_layout.is_full_page() {
            "ATLAS / FULL PAGE / RESIDENT GRAPH READY".into()
        } else {
            "ATLAS / DRAWER / RESIDENT GRAPH READY".into()
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
        self.status = "PIPELINE / ANALYZING VERIFIED ACTIVE DOCUMENT".into();
        cx.notify();
        let kernel = Arc::clone(&self.kernel);
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |shell, async_cx| {
            let result = background
                .spawn(async move { kernel.run_active_document_pipeline() })
                .await;
            if let Err(error) = shell.update_in(async_cx, |this, window, cx| {
                this.graph_rebuild_pending = false;
                match result {
                    Ok(command) => match command.outcome {
                        KernelOutcome::GraphRebuilt(receipt) => {
                            let caps = this
                                .kernel
                                .execute(KernelCommand::SetManifold(Manifold::Caps));
                            if let Err(error) = caps {
                                lifecycle::mark_proof_failed();
                                this.status = format!(
                                    "PIPELINE PUBLISHED / G{} / CAPS BLOCKED / {error}",
                                    receipt.publication.generation_id
                                )
                                .into();
                                cx.notify();
                                return;
                            }
                            this.apply_kernel_highlights(cx);
                            let sync = this
                                .graph
                                .borrow()
                                .as_ref()
                                .map(|graph| graph.sync_kernel_state());
                            match sync {
                                Some(Ok(())) => {
                                    this.status = format!(
                                        "PIPELINE LIVE / G{} / CAPS / {}N / {}E / {} US",
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
                                        "PIPELINE BLOCKED / G{} PUBLISHED / PRESENTATION {error:#}",
                                        receipt.publication.generation_id
                                    )
                                    .into();
                                }
                                None => match graph_viewport::parent_window_handle(window) {
                                    Ok(parent) => {
                                        this.graph_init_error = None;
                                        this.status = format!(
                                            "PIPELINE PUBLISHED / G{} / STARTING GRAPH HOST",
                                            receipt.publication.generation_id
                                        )
                                        .into();
                                        this.start_graph_host(parent, window, cx);
                                    }
                                    Err(error) => {
                                        lifecycle::mark_proof_failed();
                                        this.graph_init_error = Some(format!("{error:#}"));
                                        this.status = format!(
                                            "PIPELINE BLOCKED / G{} PUBLISHED / HOST {error:#}",
                                            receipt.publication.generation_id
                                        )
                                        .into();
                                    }
                                },
                            }
                        }
                        _ => {
                            lifecycle::mark_proof_failed();
                            this.status = "PIPELINE BLOCKED / RECEIPT CONTRACT MISMATCH".into();
                        }
                    },
                    Err(KernelError::AnalysisProducerCancelled) => {
                        this.status = "PIPELINE CANCELLED / PREVIOUS GENERATION PRESERVED".into();
                    }
                    Err(error) => {
                        eprintln!("PHOENIX_NATIVE_PIPELINE_FAILED {error:#}");
                        this.status = format!("PIPELINE BLOCKED / {error}").into();
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

    pub(super) fn render_drawer_surface(
        &self,
        tabs_in_app_header: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let body = match self.drawer_tab {
            DrawerTab::Graph => self.render_graph_drawer(cx),
            DrawerTab::AtlasControl => self.render_atlas_control(window, cx).into_any_element(),
            _ => scene_error_panel(
                "PHX_DORMANT_SURFACE",
                "This surface is not active",
                "Dormant tabs carry no routes, commands, or data models.",
            )
            .into_any_element(),
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(!tabs_in_app_header, |surface| surface.border_t_1())
            .border_color(rgb(BORDER_BRIGHT))
            .bg(rgb(SURFACE))
            .child(self.render_drawer_tabs(tabs_in_app_header, cx))
            .child(body)
    }

    fn render_graph_drawer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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
            .bg(rgb(SURFACE))
            .child(self.render_graph_controls(cx))
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .child(split),
            )
            .into_any_element()
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

    fn render_drawer_tabs(&self, in_app_header: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs = div()
            .h(px(if in_app_header { 44. } else { 42. }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(if in_app_header { SURFACE } else { 0x171918 }));
        for tab in DrawerTab::ALL {
            let selected = self.drawer_tab == tab;
            let enabled = tab.is_active_product();
            tabs = tabs.child(
                div()
                    .id(("drawer-tab", tab as usize))
                    .h_full()
                    .flex()
                    .items_center()
                    .px_3()
                    .text_xs()
                    .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                    .when(selected, |item| {
                        item.border_b_2()
                            .border_color(rgb(ACCENT))
                            .bg(rgb(0x1d2321))
                    })
                    .when(enabled, |item| {
                        item.cursor_pointer()
                            .hover(|hover| hover.bg(rgb(0x202624)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_drawer_tab(tab, window, cx);
                            }))
                    })
                    .when(!enabled, |item| item.opacity(0.42))
                    .child(tab.label()),
            );
        }
        tabs.overflow_hidden()
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
        layout.set_height(1_440.);
        assert_eq!(layout.height(), 1_440.);
        layout.set_height(f32::NAN);
        assert_eq!(layout.height(), 1_440.);
    }

    #[test]
    fn full_page_is_explicit_and_closing_clears_it() {
        let mut layout = DrawerLayout::new(false);
        layout.toggle_full_page();
        assert!(layout.is_open());
        assert!(layout.is_full_page());
        layout.toggle();
        assert!(!layout.is_open());
        assert!(!layout.is_full_page());
    }

    #[test]
    fn inactive_tabs_are_compile_time_markup_only() {
        assert!(DrawerTab::Graph.is_active_product());
        assert!(DrawerTab::AtlasControl.is_active_product());
        assert!(!DrawerTab::Patterns.is_active_product());
        assert!(!DrawerTab::PlotThreads.is_active_product());
        assert!(!DrawerTab::Worldbuilding.is_active_product());
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
