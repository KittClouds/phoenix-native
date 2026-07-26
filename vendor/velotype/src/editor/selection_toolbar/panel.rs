use super::*;

impl Editor {
    pub(super) fn render_link_panel(
        &self,
        toolbar_position: Point<Pixels>,
        lease: &SelectionLease,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.selection_toolbar.link_input.as_ref()?;
        let y = toolbar_position.y + px(TOOLBAR_HEIGHT + 6.0);
        let has_link = matches!(lease.link_state, SelectionLinkState::On(_));
        let mut actions = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.0))
            .child(
                div()
                    .id("selection-link-apply")
                    .px(px(10.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .bg(rgba(TOOLBAR_ACTIVE_BG))
                    .text_color(rgba(TOOLBAR_TEXT_ACTIVE))
                    .text_size(px(11.0))
                    .child("Apply")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, _, _, cx| {
                            editor.apply_link_editor(cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        if has_link {
            actions = actions.child(
                div()
                    .id("selection-link-remove")
                    .px(px(10.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_color(rgba(TOOLBAR_TEXT))
                    .hover(|button| button.bg(rgba(TOOLBAR_HOVER_BG)))
                    .text_size(px(11.0))
                    .child("Remove")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, _, _, cx| {
                            editor.remove_selection_link(cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }

        Some(
            div()
                .id("selection-link-panel")
                .absolute()
                .left(toolbar_position.x)
                .top(y)
                .w(px(LINK_PANEL_WIDTH))
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .occlude()
                .rounded(px(7.0))
                .border(px(1.0))
                .border_color(rgba(TOOLBAR_BORDER))
                .bg(rgba(TOOLBAR_BG))
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgba(TOOLBAR_TEXT))
                        .child("Link destination"),
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
                .when_some(self.selection_toolbar.link_error.clone(), |panel, error| {
                    panel.child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgba(TOOLBAR_ERROR))
                            .child(error),
                    )
                })
                .child(actions)
                .into_any_element(),
        )
    }
}
