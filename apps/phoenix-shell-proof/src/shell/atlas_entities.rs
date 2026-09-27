use super::drawer::ACCENT;
use super::graph_controls::surface_segment;
use super::style_hub::GraphSidebarPanel;
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, uniform_list, Context, Entity, IntoElement, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Selectable, Sizable, StyledExt};
use phoenix_app_core::{AtlasEntity, AtlasRegistry, GraphSelectionCommand, KernelCommand};
use phoenix_scene_contract::{EntityKind, GraphColorKey, GraphViewState, HighlightPalette};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const ENTITY_ROW_HEIGHT: f32 = 42.;
const KIND_TILE_HEIGHT: f32 = 22.;
const SIDEBAR_BG: u32 = 0x121816;
const QUIET: u32 = 0x6f7a76;
const _: () = assert!(ENTITY_ROW_HEIGHT <= 44.);
const _: () = assert!(KIND_TILE_HEIGHT <= 24.);

impl PhoenixShell {
    pub(super) fn render_atlas_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.kernel_snapshot();
        let atlas = snapshot
            .as_ref()
            .map(|snapshot| Arc::clone(&snapshot.atlas_registry))
            .unwrap_or_else(empty_atlas);
        let palette = snapshot
            .as_ref()
            .map(|snapshot| *snapshot.highlight_palette)
            .unwrap_or_default();
        let graph_view = snapshot
            .as_ref()
            .map(|snapshot| snapshot.graph_view)
            .unwrap_or_default();
        let panel = self.graph_sidebar_panel;
        let shell = div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SIDEBAR_BG))
            .child(atlas_header(&atlas, graph_view, panel, cx));

        if panel == GraphSidebarPanel::StyleHub {
            return shell.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.render_style_hub(snapshot.as_ref(), cx)),
            );
        }

        let query = self.atlas_search.read(cx).value().trim().to_string();
        let visible = atlas
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| entity_matches(entity, &query).then_some(index))
            .collect::<Vec<_>>();
        let visible: Arc<[usize]> = visible.into();
        let list_entities = Arc::clone(&atlas.entities);
        let list_visible = Arc::clone(&visible);
        let selected_entity = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.graph_selection.entity_id);
        let kernel = Arc::clone(&self.kernel);
        let graph = Rc::clone(&self.graph);
        let shell_entity = cx.entity();
        let list = uniform_list(
            "canonical-atlas-entities",
            visible.len(),
            move |range, _, _| {
                range
                    .map(|row| {
                        let entity = &list_entities[list_visible[row]];
                        atlas_entity_row(
                            entity,
                            palette,
                            selected_entity == Some(entity.stable_id),
                            Arc::clone(&kernel),
                            Rc::clone(&graph),
                            shell_entity.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .h_full();

        shell
            .child(
                div()
                    .px_2()
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(Input::new(&self.atlas_search).small()),
                    )
                    .child(
                        Button::new("registry-add-entity")
                            .icon(gpui_component::IconName::Plus)
                            .tooltip("Add an entity")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_registry_create(window, cx);
                            })),
                    ),
            )
            .child(kind_summary(self, &atlas))
            .child(
                div()
                    .mt(px(6.))
                    .px_2()
                    .pb(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(QUIET))
                    .child(format!("{} in this note", visible.len()))
                    .child(format!(
                        "{} tagged · {} found · rev {}",
                        atlas.user_tagged_source_count,
                        atlas.ner_source_count,
                        atlas.registry_revision
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .pb_1()
                    .when(visible.is_empty(), |body| {
                        body.flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .px_4()
                            .text_center()
                            .child(div().text_sm().text_color(rgb(TEXT_MUTED)).child(
                                if atlas.entities.is_empty() {
                                    "No entity authority yet."
                                } else {
                                    "No matching identity."
                                },
                            ))
                            .when(atlas.entities.is_empty(), |message| {
                                message.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x66716e))
                                        .child("Tag text to begin."),
                                )
                            })
                    })
                    .when(!visible.is_empty(), |body| body.child(list)),
            )
    }
}

/// One compact row: title with count, the Entities/Atlas surface switch, and
/// the appearance toggle.
fn atlas_header(
    atlas: &AtlasRegistry,
    graph_view: GraphViewState,
    panel: GraphSidebarPanel,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    div()
        .h(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child("Identities"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(QUIET))
                .child(atlas.entities.len().to_string()),
        )
        .child(div().flex_1())
        .child(surface_segment(graph_view.surface, cx))
        .child(
            Button::new("graph-sidebar-panel-toggle")
                .icon(match panel {
                    GraphSidebarPanel::Registry => gpui_component::IconName::Palette,
                    GraphSidebarPanel::StyleHub => gpui_component::IconName::Menu,
                })
                .tooltip(match panel {
                    GraphSidebarPanel::Registry => "Appearance",
                    GraphSidebarPanel::StyleHub => "Back to identities",
                })
                .small()
                .ghost()
                .selected(panel == GraphSidebarPanel::StyleHub)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.graph_sidebar_panel = match this.graph_sidebar_panel {
                        GraphSidebarPanel::Registry => GraphSidebarPanel::StyleHub,
                        GraphSidebarPanel::StyleHub => GraphSidebarPanel::Registry,
                    };
                    cx.notify();
                })),
        )
}

fn kind_summary(shell: &PhoenixShell, atlas: &AtlasRegistry) -> impl IntoElement {
    let mut rows = div().mt(px(6.)).px_2().grid().grid_cols(2).gap(px(3.));
    for kind in EntityKind::TOOLBAR {
        let count = atlas
            .entities
            .iter()
            .filter(|entity| entity.kind == kind)
            .count();
        if count == 0 {
            continue;
        }
        let color_key = entity_color_key(kind);
        rows = rows.child(
            div()
                .h(px(KIND_TILE_HEIGHT))
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .rounded_md()
                .bg(rgb(0x161d1b))
                .child(shell.graph_color_picker(color_key))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_xs()
                        .text_color(rgb(0xaab9b4))
                        .child(compact_kind_label(kind)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(count.to_string()),
                ),
        );
    }
    rows
}

fn atlas_entity_row(
    entity: &AtlasEntity,
    palette: HighlightPalette,
    selected: bool,
    kernel: Arc<phoenix_app_core::PhoenixKernel>,
    graph: Rc<RefCell<Option<crate::graph_window::GraphWindow>>>,
    shell: Entity<PhoenixShell>,
) -> impl IntoElement {
    let stable_id = entity.stable_id;
    let label = Arc::clone(&entity.label);
    let kind = entity
        .custom_kind
        .as_ref()
        .map(|kind| kind.to_string())
        .unwrap_or_else(|| entity.kind.label().to_owned());
    let marker_color = if selected {
        ACCENT
    } else {
        super::palette_controls::rgba_u32(palette.graph.color(entity_color_key(entity.kind)))
    };
    div()
        .id(SharedString::from(format!("atlas-entity-{stable_id}")))
        .h(px(ENTITY_ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .mx_1()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |row| row.bg(rgb(0x163129)))
        .when(!selected, |row| row.hover(|row| row.bg(rgb(0x19201e))))
        .on_click(move |_, _, cx| {
            if kernel
                .execute(KernelCommand::SetGraphSelection(
                    GraphSelectionCommand::AtlasEntity(stable_id),
                ))
                .is_ok()
            {
                if let Some(graph) = graph.borrow().as_ref() {
                    let _ = graph.sync_kernel_state();
                }
                cx.refresh_windows();
            }
        })
        .child(
            div()
                .w(px(2.))
                .h(px(22.))
                .flex_shrink_0()
                .rounded_full()
                .bg(rgb(marker_color)),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .font_medium()
                        .text_color(rgb(TEXT))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .mt(px(1.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(kind)
                        .when(entity.sources.user_tagged, |row| {
                            row.child(source_chip("Tagged", 0x174438, ACCENT))
                        })
                        .when(entity.sources.ner, |row| {
                            row.child(source_chip("Found", 0x24334a, 0x78aaff))
                        }),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(QUIET))
                .child(format!("{}\u{d7}", entity.mention_count)),
        )
        .child(
            div()
                .id(("registry-edit", stable_id as usize))
                .px(px(6.))
                .py(px(1.))
                .rounded_md()
                .text_xs()
                .text_color(rgb(QUIET))
                .hover(|edit| edit.bg(rgb(0x1f2826)).text_color(rgb(ACCENT)))
                .child("Edit")
                .on_click(move |_, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.open_registry_edit(stable_id, window, cx);
                    });
                }),
        )
}

fn source_chip(label: &'static str, background: u32, foreground: u32) -> impl IntoElement {
    div()
        .px_1()
        .rounded_sm()
        .bg(rgb(background))
        .text_color(rgb(foreground))
        .child(label)
}

fn compact_kind_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Character => "Characters",
        EntityKind::Location => "Places",
        EntityKind::Npc => "NPC",
        EntityKind::Faction => "Groups",
        EntityKind::Network => "Networks",
        EntityKind::Creature => "Creatures",
        EntityKind::Event => "Events",
        EntityKind::Concept => "Ideas",
        EntityKind::Custom => "Custom",
    }
}

pub(super) const fn entity_color_key(kind: EntityKind) -> GraphColorKey {
    match kind {
        EntityKind::Character => GraphColorKey::Characters,
        EntityKind::Location => GraphColorKey::Locations,
        EntityKind::Npc => GraphColorKey::Npcs,
        EntityKind::Faction => GraphColorKey::Factions,
        EntityKind::Event => GraphColorKey::Events,
        EntityKind::Concept => GraphColorKey::Concepts,
        EntityKind::Network => GraphColorKey::Networks,
        EntityKind::Creature => GraphColorKey::Creatures,
        EntityKind::Custom => GraphColorKey::OtherEntities,
    }
}

fn entity_matches(entity: &AtlasEntity, query: &str) -> bool {
    query.is_empty()
        || contains_ascii_case_insensitive(&entity.label, query)
        || contains_ascii_case_insensitive(entity.kind.label(), query)
        || entity
            .custom_kind
            .as_ref()
            .is_some_and(|kind| contains_ascii_case_insensitive(kind, query))
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn empty_atlas() -> Arc<AtlasRegistry> {
    Arc::new(AtlasRegistry {
        registry_revision: 0,
        ner_revision: 0,
        entities: Arc::from([]),
        ner_source_count: 0,
        user_tagged_source_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_workspace::EntitySourceMask;

    fn entity(label: &str) -> AtlasEntity {
        AtlasEntity {
            stable_id: 7,
            label: Arc::from(label),
            kind: EntityKind::Location,
            custom_kind: None,
            sources: EntitySourceMask::USER_TAGGED,
            mention_count: 1,
        }
    }

    #[test]
    fn atlas_search_is_case_insensitive_without_label_identity_semantics() {
        let new_rome = entity("New Rome");
        assert!(entity_matches(&new_rome, "rome"));
        assert!(entity_matches(&new_rome, "LOCATION"));
        assert!(!entity_matches(&new_rome, "Ryan"));
    }

    #[test]
    fn compact_kind_labels_use_bounded_copy() {
        assert_eq!(compact_kind_label(EntityKind::Character), "Characters");
        assert_eq!(compact_kind_label(EntityKind::Location), "Places");
    }
}
