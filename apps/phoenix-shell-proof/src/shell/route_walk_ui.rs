//! Guided route walk transport. The graph thread owns the frozen route and
//! the animation; the shell only sends transport commands and renders the
//! status it publishes at step boundaries.

use super::{drawer::ACCENT, PhoenixShell, TEXT, TEXT_MUTED};
use crate::graph_window::RouteWalkRequest;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Sizable};
use graph_render_wgpu::{RouteWalkCommand, RouteWalkNotice, RouteWalkPlayback, RouteWalkStatus};

const STRIP_BG: u32 = 0x0f1720;
const WALK_WARN: u32 = 0xe3b26a;
const STEP_FUTURE: u32 = 0x2a3a38;
const STEP_VISITED: u32 = 0x3f9c83;
const STEP_CURRENT: u32 = 0xf4f1e6;

impl PhoenixShell {
    pub(super) fn route_walk_status(&self) -> Option<RouteWalkStatus> {
        self.graph
            .borrow()
            .as_ref()
            .map(|graph| graph.route_walk_status())
    }

    /// The strip is visible while a walk runs, or until an outcome from a
    /// refused start is dismissed.
    pub(super) fn route_walk_strip_visible(&self, status: &RouteWalkStatus) -> bool {
        status.active || (status.notice.is_some() && status.revision != self.route_walk_dismissed)
    }

    pub(super) fn render_route_walk_strip(
        &self,
        status: &RouteWalkStatus,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut strip = super::graph_toolbar::mode_strip(STRIP_BG)
            .child(super::graph_toolbar::strip_mark("Route walk", ACCENT));
        if status.active {
            let snapshot = self.kernel_snapshot();
            let label = |id: graph_model::NodeId| {
                snapshot
                    .as_ref()
                    .and_then(|snapshot| super::source_local::node_label(snapshot, id.0))
                    .unwrap_or_else(|| format!("Node {}", id.0))
            };
            let route = status
                .endpoints
                .map(|(from, to)| format!("{} → {}", label(from), label(to)))
                .unwrap_or_default();
            strip = strip
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(route),
                        )
                        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(format!(
                            "Step {} / {}",
                            status.position + 1,
                            status.node_count
                        ))),
                )
                .child(step_track(status));
        }
        if let Some(notice) = status.notice {
            strip = strip.child(
                div()
                    .text_xs()
                    .text_color(rgb(if notice == RouteWalkNotice::Arrived {
                        ACCENT
                    } else {
                        WALK_WARN
                    }))
                    .child(notice_text(notice)),
            );
        }
        strip = strip.child(div().flex_1());
        if status.active {
            let playing = status.playback == RouteWalkPlayback::Playing;
            strip = strip
                .child(transport_button(
                    "route-walk-previous",
                    "‹ Previous",
                    status.position == 0 && status.traversing_to.is_none(),
                    RouteWalkCommand::Previous,
                    cx,
                ))
                .child(
                    Button::new("route-walk-play")
                        .label(if playing { "Pause" } else { "Play" })
                        .small()
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let command = if playing {
                                RouteWalkCommand::Pause
                            } else {
                                RouteWalkCommand::Play
                            };
                            this.send_route_walk(RouteWalkRequest::Transport(command), cx);
                        })),
                )
                .child(transport_button(
                    "route-walk-next",
                    "Next ›",
                    status.position + 1 >= status.node_count && status.traversing_to.is_none(),
                    RouteWalkCommand::Next,
                    cx,
                ))
                .child(
                    Button::new("route-walk-exit")
                        .label("Exit")
                        .tooltip("Leave the walk. The atlas returns to its prior view.")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send_route_walk(RouteWalkRequest::Exit, cx);
                        })),
                );
        } else {
            let revision = status.revision;
            strip = strip.child(
                Button::new("route-walk-dismiss")
                    .label("Dismiss")
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.route_walk_dismissed = revision;
                        cx.notify();
                    })),
            );
        }
        strip
    }

    pub(super) fn render_flow_strip(
        &self,
        flow: graph_render_wgpu::FlowStatus,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        super::graph_toolbar::mode_strip(STRIP_BG)
            .child(super::graph_toolbar::strip_mark("Flow", ACCENT))
            .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(format!(
                "{} document{} \u{b7} {} connections \u{b7} {} levels",
                flow.documents,
                if flow.documents == 1 { "" } else { "s" },
                flow.connections,
                flow.levels
            )))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(if flow.settled { ACCENT } else { TEXT }))
                    .child(if flow.settled { "Settled" } else { "Flowing\u{2026}" }),
            )
            .child(div().flex_1())
            .child(
                Button::new("document-flow-replay")
                    .label("Replay")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_route_walk(RouteWalkRequest::FlowStart, cx);
                    })),
            )
            .child(
                Button::new("document-flow-exit")
                    .label("Exit")
                    .tooltip("Leave the flow. The atlas returns to its prior view.")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_route_walk(RouteWalkRequest::FlowExit, cx);
                    })),
            )
    }

    pub(super) fn send_route_walk(&mut self, request: RouteWalkRequest, cx: &mut Context<Self>) {
        let result = self
            .graph
            .borrow()
            .as_ref()
            .map(|graph| graph.route_walk(request));
        self.status = match result {
            Some(Ok(())) => format!("ROUTE WALK / {request:?}").to_uppercase().into(),
            Some(Err(error)) => format!("ROUTE WALK BLOCKED / {error:#}").into(),
            None => "ROUTE WALK / GRAPH OPTIONAL".into(),
        };
        cx.notify();
    }
}

fn transport_button(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    command: RouteWalkCommand,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    Button::new(id)
        .label(label)
        .small()
        .ghost()
        .disabled(disabled)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.send_route_walk(RouteWalkRequest::Transport(command), cx);
        }))
}

/// One mark per route node: walked, current, and ahead read at a glance.
fn step_track(status: &RouteWalkStatus) -> impl IntoElement {
    let count = status.node_count.max(1);
    let width = (160.0 / count as f32).clamp(2.0, 12.0);
    let mut track = div().flex().items_center().gap(px(2.));
    for index in 0..status.node_count {
        let (color, height) = if index == status.position {
            (STEP_CURRENT, 10.)
        } else if index < status.position {
            (STEP_VISITED, 6.)
        } else if Some(index) == status.traversing_to {
            (ACCENT, 8.)
        } else {
            (STEP_FUTURE, 6.)
        };
        track = track.child(div().w(px(width)).h(px(height)).rounded_sm().bg(rgb(color)));
    }
    track
}

fn notice_text(notice: RouteWalkNotice) -> SharedString {
    match notice {
        RouteWalkNotice::NeedsEndpoints => {
            "Select a node, then Shift+click a second node to set the route end.".into()
        }
        RouteWalkNotice::NoPath => "No visible route connects these endpoints.".into(),
        RouteWalkNotice::OverBound { limit } => {
            format!("Route exceeds the {limit}-edge walk bound; the walk did not start.").into()
        }
        RouteWalkNotice::Arrived => "Arrived.".into(),
        RouteWalkNotice::Unavailable { step } => format!(
            "Step {} is hidden by the current view. The route is kept; show it to continue.",
            step + 1
        )
        .into(),
        RouteWalkNotice::Broken { step } => format!(
            "Route broken at step {}: that member left the scene.",
            step + 1
        )
        .into(),
        RouteWalkNotice::NoDocumentFlow => {
            "No visible document has connections to flow through.".into()
        }
        RouteWalkNotice::SceneChanged => {
            "The scene changed and the route could not be resolved; the walk ended.".into()
        }
    }
}
