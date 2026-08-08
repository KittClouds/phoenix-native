use super::session::KammiSession;
use super::settings::KammiSettings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const MAX_SESSIONS: usize = 32;
const MAX_MESSAGES_PER_SESSION: usize = 256;
const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const HISTORY_FILE_NAME: &str = "workspace-v1.json.kammi-v1.json";

#[derive(Serialize, Deserialize, Debug)]
pub struct KammiStoreV1 {
    pub format: String,
    pub settings: KammiSettings,
    pub selected_session: Option<u64>,
    pub sessions: Vec<KammiSession>,
}

impl Default for KammiStoreV1 {
    fn default() -> Self {
        Self {
            format: "kammi-v1".to_string(),
            settings: KammiSettings::default(),
            selected_session: None,
            sessions: Vec::new(),
        }
    }
}

impl KammiStoreV1 {
    pub fn file_path(workspace_dir: &Path) -> PathBuf {
        workspace_dir.join(HISTORY_FILE_NAME)
    }

    pub fn load(workspace_dir: &Path) -> Result<Option<Self>> {
        let path = Self::file_path(workspace_dir);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read Kammi store at {}", path.display()))?;
        let store: Self = serde_json::from_str(&content)
            .with_context(|| format!("parse Kammi store at {}", path.display()))?;
        Ok(Some(store))
    }

    pub fn save(
        workspace_dir: &Path,
        settings: &KammiSettings,
        active_session: &KammiSession,
        history: &VecDeque<KammiSession>,
    ) -> Result<()> {
        let path = Self::file_path(workspace_dir);
        let clamped_sessions = sessions_for_store(active_session, history);

        let store = Self {
            format: "kammi-v1".to_string(),
            settings: settings.clone(),
            selected_session: Some(active_session.id),
            sessions: clamped_sessions,
        };

        let json = serde_json::to_string_pretty(&store)?;

        if json.len() > MAX_HISTORY_BYTES {
            anyhow::bail!("Kammi store size exceeds 8MB limit");
        }

        let tmp_path = workspace_dir.join(format!("{HISTORY_FILE_NAME}.tmp"));
        fs::write(&tmp_path, json.as_bytes())
            .with_context(|| format!("write temporary Kammi store to {}", tmp_path.display()))?;

        fs::rename(&tmp_path, &path).or_else(|_| -> std::io::Result<()> {
            fs::copy(&tmp_path, &path)?;
            let _ = fs::remove_file(&tmp_path);
            Ok(())
        })?;

        Ok(())
    }
}

fn sessions_for_store(
    active_session: &KammiSession,
    history: &VecDeque<KammiSession>,
) -> Vec<KammiSession> {
    let mut sessions = Vec::with_capacity(MAX_SESSIONS.min(history.len().saturating_add(1)));
    sessions.push(active_session.clone());
    sessions.extend(
        history
            .iter()
            .filter(|session| session.id != active_session.id)
            .take(MAX_SESSIONS - 1)
            .cloned(),
    );
    for session in &mut sessions {
        if session.messages.len() > MAX_MESSAGES_PER_SESSION {
            let start = session.messages.len() - MAX_MESSAGES_PER_SESSION;
            session.messages = session.messages[start..].to_vec();
        }
    }
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_session_is_always_persisted_once_and_first() {
        let active = KammiSession::new(7);
        let mut history = VecDeque::new();
        history.push_back(active.clone());
        history.push_back(KammiSession::new(8));

        let sessions = sessions_for_store(&active, &history);

        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            [7, 8]
        );
    }
}
