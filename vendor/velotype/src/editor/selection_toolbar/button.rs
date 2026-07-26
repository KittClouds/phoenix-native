use super::*;

impl Editor {
    pub(super) fn render_toolbar_button(
        &self,
        id: &'static str,
        label: &'static str,
        command: SelectionCommand,
        state: SelectionMarkState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let unavailable = state == SelectionMarkState::Unavailable;
        let selected = matches!(state, SelectionMarkState::On | SelectionMarkState::Mixed);
        let button = div()
            .id(id)
            .w(px(34.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .bg(if selected {
                rgba(TOOLBAR_ACTIVE_BG)
            } else {
                rgba(TOOLBAR_BG)
            })
            .text_size(px(12.0))
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(if selected {
                rgba(TOOLBAR_TEXT_ACTIVE)
            } else {
                rgba(TOOLBAR_TEXT)
            })
            .opacity(if unavailable { 0.34 } else { 1.0 })
            .when(!unavailable, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(rgba(TOOLBAR_HOVER_BG)))
                    .active(|button| button.opacity(0.86))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _, window, cx| {
                            editor.on_selection_toolbar_command(command, window, cx);
                            cx.stop_propagation();
                        }),
                    )
            })
            .child(label);
        button.into_any_element()
    }
}
