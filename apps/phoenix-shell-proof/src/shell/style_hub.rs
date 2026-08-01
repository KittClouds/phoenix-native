use super::drawer::{ACCENT, ACCENT_DIM};
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::StyledExt;
use phoenix_app_core::KernelSnapshot;
use phoenix_scene_contract::{
    FamilyMask, GraphLens, GraphSurface, GraphViewState, RelationFamily, ReviewMask,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GraphSidebarPanel {
    #[default]
    Registry,
    StyleHub,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TopologySummary {
    nodes: usize,
    edges: usize,
    node_lanes: [usize; 4],
    edge_lanes: [usize; 4],
    unclassified_nodes: usize,
    unclassified_edges: usize,
    accepted_edges: usize,
    proposed_edges: usize,
    relations: [usize; RelationFamily::ALL.len()],
}

impl TopologySummary {
    fn from_snapshot(snapshot: Option<&KernelSnapshot>) -> Self {
        let Some(index) = snapshot.and_then(|snapshot| snapshot.scene_product_index.as_deref())
        else {
            return Self::default();
        };
        let mut summary = Self {
            nodes: index.nodes().len(),
            edges: index.edges().len(),
            ..Self::default()
        };
        for node in index.nodes() {
            if let Some(slot) = lane_slot(node.family_mask) {
                summary.node_lanes[slot] += 1;
            } else {
                summary.unclassified_nodes += 1;
            }
        }
        for edge in index.edges() {
            if let Some(slot) = lane_slot(edge.family_mask) {
                summary.edge_lanes[slot] += 1;
            } else {
                summary.unclassified_edges += 1;
            }
            if edge.review_mask & ReviewMask::ACCEPTED.0 != 0 {
                summary.accepted_edges += 1;
            }
            if edge.review_mask & ReviewMask::PROPOSED.0 != 0 {
                summary.proposed_edges += 1;
            }
            for (slot, relation) in RelationFamily::ALL.into_iter().enumerate() {
                if edge.relation_mask & relation.mask().0 != 0 {
                    summary.relations[slot] += 1;
                }
            }
        }
        summary
    }
}

impl PhoenixShell {
    pub(super) fn render_style_hub(
        &self,
        snapshot: Option<&KernelSnapshot>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = snapshot.map_or_else(GraphViewState::default, |snapshot| snapshot.graph_view);
        let summary = TopologySummary::from_snapshot(snapshot);
        let bound = snapshot
            .and_then(|snapshot| snapshot.scene_product_index.as_ref())
            .is_some();
        let topology_drift = summary.unclassified_nodes != 0 || summary.unclassified_edges != 0;

        div()
            .m_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(0x2d4c43))
            .bg(rgb(0x101715))
            .child(
                div()
                    .px_2()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(rgb(ACCENT))
                                    .child("STYLE HUB"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_full()
                                    .bg(rgb(if !bound || topology_drift {
                                        0x302214
                                    } else {
                                        ACCENT_DIM
                                    }))
                                    .text_xs()
                                    .text_color(rgb(if !bound || topology_drift {
                                        0xf0ad4e
                                    } else {
                                        ACCENT
                                    }))
                                    .child(if !bound {
                                        "NO INDEX"
                                    } else if topology_drift {
                                        "DRIFT"
                                    } else {
                                        "VERIFIED"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("Published topology authority"),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .pb_2()
                    .grid()
                    .grid_cols(2)
                    .gap_1()
                    .child(metric("NODES", summary.nodes))
                    .child(metric("EDGES", summary.edges)),
            )
            .when(topology_drift, |hub| {
                hub.child(
                    div()
                        .mx_2()
                        .mb_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x765321))
                        .bg(rgb(0x251c12))
                        .text_xs()
                        .text_color(rgb(0xf0ad4e))
                        .child(format!(
                            "UNCLASSIFIED  {} N / {} E",
                            summary.unclassified_nodes, summary.unclassified_edges
                        )),
                )
            })
            .child(
                div()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .child(section_label("NODE LANES"))
                    .child(lane_grid(view, summary, cx)),
            )
            .child(
                div()
                    .mt_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .pb_2()
                    .child(section_label("EDGE TRUTH"))
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(review_metric(
                                "ACCEPTED",
                                summary.accepted_edges,
                                view.reviews.contains(ReviewMask::ACCEPTED),
                            ))
                            .child(review_metric(
                                "PROPOSED",
                                summary.proposed_edges,
                                view.reviews.contains(ReviewMask::PROPOSED),
                            )),
                    )
                    .child(relation_inventory(view, summary, cx)),
            )
    }
}

fn lane_grid(
    view: GraphViewState,
    summary: TopologySummary,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, lens, mask, color)) in [
        (
            "ENTITIES",
            GraphLens::Entities,
            FamilyMask::ENTITIES,
            0x2f80ff,
        ),
        (
            "STRUCTURE",
            GraphLens::Structure,
            FamilyMask::STRUCTURE,
            0xe03b78,
        ),
        ("FACTS", GraphLens::Facts, FamilyMask::FACTS, 0xff7733),
        (
            "DISCOURSE",
            GraphLens::Discourse,
            FamilyMask::DISCOURSE,
            0x9a68ff,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let selected = view.families.contains(mask);
        let node_count = summary.node_lanes[slot];
        let edge_count = summary.edge_lanes[slot];
        grid = grid.child(
            div()
                .id(("style-lane", slot))
                .min_w_0()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(if selected { 0x397765 } else { BORDER }))
                .bg(rgb(if selected { 0x183d34 } else { 0x111514 }))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mutate_graph_view(
                        |next| {
                            let toggled = next.families.toggled(mask);
                            if toggled.is_valid_selection() {
                                next.families = toggled;
                                next.lens = lens;
                                next.surface = GraphSurface::Atlas;
                            }
                        },
                        "STYLE HUB",
                        cx,
                    );
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().size(px(7.)).rounded_full().bg(rgb(color)))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_xs()
                                .font_semibold()
                                .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                                .child(label),
                        ),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(format!("{node_count} N / {edge_count} E")),
                ),
        );
    }
    grid
}

fn relation_inventory(
    view: GraphViewState,
    summary: TopologySummary,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut rows = div().mt_2().flex().flex_wrap().gap_1();
    for (slot, relation) in RelationFamily::ALL.into_iter().enumerate() {
        let count = summary.relations[slot];
        if count == 0 {
            continue;
        }
        let selected = view.relations.contains(relation);
        rows = rows.child(
            div()
                .id(("style-relation", slot))
                .px_1()
                .py(px(2.))
                .rounded_md()
                .border_1()
                .border_color(rgb(if selected { 0x397765 } else { BORDER }))
                .bg(rgb(if selected { 0x172a24 } else { 0x111514 }))
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mutate_graph_view(
                        |next| {
                            let toggled = next.relations.toggled(relation);
                            if toggled.is_valid_selection() {
                                next.relations = toggled;
                            }
                        },
                        "STYLE HUB",
                        cx,
                    );
                }))
                .child(format!("{} {count}", relation_label(relation))),
        );
    }
    rows
}

fn lane_slot(mask: u64) -> Option<usize> {
    if mask & FamilyMask::FACTS.0 != 0 {
        Some(2)
    } else if mask & FamilyMask::DISCOURSE.0 != 0 {
        Some(3)
    } else if mask & FamilyMask::STRUCTURE.0 != 0 {
        Some(1)
    } else if mask & FamilyMask::ENTITIES.0 != 0 {
        Some(0)
    } else {
        None
    }
}

fn metric(label: &'static str, count: usize) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(0x151b19))
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(label))
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(count.to_string()),
        )
}

fn review_metric(label: &'static str, count: usize, visible: bool) -> impl IntoElement {
    div()
        .flex_1()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(if visible { 0x172a24 } else { 0x151716 }))
        .text_xs()
        .text_color(rgb(if visible { ACCENT } else { TEXT_MUTED }))
        .child(format!("{label} {count}"))
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(rgb(TEXT_MUTED))
        .child(label)
}

const fn relation_label(relation: RelationFamily) -> &'static str {
    match relation {
        RelationFamily::CoOccurrence => "CO",
        RelationFamily::Observation => "OBS",
        RelationFamily::Communication => "COM",
        RelationFamily::Causal => "CAUSE",
        RelationFamily::Temporal => "TIME",
        RelationFamily::Structural => "STRUCT",
        RelationFamily::Identity => "IDENT",
        RelationFamily::Relationship => "REL",
        RelationFamily::Event => "EVENT",
        RelationFamily::MemoryState => "STATE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_evidence_edges_are_classified_as_structure_not_entities() {
        assert_eq!(
            lane_slot(FamilyMask::STRUCTURE.0 | FamilyMask::ENTITIES.0),
            Some(1)
        );
    }

    #[test]
    fn style_hub_covers_every_native_relation_family() {
        for relation in RelationFamily::ALL {
            assert!(!relation_label(relation).is_empty());
        }
    }

    #[test]
    fn unknown_family_bits_are_never_reported_as_verified_topology() {
        assert_eq!(lane_slot(0), None);
        assert_eq!(lane_slot(1_u64 << 63), None);
    }
}
