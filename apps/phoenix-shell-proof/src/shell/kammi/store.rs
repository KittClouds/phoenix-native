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
    pub fn file_path(workspace_path: &Path) -> PathBuf {
        workspace_path.with_file_name(HISTORY_FILE_NAME)
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

        let tmp_path = pending_path(&path);
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

fn pending_path(path: &Path) -> PathBuf {
    let mut pending = path.as_os_str().to_os_string();
    pending.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(pending)
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

    #[test]
    fn store_is_a_sibling_of_the_workspace_manifest() {
        let workspace = Path::new(r"C:\Users\test\Phoenix\workspace-v1.json");

        assert_eq!(
            KammiStoreV1::file_path(workspace),
            Path::new(r"C:\Users\test\Phoenix\workspace-v1.json.kammi-v1.json")
        );
    }

    #[test]
    fn pending_store_is_a_sibling_not_a_child_of_the_workspace_manifest() {
        let store = Path::new(r"C:\Users\test\Phoenix\workspace-v1.json.kammi-v1.json");
        let pending = pending_path(store);

        assert_eq!(pending.parent(), store.parent());
        assert!(pending
            .file_name()
            .expect("pending file name")
            .to_string_lossy()
            .starts_with("workspace-v1.json.kammi-v1.json."));
    }

    #[test]
    fn save_and_load_round_trip_next_to_a_workspace_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phoenix-kammi-store-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create test directory");
        let workspace = root.join("workspace-v1.json");
        fs::write(&workspace, b"{}").expect("write workspace fixture");

        let mut settings = KammiSettings::default();
        settings
            .select_or_add_model("google/gemini-3.6-flash")
            .expect("valid model");
        settings.reasoning = super::super::settings::ReasoningLevel::High;
        let active = KammiSession::new(11);
        let history = VecDeque::new();

        KammiStoreV1::save(&workspace, &settings, &active, &history).expect("save Kammi store");
        let loaded = KammiStoreV1::load(&workspace)
            .expect("load Kammi store")
            .expect("stored payload");

        assert_eq!(loaded.settings, settings);
        assert_eq!(loaded.selected_session, Some(11));
        assert_eq!(loaded.sessions.len(), 1);
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
