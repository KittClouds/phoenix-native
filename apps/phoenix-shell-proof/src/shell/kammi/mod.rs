pub mod provider;
pub mod session;
pub mod settings;
mod settings_ui;
pub mod store;
pub mod ui;

use gpui::{AppContext as _, Entity, ScrollHandle, Task, Window};
use gpui_component::input::InputState;
use provider::{spawn_provider_runtime, KammiProviderRuntime};
use session::{KammiSession, MessageState, PendingInsertion};
use settings::{load_openrouter_key, KammiSettings};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RightSidebarPage {
    #[default]
    Inspector,
    Analytics,
    Kammi,
}

impl RightSidebarPage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inspector => "INSPECTOR",
            Self::Analytics => "ANALYTICS",
            Self::Kammi => "KAMMI",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KammiTab {
    #[default]
    Session,
    Context,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KammiPanel {
    #[default]
    Chat,
    History,
    Settings,
}

#[derive(Clone, Debug, Default)]
pub enum GenerationState {
    #[default]
    Idle,
    Streaming {
        request_id: u64,
    },
    Failed {
        message: String,
    },
}

#[allow(dead_code)]
pub enum ProviderStatus {
    Unconfigured,
    Ready { model: String },
    Generating { model: String },
    Error { message: String },
}

pub struct KammiState {
    pub tab: KammiTab,
    pub panel: KammiPanel,
    pub composer: Entity<InputState>,
    pub model_input: Entity<InputState>,
    pub api_key_input: Entity<InputState>,
    pub system_prompt_input: Entity<InputState>,
    pub session: KammiSession,
    pub history: VecDeque<KammiSession>,
    pub provider: KammiProviderRuntime,
    pub provider_task: Option<Task<()>>,
    pub generation: GenerationState,
    pub next_request_id: u64,
    pub has_api_key: bool,
    pub settings: KammiSettings,
    #[allow(dead_code)]
    pub scroll: ScrollHandle,
    pub pending_insertion: Option<PendingInsertion>,
    pub error_banner: Option<String>,
}

impl KammiState {
    pub fn new(
        window: &mut Window,
        cx: &mut gpui::Context<crate::shell::PhoenixShell>,
    ) -> anyhow::Result<Self> {
        let composer = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 8)
                .placeholder("Message Kammi...")
        });
        let model_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("provider/model (e.g. openai/gpt-4o)")
        });
        let api_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("OpenRouter API key")
        });
        let system_prompt_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(4, 12)
                .placeholder("Define Kammi's role, voice, boundaries, and working style...")
        });

        let provider = spawn_provider_runtime()?;
        let has_api_key = load_openrouter_key().ok().flatten().is_some();
        let scroll = ScrollHandle::new();

        Ok(Self {
            tab: KammiTab::Session,
            panel: KammiPanel::Chat,
            composer,
            model_input,
            api_key_input,
            system_prompt_input,
            session: KammiSession::new(1),
            history: VecDeque::new(),
            provider,
            provider_task: None,
            generation: GenerationState::Idle,
            next_request_id: 2,
            has_api_key,
            settings: KammiSettings::default(),
            scroll,
            pending_insertion: None,
            error_banner: None,
        })
    }

    pub fn provider_status(&self) -> ProviderStatus {
        if matches!(self.generation, GenerationState::Streaming { .. }) {
            return ProviderStatus::Generating {
                model: self.settings.model.clone(),
            };
        }
        if let GenerationState::Failed { message } = &self.generation {
            return ProviderStatus::Error {
                message: message.clone(),
            };
        }
        if self.has_api_key && !self.settings.model.trim().is_empty() {
            ProviderStatus::Ready {
                model: self.settings.model.clone(),
            }
        } else {
            ProviderStatus::Unconfigured
        }
    }

    pub fn active_request_id(&self) -> Option<u64> {
        match self.generation {
            GenerationState::Streaming { request_id } => Some(request_id),
            _ => None,
        }
    }

    pub fn take_next_identity(&mut self) -> Option<u64> {
        let identity = self.next_request_id;
        self.next_request_id = identity.checked_add(1)?;
        Some(identity)
    }

    pub fn archive_current_session(&mut self) {
        if self.session.messages.is_empty() {
            return;
        }
        self.history.retain(|session| session.id != self.session.id);
        self.history.push_front(self.session.clone());
        self.history.truncate(store::MAX_SESSIONS);
    }

    pub fn finish(&mut self, request_id: u64) {
        if self.active_request_id() == Some(request_id) {
            self.generation = GenerationState::Idle;
            if let Some(msg) = self.session.streaming_assistant_mut(request_id) {
                msg.state = MessageState::Complete;
            }
            self.session.update_title_from_first_message();
        }
    }

    pub fn fail(&mut self, request_id: u64, error: String) {
        if self.active_request_id() == Some(request_id) {
            self.generation = GenerationState::Failed {
                message: error.clone(),
            };
            self.error_banner = Some(error);
            if let Some(msg) = self.session.streaming_assistant_mut(request_id) {
                msg.state = MessageState::Failed;
            }
        }
    }

    pub fn cancelled(&mut self, request_id: u64) {
        if self.active_request_id() == Some(request_id) {
            self.generation = GenerationState::Idle;
            if let Some(msg) = self.session.streaming_assistant_mut(request_id) {
                msg.state = MessageState::Interrupted;
            }
        }
    }
}
