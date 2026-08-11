use super::{
    provider::{ProviderMessage, ProviderRequest},
    session::{KammiMessage, KammiRole, KammiSession, MessageState, PendingInsertion},
    settings::{
        clear_openrouter_key, contains_openrouter_key, store_openrouter_key, validate_model_id,
    },
    GenerationState, KammiPanel, KammiTab, ProviderStatus, RightSidebarPage,
};
use crate::shell::{PhoenixShell, BORDER, CANVAS, SURFACE, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, FontWeight, IntoElement, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Sizable};
use uuid::Uuid;
use velotype::{AgentBlockDraft, AgentDocumentOp};

const ACCENT: u32 = 0x57e2bb;
const ACCENT_DARK: u32 = 0x0d3029;
const CARD_BG: u32 = 0x1c1f20;

impl PhoenixShell {
    pub(crate) fn render_right_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        match self.right_sidebar_page {
            RightSidebarPage::Inspector => self.render_inspector(cx).into_any_element(),
            RightSidebarPage::Analytics => self.render_analytics(cx).into_any_element(),
            RightSidebarPage::Kammi => self.render_kammi(cx).into_any_element(),
        }
    }

    pub(crate) fn render_right_sidebar_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(crate::shell::view::SHELL_HEADER_HEIGHT))
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .px_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                Button::new("right-page-inspector")
                    .label("INSPECT")
                    .small()
                    .min_w_0()
                    .flex_1()
                    .px_1()
                    .when(
                        self.right_sidebar_page != RightSidebarPage::Inspector,
                        |button| button.ghost(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_sidebar_page = RightSidebarPage::Inspector;
                        this.analytics_highlight = None;
                        this.apply_kernel_highlights(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("right-page-analytics")
                    .label("ANALYTICS")
                    .small()
                    .min_w_0()
                    .flex_1()
                    .px_1()
                    .when(
                        self.right_sidebar_page != RightSidebarPage::Analytics,
                        |button| button.ghost(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_sidebar_page = RightSidebarPage::Analytics;
                        this.right_sidebar_width = this.right_sidebar_width.max(400.);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("right-page-kammi")
                    .label("KAMMI")
                    .small()
                    .min_w_0()
                    .flex_1()
                    .px_1()
                    .when(
                        self.right_sidebar_page != RightSidebarPage::Kammi,
                        |button| button.ghost(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_sidebar_page = RightSidebarPage::Kammi;
                        this.analytics_highlight = None;
                        this.apply_kernel_highlights(cx);
                        cx.notify();
                    })),
            )
            .when(
                self.right_sidebar_page == RightSidebarPage::Kammi,
                |header| {
                    header.child(
                        div()
                            .size(px(6.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    )
                },
            )
            .when(
                self.right_sidebar_page == RightSidebarPage::Analytics,
                |header| {
                    header.child(
                        div()
                            .size(px(6.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    )
                },
            )
    }

    fn render_kammi(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.kammi.provider_status();
        let (status_text, status_color) = match &status {
            ProviderStatus::Unconfigured => ("PROVIDER OFF", TEXT_MUTED),
            ProviderStatus::Ready { .. } => ("OPENROUTER", ACCENT),
            ProviderStatus::Generating { .. } => ("GENERATING ●", ACCENT),
            ProviderStatus::Error { .. } => ("ERROR", 0xee5555),
        };

        let model_label = if self.kammi.settings.model.trim().is_empty() {
            "no model set".to_string()
        } else {
            self.kammi.settings.model.clone()
        };

        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CANVAS))
            .child(self.render_right_sidebar_header(cx))
            .child(
                div()
                    .h_10()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .px_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Button::new("kammi-new-session")
                            .label("+ NEW")
                            .small()
                            .px_1()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_kammi_session(window, cx);
                            })),
                    )
                    .child(
                        Button::new("kammi-history-btn")
                            .label("HISTORY")
                            .small()
                            .px_1()
                            .when(self.kammi.panel != KammiPanel::History, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.kammi.panel == KammiPanel::History {
                                    this.kammi.panel = KammiPanel::Chat;
                                } else {
                                    this.kammi.panel = KammiPanel::History;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(rgb(status_color))
                            .child(status_text),
                    )
                    .child(
                        Button::new("kammi-settings-btn")
                            .label("⚙")
                            .small()
                            .px_1()
                            .when(self.kammi.panel != KammiPanel::Settings, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.kammi.panel == KammiPanel::Settings {
                                    this.kammi.panel = KammiPanel::Chat;
                                } else {
                                    this.kammi.panel = KammiPanel::Settings;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(0x131516))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!("Model: {model_label}")),
                    ),
            )
            .child(match self.kammi.panel {
                KammiPanel::Settings => self.render_kammi_settings(cx).into_any_element(),
                KammiPanel::History => self.render_kammi_history(cx).into_any_element(),
                KammiPanel::Chat => self.render_kammi_chat(cx).into_any_element(),
            })
    }

    #[allow(dead_code)]
    fn render_kammi_settings_legacy(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key_status = if self.kammi.has_api_key {
            "API key: CONFIGURED (Stored in Windows Credential Manager)"
        } else {
            "API key: NOT CONFIGURED"
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p_4()
            .gap_4()
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT))
                    .child("KAMMI SETTINGS"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .child("OpenRouter API Key"),
                    )
                    .child(Input::new(&self.kammi.api_key_input).small())
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(key_status),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .mt_1()
                            .child(
                                Button::new("save-api-key")
                                    .label("UPDATE KEY")
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_kammi_api_key(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("clear-api-key")
                                    .label("CLEAR KEY")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.clear_kammi_api_key(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .child("Model ID"),
                    )
                    .child(Input::new(&self.kammi.model_input).small())
                    .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(
                        "OpenRouter model ID. Example: openai/gpt-4o, anthropic/claude-3.5-sonnet",
                    ))
                    .child(
                        Button::new("save-model-id")
                            .label("SAVE MODEL")
                            .small()
                            .mt_1()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_kammi_model(window, cx);
                            })),
                    ),
            )
            .when_some(self.kammi.error_banner.as_ref(), |this, err| {
                this.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x3a1818))
                        .text_xs()
                        .text_color(rgb(0xee8888))
                        .child(err.clone()),
                )
            })
            .child(
                Button::new("back-to-chat")
                    .label("← RETURN TO CHAT")
                    .small()
                    .ghost()
                    .mt_auto()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.kammi.panel = KammiPanel::Chat;
                        cx.notify();
                    })),
            )
    }

    fn render_kammi_history(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let history_items = self.kammi.history.iter().cloned().collect::<Vec<_>>();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p_3()
            .gap_2()
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT))
                    .child("CONVERSATION HISTORY"),
            )
            .when(history_items.is_empty(), |this| {
                this.child(
                    div()
                        .p_4()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child("No past conversations saved."),
                )
            })
            .children(history_items.into_iter().map(|session| {
                let s_id = session.id;
                let title = session.title.clone();
                let msg_count = session.messages.len();
                let is_current = session.id == self.kammi.session.id;

                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if is_current { ACCENT } else { BORDER }))
                    .bg(rgb(CARD_BG))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!("{msg_count} messages"))
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_color(rgb(0xee8888))
                                    .hover(|this| this.bg(rgb(0x311818)))
                                    .child("DELETE")
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.delete_kammi_history_session(s_id, cx);
                                        }),
                                    ),
                            ),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.load_kammi_session(s_id, cx);
                        }),
                    )
            }))
            .child(
                Button::new("back-to-chat-from-history")
                    .label("← RETURN TO CHAT")
                    .small()
                    .ghost()
                    .mt_auto()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.kammi.panel = KammiPanel::Chat;
                        cx.notify();
                    })),
            )
    }

    fn render_kammi_chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let note = self
            .selected_entry()
            .map_or_else(|| "NO ACTIVE NOTE".into(), |entry| entry.name.clone());

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .mx_3()
                    .mt_2()
                    .p_1()
                    .flex()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(0x111313))
                    .child(
                        Button::new("tab-session")
                            .label("SESSION")
                            .small()
                            .flex_1()
                            .when(self.kammi.tab != KammiTab::Session, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.kammi.tab = KammiTab::Session;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-context")
                            .label("CONTEXT")
                            .small()
                            .flex_1()
                            .when(self.kammi.tab != KammiTab::Context, |b| b.ghost())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.kammi.tab = KammiTab::Context;
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.kammi.error_banner.as_ref(), |chat, error| {
                chat.child(
                    div()
                        .mx_3()
                        .mt_2()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x703333))
                        .bg(rgb(0x311818))
                        .text_xs()
                        .text_color(rgb(0xee8888))
                        .child(error.clone()),
                )
            })
            .child(match self.kammi.tab {
                KammiTab::Context => self.render_kammi_context_tab(note, cx).into_any_element(),
                KammiTab::Session => self.render_kammi_session_tab(note, cx).into_any_element(),
            })
            .child(
                div()
                    .flex_shrink_0()
                    .p_3()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(Input::new(&self.kammi.composer).small())
                    .child(div().flex().gap_2().mt_2().child(
                        if matches!(self.kammi.generation, GenerationState::Streaming { .. }) {
                            Button::new("kammi-stop")
                                .label("STOP")
                                .small()
                                .w_full()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.stop_kammi_generation(cx);
                                }))
                        } else {
                            Button::new("kammi-send")
                                .label("SEND")
                                .small()
                                .w_full()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_kammi(window, cx);
                                }))
                        },
                    )),
            )
    }

    fn render_kammi_context_tab(&self, note: String, cx: &mut Context<Self>) -> impl IntoElement {
        let anchor_info = self.editor.read(cx).current_agent_anchor(cx);
        let (anchor_desc, rev_desc) = match anchor_info {
            Ok(anchor) => (
                format!("Block ID: {}", anchor.block_id),
                format!("Revision: {}", anchor.editor_revision),
            ),
            Err(_) => (
                "No active block caret".to_string(),
                "Revision: -".to_string(),
            ),
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p_4()
            .gap_3()
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(ACCENT))
                    .child("SESSION CONTEXT"),
            )
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Active Note"),
                    )
                    .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(note)),
            )
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Document Insertion Anchor"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(anchor_desc),
                    )
                    .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(rev_desc)),
            )
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Context Inclusion Flags"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("[ ] Active Note text"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("[ ] Graph neighborhood"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("[ ] Persistent Memory"),
                    ),
            )
    }

    fn render_kammi_session_tab(&self, note: String, cx: &mut Context<Self>) -> impl IntoElement {
        if self.kammi.session.messages.is_empty() {
            return div()
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
                        .child(
                            "The sidebar owns the conversation. The editor remains the document.",
                        ),
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
                )
                .into_any_element();
        }

        let messages = self.kammi.session.messages.clone();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p_3()
            .gap_3()
            .overflow_y_scrollbar()
            .children(
                messages
                    .into_iter()
                    .map(|msg| self.render_kammi_message(msg, cx)),
            )
            .into_any_element()
    }

    fn render_kammi_message(
        &self,
        message: KammiMessage,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let msg_id = message.id;
        let is_user = message.role == KammiRole::User;

        if is_user {
            div()
                .p_3()
                .rounded_lg()
                .bg(rgb(CARD_BG))
                .border_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_MUTED))
                        .child("YOU"),
                )
                .child(
                    div()
                        .mt_1()
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .child(message.content),
                )
        } else {
            let model_tag = message
                .model
                .unwrap_or_else(|| self.kammi.settings.model.clone());
            let is_streaming = message.state == MessageState::Streaming;
            let is_failed =
                message.state == MessageState::Failed || message.state == MessageState::Interrupted;

            div()
                .p_3()
                .rounded_lg()
                .bg(rgb(0x151a19))
                .border_1()
                .border_color(rgb(0x23443e))
                .child(
                    div().flex().items_center().gap_2().child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(ACCENT))
                            .child(format!("KAMMI · {model_tag}")),
                    ),
                )
                .child(div().mt_1().text_sm().text_color(rgb(TEXT)).child(format!(
                    "{}{}",
                    message.content,
                    if is_streaming { " ▌" } else { "" }
                )))
                .when(is_failed, |card| {
                    card.child(
                        div()
                            .mt_2()
                            .text_xs()
                            .text_color(rgb(0xee7777))
                            .child("⚠ Stream interrupted"),
                    )
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .mt_2()
                        .child(
                            Button::new(("copy", msg_id as usize))
                                .label("COPY")
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy_message_content(msg_id, cx);
                                })),
                        )
                        .child(
                            Button::new(("insert", msg_id as usize))
                                .label("INSERT")
                                .small()
                                .ghost()
                                .disabled(is_streaming)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.insert_message_into_editor(msg_id, cx);
                                })),
                        )
                        .child(
                            Button::new(("retry", msg_id as usize))
                                .label("RETRY")
                                .small()
                                .ghost()
                                .disabled(is_streaming)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.retry_kammi_generation(window, cx);
                                })),
                        ),
                )
        }
    }

    fn start_new_kammi_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(request_id) = self.kammi.active_request_id() {
            if let Err(error) = self.kammi.provider.try_cancel(request_id) {
                self.kammi.error_banner = Some(format!(
                    "Could not stop the active OpenRouter request: {error}"
                ));
                cx.notify();
                return;
            }
            self.kammi.cancelled(request_id);
        }
        self.kammi.archive_current_session();
        let Some(new_id) = self.kammi.take_next_identity() else {
            self.kammi.error_banner = Some("Kammi identity space is exhausted".to_string());
            cx.notify();
            return;
        };
        self.kammi.session = KammiSession::new(new_id);
        self.kammi.generation = GenerationState::Idle;
        self.kammi.pending_insertion = None;
        self.kammi.error_banner = None;
        self.kammi
            .composer
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.save_kammi_history();
        cx.notify();
    }

    fn load_kammi_session(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(request_id) = self.kammi.active_request_id() {
            if let Err(error) = self.kammi.provider.try_cancel(request_id) {
                self.kammi.error_banner = Some(format!(
                    "Could not stop the active OpenRouter request: {error}"
                ));
                cx.notify();
                return;
            }
            self.kammi.cancelled(request_id);
        }
        if let Some(position) = self
            .kammi
            .history
            .iter()
            .position(|session| session.id == session_id)
        {
            let Some(session) = self.kammi.history.remove(position) else {
                return;
            };
            if !self.kammi.session.messages.is_empty() && self.kammi.session.id != session_id {
                self.kammi.archive_current_session();
            }
            self.kammi.session = session;
            self.kammi.panel = KammiPanel::Chat;
            self.save_kammi_history();
            cx.notify();
        }
    }

    fn delete_kammi_history_session(&mut self, session_id: u64, cx: &mut Context<Self>) {
        self.kammi
            .history
            .retain(|session| session.id != session_id);
        self.save_kammi_history();
        self.status = "KAMMI / CONVERSATION DELETED".into();
        cx.notify();
    }

    #[allow(dead_code)]
    fn save_kammi_api_key_legacy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key_text = self.kammi.api_key_input.read(cx).value().trim().to_string();
        if key_text.is_empty() {
            self.kammi.error_banner = Some("API key cannot be empty".to_string());
            cx.notify();
            return;
        }
        match store_openrouter_key(&key_text) {
            Ok(()) => {
                self.kammi.has_api_key = true;
                self.kammi.error_banner = None;
                self.kammi
                    .api_key_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.status = "KAMMI / OPENROUTER API KEY STORED".into();
            }
            Err(err) => {
                self.kammi.error_banner = Some(format!("Failed to store API key: {err}"));
            }
        }
        cx.notify();
    }

    #[allow(dead_code)]
    fn clear_kammi_api_key_legacy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match clear_openrouter_key() {
            Ok(()) => {
                self.kammi.has_api_key = false;
                self.kammi
                    .api_key_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.status = "KAMMI / API KEY CLEARED".into();
            }
            Err(err) => {
                self.kammi.error_banner = Some(format!("Failed to clear API key: {err}"));
            }
        }
        cx.notify();
    }

    #[allow(dead_code)]
    fn save_kammi_model_legacy(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let model_text = self.kammi.model_input.read(cx).value().trim().to_string();
        match validate_model_id(&model_text) {
            Ok(validated) => {
                self.kammi.settings.model = validated;
                self.kammi.error_banner = None;
                self.save_kammi_history();
                self.status = format!("KAMMI / MODEL SET TO {}", self.kammi.settings.model).into();
            }
            Err(err) => {
                self.kammi.error_banner = Some(format!("Invalid model ID: {err}"));
            }
        }
        cx.notify();
    }

    fn send_kammi(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kammi.active_request_id().is_some() {
            return;
        }

        if !self.kammi.has_api_key {
            self.kammi.panel = KammiPanel::Settings;
            self.kammi.error_banner =
                Some("Please configure an OpenRouter API key first.".to_string());
            cx.notify();
            return;
        }

        if self.kammi.settings.model.trim().is_empty() {
            self.kammi.panel = KammiPanel::Settings;
            self.kammi.error_banner =
                Some("Please set a model ID in Settings (e.g. openai/gpt-4o).".to_string());
            cx.notify();
            return;
        }

        let prompt = self.kammi.composer.read(cx).value().trim().to_string();
        if prompt.is_empty() {
            return;
        }
        if contains_openrouter_key(&prompt) {
            self.kammi
                .composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.kammi.panel = KammiPanel::Settings;
            self.kammi.error_banner = Some(
                "A credential-looking value was blocked from chat history. Paste API keys only into the protected Connection field."
                    .to_string(),
            );
            cx.notify();
            return;
        }

        self.kammi
            .composer
            .update(cx, |input, cx| input.set_value("", window, cx));

        let Some(user_id) = self.kammi.take_next_identity() else {
            self.kammi.error_banner = Some("Kammi identity space is exhausted".to_string());
            cx.notify();
            return;
        };
        self.kammi.session.messages.push(KammiMessage {
            id: user_id,
            role: KammiRole::User,
            content: prompt.clone(),
            model: None,
            state: MessageState::Complete,
        });

        let Some(assistant_req_id) = self.kammi.take_next_identity() else {
            self.kammi.error_banner = Some("Kammi identity space is exhausted".to_string());
            cx.notify();
            return;
        };
        self.kammi.session.messages.push(KammiMessage {
            id: assistant_req_id,
            role: KammiRole::Assistant,
            content: String::new(),
            model: Some(self.kammi.settings.model.clone()),
            state: MessageState::Streaming,
        });

        self.kammi.session.update_title_from_first_message();

        if let Ok(anchor) = self.editor.read(cx).current_agent_anchor(cx) {
            let digest = super::session::digest_messages(&self.kammi.session.messages);
            self.kammi.pending_insertion = Some(PendingInsertion {
                request_id: assistant_req_id,
                anchor,
                context_digest: digest,
            });
        }

        let mut provider_msgs = Vec::with_capacity(
            self.kammi.session.messages.len()
                + usize::from(!self.kammi.settings.system_prompt.is_empty()),
        );
        if !self.kammi.settings.system_prompt.is_empty() {
            provider_msgs.push(ProviderMessage {
                role: KammiRole::System,
                content: self.kammi.settings.system_prompt.clone(),
            });
        }
        provider_msgs.extend(
            self.kammi
                .session
                .messages
                .iter()
                .filter(|message| message.id != assistant_req_id)
                .map(|message| ProviderMessage {
                    role: message.role,
                    content: message.content.clone(),
                }),
        );

        let req = ProviderRequest {
            request_id: assistant_req_id,
            model: self.kammi.settings.model.clone(),
            messages: provider_msgs,
            reasoning: self.kammi.settings.reasoning,
        };

        self.kammi.generation = GenerationState::Streaming {
            request_id: assistant_req_id,
        };
        if let Err(error) = self.kammi.provider.try_generate(req) {
            self.kammi.fail(assistant_req_id, error.to_string());
            self.save_kammi_history();
        }

        cx.notify();
    }

    fn stop_kammi_generation(&mut self, cx: &mut Context<Self>) {
        if let Some(req_id) = self.kammi.active_request_id() {
            match self.kammi.provider.try_cancel(req_id) {
                Ok(()) => {
                    self.kammi.cancelled(req_id);
                    self.save_kammi_history();
                }
                Err(error) => {
                    self.kammi.error_banner =
                        Some(format!("Could not stop the OpenRouter request: {error}"));
                }
            }
            cx.notify();
        }
    }

    fn retry_kammi_generation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kammi.active_request_id().is_some() {
            return;
        }
        if let Some(last) = self.kammi.session.messages.last() {
            if last.role == KammiRole::Assistant {
                self.kammi.session.messages.pop();
            }
        }
        if let Some(last_user) = self.kammi.session.messages.last().cloned() {
            if last_user.role == KammiRole::User {
                self.kammi.session.messages.pop();
                self.kammi.composer.update(cx, |input, cx| {
                    input.set_value(&last_user.content, window, cx);
                });
                self.send_kammi(window, cx);
            }
        }
    }

    fn copy_message_content(&self, msg_id: u64, cx: &mut Context<Self>) {
        if let Some(msg) = self.kammi.session.messages.iter().find(|m| m.id == msg_id) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(msg.content.clone()));
        }
    }

    fn insert_message_into_editor(&mut self, msg_id: u64, cx: &mut Context<Self>) {
        let Some(msg) = self
            .kammi
            .session
            .messages
            .iter()
            .find(|m| m.id == msg_id)
            .cloned()
        else {
            return;
        };

        let pending = self.kammi.pending_insertion.clone();
        let model = msg
            .model
            .clone()
            .unwrap_or_else(|| self.kammi.settings.model.clone());

        let result = if let Some(pending) = pending {
            self.editor.update(cx, |editor, cx| {
                editor.execute_agent_document_op(
                    AgentDocumentOp::InsertAfter {
                        anchor: pending.anchor,
                        invocation_id: Uuid::new_v4(),
                        turn_id: Uuid::new_v4(),
                        model: model.into(),
                        context_digest: pending.context_digest,
                        blocks: vec![AgentBlockDraft::paragraph(msg.content.clone())],
                    },
                    cx,
                )
            })
        } else {
            let anchor = match self.editor.read(cx).current_agent_anchor(cx) {
                Ok(a) => a,
                Err(err) => {
                    self.status = format!("KAMMI INSERT BLOCKED / {err:?}").into();
                    cx.notify();
                    return;
                }
            };
            let digest = super::session::digest_messages(&self.kammi.session.messages);
            self.editor.update(cx, |editor, cx| {
                editor.execute_agent_document_op(
                    AgentDocumentOp::InsertAfter {
                        anchor,
                        invocation_id: Uuid::new_v4(),
                        turn_id: Uuid::new_v4(),
                        model: model.into(),
                        context_digest: digest,
                        blocks: vec![AgentBlockDraft::paragraph(msg.content.clone())],
                    },
                    cx,
                )
            })
        };

        match result {
            Ok(_) => {
                self.status = "KAMMI / RESPONSE INSERTED".into();
            }
            Err(error) => {
                self.status = format!("KAMMI INSERT BLOCKED / {error:?}").into();
            }
        }
        cx.notify();
    }
}
