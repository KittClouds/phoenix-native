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
    pub model: String,
    #[serde(default)]
    pub saved_models: Vec<String>,
    #[serde(default)]
    pub reasoning: ReasoningLevel,
    #[serde(default)]
    pub system_prompt: String,
}

impl Default for KammiSettings {
    fn default() -> Self {
        Self {
            model: String::new(),
            saved_models: Vec::new(),
            reasoning: ReasoningLevel::Auto,
            system_prompt: String::new(),
        }
    }
}

impl KammiSettings {
    pub fn normalize(mut self) -> Self {
        self.model = self.model.trim().to_owned();
        self.system_prompt = self.system_prompt.trim().to_owned();

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
}
