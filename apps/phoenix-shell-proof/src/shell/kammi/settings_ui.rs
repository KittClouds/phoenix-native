use super::{
    provider::validate_llama_cpp_settings,
    settings::{
        clear_openrouter_key, store_openrouter_key, LlamaPerformanceProfile, ProviderBackend,
        ReasoningLevel, MAX_SAVED_MODELS,
    },
    KammiPanel,
};
use crate::shell::{PhoenixShell, BORDER, SURFACE, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, FontWeight, IntoElement, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::scroll::ScrollableElement;
use gpui_component::Sizable;

const ACCENT: u32 = 0x57e2bb;
const ACCENT_DARK: u32 = 0x0d3029;
const CARD_BG: u32 = 0x191c1d;
const DANGER: u32 = 0xee7777;

impl PhoenixShell {
    pub(super) fn render_kammi_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key_status = if self.kammi.has_api_key {
            ("READY", ACCENT, "Protected by Windows Credential Manager")
        } else {
            (
                "KEY REQUIRED",
                0xe6a45e,
                "Add an OpenRouter key to enable chat",
            )
        };
        let backend = self.kammi.settings.backend;
        let active_model_label = self.kammi.settings.active_model_label();
        let active_model = if active_model_label.is_empty() {
            "No model selected".to_string()
        } else {
            active_model_label
        };
        let saved_models = self.kammi.settings.saved_models.clone();
        let active_reasoning = self.kammi.settings.reasoning;
        let local_performance = self.kammi.settings.llama_cpp.performance;
        let prompt_bytes = self.kammi.settings.system_prompt.len();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scrollbar()
            .bg(rgb(0x101212))
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("KAMMI CONFIGURATION"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(rgb(ACCENT_DARK))
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(ACCENT))
                                    .child(backend.label()),
                            ),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("Connection, model library, reasoning, and persistent instructions."),
                    ),
            )
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("INFERENCE PROVIDER"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Choose remote OpenRouter or a process-isolated local llama.cpp GGUF runtime."),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("kammi-provider-openrouter")
                                            .label("OPENROUTER")
                                            .small()
                                            .when(backend != ProviderBackend::OpenRouter, |button| button.ghost())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.select_kammi_provider(ProviderBackend::OpenRouter, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("kammi-provider-llama-cpp")
                                            .label("LLAMA.CPP / GGUF")
                                            .small()
                                            .when(backend != ProviderBackend::LlamaCpp, |button| button.ghost())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.select_kammi_provider(ProviderBackend::LlamaCpp, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .bg(rgb(key_status.1)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child("OPENROUTER CONNECTION"),
                                    )
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(key_status.1))
                                            .child(key_status.0),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .mb_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(key_status.2),
                            )
                            .child(Input::new(&self.kammi.api_key_input).small())
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("kammi-store-api-key")
                                            .label(if self.kammi.has_api_key {
                                                "REPLACE KEY"
                                            } else {
                                                "STORE KEY"
                                            })
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_kammi_api_key(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("kammi-clear-api-key")
                                            .label("CLEAR")
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
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(if backend == ProviderBackend::LlamaCpp {
                                0x255a4d
                            } else {
                                BORDER
                            }))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("LOCAL LLAMA.CPP"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .mb_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Phoenix launches llama-server out of process, keeps it warm, and streams its OpenAI-compatible response. Paths must be absolute."),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("LLAMA-SERVER EXECUTABLE"),
                            )
                            .child(Input::new(&self.kammi.llama_server_input).small())
                            .child(
                                div()
                                    .mt_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("GGUF MODEL"),
                            )
                            .child(Input::new(&self.kammi.llama_model_input).small())
                            .child(
                                div()
                                    .mt_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("LOCAL API ENDPOINT"),
                            )
                            .child(Input::new(&self.kammi.llama_endpoint_input).small())
                            .child(
                                div()
                                    .mt_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(format!(
                                        "Context {} · GPU layers {} · threads {}",
                                        self.kammi.settings.llama_cpp.context_size,
                                        self.kammi.settings.llama_cpp.gpu_layers,
                                        if self.kammi.settings.llama_cpp.threads == 0 {
                                            "auto"
                                        } else {
                                            "configured"
                                        }
                                    )),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("LOCAL PERFORMANCE PROFILE"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(local_performance.description()),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("kammi-llama-performance-auto")
                                            .label("AUTO")
                                            .small()
                                            .when(
                                                local_performance
                                                    != LlamaPerformanceProfile::Auto,
                                                |button| button.ghost(),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_kammi_llama_performance(
                                                    LlamaPerformanceProfile::Auto,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("kammi-llama-performance-single-slot")
                                            .label("SINGLE SLOT / LOW LATENCY")
                                            .small()
                                            .when(
                                                local_performance
                                                    != LlamaPerformanceProfile::SingleSlot,
                                                |button| button.ghost(),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_kammi_llama_performance(
                                                    LlamaPerformanceProfile::SingleSlot,
                                                    cx,
                                                );
                                            })),
                                    ),
                            )
                            .child(
                                Button::new("kammi-save-llama-cpp")
                                    .label("VALIDATE & USE GGUF")
                                    .small()
                                    .mt_2()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_kammi_llama_cpp(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("OPENROUTER MODEL LIBRARY"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Paste an exact OpenRouter provider/model ID. Saved models stay available here."),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(0x111414))
                                    .border_1()
                                    .border_color(rgb(0x255a4d))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(TEXT_MUTED))
                                            .child("ACTIVE MODEL"),
                                    )
                                    .child(
                                        div()
                                            .mt_0p5()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(ACCENT))
                                            .child(active_model),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .child(Input::new(&self.kammi.model_input).small())
                                    .child(
                                        Button::new("kammi-add-model")
                                            .label("ADD & USE MODEL")
                                            .small()
                                            .mt_2()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_kammi_model(window, cx);
                                            })),
                                    ),
                            )
                            .when(saved_models.is_empty(), |card| {
                                card.child(
                                    div()
                                        .mt_3()
                                        .p_3()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .text_xs()
                                        .text_color(rgb(TEXT_MUTED))
                                        .child("No saved models yet."),
                                )
                            })
                            .children(saved_models.into_iter().enumerate().map(|(index, model)| {
                                let is_active = model == self.kammi.settings.model;
                                let select_model = model.clone();
                                let remove_model = model.clone();
                                div()
                                    .mt_2()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new(("kammi-select-model", index))
                                            .label(model)
                                            .small()
                                            .min_w_0()
                                            .flex_1()
                                            .when(!is_active, |button| button.ghost())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.select_kammi_model(&select_model, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new(("kammi-remove-model", index))
                                            .label("REMOVE")
                                            .small()
                                            .ghost()
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.remove_kammi_model(&remove_model, window, cx);
                                            })),
                                    )
                            }))
                            .child(
                                div()
                                    .mt_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(format!("Up to {MAX_SAVED_MODELS} models are retained per workspace.")),
                            ),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child("REASONING EFFORT"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(active_reasoning.description()),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .flex_wrap()
                                    .gap_1()
                                    .children(ReasoningLevel::ALL.into_iter().enumerate().map(
                                        |(index, level)| {
                                            Button::new(("kammi-reasoning", index))
                                                .label(level.label())
                                                .small()
                                                .when(level != active_reasoning, |button| button.ghost())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.set_kammi_reasoning(level, cx);
                                                }))
                                        },
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child("SYSTEM PROMPT"),
                                    )
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_xs()
                                            .text_color(rgb(TEXT_MUTED))
                                            .child(format!("{prompt_bytes} bytes")),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .mb_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Persistent instructions are sent first on every provider request."),
                            )
                            .child(Input::new(&self.kammi.system_prompt_input).small())
                            .child(
                                Button::new("kammi-save-system-prompt")
                                    .label("SAVE INSTRUCTIONS")
                                    .small()
                                    .mt_2()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_kammi_system_prompt(cx);
                                    })),
                            ),
                    )
                    .when_some(self.kammi.error_banner.as_ref(), |content, error| {
                        content.child(
                            div()
                                .p_3()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(0x703333))
                                .bg(rgb(0x311818))
                                .text_xs()
                                .text_color(rgb(DANGER))
                                .child(error.clone()),
                        )
                    })
                    .child(
                        Button::new("kammi-back-to-chat")
                            .label("RETURN TO CHAT")
                            .small()
                            .ghost()
                            .mt_1()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.kammi.panel = KammiPanel::Chat;
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(super) fn save_kammi_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key_text = self.kammi.api_key_input.read(cx).value().trim().to_string();
        if key_text.is_empty() {
            self.kammi.error_banner = Some("Paste an OpenRouter API key first.".to_string());
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
                self.status = "KAMMI / OPENROUTER KEY PROTECTED".into();
            }
            Err(error) => {
                self.kammi.error_banner = Some(format!("Could not store the API key: {error}"));
            }
        }
        cx.notify();
    }

    fn select_kammi_provider(&mut self, backend: ProviderBackend, cx: &mut Context<Self>) {
        self.kammi.settings.backend = backend;
        self.kammi.error_banner = None;
        self.save_kammi_history();
        self.status = format!("KAMMI / {} SELECTED", backend.label()).into();
        cx.notify();
    }

    fn save_kammi_llama_cpp(&mut self, cx: &mut Context<Self>) {
        let mut settings = self.kammi.settings.llama_cpp.clone();
        settings.server_path = self
            .kammi
            .llama_server_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        settings.model_path = self
            .kammi
            .llama_model_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        settings.endpoint = self
            .kammi
            .llama_endpoint_input
            .read(cx)
            .value()
            .trim()
            .trim_end_matches('/')
            .to_owned();
        settings = settings.normalize();

        match validate_llama_cpp_settings(&settings) {
            Ok(()) => {
                self.kammi.settings.llama_cpp = settings;
                self.kammi.settings.backend = ProviderBackend::LlamaCpp;
                self.kammi.error_banner = None;
                self.save_kammi_history();
                self.status = format!(
                    "KAMMI / LOCAL GGUF {} ACTIVE",
                    self.kammi.settings.llama_cpp.model_label()
                )
                .into();
            }
            Err(error) => {
                self.kammi.error_banner = Some(format!("Invalid llama.cpp setup: {error:#}"));
            }
        }
        cx.notify();
    }

    fn set_kammi_llama_performance(
        &mut self,
        performance: LlamaPerformanceProfile,
        cx: &mut Context<Self>,
    ) {
        self.kammi.settings.llama_cpp.performance = performance;
        self.kammi.error_banner = None;
        self.save_kammi_history();
        self.status = format!(
            "KAMMI / LOCAL PERFORMANCE {} (APPLIES ON NEXT REQUEST)",
            performance.label()
        )
        .into();
        cx.notify();
    }

    pub(super) fn clear_kammi_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match clear_openrouter_key() {
            Ok(()) => {
                self.kammi.has_api_key = false;
                self.kammi.error_banner = None;
                self.kammi
                    .api_key_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.status = "KAMMI / OPENROUTER KEY CLEARED".into();
            }
            Err(error) => {
                self.kammi.error_banner = Some(format!("Could not clear the API key: {error}"));
            }
        }
        cx.notify();
    }

    pub(super) fn save_kammi_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.kammi.model_input.read(cx).value().to_string();
        match self.kammi.settings.select_or_add_model(&model) {
            Ok(()) => {
                self.kammi.settings.backend = ProviderBackend::OpenRouter;
                self.kammi.error_banner = None;
                self.kammi
                    .model_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.save_kammi_history();
                self.status = format!("KAMMI / MODEL {} ACTIVE", self.kammi.settings.model).into();
            }
            Err(error) => {
                self.kammi.error_banner = Some(format!("Invalid model ID: {error}"));
            }
        }
        cx.notify();
    }

    fn select_kammi_model(&mut self, model: &str, cx: &mut Context<Self>) {
        if self.kammi.settings.select_or_add_model(model).is_ok() {
            self.kammi.settings.backend = ProviderBackend::OpenRouter;
            self.kammi.error_banner = None;
            self.save_kammi_history();
            self.status = format!("KAMMI / MODEL {} ACTIVE", self.kammi.settings.model).into();
        }
        cx.notify();
    }

    fn remove_kammi_model(&mut self, model: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.kammi.settings.remove_model(model);
        self.kammi.model_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.save_kammi_history();
        self.status = "KAMMI / SAVED MODEL REMOVED".into();
        cx.notify();
    }

    fn set_kammi_reasoning(&mut self, level: ReasoningLevel, cx: &mut Context<Self>) {
        self.kammi.settings.reasoning = level;
        self.kammi.error_banner = None;
        self.save_kammi_history();
        self.status = format!("KAMMI / REASONING {}", level.label()).into();
        cx.notify();
    }

    fn save_kammi_system_prompt(&mut self, cx: &mut Context<Self>) {
        let prompt = self.kammi.system_prompt_input.read(cx).value().to_string();
        match self.kammi.settings.set_system_prompt(&prompt) {
            Ok(()) => {
                self.kammi.error_banner = None;
                self.save_kammi_history();
                self.status = "KAMMI / SYSTEM PROMPT SAVED".into();
            }
            Err(error) => {
                self.kammi.error_banner = Some(format!("Invalid system prompt: {error}"));
            }
        }
        cx.notify();
    }
}
