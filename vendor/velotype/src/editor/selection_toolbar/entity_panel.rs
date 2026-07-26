use super::*;

const ENTITY_PANEL_WIDTH: f32 = 176.0;

impl Editor {
    fn open_custom_entity_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(String::new())));
        input.read(cx).focus_handle.focus(window);
        self.selection_toolbar.custom_entity_input = Some(input);
        self.selection_toolbar.entity_error = None;
        cx.notify();
    }

    fn emit_entity_tag(
        &mut self,
        kind: EntityTagKind,
        custom_kind: Option<SharedString>,
        expected: &SelectionIdentity,
        cx: &mut Context<Self>,
    ) {
        if self.validated_selection_slices(expected, cx).is_none() {
            self.selection_toolbar.entity_error = Some("The selection changed.".into());
            cx.notify();
            return;
        }
        let source = self.current_document_source(cx);
        let range = expected.source.range.clone();
        let Some(surface) = source.get(range.clone()) else {
            self.selection_toolbar.entity_error =
                Some("The selection is not source-aligned.".into());
            cx.notify();
            return;
        };
        if surface.is_empty() || surface != surface.trim() {
            self.selection_toolbar.entity_error =
                Some("Select the entity name without surrounding spaces.".into());
            cx.notify();
            return;
        }
        cx.emit(EditorEvent::EntityTagRequested(EntityTagRequest {
            kind,
            custom_kind,
            source_range: range,
            surface: surface.to_owned().into(),
            editor_revision: expected.document_revision,
        }));
        self.dismiss_selection_toolbar(cx);
    }

    fn choose_entity_kind(&mut self, kind: EntityTagKind, cx: &mut Context<Self>) {
        let Some(identity) = self
            .selection_toolbar
            .lease
            .as_ref()
            .map(|lease| lease.identity.clone())
        else {
            return;
        };
        self.emit_entity_tag(kind, None, &identity, cx);
    }

    fn apply_custom_entity_kind(&mut self, cx: &mut Context<Self>) {
        let Some(identity) = self
            .selection_toolbar
            .lease
            .as_ref()
            .map(|lease| lease.identity.clone())
        else {
            return;
        };
        let Some(input) = self.selection_toolbar.custom_entity_input.as_ref() else {
            return;
        };
        let value = input.read(cx).display_text().trim().to_owned();
        if value.is_empty() || value.len() > MAX_CUSTOM_KIND_BYTES {
            self.selection_toolbar.entity_error =
                Some("Custom kinds must contain 1 to 64 UTF-8 bytes.".into());
            cx.notify();
            return;
        }
        if value.chars().any(char::is_control) {
            self.selection_toolbar.entity_error =
                Some("Custom kinds cannot contain control characters.".into());
            cx.notify();
            return;
        }
        self.emit_entity_tag(EntityTagKind::Custom, Some(value.into()), &identity, cx);
    }

    fn render_entity_row(
        &self,
        id: &'static str,
        label: &'static str,
        kind: EntityTagKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .h(px(28.0))
            .px(px(9.0))
            .flex()
            .items_center()
            .rounded(px(5.0))
            .cursor_pointer()
            .text_size(px(11.0))
            .text_color(rgba(TOOLBAR_TEXT))
            .hover(|row| {
                row.bg(rgba(TOOLBAR_HOVER_BG))
                    .text_color(rgba(TOOLBAR_TEXT_ACTIVE))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, _, window, cx| {
                    if kind == EntityTagKind::Custom {
                        editor.open_custom_entity_editor(window, cx);
                    } else {
                        editor.choose_entity_kind(kind, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .child(label)
            .into_any_element()
    }

    pub(super) fn render_entity_panel(
        &self,
        toolbar_position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.selection_toolbar.entity_panel_open {
            return None;
        }
        let y = toolbar_position.y + px(TOOLBAR_HEIGHT + 6.0);
        let panel = div()
            .id("selection-entity-panel")
            .absolute()
            .left(toolbar_position.x + px(188.0))
            .top(y)
            .w(px(ENTITY_PANEL_WIDTH))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .occlude()
            .rounded(px(7.0))
            .border(px(1.0))
            .border_color(rgba(TOOLBAR_BORDER))
            .bg(rgba(TOOLBAR_BG))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        if let Some(input) = self.selection_toolbar.custom_entity_input.as_ref() {
            return Some(
                panel
                    .child(
                        div()
                            .px(px(3.0))
                            .pb(px(3.0))
                            .text_size(px(10.0))
                            .text_color(rgba(TOOLBAR_TEXT))
                            .child("Custom entity kind"),
                    )
                    .child(
                        div()
                            .h(px(34.0))
                            .w_full()
                            .overflow_hidden()
                            .rounded(px(5.0))
                            .border(px(1.0))
                            .border_color(rgba(TOOLBAR_BORDER))
                            .bg(rgba(0x050b09ff))
                            .child(input.clone()),
                    )
                    .when_some(
                        self.selection_toolbar.entity_error.clone(),
                        |panel, error| {
                            panel.child(
                                div()
                                    .pt(px(3.0))
                                    .text_size(px(10.0))
                                    .text_color(rgba(TOOLBAR_ERROR))
                                    .child(error),
                            )
                        },
                    )
                    .child(
                        div()
                            .id("selection-entity-custom-apply")
                            .mt(px(4.0))
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.0))
                            .cursor_pointer()
                            .bg(rgba(TOOLBAR_ACTIVE_BG))
                            .text_color(rgba(TOOLBAR_TEXT_ACTIVE))
                            .text_size(px(11.0))
                            .child("Tag selection")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|editor, _, _, cx| {
                                    editor.apply_custom_entity_kind(cx);
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        }

        Some(
            panel
                .child(self.render_entity_row(
                    "selection-entity-character",
                    "Character",
                    EntityTagKind::Character,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-location",
                    "Location",
                    EntityTagKind::Location,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-npc",
                    "NPC",
                    EntityTagKind::Npc,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-faction",
                    "Faction",
                    EntityTagKind::Faction,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-event",
                    "Event",
                    EntityTagKind::Event,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-concept",
                    "Concept",
                    EntityTagKind::Concept,
                    cx,
                ))
                .child(self.render_entity_row(
                    "selection-entity-custom",
                    "Custom...",
                    EntityTagKind::Custom,
                    cx,
                ))
                .when_some(
                    self.selection_toolbar.entity_error.clone(),
                    |panel, error| {
                        panel.child(
                            div()
                                .px(px(4.0))
                                .pt(px(3.0))
                                .text_size(px(10.0))
                                .text_color(rgba(TOOLBAR_ERROR))
                                .child(error),
                        )
                    },
                )
                .into_any_element(),
        )
    }
}
