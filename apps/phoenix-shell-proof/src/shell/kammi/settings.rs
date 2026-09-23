use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "Phoenix Native";
const OPENROUTER_USER: &str = "openrouter-api-key";
pub const MAX_SAVED_MODELS: usize = 24;
pub const MAX_SYSTEM_PROMPT_BYTES: usize = 32 * 1024;
const MAX_OPENROUTER_KEY_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBackend {
    #[default]
    OpenRouter,
    LlamaCpp,
}

/// Runtime tuning for the local llama.cpp server.
///
/// `Auto` preserves llama.cpp's defaults. `SingleSlot` matches Phoenix's
/// current request model: one supervised request at a time, with one KV slot
/// and flash attention enabled. Keeping this explicit avoids changing the
/// active local setup merely by upgrading Phoenix.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlamaPerformanceProfile {
    #[default]
    SingleSlot,
    Auto,
}

impl LlamaPerformanceProfile {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::SingleSlot => "SINGLE SLOT",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Auto => "Use llama.cpp runtime defaults.",
            Self::SingleSlot => {
                "One request at a time, one KV slot, and flash attention for lower latency."
            }
        }
    }
}

impl ProviderBackend {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER",
            Self::LlamaCpp => "LLAMA.CPP",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LlamaCppSettings {
    #[serde(default)]
    pub server_path: String,
    #[serde(default)]
    pub model_path: String,
    #[serde(default = "default_llama_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_llama_context_size")]
    pub context_size: u32,
    #[serde(default = "default_llama_gpu_layers")]
    pub gpu_layers: u32,
    #[serde(default)]
    pub threads: u32,
    #[serde(default)]
    pub performance: LlamaPerformanceProfile,
}

impl Default for LlamaCppSettings {
    fn default() -> Self {
        Self {
            server_path: String::new(),
            model_path: String::new(),
            endpoint: default_llama_endpoint(),
            context_size: default_llama_context_size(),
            gpu_layers: default_llama_gpu_layers(),
            threads: 0,
            performance: LlamaPerformanceProfile::default(),
        }
    }
}

impl LlamaCppSettings {
    pub fn normalize(mut self) -> Self {
        self.server_path = self.server_path.trim().to_owned();
        self.model_path = self.model_path.trim().to_owned();
        self.endpoint = self.endpoint.trim().trim_end_matches('/').to_owned();
        if self.endpoint.is_empty() {
            self.endpoint = default_llama_endpoint();
        }
        self.context_size = self.context_size.clamp(512, 1_048_576);
        self.gpu_layers = self.gpu_layers.min(4_096);
        self.threads = self.threads.min(1_024);
        self
    }

    pub fn model_label(&self) -> String {
        std::path::Path::new(&self.model_path)
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("local-gguf")
            .to_owned()
    }
}

fn default_llama_endpoint() -> String {
    "http://127.0.0.1:8080/v1".to_owned()
}

const fn default_llama_context_size() -> u32 {
    8_192
}

const fn default_llama_gpu_layers() -> u32 {
    99
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningLevel {
    #[default]
    Auto,
    None,
    Minimal,
    Low,
    Medium,
    High,
    Max,
    Xhigh,
}

impl ReasoningLevel {
    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::None,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Max,
        Self::Xhigh,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::None => "OFF",
            Self::Minimal => "MIN",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Max => "MAX",
            Self::Xhigh => "XHIGH",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Auto => "Let the selected model choose its native reasoning behavior.",
            Self::None => "Disable reasoning for models that honor OpenRouter effort controls.",
            Self::Minimal => "Use the smallest available reasoning budget.",
            Self::Low => "Favor speed with a light reasoning pass.",
            Self::Medium => "Balance response speed and reasoning depth.",
            Self::High => "Spend more time on difficult requests.",
            Self::Max => "Request the model's maximum supported reasoning depth.",
            Self::Xhigh => "Request extra-high reasoning where the provider supports it.",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct KammiSettings {
    #[serde(default)]
    pub backend: ProviderBackend,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub saved_models: Vec<String>,
    #[serde(default)]
    pub reasoning: ReasoningLevel,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub llama_cpp: LlamaCppSettings,
}

impl Default for KammiSettings {
    fn default() -> Self {
        Self {
            backend: ProviderBackend::OpenRouter,
            model: String::new(),
            saved_models: Vec::new(),
            reasoning: ReasoningLevel::Auto,
            system_prompt: String::new(),
            llama_cpp: LlamaCppSettings::default(),
        }
    }
}

impl KammiSettings {
    pub fn normalize(mut self) -> Self {
        self.model = self.model.trim().to_owned();
        self.system_prompt = self.system_prompt.trim().to_owned();
        self.llama_cpp = self.llama_cpp.normalize();

        let mut models = Vec::with_capacity(self.saved_models.len().min(MAX_SAVED_MODELS));
        if !self.model.is_empty() {
            models.push(self.model.clone());
        }
        for candidate in self.saved_models {
            let candidate = candidate.trim();
            if candidate.is_empty() || models.iter().any(|model| model == candidate) {
                continue;
            }
            models.push(candidate.to_owned());
            if models.len() == MAX_SAVED_MODELS {
                break;
            }
        }
        self.saved_models = models;
        self
    }

    pub fn active_model_label(&self) -> String {
        match self.backend {
            ProviderBackend::OpenRouter => self.model.clone(),
            ProviderBackend::LlamaCpp => self.llama_cpp.model_label(),
        }
    }

    pub fn select_or_add_model(&mut self, model: &str) -> Result<()> {
        let model = validate_model_id(model)?;
        self.saved_models.retain(|saved| saved != &model);
        self.saved_models.insert(0, model.clone());
        self.saved_models.truncate(MAX_SAVED_MODELS);
        self.model = model;
        Ok(())
    }

    pub fn remove_model(&mut self, model: &str) {
        self.saved_models.retain(|saved| saved != model);
        if self.model == model {
            self.model = self.saved_models.first().cloned().unwrap_or_default();
        }
    }

    pub fn set_system_prompt(&mut self, prompt: &str) -> Result<()> {
        let prompt = prompt.trim();
        anyhow::ensure!(
            prompt.len() <= MAX_SYSTEM_PROMPT_BYTES,
            "system prompt exceeds {MAX_SYSTEM_PROMPT_BYTES} UTF-8 bytes"
        );
        self.system_prompt.clear();
        self.system_prompt.push_str(prompt);
        Ok(())
    }
}

pub fn validate_model_id(model: &str) -> Result<String> {
    let model = model.trim();
    anyhow::ensure!(!model.is_empty(), "model ID is required");
    anyhow::ensure!(model.len() <= 256, "model ID is too long");
    let Some((provider, name)) = model.split_once('/') else {
        anyhow::bail!("model ID must use provider/model form");
    };
    anyhow::ensure!(
        !provider.is_empty() && !name.is_empty(),
        "model ID must include a provider and model name"
    );
    anyhow::ensure!(
        model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._/:@".contains(&byte)),
        "model ID contains unsupported characters"
    );
    Ok(model.to_owned())
}

pub fn store_openrouter_key(api_key: &str) -> Result<()> {
    let api_key = validate_openrouter_key(api_key)?;
    let entry =
        Entry::new(SERVICE, OPENROUTER_USER).context("create OpenRouter credential entry")?;
    entry
        .set_password(api_key)
        .context("store OpenRouter API key")
}

pub fn load_openrouter_key() -> Result<Option<String>> {
    let entry =
        Entry::new(SERVICE, OPENROUTER_USER).context("create OpenRouter credential entry")?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(validate_openrouter_key(&secret)?.to_owned())),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("read OpenRouter API key"),
    }
}

pub fn validate_openrouter_key(api_key: &str) -> Result<&str> {
    let api_key = api_key.trim();
    anyhow::ensure!(!api_key.is_empty(), "API key is empty");
    anyhow::ensure!(
        api_key.len() <= MAX_OPENROUTER_KEY_BYTES,
        "API key is unexpectedly long"
    );
    anyhow::ensure!(
        api_key.starts_with("sk-or-") && api_key.len() >= 32,
        "API key does not have the expected OpenRouter format"
    );
    anyhow::ensure!(
        api_key.bytes().all(|byte| byte.is_ascii_graphic()),
        "API key contains whitespace or unsupported characters"
    );
    Ok(api_key)
}

pub fn contains_openrouter_key(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| !ch.is_ascii_graphic());
        token.starts_with("sk-or-") && token.len() >= 32
    })
}

pub fn clear_openrouter_key() -> Result<()> {
    let entry =
        Entry::new(SERVICE, OPENROUTER_USER).context("create OpenRouter credential entry")?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("delete OpenRouter API key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_json_migrates_with_safe_defaults() {
        let settings: KammiSettings =
            serde_json::from_str(r#"{"model":"google/gemini-2.5-flash"}"#).unwrap();
        let settings = settings.normalize();

        assert_eq!(settings.model, "google/gemini-2.5-flash");
        assert_eq!(settings.saved_models, ["google/gemini-2.5-flash"]);
        assert_eq!(settings.reasoning, ReasoningLevel::Auto);
        assert_eq!(settings.backend, ProviderBackend::OpenRouter);
        assert_eq!(settings.llama_cpp.endpoint, "http://127.0.0.1:8080/v1");
        assert_eq!(
            settings.llama_cpp.performance,
            LlamaPerformanceProfile::SingleSlot
        );
        assert!(settings.system_prompt.is_empty());
    }

    #[test]
    fn selecting_models_is_deduplicated_and_most_recent_first() {
        let mut settings = KammiSettings::default();
        settings.select_or_add_model("openai/gpt-4o").unwrap();
        settings
            .select_or_add_model("google/gemini-2.5-flash")
            .unwrap();
        settings.select_or_add_model("openai/gpt-4o").unwrap();

        assert_eq!(settings.model, "openai/gpt-4o");
        assert_eq!(
            settings.saved_models,
            ["openai/gpt-4o", "google/gemini-2.5-flash"]
        );
    }

    #[test]
    fn removing_the_active_model_selects_the_next_saved_model() {
        let mut settings = KammiSettings::default();
        settings.select_or_add_model("openai/gpt-4o").unwrap();
        settings
            .select_or_add_model("google/gemini-2.5-flash")
            .unwrap();
        settings.remove_model("google/gemini-2.5-flash");

        assert_eq!(settings.model, "openai/gpt-4o");
        assert_eq!(settings.saved_models, ["openai/gpt-4o"]);
    }

    #[test]
    fn model_ids_reject_labels_and_whitespace() {
        assert!(validate_model_id("Gemini Flash").is_err());
        assert!(validate_model_id("google/gemini flash").is_err());
        assert!(validate_model_id("google/gemini-2.5-flash").is_ok());
    }

    #[test]
    fn openrouter_keys_are_shape_checked_without_persisting_them() {
        assert!(validate_openrouter_key("not-a-key").is_err());
        assert!(validate_openrouter_key("sk-or-short").is_err());
        assert!(validate_openrouter_key("sk-or-v1-valid_but contains-space").is_err());
        assert!(validate_openrouter_key("sk-or-v1-123456789012345678901234").is_ok());
    }

    #[test]
    fn chat_secret_guard_detects_embedded_openrouter_keys() {
        assert!(contains_openrouter_key(
            "please use sk-or-v1-123456789012345678901234 now"
        ));
        assert!(!contains_openrouter_key(
            "describe the sk-or-key naming scheme"
        ));
    }

    #[test]
    fn local_model_label_is_derived_without_retaining_the_full_path() {
        let settings = LlamaCppSettings {
            model_path: r"D:\private-models\Qwen3-8B-Q4_K_M.gguf".into(),
            ..LlamaCppSettings::default()
        };

        assert_eq!(settings.model_label(), "Qwen3-8B-Q4_K_M");
    }
}
