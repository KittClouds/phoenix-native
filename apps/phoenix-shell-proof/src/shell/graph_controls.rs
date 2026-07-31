use super::{
    drawer::{ACCENT, ACCENT_DIM},
    PhoenixShell, BORDER, BORDER_BRIGHT, TEXT, TEXT_MUTED,
};
use crate::lifecycle;
use gpui::{div, prelude::*, px, rgb, Context, Corner, IntoElement, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::popover::Popover;
use gpui_component::{Disableable, Sizable};
use phoenix_app_core::{GraphProvenanceReceipt, KernelCommand, KernelOutcome};
use phoenix_scene_contract::{
    GraphAction, GraphScope, GraphSurface, GraphViewState, Manifold, RelationFamily, ReviewMask,
    SceneSource,
};

const CONTROL_BG: u32 = 0x111514;
const CONTROL_RAISED: u32 = 0x1b211f;
const CONTROL_ACTIVE: u32 = 0x183d34;
const CONTROL_ACTIVE_BORDER: u32 = 0x317862;
const VIOLET: u32 = 0xa991ff;
const VIOLET_DIM: u32 = 0x2a2443;
const POPOVER_BG: u32 = 0x181c1b;

impl PhoenixShell {
    pub(super) fn render_graph_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self
            .kernel_snapshot()
            .map(|snapshot| snapshot.graph_view)
            .unwrap_or_default();
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(rgb(BORDER_BRIGHT))
            .bg(rgb(CONTROL_BG))
            .when(has_atlas_control_rail(view.surface), |controls| {
                controls.child(self.render_primary_controls(view, cx))
            })
            .child(self.render_secondary_controls(view, cx))
    }

    fn render_primary_controls(
        &self,
        view: GraphViewState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut row = div()
            .h(px(42.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2();
        row = row
            .child(review_toggle("ACCEPTED", ReviewMask::ACCEPTED, view, cx))
            .child(review_toggle("PROPOSED", ReviewMask::PROPOSED, view, cx))
            .child(self.relation_popover(view, cx))
            .child(self.scope_popover(view, cx));
        row.child(div().flex_1())
    }

    fn render_secondary_controls(
        &self,
        view: GraphViewState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut row = div()
            .h(px(40.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_t_1()
            .border_color(rgb(BORDER));
        row = row
            .child(div().text_xs().text_color(rgb(0x59635f)).child("SPACE"))
            .child(manifold_segment(view.manifold, cx));
        row.child(div().flex_1())
            .child(self.provenance_popover(cx))
            .child(action_button("graph-fit", "FIT", GraphAction::Fit, cx))
            .child(action_button(
                "graph-reset",
                "RESET",
                GraphAction::Reset,
                cx,
            ))
            .child(
                Button::new("native-scene-rebuild")
                    .label(if self.graph_rebuild_pending {
                        "BUILDING"
                    } else {
                        "REBUILD"
                    })
                    .small()
                    .ghost()
                    .disabled(self.graph_rebuild_pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_native_scene_rebuild(window, cx);
                    })),
            )
    }

    fn relation_popover(&self, view: GraphViewState, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = cx.entity();
        let selected = RelationFamily::ALL
            .into_iter()
            .filter(|family| view.relations.contains(*family))
            .count();
        Popover::new("graph-relations-popover")
            .anchor(Corner::BottomLeft)
            .appearance(false)
            .trigger(
                Button::new("graph-relations-trigger")
                    .label(format!("REL / {selected}"))
                    .small()
                    .ghost(),
            )
            .content(move |_, _, _| {
                let mut menu = popover_card("RELATION FAMILIES", "Independent edge policy masks.");
                for family in RelationFamily::ALL {
                    let shell = shell.clone();
                    menu = menu.child(popover_option(
                        ("relation-option", family as usize),
                        relation_label(family),
                        view.relations.contains(family),
                        move |_, cx| {
                            shell.update(cx, |this, cx| {
                                this.mutate_graph_view(
                                    |next| next.relations = next.relations.toggled(family),
                                    "RELATIONS",
                                    cx,
                                );
                            });
                        },
                    ));
                }
                menu
            })
    }

    fn scope_popover(&self, view: GraphViewState, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = cx.entity();
        Popover::new("graph-scope-popover")
            .anchor(Corner::BottomRight)
            .appearance(false)
            .trigger(
                Button::new("graph-scope-trigger")
                    .label(scope_label(view.scope))
                    .small()
                    .ghost(),
            )
            .content(move |state, window, popover_cx| {
                let popover = popover_cx.entity();
                let mut menu =
                    popover_card("SCOPE", "Membership masks preserve stable node identities.");
                for scope in [
                    GraphScope::Global,
                    GraphScope::Narrative,
                    GraphScope::Note,
                    GraphScope::Compare,
                ] {
                    let shell = shell.clone();
                    let popover = popover.clone();
                    menu = menu.child(popover_option(
                        ("scope-option", scope as usize),
                        scope_label(scope),
                        scope == view.scope,
                        move |window, cx| {
                            shell.update(cx, |this, cx| {
                                this.mutate_graph_view(|next| next.scope = scope, "SCOPE", cx);
                            });
                            popover.update(cx, |state, cx| state.dismiss(window, cx));
                        },
                    ));
                }
                let _ = state;
                let _ = window;
                menu
            })
    }

    fn provenance_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let publication = self.graph_provenance;
        Popover::new("graph-provenance-popover")
            .anchor(Corner::BottomRight)
            .appearance(false)
            .trigger(
                Button::new("graph-provenance-trigger")
                    .label("SOURCE")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.kernel.execute(KernelCommand::RequestGraphProvenance) {
                            Ok(receipt) => match receipt.outcome {
                                KernelOutcome::GraphProvenance(publication) => {
                                    this.graph_provenance = publication;
                                    this.status = "GRAPH PROVENANCE / VERIFIED".into();
                                }
                                _ => {
                                    this.status =
                                        "GRAPH BLOCKED / PROVENANCE RECEIPT MISMATCH".into();
                                }
                            },
                            Err(error) => {
                                this.status =
                                    format!("GRAPH BLOCKED / PROVENANCE / {error}").into();
                            }
                        }
                        cx.notify();
                    })),
            )
            .content(move |_, _, _| provenance_card(publication))
    }

    pub(super) fn mutate_graph_view(
        &mut self,
        mutate: impl FnOnce(&mut GraphViewState),
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(mut view) = self.kernel_snapshot().map(|snapshot| snapshot.graph_view) else {
            self.status = format!("{label} BLOCKED / KERNEL SNAPSHOT").into();
            cx.notify();
            return;
        };
        mutate(&mut view);
        match self
            .kernel
            .execute(KernelCommand::SetGraphView(Box::new(view)))
        {
            Ok(_) => match self
                .graph
                .borrow()
                .as_ref()
                .map(|graph| graph.sync_kernel_state())
            {
                Some(Ok(())) => self.status = format!("{label} / APPLIED IN PLACE").into(),
                Some(Err(error)) => {
                    lifecycle::mark_proof_failed();
                    self.status = format!("{label} BLOCKED / {error:#}").into();
                }
                None => self.status = format!("{label} / STORED / GRAPH OPTIONAL").into(),
            },
            Err(error) => self.status = format!("{label} BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn dispatch_manifold(&mut self, manifold: Manifold, cx: &mut Context<Self>) {
        match self.kernel.execute(KernelCommand::SetManifold(manifold)) {
            Ok(_) => match self
                .graph
                .borrow()
                .as_ref()
                .map(|graph| graph.sync_kernel_state())
            {
                Some(Ok(())) => self.status = format!("SPACE / {manifold:?}").into(),
                Some(Err(error)) => {
                    lifecycle::mark_proof_failed();
                    self.status = format!("SPACE BLOCKED / {error:#}").into();
                }
                None => self.status = format!("SPACE / {manifold:?} / GRAPH OPTIONAL").into(),
            },
            Err(error) => self.status = format!("SPACE BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn dispatch_graph_action(&mut self, action: GraphAction, cx: &mut Context<Self>) {
        match self
            .kernel
            .execute(KernelCommand::DispatchGraphAction(action))
        {
            Ok(_) => {
                let result = self.graph.borrow().as_ref().map(|graph| match action {
                    GraphAction::Fit => graph.fit_graph(),
                    GraphAction::Reset => graph.reset_camera(),
                });
                match result {
                    Some(Ok(())) => self.status = format!("GRAPH / {action:?}").into(),
                    Some(Err(error)) => {
                        lifecycle::mark_proof_failed();
                        self.status = format!("GRAPH BLOCKED / {action:?} / {error:#}").into();
                    }
                    None => self.status = format!("GRAPH / {action:?} / GRAPH OPTIONAL").into(),
                }
            }
            Err(error) => self.status = format!("GRAPH BLOCKED / {action:?} / {error}").into(),
        }
        cx.notify();
    }
}

pub(super) fn surface_segment(
    surface: GraphSurface,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let mut segment = div()
        .w_full()
        .flex()
        .items_center()
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(0x0d100f));
    for candidate in [GraphSurface::Entities, GraphSurface::Atlas] {
        let selected = candidate == surface;
        segment = segment.child(
            div()
                .id(("graph-surface", candidate as usize))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(if selected { ACCENT } else { TEXT_MUTED }))
                .when(selected, |item| {
                    item.bg(rgb(CONTROL_ACTIVE))
                        .border_1()
                        .border_color(rgb(CONTROL_ACTIVE_BORDER))
                })
                .hover(|item| item.bg(rgb(CONTROL_RAISED)).text_color(rgb(TEXT)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mutate_graph_view(|next| next.surface = candidate, "SURFACE", cx);
                }))
                .child(match candidate {
                    GraphSurface::Entities => "ENTITIES",
                    GraphSurface::Atlas => "ATLAS",
                }),
        );
    }
    segment
}

fn manifold_segment(active: Manifold, cx: &mut Context<PhoenixShell>) -> impl IntoElement {
    let mut segment = div()
        .flex()
        .items_center()
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(0x0d100f));
    for manifold in Manifold::ALL {
        let selected = manifold == active;
        segment = segment.child(
            div()
                .id(("manifold-selector", manifold as usize))
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(if selected { VIOLET } else { TEXT_MUTED }))
                .when(selected, |item| {
                    item.bg(rgb(VIOLET_DIM))
                        .border_1()
                        .border_color(rgb(0x534783))
                })
                .hover(|item| item.bg(rgb(CONTROL_RAISED)).text_color(rgb(TEXT)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch_manifold(manifold, cx);
                }))
                .child(manifold_label(manifold)),
        );
    }
    segment
}

fn review_toggle(
    label: &'static str,
    mask: ReviewMask,
    view: GraphViewState,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let selected = view.reviews.contains(mask);
    div()
        .id(("review-toggle", mask.0 as usize))
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(if selected {
            CONTROL_ACTIVE_BORDER
        } else {
            BORDER
        }))
        .bg(rgb(if selected { CONTROL_ACTIVE } else { CONTROL_BG }))
        .cursor_pointer()
        .text_xs()
        .text_color(rgb(if selected { ACCENT } else { TEXT_MUTED }))
        .hover(|item| item.bg(rgb(CONTROL_RAISED)).text_color(rgb(TEXT)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.mutate_graph_view(
                |next| next.reviews = next.reviews.toggled(mask),
                "REVIEWS",
                cx,
            );
        }))
        .child(label)
}

fn action_button(
    id: &'static str,
    label: &'static str,
    action: GraphAction,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    Button::new(id)
        .label(label)
        .small()
        .ghost()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch_graph_action(action, cx);
        }))
}

fn popover_card(title: &'static str, detail: &'static str) -> gpui::Div {
    div()
        .w(px(248.))
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER_BRIGHT))
        .bg(rgb(POPOVER_BG))
        .shadow_lg()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(ACCENT))
                .child(title),
        )
        .child(
            div()
                .mt_1()
                .mb_2()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(detail),
        )
}

fn popover_option(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    selected: bool,
    on_click: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_2()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
        .when(selected, |item| item.bg(rgb(ACCENT_DIM)))
        .hover(|item| item.bg(rgb(CONTROL_RAISED)).text_color(rgb(TEXT)))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label)
        .child(
            div()
                .text_xs()
                .text_color(rgb(if selected { ACCENT } else { 0x48504e }))
                .child(if selected { "●" } else { "○" }),
        )
}

fn provenance_card(provenance: Option<GraphProvenanceReceipt>) -> impl IntoElement {
    let card = popover_card("SCENE AUTHORITY", "Compact native generation provenance.").w(px(304.));
    match provenance {
        Some(receipt) => card
            .child(provenance_row(
                "AUTHORITY",
                match receipt.source {
                    SceneSource::Archive => "ARCHIVE",
                    SceneSource::Backend => "BACKEND",
                    SceneSource::RegistryOnly => "REGISTRY ONLY",
                    SceneSource::VerificationFixture => "VERIFIED FIXTURE",
                },
            ))
            .child(provenance_row(
                "GENERATION",
                format!("G{}", receipt.generation_id),
            ))
            .child(provenance_row(
                "GEOMETRY",
                format!("{} N / {} E", receipt.node_count, receipt.edge_count),
            ))
            .child(provenance_row(
                "REGISTRY",
                format!("REV {}", receipt.registry_revision),
            ))
            .child(provenance_row("COHORT", short_hash(receipt.cohort_hash)))
            .child(provenance_row(
                "PRODUCT",
                receipt
                    .product_index_hash
                    .map(short_hash)
                    .unwrap_or_else(|| "UNBOUND".to_owned()),
            )),
        None => card.child(
            div()
                .mt_2()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child("No published native scene."),
        ),
    }
}

fn provenance_row(label: &'static str, value: impl Into<gpui::SharedString>) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .py_1()
        .text_xs()
        .child(div().text_color(rgb(TEXT_MUTED)).child(label))
        .child(div().text_color(rgb(TEXT)).child(value.into()))
}

fn short_hash(hash: [u8; 32]) -> String {
    let mut value = String::with_capacity(12);
    for byte in &hash[..6] {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

const fn scope_label(scope: GraphScope) -> &'static str {
    match scope {
        GraphScope::Global => "GLOBAL",
        GraphScope::Narrative => "NARRATIVE",
        GraphScope::Note => "NOTE",
        GraphScope::Compare => "COMPARE",
    }
}

const fn relation_label(family: RelationFamily) -> &'static str {
    match family {
        RelationFamily::CoOccurrence => "Co-occurrence",
        RelationFamily::Observation => "Observation",
        RelationFamily::Communication => "Communication",
        RelationFamily::Causal => "Causal",
        RelationFamily::Temporal => "Temporal",
        RelationFamily::Structural => "Structural",
        RelationFamily::Identity => "Identity",
        RelationFamily::Relationship => "Relationship",
        RelationFamily::Event => "Event",
        RelationFamily::MemoryState => "Memory/state",
    }
}

const fn manifold_label(manifold: Manifold) -> &'static str {
    match manifold {
        Manifold::Hybrid => "HYBRID",
        Manifold::Hopf => "HOPF",
        Manifold::Caps => "CAPS",
        Manifold::Transit => "TRANSIT",
        Manifold::Siegel => "SIEGEL",
    }
}

const fn has_atlas_control_rail(surface: GraphSurface) -> bool {
    matches!(surface, GraphSurface::Atlas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angular_submodes_do_not_exist_in_native_surface_labels() {
        let visible = Manifold::ALL.map(manifold_label);
        for removed in ["PROJECTION", "FINSLER", "SHELL", "MULTI"] {
            assert!(!visible.contains(&removed));
        }
    }

    #[test]
    fn exactly_two_product_surfaces_are_exposed() {
        assert_eq!([GraphSurface::Entities, GraphSurface::Atlas].len(), 2);
    }

    #[test]
    fn entity_surface_does_not_reserve_an_empty_control_rail() {
        assert!(!has_atlas_control_rail(GraphSurface::Entities));
        assert!(has_atlas_control_rail(GraphSurface::Atlas));
    }
}
