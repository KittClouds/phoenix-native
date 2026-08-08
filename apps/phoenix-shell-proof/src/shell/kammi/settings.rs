use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "Phoenix Native";
const OPENROUTER_USER: &str = "openrouter-api-key";

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct KammiSettings {
    pub model: String,
}

pub fn validate_model_id(model: &str) -> Result<String> {
    let model = model.trim();
    anyhow::ensure!(!model.is_empty(), "model ID is required");
    anyhow::ensure!(model.len() <= 256, "model ID is too long");
    Ok(model.to_owned())
}

pub fn store_openrouter_key(api_key: &str) -> Result<()> {
    let api_key = api_key.trim();
    anyhow::ensure!(!api_key.is_empty(), "API key is empty");
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
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("read OpenRouter API key"),
    }
}

pub fn clear_openrouter_key() -> Result<()> {
    let entry =
        Entry::new(SERVICE, OPENROUTER_USER).context("create OpenRouter credential entry")?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("delete OpenRouter API key"),
    }
}
