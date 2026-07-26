use super::{PhoenixShell, BORDER, BORDER_BRIGHT, SURFACE, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Sizable};

const ACCENT: u32 = 0x57e2bb;
const FOOTER_BG: u32 = 0x171918;

impl PhoenixShell {
    pub(super) fn render_left_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_8()
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .border_t_1()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(FOOTER_BG))
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .child(div().text_color(rgb(ACCENT)).child("LOCAL / NATIVE"))
            .child(
                div().ml_auto().child(
                    Button::new("footer-toggle-files")
                        .label("FILES")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.left_open = false;
                            cx.notify();
                        })),
                ),
            )
    }

    pub(super) fn render_center_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_count = self.entity_count();
        let graph_available = self.graph.borrow().is_some()
            || self.scene_error.is_some()
            || self.graph_init_error.is_some();
        let alert = actionable_status(&self.status);
        div()
            .h_8()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .child(
                Button::new("footer-entity-pill")
                    .label(format!("ATLAS  ·  {entity_count} ENTITIES"))
                    .small()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(rgb(if self.drawer_layout.is_open() {
                        ACCENT
                    } else {
                        BORDER_BRIGHT
                    }))
                    .bg(rgb(if self.drawer_layout.is_open() {
                        0x173b32
                    } else {
                        0x202423
                    }))
                    .text_color(rgb(ACCENT))
                    .disabled(!graph_available)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer(cx);
                    })),
            )
            .when_some(alert, |footer, alert| {
                footer.child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_color(rgb(0xe6a06f))
                        .child(alert),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!self.left_open, |controls| {
                        controls.child(
                            Button::new("footer-show-files")
                                .label("FILES")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.left_open = true;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!self.right_open, |controls| {
                        controls.child(
                            Button::new("footer-show-inspector")
                                .label("INSPECTOR")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.right_open = true;
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }

    pub(super) fn render_right_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_8()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .px_3()
            .border_t_1()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(FOOTER_BG))
            .child(
                Button::new("footer-toggle-inspector")
                    .label("INSPECTOR")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_open = false;
                        cx.notify();
                    })),
            )
    }
}

fn actionable_status(status: &SharedString) -> Option<SharedString> {
    let status_text: &str = status.as_ref();
    ["BLOCKED", "SAVE BLOCKED", "GRAPH BLOCKED", "CONFIRM"]
        .into_iter()
        .any(|prefix| status_text.starts_with(prefix))
        .then(|| status.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_status_does_not_pollute_the_footer() {
        assert!(actionable_status(&"READY / SHARED NATIVE KERNEL ONLINE".into()).is_none());
        assert!(actionable_status(&"ATLAS / DRAWER OPEN / RESIDENT GRAPH READY".into()).is_none());
    }

    #[test]
    fn blocking_and_confirmation_status_remain_visible() {
        assert!(actionable_status(&"BLOCKED / SAVE FIRST".into()).is_some());
        assert!(actionable_status(&"SAVE BLOCKED / STALE LEASE".into()).is_some());
        assert!(actionable_status(&"GRAPH BLOCKED / SURFACE LOST".into()).is_some());
        assert!(actionable_status(&"CONFIRM / DELETE BRANCH".into()).is_some());
    }
}
