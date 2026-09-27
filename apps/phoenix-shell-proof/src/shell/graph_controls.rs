use super::{drawer::ACCENT, PhoenixShell, BORDER_BRIGHT, TEXT, TEXT_MUTED};
use crate::lifecycle;
use gpui::{div, prelude::*, px, rgb, Context, Corner, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::popover::Popover;
use gpui_component::Sizable;
use phoenix_app_core::{GraphProvenanceReceipt, KernelCommand, KernelOutcome};
use phoenix_scene_archive::{PageKey, PageKind};
use phoenix_scene_contract::{
    FamilyMask, GraphAction, GraphEdgePresentation, GraphNavigationOverlay, GraphScope,
    GraphSurface, GraphTopologyEmphasis, GraphViewState, Manifold, RelationFamily, ResidentScene,
    SceneSource,
};

const CONTROL_RAISED: u32 = 0x1a201e;
const POPOVER_BG: u32 = 0x181c1b;

impl PhoenixShell {
    /// Toolbar, optional settings shelf, then whichever mode strips are live.
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
            .child(self.render_graph_toolbar(view, cx))
            .children(self.render_graph_shelf(view, cx))
            .when(view.source_local, |controls| {
                controls.child(self.render_source_local_strip(cx))
            })
            .when_some(
                self.route_walk_status()
                    .filter(|status| self.route_walk_strip_visible(status)),
                |controls, status| controls.child(self.render_route_walk_strip(&status, cx)),
            )
            .when_some(
                self.route_walk_status().and_then(|status| status.flow),
                |controls, flow| controls.child(self.render_flow_strip(flow, cx)),
            )
            .when_some(
                self.route_walk_status().and_then(|status| status.story),
                |controls, story| controls.child(self.render_story_strip(&story, cx)),
            )
            .when(view.manifold == Manifold::Caps, |row| {
                row.child(self.render_caps_space(cx))
            })
    }

    pub(super) fn provenance_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let publication = self.graph_provenance;
        Popover::new("graph-provenance-popover")
            .anchor(Corner::BottomRight)
            .appearance(false)
            .trigger(
                Button::new("graph-provenance-trigger")
                    .label("Provenance")
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

    pub(super) fn dispatch_manifold(&mut self, manifold: Manifold, cx: &mut Context<Self>) {
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

    pub(super) fn dispatch_graph_action(&mut self, action: GraphAction, cx: &mut Context<Self>) {
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

/// Which prepared path pages (straight, curved, bundled) the resident scene
/// carries for a manifold.
pub(super) fn edge_pages_available(scene: &ResidentScene, manifold: Manifold) -> [bool; 3] {
    [
        PageKind::StraightPaths,
        PageKind::CurvedPaths,
        PageKind::BundledPaths,
    ]
    .map(|kind| {
        scene
            .archive()
            .has_page(PageKey::manifold(kind, manifold.into()))
    })
}

/// Edge styles offered for the current scene: manifold and hidden always,
/// prepared path styles only when their pages are resident.
pub(super) fn available_edge_presentations(available: [bool; 3]) -> Vec<GraphEdgePresentation> {
    [
        (GraphEdgePresentation::Manifold, true),
        (GraphEdgePresentation::Straight, available[0]),
        (GraphEdgePresentation::Curved, available[1]),
        (GraphEdgePresentation::Bundled, available[2]),
        (GraphEdgePresentation::Hidden, true),
    ]
    .into_iter()
    .filter_map(|(style, present)| present.then_some(style))
    .collect()
}

pub(super) const fn edge_presentation_label(style: GraphEdgePresentation) -> &'static str {
    match style {
        GraphEdgePresentation::Manifold => "Manifold",
        GraphEdgePresentation::Straight => "Straight",
        GraphEdgePresentation::Curved => "Curved",
        GraphEdgePresentation::Bundled => "Bundled",
        GraphEdgePresentation::Hidden => "Hidden",
    }
}

pub(super) const fn topology_emphasis_label(emphasis: GraphTopologyEmphasis) -> &'static str {
    match emphasis {
        GraphTopologyEmphasis::Off => "Off",
        GraphTopologyEmphasis::Structure => "Structure",
        GraphTopologyEmphasis::Facts => "Facts",
        GraphTopologyEmphasis::Discourse => "Discourse",
    }
}

pub(super) const fn navigation_overlay_label(overlay: GraphNavigationOverlay) -> &'static str {
    match overlay {
        GraphNavigationOverlay::Off => "Off",
        GraphNavigationOverlay::Backbone => "Backbone",
        GraphNavigationOverlay::Bridges => "Bridges",
        GraphNavigationOverlay::Both => "Both",
    }
}

/// Compact Entities / Atlas surface switch used by the sidebar header.
pub(super) fn surface_segment(
    surface: GraphSurface,
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
        .border_color(rgb(0x252c2a))
        .bg(rgb(0x0b0e0d));
    for candidate in [GraphSurface::Entities, GraphSurface::Atlas] {
        let selected = candidate == surface;
        segment = segment.child(
            div()
                .id(("graph-surface", candidate as usize))
                .h_full()
                .flex()
                .items_center()
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(if selected { 0x8ff0d2 } else { TEXT_MUTED }))
                .when(selected, |item| item.bg(rgb(0x1a3a32)))
                .when(!selected, |item| {
                    item.hover(|item| item.bg(rgb(CONTROL_RAISED)).text_color(rgb(TEXT)))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mutate_graph_view(|next| next.surface = candidate, "SURFACE", cx);
                }))
                .child(match candidate {
                    GraphSurface::Entities => "Entities",
                    GraphSurface::Atlas => "Atlas",
                }),
        );
    }
    segment
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

pub(super) const fn scope_label(scope: GraphScope) -> &'static str {
    match scope {
        GraphScope::Global => "Global",
        GraphScope::Narrative => "Narrative",
        GraphScope::Note => "Note",
        GraphScope::Compare => "Compare",
    }
}

pub(super) fn document_detail_visible(view: GraphViewState) -> bool {
    view.topology_families.intersects(FamilyMask(
        FamilyMask::PARAGRAPHS.0 | FamilyMask::SENTENCES.0,
    ))
}

/// Shows or hides paragraph and sentence detail; nothing else changes.
pub(super) fn set_document_detail(view: &mut GraphViewState, show: bool) {
    for detail in [FamilyMask::PARAGRAPHS, FamilyMask::SENTENCES] {
        if view.topology_families.contains(detail) != show {
            view.toggle_topology_family(detail);
        }
    }
}

pub(super) const fn relation_label(family: RelationFamily) -> &'static str {
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

pub(super) const fn manifold_label(manifold: Manifold) -> &'static str {
    match manifold {
        Manifold::Hybrid => "Hybrid",
        Manifold::Torus => "Torus",
        Manifold::Hopf => "Hopf",
        Manifold::Caps => "Caps",
        Manifold::Transit => "Transit",
        Manifold::Siegel => "Siegel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_styles_offer_only_published_paths_and_keep_hidden_reachable() {
        assert_eq!(
            available_edge_presentations([true; 3]),
            vec![
                GraphEdgePresentation::Manifold,
                GraphEdgePresentation::Straight,
                GraphEdgePresentation::Curved,
                GraphEdgePresentation::Bundled,
                GraphEdgePresentation::Hidden,
            ]
        );
        assert_eq!(
            available_edge_presentations([false, true, false]),
            vec![
                GraphEdgePresentation::Manifold,
                GraphEdgePresentation::Curved,
                GraphEdgePresentation::Hidden,
            ]
        );
    }

    #[test]
    fn overview_only_changes_sentence_and_paragraph_visibility() {
        let mut view = GraphViewState::default();
        let original = view;
        set_document_detail(&mut view, false);
        assert!(!document_detail_visible(view));
        assert!(view.is_valid());
        assert_eq!(view.relations, original.relations);
        assert_eq!(view.manifold, original.manifold);
        set_document_detail(&mut view, true);
        assert!(document_detail_visible(view));
        assert_eq!(view.topology_families, original.topology_families);
    }

    #[test]
    fn angular_submodes_do_not_exist_in_native_surface_labels() {
        let visible = Manifold::ALL.map(manifold_label);
        for removed in ["Projection", "Finsler", "Shell", "Multi"] {
            assert!(!visible.contains(&removed));
        }
        assert!(visible.contains(&"Torus"));
        assert!(visible.contains(&"Hopf"));
    }

    #[test]
    fn display_labels_cover_navigation_and_emphasis_modes() {
        assert_eq!(
            navigation_overlay_label(GraphNavigationOverlay::Bridges),
            "Bridges"
        );
        assert_eq!(
            topology_emphasis_label(GraphTopologyEmphasis::Facts),
            "Facts"
        );
    }

    #[test]
    fn exactly_two_product_surfaces_are_exposed() {
        assert_eq!([GraphSurface::Entities, GraphSurface::Atlas].len(), 2);
    }
}
