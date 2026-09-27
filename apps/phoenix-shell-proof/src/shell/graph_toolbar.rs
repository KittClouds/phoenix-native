//! The atlas toolbar: one compact row of graph controls, an optional inline
//! shelf for rarely changed settings, and the graph revision chip (G0).
//!
//! Shelves render inline under the toolbar rather than as floating popovers,
//! because the native graph child window sits above anything drawn over the
//! canvas.

use super::drawer::{DrawerTab, ACCENT};
use super::graph_controls::{
    available_edge_presentations, document_detail_visible, edge_presentation_label, manifold_label,
    navigation_overlay_label, relation_label, scope_label, set_document_detail,
    topology_emphasis_label,
};
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use crate::graph_window::RouteWalkRequest;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{IconName, Sizable};
use phoenix_scene_contract::{
    GraphAction, GraphCanvas, GraphEdgePresentation, GraphNavigationOverlay, GraphProjection,
    GraphScope, GraphSurface, GraphTopologyEmphasis, GraphViewState, Manifold, RelationFamily,
    RelationMask, ReviewMask,
};

pub(super) const BAR_BG: u32 = 0x111514;
const SHELF_BG: u32 = 0x0d1110;
const SEG_BG: u32 = 0x0b0e0d;
const SEG_ACTIVE: u32 = 0x1a3a32;
const SEG_ACTIVE_TEXT: u32 = 0x8ff0d2;
const HOVER: u32 = 0x1a201e;
const DIVIDER: u32 = 0x252c2a;
const QUIET: u32 = 0x6f7a76;
const WARN_BG: u32 = 0x33270f;
const WARN_TEXT: u32 = 0xf0c27a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GraphShelf {
    Display,
    Filters,
}

/// Whether the resident verified graph matches the open note. 3C, 4B, and
/// 4C report unavailability through this same comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GraphCurrency {
    /// No verified generation is installed (registry-only or empty scene).
    NoVerifiedGraph,
    /// No note is open to compare against.
    NoNote,
    Current {
        revision: u64,
    },
    /// The graph was built from an older revision of the open note.
    Behind {
        graph: u64,
        note: u64,
    },
    /// Same revision number, different content: treat as behind.
    ContentChanged {
        revision: u64,
    },
    /// The graph belongs to another note.
    OtherNote {
        graph_document: u64,
    },
}

/// `(document id, revision, content hash)` for the graph and the open note.
pub(super) fn classify_currency(
    graph: Option<(u64, u64, [u8; 32])>,
    note: Option<(u64, u64, [u8; 32])>,
) -> GraphCurrency {
    let Some((graph_document, graph_revision, graph_hash)) = graph else {
        return GraphCurrency::NoVerifiedGraph;
    };
    let Some((note_document, note_revision, note_hash)) = note else {
        return GraphCurrency::NoNote;
    };
    if graph_document != note_document {
        GraphCurrency::OtherNote { graph_document }
    } else if graph_revision != note_revision {
        GraphCurrency::Behind {
            graph: graph_revision,
            note: note_revision,
        }
    } else if graph_hash != note_hash {
        GraphCurrency::ContentChanged {
            revision: note_revision,
        }
    } else {
        GraphCurrency::Current {
            revision: note_revision,
        }
    }
}

impl GraphCurrency {
    /// Reader glow (4B) pauses with "Graph is behind the note" only when the
    /// graph is an older view of the note being read; another note's graph
    /// simply has nothing to glow.
    pub(super) const fn reader_glow_behind(&self) -> bool {
        matches!(
            self,
            GraphCurrency::Behind { .. } | GraphCurrency::ContentChanged { .. }
        )
    }

    pub(super) const fn needs_rebuild(&self) -> bool {
        matches!(
            self,
            Self::NoVerifiedGraph
                | Self::Behind { .. }
                | Self::ContentChanged { .. }
                | Self::OtherNote { .. }
        )
    }
}

impl PhoenixShell {
    pub(super) fn graph_currency(&self) -> GraphCurrency {
        let snapshot = self.kernel_snapshot();
        let graph = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.graph_generation_v2.as_ref())
            .map(|generation| {
                let header = generation.header();
                (
                    header.native_document_id,
                    header.document_revision,
                    header.content_hash,
                )
            });
        let note = self
            .editor_lease
            .as_ref()
            .map(|lease| (lease.entry_id.0, lease.revision.0, lease.content_hash.0));
        classify_currency(graph, note)
    }

    fn entry_name(&self, id: u64) -> Option<String> {
        let snapshot = self.kernel_snapshot()?;
        snapshot
            .workspace
            .entry(phoenix_workspace::EntryId(id))
            .map(|entry| entry.name.clone())
    }

    /// Rebuilds the open note's graph, warming models first when they are not
    /// resident. The manifold is left exactly as the user set it.
    pub(super) fn rebuild_graph_from_chip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.graph_rebuild_pending || self.analysis_warm_pending {
            return;
        }
        if self.kernel.analysis_runtime_info().ready {
            self.start_registry_graph_refresh(window, cx);
        } else {
            self.rebuild_after_warm = true;
            self.start_analysis_model_warm(window, cx);
        }
    }

    pub(super) fn render_graph_toolbar(
        &self,
        view: GraphViewState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let atlas = view.surface == GraphSurface::Atlas;
        let walk_status = self.route_walk_status();
        let walk_active = walk_status.as_ref().is_some_and(|status| status.active);
        let flow_active = walk_status.as_ref().is_some_and(|status| status.flow.is_some());
        let story_active = walk_status.as_ref().is_some_and(|status| status.story.is_some());
        let filters = active_filter_count(view);
        let shelf = self.graph_shelf;
        let atlas_visible = !self.drawer_layout.atlas_collapsed();
        div()
            .w_full()
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .px_2()
            .overflow_hidden()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BAR_BG))
            .when(self.drawer_tabs_hidden, |bar| {
                bar.child(self.tab_switch(cx)).child(divider())
            })
            .child(
                Button::new("graph-toggle-atlas-sidebar")
                    .icon(if atlas_visible {
                        IconName::PanelLeftClose
                    } else {
                        IconName::PanelLeftOpen
                    })
                    .tooltip(if atlas_visible {
                        "Hide the entities sidebar"
                    } else {
                        "Show the entities sidebar"
                    })
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer_layout.toggle_atlas();
                        cx.notify();
                    })),
            )
            .child(divider())
            .child(segmented(
                "manifold",
                &Manifold::ALL.map(|manifold| (manifold, manifold_label(manifold))),
                view.manifold,
                |this, manifold, _, cx| this.dispatch_manifold(manifold, cx),
                cx,
            ))
            .child(segmented(
                "projection",
                &[
                    (GraphProjection::Spatial, "3D"),
                    (GraphProjection::Map, "Map"),
                ],
                view.projection,
                |this, projection, _, cx| {
                    this.mutate_graph_view(|next| next.projection = projection, "PROJECTION", cx)
                },
                cx,
            ))
            .child(divider())
            .when(atlas, |bar| {
                bar.child(tool_button(
                    "graph-shelf-filters",
                    if filters > 0 {
                        SharedString::from(format!("Filters · {filters}"))
                    } else {
                        "Filters".into()
                    },
                    shelf == Some(GraphShelf::Filters),
                    "Review states, relation families, and scope",
                    |this, _, cx| this.toggle_shelf(GraphShelf::Filters, cx),
                    cx,
                ))
            })
            .child(tool_button(
                "graph-shelf-display",
                "Display".into(),
                shelf == Some(GraphShelf::Display),
                "Edges, detail, emphasis, navigation, and canvas",
                |this, _, cx| this.toggle_shelf(GraphShelf::Display, cx),
                cx,
            ))
            .child(div().flex_1().min_w(px(4.)))
            .child(tool_button(
                "graph-source-local",
                "Source".into(),
                view.source_local,
                "Ghost everything not bound to the selection's verified source passages",
                |this, _, cx| {
                    this.source_open = Default::default();
                    this.mutate_graph_view(
                        |next| next.source_local = !next.source_local,
                        "SOURCE",
                        cx,
                    );
                },
                cx,
            ))
            .child(tool_button(
                "graph-route-walk",
                "Walk".into(),
                walk_active,
                "Select a node, Shift+click a second, then walk the route between them",
                move |this, _, cx| {
                    this.send_route_walk(
                        if walk_active {
                            RouteWalkRequest::Exit
                        } else {
                            RouteWalkRequest::Start
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(tool_button(
                "graph-document-flow",
                "Flow".into(),
                flow_active,
                "Cascade from each document through every connection it reaches",
                move |this, _, cx| {
                    this.send_route_walk(
                        if flow_active {
                            RouteWalkRequest::FlowExit
                        } else {
                            RouteWalkRequest::FlowStart
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(tool_button(
                "graph-story",
                "Story".into(),
                story_active,
                "Replay the atlas in reading order: scrub to see what the graph knew by then",
                move |this, _, cx| {
                    this.send_route_walk(
                        if story_active {
                            RouteWalkRequest::StoryExit
                        } else {
                            RouteWalkRequest::StoryStart
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(divider())
            .child(tool_button(
                "graph-fit",
                "Fit".into(),
                false,
                "Fit the graph to the view",
                |this, _, cx| this.dispatch_graph_action(GraphAction::Fit, cx),
                cx,
            ))
            .child(tool_button(
                "graph-reset",
                "Reset".into(),
                false,
                "Reset the camera",
                |this, _, cx| this.dispatch_graph_action(GraphAction::Reset, cx),
                cx,
            ))
            .child(self.currency_chip(cx))
    }

    fn toggle_shelf(&mut self, shelf: GraphShelf, cx: &mut Context<Self>) {
        self.graph_shelf = (self.graph_shelf != Some(shelf)).then_some(shelf);
        cx.notify();
    }

    fn tab_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(segmented(
                "drawer-tab-switch",
                &[
                    (DrawerTab::Graph, "Graph"),
                    (DrawerTab::AtlasControl, "Control"),
                ],
                self.drawer_tab,
                |this, tab, window, cx| this.select_drawer_tab(tab, window, cx),
                cx,
            ))
            .child(
                Button::new("drawer-tabs-show")
                    .icon(IconName::ChevronDown)
                    .tooltip("Show all tabs")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer_tabs_hidden = false;
                        cx.notify();
                    })),
            )
    }

    fn currency_chip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.graph_rebuild_pending || self.analysis_warm_pending;
        let currency = self.graph_currency();
        let (text, warn): (SharedString, bool) = if busy {
            (
                if self.analysis_warm_pending {
                    "Loading models…".into()
                } else {
                    "Building graph…".into()
                },
                false,
            )
        } else {
            match &currency {
                GraphCurrency::Current { revision } => {
                    (format!("Graph · rev {revision}").into(), false)
                }
                GraphCurrency::NoNote => ("Graph".into(), false),
                GraphCurrency::NoVerifiedGraph => ("No verified graph".into(), true),
                GraphCurrency::Behind { graph, note } => {
                    (format!("Graph rev {graph} · note rev {note}").into(), true)
                }
                GraphCurrency::ContentChanged { .. } => ("Note changed since graph".into(), true),
                GraphCurrency::OtherNote { graph_document } => (
                    format!(
                        "Graph is from {}",
                        self.entry_name(*graph_document)
                            .unwrap_or_else(|| "another note".into())
                    )
                    .into(),
                    true,
                ),
            }
        };
        let rebuild = !busy && currency.needs_rebuild();
        div()
            .id("graph-currency-chip")
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(22.))
            .px_2()
            .rounded_full()
            .text_xs()
            .when(warn, |chip| {
                chip.bg(rgb(WARN_BG)).text_color(rgb(WARN_TEXT))
            })
            .when(!warn, |chip| chip.text_color(rgb(QUIET)))
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(rgb(if warn { WARN_TEXT } else { ACCENT })),
            )
            .child(text)
            .when(rebuild, |chip| {
                chip.child(
                    div()
                        .id("graph-currency-rebuild")
                        .cursor_pointer()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .hover(|link| link.text_color(rgb(TEXT)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.rebuild_graph_from_chip(window, cx);
                        }))
                        .child("Rebuild"),
                )
            })
    }

    pub(super) fn render_graph_shelf(
        &self,
        view: GraphViewState,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let shelf = self.graph_shelf?;
        let atlas = view.surface == GraphSurface::Atlas;
        let row = div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_4()
            .gap_y_1()
            .px_3()
            .py(px(5.))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SHELF_BG));
        let row = match shelf {
            GraphShelf::Display => {
                let edges = self.edge_options(view);
                row.child(group(
                    "Edges",
                    segmented(
                        "edges",
                        &edges,
                        view.edge_presentation,
                        |this, style, _, cx| {
                            this.mutate_graph_view(
                                |next| next.edge_presentation = style,
                                "EDGES",
                                cx,
                            )
                        },
                        cx,
                    ),
                ))
                .child(group(
                    "Detail",
                    segmented(
                        "detail",
                        &[(true, "Full"), (false, "Overview")],
                        document_detail_visible(view),
                        |this, full, _, cx| {
                            this.mutate_graph_view(
                                |next| set_document_detail(next, full),
                                "DETAIL",
                                cx,
                            )
                        },
                        cx,
                    ),
                ))
                .when(atlas, |row| {
                    row.child(group(
                        "Emphasis",
                        segmented(
                            "emphasis",
                            &[
                                GraphTopologyEmphasis::Off,
                                GraphTopologyEmphasis::Structure,
                                GraphTopologyEmphasis::Facts,
                                GraphTopologyEmphasis::Discourse,
                            ]
                            .map(|value| (value, topology_emphasis_label(value))),
                            view.topology_emphasis,
                            |this, value, _, cx| {
                                this.mutate_graph_view(
                                    |next| next.topology_emphasis = value,
                                    "EMPHASIS",
                                    cx,
                                )
                            },
                            cx,
                        ),
                    ))
                    .child(group(
                        "Navigation",
                        segmented(
                            "navigation",
                            &[
                                GraphNavigationOverlay::Off,
                                GraphNavigationOverlay::Backbone,
                                GraphNavigationOverlay::Bridges,
                                GraphNavigationOverlay::Both,
                            ]
                            .map(|value| (value, navigation_overlay_label(value))),
                            view.navigation_overlay,
                            |this, value, _, cx| {
                                this.mutate_graph_view(
                                    |next| next.navigation_overlay = value,
                                    "NAVIGATION",
                                    cx,
                                )
                            },
                            cx,
                        ),
                    ))
                })
                .child(group(
                    "Canvas",
                    segmented(
                        "canvas",
                        &[(GraphCanvas::Ink, "Ink"), (GraphCanvas::Grid, "Grid")],
                        view.canvas,
                        |this, canvas, _, cx| {
                            this.mutate_graph_view(|next| next.canvas = canvas, "CANVAS", cx)
                        },
                        cx,
                    ),
                ))
            }
            GraphShelf::Filters => {
                let mut reviews = div().flex().items_center().gap_1();
                for (mask, label) in [
                    (ReviewMask::ACCEPTED, "Accepted"),
                    (ReviewMask::PROPOSED, "Proposed"),
                ] {
                    reviews = reviews.child(toggle_chip(
                        ("review-chip", mask.0 as usize),
                        label,
                        view.reviews.contains(mask),
                        move |this, cx| {
                            this.mutate_graph_view(
                                |next| next.reviews = next.reviews.toggled(mask),
                                "REVIEWS",
                                cx,
                            )
                        },
                        cx,
                    ));
                }
                let all_relations = view.relations == RelationMask::ALL;
                let mut relations =
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .child(toggle_chip(
                            ("relation-chip-all", 0usize),
                            "All",
                            all_relations,
                            |this, cx| {
                                this.mutate_graph_view(
                                    |next| next.relations = RelationMask::ALL,
                                    "RELATIONS",
                                    cx,
                                )
                            },
                            cx,
                        ));
                for family in RelationFamily::ALL {
                    relations = relations.child(toggle_chip(
                        ("relation-chip", family as usize + 1),
                        relation_label(family),
                        !all_relations && view.relations.contains(family),
                        move |this, cx| {
                            this.mutate_graph_view(
                                |next| {
                                    next.relations = if next.relations == RelationMask::ALL {
                                        RelationMask::ALL.toggled(family)
                                    } else {
                                        next.relations.toggled(family)
                                    }
                                },
                                "RELATIONS",
                                cx,
                            )
                        },
                        cx,
                    ));
                }
                row.child(group("Review", reviews))
                    .child(group(
                        "Scope",
                        segmented(
                            "scope",
                            &[
                                GraphScope::Global,
                                GraphScope::Narrative,
                                GraphScope::Note,
                                GraphScope::Compare,
                            ]
                            .map(|scope| (scope, scope_label(scope))),
                            view.scope,
                            |this, scope, _, cx| {
                                this.mutate_graph_view(|next| next.scope = scope, "SCOPE", cx)
                            },
                            cx,
                        ),
                    ))
                    .child(group("Relations", relations))
            }
        };
        Some(row.into_any_element())
    }

    fn edge_options(&self, view: GraphViewState) -> Vec<(GraphEdgePresentation, &'static str)> {
        let available = self
            .kernel_snapshot()
            .and_then(|snapshot| snapshot.resident_scene)
            .map(|scene| super::graph_controls::edge_pages_available(&scene, view.manifold))
            .unwrap_or([false; 3]);
        available_edge_presentations(available)
            .into_iter()
            .map(|style| (style, edge_presentation_label(style)))
            .collect()
    }
}

/// Filters differing from the default view: review states, relation
/// families, and scope each count once.
fn active_filter_count(view: GraphViewState) -> usize {
    usize::from(view.reviews != ReviewMask::VISIBLE)
        + usize::from(view.relations != RelationMask::ALL)
        + usize::from(view.scope != GraphScope::Global)
}

/// Slim contextual strip shared by source-local, route walk, and CAPS.
pub(super) fn mode_strip(tint: u32) -> gpui::Div {
    div()
        .w_full()
        .flex_shrink_0()
        .min_h(px(30.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1()
        .px_3()
        .py(px(2.))
        .border_b_1()
        .border_color(rgb(BORDER))
        .bg(rgb(tint))
}

/// Accent bar plus a quiet mode name that opens every strip.
pub(super) fn strip_mark(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().w(px(2.)).h(px(14.)).bg(rgb(color)))
        .child(div().text_xs().text_color(rgb(color)).child(label))
}

fn divider() -> impl IntoElement {
    div().flex_shrink_0().w(px(1.)).h(px(16.)).bg(rgb(DIVIDER))
}

fn group(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().text_xs().text_color(rgb(QUIET)).child(label))
        .child(control)
}

type PickFn<T> = fn(&mut PhoenixShell, T, &mut Window, &mut Context<PhoenixShell>);

fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    active: T,
    on_pick: PickFn<T>,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut segment = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .h(px(24.))
        .p(px(2.))
        .gap(px(1.))
        .rounded_md()
        .border_1()
        .border_color(rgb(DIVIDER))
        .bg(rgb(SEG_BG));
    for (index, &(value, label)) in options.iter().enumerate() {
        let selected = value == active;
        segment = segment.child(
            div()
                .id((id, index))
                .h_full()
                .flex()
                .items_center()
                .px(px(7.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(if selected {
                    SEG_ACTIVE_TEXT
                } else {
                    TEXT_MUTED
                }))
                .when(selected, |item| item.bg(rgb(SEG_ACTIVE)))
                .when(!selected, |item| {
                    item.hover(|item| item.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_pick(this, value, window, cx);
                }))
                .child(label),
        );
    }
    segment
}

fn tool_button(
    id: &'static str,
    label: SharedString,
    active: bool,
    tooltip: &'static str,
    on_click: impl Fn(&mut PhoenixShell, &mut Window, &mut Context<PhoenixShell>) + 'static,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .h(px(24.))
        .flex()
        .items_center()
        .px(px(8.))
        .rounded_md()
        .border_1()
        .cursor_pointer()
        .text_xs()
        .when(active, |button| {
            button
                .bg(rgb(SEG_ACTIVE))
                .border_color(rgb(0x2f7a64))
                .text_color(rgb(SEG_ACTIVE_TEXT))
        })
        .when(!active, |button| {
            button
                .border_color(gpui::transparent_black())
                .text_color(rgb(0xb9c3bf))
                .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        })
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
        .child(label)
}

fn toggle_chip(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    on: bool,
    on_click: impl Fn(&mut PhoenixShell, &mut Context<PhoenixShell>) + 'static,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(22.))
        .flex()
        .items_center()
        .px(px(8.))
        .rounded_full()
        .border_1()
        .cursor_pointer()
        .text_xs()
        .when(on, |chip| {
            chip.bg(rgb(SEG_ACTIVE))
                .border_color(rgb(0x2f7a64))
                .text_color(rgb(SEG_ACTIVE_TEXT))
        })
        .when(!on, |chip| {
            chip.border_color(rgb(DIVIDER))
                .text_color(rgb(TEXT_MUTED))
                .hover(|chip| chip.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        })
        .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: [u8; 32] = [1; 32];

    #[test]
    fn currency_compares_document_revision_and_content() {
        assert_eq!(
            classify_currency(None, Some((3, 2, HASH))),
            GraphCurrency::NoVerifiedGraph
        );
        assert_eq!(
            classify_currency(Some((3, 2, HASH)), None),
            GraphCurrency::NoNote
        );
        assert_eq!(
            classify_currency(Some((3, 2, HASH)), Some((3, 2, HASH))),
            GraphCurrency::Current { revision: 2 }
        );
        assert_eq!(
            classify_currency(Some((3, 1, HASH)), Some((3, 2, [2; 32]))),
            GraphCurrency::Behind { graph: 1, note: 2 }
        );
        assert_eq!(
            classify_currency(Some((3, 2, HASH)), Some((3, 2, [2; 32]))),
            GraphCurrency::ContentChanged { revision: 2 }
        );
        assert_eq!(
            classify_currency(Some((4, 2, HASH)), Some((3, 2, HASH))),
            GraphCurrency::OtherNote { graph_document: 4 }
        );
    }

    #[test]
    fn reader_glow_is_behind_only_for_an_older_view_of_the_same_note() {
        let behind = |graph, note| classify_currency(Some(graph), Some(note)).reader_glow_behind();
        assert!(!behind((3, 2, HASH), (3, 2, HASH)));
        assert!(behind((3, 1, HASH), (3, 2, [2; 32])));
        assert!(behind((3, 2, HASH), (3, 2, [2; 32])));
        assert!(!behind((4, 2, HASH), (3, 2, HASH)));
        assert!(!classify_currency(None, Some((3, 2, HASH))).reader_glow_behind());
    }

    #[test]
    fn only_a_current_graph_or_missing_note_hides_rebuild() {
        assert!(!GraphCurrency::Current { revision: 2 }.needs_rebuild());
        assert!(!GraphCurrency::NoNote.needs_rebuild());
        assert!(GraphCurrency::Behind { graph: 1, note: 2 }.needs_rebuild());
        assert!(GraphCurrency::NoVerifiedGraph.needs_rebuild());
    }

    #[test]
    fn default_view_reports_no_active_filters() {
        let mut view = GraphViewState::default();
        assert_eq!(active_filter_count(view), 0);
        view.scope = GraphScope::Note;
        view.relations = RelationMask::ALL.toggled(RelationFamily::ALL[0]);
        assert_eq!(active_filter_count(view), 2);
    }
}
