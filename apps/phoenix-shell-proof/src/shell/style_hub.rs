use super::drawer::{ACCENT, ACCENT_DIM};
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::StyledExt;
use phoenix_app_core::KernelSnapshot;
#[allow(unused_imports)]
use phoenix_scene_contract::{
    describe_node, primary_edge_family_mask, FamilyMask, GraphColorKey, GraphLens, GraphPalette,
    GraphSurface, GraphViewState, RelationFamily, ReviewMask, VisualNodeKind, VisualNodeLane,
};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use serde::{Deserialize, Serialize};

const ENTITY_LANE_SPECS: [(&str, FamilyMask, GraphColorKey); 5] = [
    (
        "CHAR / PERSONS",
        FamilyMask::CHARACTERS,
        GraphColorKey::Characters,
    ),
    ("LOCATIONS", FamilyMask::LOCATIONS, GraphColorKey::Locations),
    ("NETWORKS", FamilyMask::NETWORKS, GraphColorKey::Networks),
    ("CREATURES", FamilyMask::CREATURES, GraphColorKey::Creatures),
    ("NPCS", FamilyMask::NPCS, GraphColorKey::Npcs),
];
const STRUCTURE_LANE_SPECS: [(&str, FamilyMask, GraphColorKey); 7] = [
    ("DOCUMENTS", FamilyMask::DOCUMENTS, GraphColorKey::Documents),
    ("EPISODES", FamilyMask::EPISODES, GraphColorKey::Episodes),
    ("CHAPTERS", FamilyMask::CHAPTERS, GraphColorKey::Chapters),
    (
        "PARAGRAPHS",
        FamilyMask::PARAGRAPHS,
        GraphColorKey::Paragraphs,
    ),
    ("SENTENCES", FamilyMask::SENTENCES, GraphColorKey::Sentences),
    ("CHUNKS", FamilyMask::CHUNKS, GraphColorKey::Chunks),
    ("EVIDENCE", FamilyMask::EVIDENCE, GraphColorKey::Evidence),
];
const FACT_LANE_SPECS: [(&str, FamilyMask, GraphColorKey); 5] = [
    ("EVENTS", FamilyMask::EVENT_FACTS, GraphColorKey::EventFacts),
    (
        "RELATIONSHIPS",
        FamilyMask::RELATIONSHIP_FACTS,
        GraphColorKey::RelationshipFacts,
    ),
    (
        "TEMPORAL",
        FamilyMask::TEMPORAL_FACTS,
        GraphColorKey::TemporalFacts,
    ),
    (
        "CAUSAL",
        FamilyMask::CAUSAL_FACTS,
        GraphColorKey::CausalFacts,
    ),
    (
        "MEMORY / STATE",
        FamilyMask::MEMORY_STATE_FACTS,
        GraphColorKey::MemoryStateFacts,
    ),
];
const DISCOURSE_LANE_SPECS: [(&str, FamilyMask, GraphColorKey); 2] = [
    (
        "IDENTITY",
        FamilyMask::IDENTITY_DISCOURSE,
        GraphColorKey::IdentityDiscourse,
    ),
    (
        "CONTEXT EVIDENCE",
        FamilyMask::CONTEXTUAL_DISCOURSE,
        GraphColorKey::ContextualDiscourse,
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

#[derive(Default)]
pub(super) struct TopologySummaryCache {
    index_hash: Option<[u8; 32]>,
    summary: TopologySummary,
}

impl TopologySummaryCache {
    fn summary_for(&mut self, snapshot: Option<&KernelSnapshot>) -> TopologySummary {
        let Some(index) = snapshot.and_then(|snapshot| snapshot.scene_product_index.as_deref())
        else {
            self.index_hash = None;
            self.summary = TopologySummary::default();
            return self.summary;
        };
        let index_hash = index.header().index_hash;
        self.get_or_compute(index_hash, || TopologySummary::from_index(index))
    }

    fn get_or_compute(
        &mut self,
        index_hash: [u8; 32],
        compute: impl FnOnce() -> TopologySummary,
    ) -> TopologySummary {
        if self.index_hash != Some(index_hash) {
            self.summary = compute();
            self.index_hash = Some(index_hash);
        }
        self.summary
    }
}

impl TopologySummary {
    fn from_index(index: &PhoenixSceneProductIndexV1) -> Self {
        let mut summary = Self::default();
        for node in index.nodes() {
            summary.record_node(node.family_mask);
        }
        for edge in index.edges() {
            summary.record_edge(edge.family_mask, edge.relation_mask, edge.review_mask);
        }
        summary
    }

    fn record_edge(&mut self, family_mask: u64, relation_mask: u64, review_mask: u32) {
        self.edges += 1;
        let primary_mask = primary_edge_family_mask(family_mask, relation_mask).0;
        let primary_lane = edge_lane_slot(primary_mask);
        if let Some(slot) = primary_lane {
            self.edge_lanes[slot] += 1;
        } else {
            self.unclassified_edges += 1;
        }
        for (slot, (_, mask, _)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
            if primary_mask & mask.0 != 0 {
                self.entity_edges[slot] += 1;
            }
        }
        match primary_lane {
            Some(1) => count_detail_edges(
                primary_mask,
                &STRUCTURE_LANE_SPECS,
                &mut self.structure_edges,
            ),
            Some(2) => count_detail_edges(primary_mask, &FACT_LANE_SPECS, &mut self.fact_edges),
            Some(3) => count_detail_edges(
                primary_mask,
                &DISCOURSE_LANE_SPECS,
                &mut self.discourse_edges,
            ),
            _ => {}
        }
        if review_mask & ReviewMask::ACCEPTED.0 != 0 {
            self.accepted_edges += 1;
        }
        if review_mask & ReviewMask::PROPOSED.0 != 0 {
            self.proposed_edges += 1;
        }
        for (slot, relation) in RelationFamily::ALL.into_iter().enumerate() {
            if relation_mask & relation.mask().0 != 0 {
                self.relations[slot] += 1;
            }
        }
    }

    fn record_node(&mut self, family_mask: u64) {
        self.nodes += 1;
        let descriptor = describe_node(family_mask);
        let primary_lane = node_lane_slot(descriptor.kind, descriptor.lane);
        if let Some(slot) = primary_lane {
            self.node_lanes[slot] += 1;
        } else {
            self.unclassified_nodes += 1;
        }
        for (slot, (_, mask, _)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
            if family_mask & mask.0 != 0 {
                if primary_lane == Some(0) {
                    self.entity_anchor_nodes[slot] += 1;
                } else {
                    self.entity_context_nodes[slot] += 1;
                }
            }
        }
        match descriptor.lane {
            VisualNodeLane::Structure if descriptor.kind != VisualNodeKind::Unknown => {
                count_detail_nodes(
                    descriptor.detail_mask,
                    &STRUCTURE_LANE_SPECS,
                    &mut self.structure_nodes,
                );
            }
            VisualNodeLane::Facts => count_detail_nodes(
                descriptor.detail_mask,
                &FACT_LANE_SPECS,
                &mut self.fact_nodes,
            ),
            VisualNodeLane::Discourse => count_detail_nodes(
                descriptor.detail_mask,
                &DISCOURSE_LANE_SPECS,
                &mut self.discourse_nodes,
            ),
            _ => {}
        }
    }
}

impl PhoenixShell {
    pub(super) fn render_style_hub(
        &self,
        snapshot: Option<&KernelSnapshot>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = snapshot.map_or_else(GraphViewState::default, |snapshot| snapshot.graph_view);
        let summary = self
            .style_hub_summary_cache
            .borrow_mut()
            .summary_for(snapshot);
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
                    .child(lane_grid(self, view, summary, cx)),
            )
            .child(
                div()
                    .mt_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .px_2()
                    .pt_2()
                    .child(section_label("ENTITY KIND LANES"))
                    .child(entity_lane_grid(self, view, summary, cx)),
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
                        self,
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
                        self,
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
                        self,
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
                                self,
                                "ACCEPTED",
                                summary.accepted_edges,
                                view.reviews.contains(ReviewMask::ACCEPTED),
                                GraphColorKey::AcceptedEdges,
                            ))
                            .child(review_metric(
                                self,
                                "PROPOSED",
                                summary.proposed_edges,
                                view.reviews.contains(ReviewMask::PROPOSED),
                                GraphColorKey::ProposedEdges,
                            )),
                    )
                    .child(relation_inventory(self, view, summary, cx)),
            )
    }
}

fn lane_grid(
    shell: &PhoenixShell,
    view: GraphViewState,
    summary: TopologySummary,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, lens, mask, key)) in [
        (
            "ENTITIES",
            GraphLens::Entities,
            FamilyMask::ENTITIES,
            GraphColorKey::Entities,
        ),
        (
            "STRUCTURE",
            GraphLens::Structure,
            FamilyMask::STRUCTURE,
            GraphColorKey::Structure,
        ),
        (
            "FACTS",
            GraphLens::Facts,
            FamilyMask::FACTS,
            GraphColorKey::Facts,
        ),
        (
            "DISCOURSE",
            GraphLens::Discourse,
            FamilyMask::DISCOURSE,
            GraphColorKey::Discourse,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let selected = view.family_is_visible(mask);
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(shell.graph_color_picker(key))
                        .child(
                            div()
                                .id(("style-lane-toggle", slot))
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .cursor_pointer()
                                .text_xs()
                                .font_semibold()
                                .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mutate_graph_view(
                                        |next| {
                                            next.toggle_family(mask);
                                            next.lens = lens;
                                            next.surface = GraphSurface::Atlas;
                                        },
                                        "STYLE HUB",
                                        cx,
                                    );
                                }))
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
    shell: &PhoenixShell,
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
        let key = relation_color_key(relation);
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(shell.graph_color_picker(key))
                .child(
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
                ),
        );
    }
    rows
}

fn entity_lane_grid(
    shell: &PhoenixShell,
    view: GraphViewState,
    summary: TopologySummary,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, mask, key)) in ENTITY_LANE_SPECS.into_iter().enumerate() {
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(shell.graph_color_picker(key))
                        .child(
                            div()
                                .id(("style-entity-toggle", slot))
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .cursor_pointer()
                                .text_xs()
                                .font_semibold()
                                .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mutate_graph_view(
                                        |next| *next = toggle_entity_lane(*next, mask),
                                        "STYLE HUB ENTITY LANE",
                                        cx,
                                    );
                                }))
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
    shell: &PhoenixShell,
    specs: &'static [(&'static str, FamilyMask, GraphColorKey)],
    node_counts: &[usize],
    edge_counts: &[usize],
    id_prefix: &'static str,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut grid = div().mt_1().grid().grid_cols(2).gap_1();
    for (slot, (label, mask, key)) in specs.iter().copied().enumerate() {
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(shell.graph_color_picker(key))
                        .child(
                            div()
                                .id((id_prefix, slot + 10_000))
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .cursor_pointer()
                                .text_xs()
                                .font_semibold()
                                .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mutate_graph_view(
                                        |next| {
                                            next.toggle_topology_family(mask);
                                            next.surface = GraphSurface::Atlas;
                                        },
                                        "STYLE HUB PRODUCT LANE",
                                        cx,
                                    );
                                }))
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
    specs: &[(&str, FamilyMask, GraphColorKey); N],
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
    specs: &[(&str, FamilyMask, GraphColorKey); N],
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

const fn node_lane_slot(kind: VisualNodeKind, lane: VisualNodeLane) -> Option<usize> {
    if matches!(kind, VisualNodeKind::Unknown) {
        return None;
    }
    match lane {
        VisualNodeLane::Entities => Some(0),
        VisualNodeLane::Structure => Some(1),
        VisualNodeLane::Facts => Some(2),
        VisualNodeLane::Discourse => Some(3),
    }
}

fn edge_lane_slot(mask: u64) -> Option<usize> {
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

fn review_metric(
    shell: &PhoenixShell,
    label: &'static str,
    count: usize,
    visible: bool,
    key: GraphColorKey,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .flex_1()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(if visible { 0x172a24 } else { 0x151716 }))
        .text_xs()
        .text_color(rgb(if visible { ACCENT } else { TEXT_MUTED }))
        .child(shell.graph_color_picker(key))
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

const fn relation_color_key(relation: RelationFamily) -> GraphColorKey {
    match relation {
        RelationFamily::CoOccurrence => GraphColorKey::CoOccurrenceEdges,
        RelationFamily::Observation => GraphColorKey::ObservationEdges,
        RelationFamily::Communication => GraphColorKey::CommunicationEdges,
        RelationFamily::Causal => GraphColorKey::CausalEdges,
        RelationFamily::Temporal => GraphColorKey::TemporalEdges,
        RelationFamily::Structural => GraphColorKey::StructuralEdges,
        RelationFamily::Identity => GraphColorKey::IdentityEdges,
        RelationFamily::Relationship => GraphColorKey::RelationshipEdges,
        RelationFamily::Event => GraphColorKey::EventEdges,
        RelationFamily::MemoryState => GraphColorKey::MemoryStateEdges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_evidence_edges_are_classified_as_structure_not_entities() {
        assert_eq!(
            edge_lane_slot(FamilyMask::STRUCTURE.0 | FamilyMask::ENTITIES.0),
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
        assert_eq!(edge_lane_slot(0), None);
        assert_eq!(edge_lane_slot(1_u64 << 63), None);
    }

    #[test]
    fn topology_summary_cache_reuses_an_immutable_product_index() {
        let mut cache = TopologySummaryCache::default();
        let mut builds = 0;
        let index_hash = [7; 32];
        let first = cache.get_or_compute(index_hash, || {
            builds += 1;
            TopologySummary {
                nodes: 11,
                edges: 17,
                ..TopologySummary::default()
            }
        });
        let second = cache.get_or_compute(index_hash, || {
            builds += 1;
            TopologySummary::default()
        });

        assert_eq!(builds, 1);
        assert_eq!(first, second);
        assert_eq!(second.nodes, 11);
        assert_eq!(second.edges, 17);
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
    fn fact_detail_toggle_keeps_the_broad_facts_lane_coupled() {
        let view = GraphViewState {
            surface: GraphSurface::Atlas,
            families: FamilyMask::ALL,
            ..GraphViewState::default()
        };
        let mut toggled = view;
        toggled.toggle_topology_family(FamilyMask::RELATIONSHIP_FACTS);
        assert!(toggled.families.contains(FamilyMask::FACTS));
        assert!(!toggled
            .topology_families
            .contains(FamilyMask::RELATIONSHIP_FACTS));
        assert!(toggled.topology_families.contains(FamilyMask::EVENT_FACTS));
    }

    #[test]
    fn compiler_real_census_and_renderer_share_primary_semantic_identity() {
        for (broad, specs, lane) in [
            (FamilyMask::FACTS, &FACT_LANE_SPECS[..], 2_usize),
            (FamilyMask::DISCOURSE, &DISCOURSE_LANE_SPECS[..], 3_usize),
        ] {
            for (_, detail, expected_key) in specs.iter().copied() {
                let family_mask =
                    broad.0 | detail.0 | FamilyMask::CHARACTERS.0 | FamilyMask::LOCATIONS.0;
                let descriptor = describe_node(family_mask);
                let mut summary = TopologySummary::default();
                summary.record_node(family_mask);

                assert_eq!(summary.nodes, 1);
                assert_eq!(summary.node_lanes[lane], 1);
                assert_eq!(GraphPalette::node_key(descriptor, 0), Some(expected_key));
            }
        }
    }

    #[test]
    fn published_fact_edge_census_is_relation_owned_not_endpoint_inherited() {
        let mut summary = TopologySummary::default();
        for (detail, relation, count) in [
            (FamilyMask::EVENT_FACTS, RelationFamily::Event, 227_usize),
            (FamilyMask::CAUSAL_FACTS, RelationFamily::Causal, 14),
            (
                FamilyMask::MEMORY_STATE_FACTS,
                RelationFamily::MemoryState,
                82,
            ),
        ] {
            let compiler_real_composite = FamilyMask::FACTS.0
                | detail.0
                | FamilyMask::EVENT_FACTS.0
                | FamilyMask::CHARACTERS.0;
            for _ in 0..count {
                summary.record_edge(
                    compiler_real_composite,
                    relation.mask().0,
                    ReviewMask::ACCEPTED.0,
                );
            }
        }

        assert_eq!(summary.edges, 323);
        assert_eq!(summary.edge_lanes, [0, 0, 323, 0]);
        assert_eq!(summary.fact_edges, [227, 0, 0, 14, 82]);
        assert_eq!(summary.relations[RelationFamily::Event as usize], 227);
        assert_eq!(summary.relations[RelationFamily::Causal as usize], 14);
        assert_eq!(summary.relations[RelationFamily::MemoryState as usize], 82);
        assert_eq!(summary.entity_edges, [0; ENTITY_LANE_SPECS.len()]);
    }
}
