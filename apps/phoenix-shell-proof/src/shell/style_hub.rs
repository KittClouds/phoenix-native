use super::drawer::{ACCENT, ACCENT_DIM};
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::StyledExt;
use phoenix_app_core::KernelSnapshot;
use phoenix_scene_contract::{
    FamilyMask, GraphLens, GraphSurface, GraphViewState, RelationFamily, ReviewMask,
};
use serde::{Deserialize, Serialize};

const ENTITY_LANE_SPECS: [(&str, FamilyMask, u32); 5] = [
    ("CHAR / PERSONS", FamilyMask::CHARACTERS, 0x2f80ff),
    ("LOCATIONS", FamilyMask::LOCATIONS, 0x00c48c),
    ("NETWORKS", FamilyMask::NETWORKS, 0x22d3ee),
    ("CREATURES", FamilyMask::CREATURES, 0xf59e0b),
    ("NPCS", FamilyMask::NPCS, 0xa855f7),
];
const STRUCTURE_LANE_SPECS: [(&str, FamilyMask, u32); 4] = [
    ("DOCUMENTS", FamilyMask::DOCUMENTS, 0x3d8cf5),
    ("EPISODES", FamilyMask::EPISODES, 0xb852f0),
    ("CHUNKS", FamilyMask::CHUNKS, 0xf04482),
    ("EVIDENCE", FamilyMask::EVIDENCE, 0x8b5cf6),
];
const FACT_LANE_SPECS: [(&str, FamilyMask, u32); 5] = [
    ("EVENTS", FamilyMask::EVENT_FACTS, 0xfb6f26),
    ("RELATIONSHIPS", FamilyMask::RELATIONSHIP_FACTS, 0xe84fa8),
    ("TEMPORAL", FamilyMask::TEMPORAL_FACTS, 0xf4df23),
    ("CAUSAL", FamilyMask::CAUSAL_FACTS, 0xff5964),
    ("MEMORY / STATE", FamilyMask::MEMORY_STATE_FACTS, 0x22d36f),
];
const DISCOURSE_LANE_SPECS: [(&str, FamilyMask, u32); 2] = [
    ("IDENTITY", FamilyMask::IDENTITY_DISCOURSE, 0x9858f5),
    (
        "CONTEXT EVIDENCE",
        FamilyMask::CONTEXTUAL_DISCOURSE,
        0x35c7d9,
    ),
];

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
    entity_anchor_nodes: [usize; ENTITY_LANE_SPECS.len()],
    entity_context_nodes: [usize; ENTITY_LANE_SPECS.len()],
    entity_edges: [usize; ENTITY_LANE_SPECS.len()],
    structure_nodes: [usize; STRUCTURE_LANE_SPECS.len()],
    structure_edges: [usize; STRUCTURE_LANE_SPECS.len()],
    fact_nodes: [usize; FACT_LANE_SPECS.len()],
    fact_edges: [usize; FACT_LANE_SPECS.len()],
    discourse_nodes: [usize; DISCOURSE_LANE_SPECS.len()],
    discourse_edges: [usize; DISCOURSE_LANE_SPECS.len()],
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
            let primary_lane = lane_slot(node.family_mask);
            if let Some(slot) = primary_lane {
                summary.node_lanes[slot] += 1;
            } else {
                summary.unclassified_nodes += 1;
            }
            for (slot, (_, mask, _)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
                if node.family_mask & mask.0 != 0 {
                    if primary_lane == Some(0) {
                        summary.entity_anchor_nodes[slot] += 1;
                    } else {
                        summary.entity_context_nodes[slot] += 1;
                    }
                }
            }
            match primary_lane {
                Some(1) => count_detail_nodes(
                    node.family_mask,
                    &STRUCTURE_LANE_SPECS,
                    &mut summary.structure_nodes,
                ),
                Some(2) => {
                    count_detail_nodes(node.family_mask, &FACT_LANE_SPECS, &mut summary.fact_nodes)
                }
                Some(3) => count_detail_nodes(
                    node.family_mask,
                    &DISCOURSE_LANE_SPECS,
                    &mut summary.discourse_nodes,
                ),
                _ => {}
            }
        }
        for edge in index.edges() {
            let primary_lane = lane_slot(edge.family_mask);
            if let Some(slot) = primary_lane {
                summary.edge_lanes[slot] += 1;
            } else {
                summary.unclassified_edges += 1;
            }
            for (slot, (_, mask, _)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
                if edge.family_mask & mask.0 != 0 {
                    summary.entity_edges[slot] += 1;
                }
            }
            match primary_lane {
                Some(1) => count_detail_edges(
                    edge.family_mask,
                    &STRUCTURE_LANE_SPECS,
                    &mut summary.structure_edges,
                ),
                Some(2) => {
                    count_detail_edges(edge.family_mask, &FACT_LANE_SPECS, &mut summary.fact_edges)
                }
                Some(3) => count_detail_edges(
                    edge.family_mask,
                    &DISCOURSE_LANE_SPECS,
                    &mut summary.discourse_edges,
                ),
                _ => {}
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
            .child(visual_hierarchy_legend())
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
                    .child(section_label("ENTITY KIND LANES"))
                    .child(entity_lane_grid(view, summary, cx)),
            )
            .child(
                div()
                    .mt_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .child(section_label("STRUCTURE PRODUCTS"))
                    .child(detail_lane_grid(
                        view,
                        &STRUCTURE_LANE_SPECS,
                        &summary.structure_nodes,
                        &summary.structure_edges,
                        "structure-detail",
                        cx,
                    )),
            )
            .child(
                div()
                    .mt_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .child(section_label("FACT PRODUCTS"))
                    .child(detail_lane_grid(
                        view,
                        &FACT_LANE_SPECS,
                        &summary.fact_nodes,
                        &summary.fact_edges,
                        "fact-detail",
                        cx,
                    )),
            )
            .child(
                div()
                    .mt_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .child(section_label("DISCOURSE PRODUCTS"))
                    .child(detail_lane_grid(
                        view,
                        &DISCOURSE_LANE_SPECS,
                        &summary.discourse_nodes,
                        &summary.discourse_edges,
                        "discourse-detail",
                        cx,
                    )),
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

fn entity_lane_grid(
    view: GraphViewState,
    summary: TopologySummary,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, mask, color)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
        let selected = view.entity_families.contains(mask);
        grid = grid.child(
            div()
                .id(("style-entity-lane", slot))
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
                            *next = toggle_entity_lane(*next, mask);
                        },
                        "STYLE HUB ENTITY LANE",
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
                        .child(format!(
                            "{} A / {} C / {} E",
                            summary.entity_anchor_nodes[slot],
                            summary.entity_context_nodes[slot],
                            summary.entity_edges[slot]
                        )),
                ),
        );
    }
    grid
}

fn detail_lane_grid(
    view: GraphViewState,
    specs: &'static [(&'static str, FamilyMask, u32)],
    node_counts: &[usize],
    edge_counts: &[usize],
    id_prefix: &'static str,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, mask, color)) in specs.iter().copied().enumerate() {
        let selected = view.topology_families.contains(mask);
        let node_count = node_counts.get(slot).copied().unwrap_or(0);
        let edge_count = edge_counts.get(slot).copied().unwrap_or(0);
        grid = grid.child(
            div()
                .id((id_prefix, slot))
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
                            let toggled = next.topology_families.toggled(mask);
                            if toggled.is_valid_topology_selection() {
                                next.topology_families = toggled;
                                next.surface = GraphSurface::Atlas;
                            }
                        },
                        "STYLE HUB PRODUCT LANE",
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

fn count_detail_nodes<const N: usize>(
    family_mask: u64,
    specs: &[(&str, FamilyMask, u32); N],
    counts: &mut [usize; N],
) {
    for (slot, (_, mask, _)) in specs.iter().enumerate() {
        if family_mask & mask.0 != 0 {
            counts[slot] += 1;
        }
    }
}

fn count_detail_edges<const N: usize>(
    family_mask: u64,
    specs: &[(&str, FamilyMask, u32); N],
    counts: &mut [usize; N],
) {
    for (slot, (_, mask, _)) in specs.iter().enumerate() {
        if family_mask & mask.0 != 0 {
            counts[slot] += 1;
        }
    }
}

fn toggle_entity_lane(mut view: GraphViewState, mask: FamilyMask) -> GraphViewState {
    let toggled = view.entity_families.toggled(mask);
    if toggled.is_valid_entity_selection() {
        view.entity_families = toggled;
    }
    view
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

fn visual_hierarchy_legend() -> impl IntoElement {
    div()
        .mx_2()
        .mb_2()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(0x121a18))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(rgb(TEXT_MUTED))
                .child("VISUAL HIERARCHY"),
        )
        .child(
            div()
                .mt(px(2.))
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child("Role metadata scales spheres; edge width stays topology-stable."),
        )
        .child(
            div()
                .mt_1()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(visual_role_chip("ROOT", 0x3d8cf5))
                .child(visual_role_chip("ANCHOR", 0x8b5cf6))
                .child(visual_role_chip("HUB", 0x00c48c))
                .child(visual_role_chip("ORDINARY", 0x66727a)),
        )
}

fn visual_role_chip(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .px_1()
        .py(px(2.))
        .rounded_md()
        .bg(rgb(0x17211e))
        .flex()
        .items_center()
        .gap_1()
        .child(div().size(px(6.)).rounded_full().bg(rgb(color)))
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(label))
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

    #[test]
    fn entity_kind_toggle_never_changes_the_active_surface() {
        for surface in [GraphSurface::Entities, GraphSurface::Atlas] {
            let view = GraphViewState {
                surface,
                ..GraphViewState::default()
            };
            let toggled = toggle_entity_lane(view, FamilyMask::CHARACTERS);
            assert_eq!(toggled.surface, surface);
            assert!(!toggled.entity_families.contains(FamilyMask::CHARACTERS));
            assert_eq!(toggled.families, view.families);
        }
    }

    #[test]
    fn fact_detail_toggle_is_independent_of_the_broad_facts_lane() {
        let view = GraphViewState {
            surface: GraphSurface::Atlas,
            families: FamilyMask::ALL,
            ..GraphViewState::default()
        };
        let mut toggled = view;
        toggled.topology_families = toggled
            .topology_families
            .toggled(FamilyMask::RELATIONSHIP_FACTS);
        assert!(toggled.families.contains(FamilyMask::FACTS));
        assert!(!toggled
            .topology_families
            .contains(FamilyMask::RELATIONSHIP_FACTS));
        assert!(toggled.topology_families.contains(FamilyMask::EVENT_FACTS));
    }
}
