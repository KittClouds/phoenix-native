use super::drawer::{ACCENT, ACCENT_DIM};
use super::graph_controls::surface_segment;
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgb, uniform_list, Context,
    IntoElement, SharedString,
};
use gpui_component::input::Input;
use gpui_component::{Sizable, StyledExt};
use phoenix_app_core::{AtlasEntity, AtlasRegistry, GraphSelectionCommand, KernelCommand};
use phoenix_scene_contract::{EntityKind, GraphViewState, HighlightPalette};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const ENTITY_ROW_HEIGHT: f32 = 44.;
const KIND_TILE_HEIGHT: f32 = 24.;
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
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .h_full();

        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(linear_gradient(
                155.,
                linear_color_stop(rgb(0x092820), 0.),
                linear_color_stop(rgb(0x101211), 1.),
            ))
            .child(atlas_header(&atlas, graph_view, cx))
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .child(Input::new(&self.atlas_search).small()),
            )
            .child(kind_summary(&atlas, palette))
            .child(
                div()
                    .mt_1()
                    .px_2()
                    .pb_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("IDENTITIES")
                    .child(format!("{} SHOWN", visible.len())),
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

fn atlas_header(
    atlas: &AtlasRegistry,
    graph_view: GraphViewState,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    div()
        .px_2()
        .pt_2()
        .pb_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(rgb(ACCENT))
                        .child("ATLAS ENTITIES"),
                )
                .child(
                    div()
                        .min_w(px(28.))
                        .px_2()
                        .py(px(2.))
                        .rounded_full()
                        .bg(rgb(ACCENT_DIM))
                        .text_xs()
                        .text_center()
                        .text_color(rgb(ACCENT))
                        .child(atlas.entities.len().to_string()),
                ),
        )
        .child(
            div()
                .mt_1()
                .w_full()
                .child(surface_segment(graph_view.surface, cx)),
        )
        .child(
            div()
                .mt_1()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(authority_stat(
                    "USER",
                    atlas.user_tagged_source_count.to_string(),
                ))
                .child(authority_stat("NER", atlas.ner_source_count.to_string()))
                .child(authority_stat("REV", atlas.registry_revision.to_string())),
        )
}

fn authority_stat(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .px_1()
        .py(px(2.))
        .rounded_sm()
        .bg(rgb(0x111715))
        .child(label)
        .child(div().text_color(rgb(0xb5c2be)).child(value))
}

fn kind_summary(atlas: &AtlasRegistry, palette: HighlightPalette) -> impl IntoElement {
    let mut rows = div().mt_2().px_2().grid().grid_cols(3).gap_1();
    for kind in EntityKind::TOOLBAR {
        let count = atlas
            .entities
            .iter()
            .filter(|entity| entity.kind == kind)
            .count();
        if count == 0 {
            continue;
        }
        rows = rows.child(
            div()
                .h(px(KIND_TILE_HEIGHT))
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x24302c))
                .bg(rgb(0x121816))
                .child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(rgb(family_color(kind, palette))),
                )
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
) -> impl IntoElement {
    let stable_id = entity.stable_id;
    let label = Arc::clone(&entity.label);
    let kind = entity
        .custom_kind
        .as_ref()
        .map(|kind| kind.to_string())
        .unwrap_or_else(|| entity.kind.label().to_uppercase());
    let marker_color = if selected {
        ACCENT
    } else {
        family_color(entity.kind, palette)
    };
    div()
        .id(SharedString::from(format!("atlas-entity-{stable_id}")))
        .h(px(ENTITY_ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(rgb(0x26302d))
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(linear_gradient(
                90.,
                linear_color_stop(rgb(0x123f34), 0.),
                linear_color_stop(rgb(0x17211e), 1.),
            ))
        })
        .hover(|row| row.bg(rgb(0x17211e)))
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
                .h(px(28.))
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
                        .text_sm()
                        .font_medium()
                        .text_color(rgb(TEXT))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(kind)
                        .when(entity.sources.user_tagged, |row| {
                            row.child(source_chip("USER", 0x174438, ACCENT))
                        })
                        .when(entity.sources.ner, |row| {
                            row.child(source_chip("NER", 0x24334a, 0x78aaff))
                        }),
                ),
        )
        .child(
            div()
                .min_w(px(30.))
                .px_1()
                .py(px(2.))
                .rounded_md()
                .bg(rgb(0x111715))
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(format!("{}x", entity.mention_count)),
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
        EntityKind::Character => "CHAR",
        EntityKind::Location => "PLACE",
        EntityKind::Npc => "NPC",
        EntityKind::Faction => "GROUP",
        EntityKind::Event => "EVENT",
        EntityKind::Concept => "IDEA",
        EntityKind::Custom => "CUSTOM",
    }
}

fn family_color(kind: EntityKind, palette: HighlightPalette) -> u32 {
    let [red, green, blue, _] = palette.for_family(kind.family()).primary;
    ((red * 255.).round() as u32) << 16
        | ((green * 255.).round() as u32) << 8
        | (blue * 255.).round() as u32
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
        assert_eq!(compact_kind_label(EntityKind::Character), "CHAR");
        assert_eq!(compact_kind_label(EntityKind::Location), "PLACE");
    }
}
