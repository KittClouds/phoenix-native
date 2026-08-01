use super::{PhoenixShell, BORDER, CANVAS, SURFACE, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, FontWeight, IntoElement, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{Disableable, Sizable};
use serde::{Deserialize, Serialize};

const ACCENT: u32 = 0x57e2bb;
const ACCENT_DARK: u32 = 0x0d3029;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RightSidebarPage {
    #[default]
    Inspector,
    Kammi,
}

impl RightSidebarPage {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Inspector => "INSPECTOR",
            Self::Kammi => "KAMMI",
        }
    }
}

impl PhoenixShell {
    pub(super) fn render_right_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        match self.right_sidebar_page {
            RightSidebarPage::Inspector => self.render_inspector(cx).into_any_element(),
            RightSidebarPage::Kammi => self.render_kammi(cx).into_any_element(),
        }
    }

    pub(super) fn render_right_sidebar_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(super::view::SHELL_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                Button::new("right-page-inspector")
                    .label("INSPECTOR")
                    .small()
                    .when(
                        self.right_sidebar_page != RightSidebarPage::Inspector,
                        |button| button.ghost(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_sidebar_page = RightSidebarPage::Inspector;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("right-page-kammi")
                    .label("KAMMI")
                    .small()
                    .when(
                        self.right_sidebar_page != RightSidebarPage::Kammi,
                        |button| button.ghost(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_sidebar_page = RightSidebarPage::Kammi;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml_auto()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(ACCENT_DARK))
                    .text_xs()
                    .text_color(rgb(ACCENT))
                    .child("SIM"),
            )
    }

    fn render_kammi(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let note = self
            .selected_entry()
            .map_or_else(|| "NO ACTIVE NOTE".into(), |entry| entry.name.clone());
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CANVAS))
            .child(self.render_right_sidebar_header(cx))
            .child(
                div()
                    .h_10()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Button::new("kammi-new-invocation")
                            .label("+ INVOKE")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.invoke_kammi_simulator(window, cx);
                            })),
                    )
                    .child(
                        Button::new("kammi-history")
                            .label("HISTORY")
                            .small()
                            .ghost()
                            .disabled(true),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("PROVIDER OFF"),
                    ),
            )
            .child(
                div()
                    .mx_3()
                    .mt_3()
                    .p_1()
                    .flex()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(0x111313))
                    .child(
                        div()
                            .flex_1()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(ACCENT_DARK))
                            .text_center()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child("SESSION"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .py_2()
                            .text_center()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("CONTEXT"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px_6()
                    .text_center()
                    .child(
                        div()
                            .size(px(64.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(20.))
                            .border_1()
                            .border_color(rgb(0x185347))
                            .bg(rgb(ACCENT_DARK))
                            .text_3xl()
                            .text_color(rgb(ACCENT))
                            .child("✦"),
                    )
                    .child(
                        div()
                            .mt_4()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Kammi"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .max_w(px(280.))
                            .text_sm()
                            .text_color(rgb(TEXT_MUTED))
                            .child("The note is the conversation. Simulation inserts a provisional agent block at the current caret through the production editor contract."),
                    )
                    .child(
                        div()
                            .mt_4()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x202423))
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(note),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .p_3()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(Input::new(&self.kammi_prompt).small())
                    .child(
                        Button::new("kammi-insert-simulation")
                            .label("INSERT PROVISIONAL BLOCK")
                            .mt_2()
                            .w_full()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.invoke_kammi_simulator(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("No network, model, or hidden context is active."),
                    ),
            )
    }

    fn invoke_kammi_simulator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.kammi_prompt.read(cx).value().trim().to_string();
        let text = if prompt.is_empty() {
            "This is a simulated Kammi response. It entered through the same typed editor boundary a future provider will use."
                .to_string()
        } else {
            prompt
        };
        let result = self.editor.update(cx, |editor, cx| {
            editor.simulate_agent_response_at_caret(text, cx)
        });
        match result {
            Ok(_) => {
                self.kammi_prompt
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.status = "KAMMI / PROVISIONAL BLOCK INSERTED".into();
            }
            Err(error) => {
                self.status = format!("KAMMI BLOCKED / {error:?}").into();
            }
        }
        cx.notify();
    }
}
