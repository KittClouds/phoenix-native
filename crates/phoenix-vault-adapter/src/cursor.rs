use super::{safe_component, AcknowledgedCommit, MirrorOutbox, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RevisionMapping {
    pub remote_revision: u64,
    pub artifact_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceTrack {
    pub latest_local_revision: u64,
    pub latest_remote_revision: u64,
    pub revisions: BTreeMap<u64, RevisionMapping>,
}

/// Phoenix mirror cursor, separate from the Library's durable authority. It
/// remembers local-to-remote revision bindings needed for old Reader locators.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MirrorCursor {
    pub schema: String,
    pub vault_id: String,
    pub actor_id: String,
    pub source_epoch: u64,
    pub sources: BTreeMap<String, SourceTrack>,
}

impl MirrorCursor {
    pub fn empty(vault_id: &str, actor_id: &str) -> Result<Self> {
        if !safe_component(vault_id) || !safe_component(actor_id) {
            return Err("invalid mirror cursor identity".into());
        }
        Ok(Self {
            schema: "PHOENIX_VAULT_MIRROR_CURSOR_V1".into(),
            vault_id: vault_id.into(),
            actor_id: actor_id.into(),
            source_epoch: 0,
            sources: BTreeMap::new(),
        })
    }

    pub fn remote_revision(&self, source_id: &str, local_revision: u64) -> Option<u64> {
        self.sources
            .get(source_id)?
            .revisions
            .get(&local_revision)
            .map(|mapping| mapping.remote_revision)
    }

    pub fn expected_base(&self, source_id: &str) -> u64 {
        self.sources
            .get(source_id)
            .map_or(0, |source| source.latest_remote_revision)
    }

    pub fn apply_ack(&mut self, ack: &AcknowledgedCommit) -> Result<()> {
        if self.vault_id != ack.vault_id
            || self.actor_id != ack.actor_id
            || !safe_component(&ack.source_id)
            || ack.local_revision == 0
        {
            return Err("mirror acknowledgement identity mismatch".into());
        }
        let mapping = RevisionMapping {
            remote_revision: ack.remote_revision,
            artifact_id: ack.artifact_id.clone(),
        };
        if let Some(existing) = self
            .sources
            .get(&ack.source_id)
            .and_then(|track| track.revisions.get(&ack.local_revision))
        {
            if existing == &mapping && ack.remote_epoch <= self.source_epoch {
                return Ok(());
            }
            return Err("mirror acknowledgement conflicts with saved mapping".into());
        }
        if ack.remote_epoch != self.source_epoch + 1 {
            return Err("mirror acknowledgement source epoch discontinuity".into());
        }
        let track = self
            .sources
            .entry(ack.source_id.clone())
            .or_insert_with(|| SourceTrack {
                latest_local_revision: 0,
                latest_remote_revision: 0,
                revisions: BTreeMap::new(),
            });
        if ack.remote_revision != track.latest_remote_revision + 1
            || (track.latest_local_revision != 0
                && ack.local_revision != track.latest_local_revision + 1)
        {
            return Err("mirror acknowledgement source revision discontinuity".into());
        }
        track.revisions.insert(ack.local_revision, mapping);
        track.latest_local_revision = ack.local_revision;
        track.latest_remote_revision = ack.remote_revision;
        self.source_epoch = ack.remote_epoch;
        Ok(())
    }
}

pub struct MirrorCursorStore {
    path: PathBuf,
    vault_id: String,
    actor_id: String,
}

impl MirrorCursorStore {
    pub fn at(path: impl Into<PathBuf>, vault_id: &str, actor_id: &str) -> Result<Self> {
        MirrorCursor::empty(vault_id, actor_id)?;
        Ok(Self {
            path: path.into(),
            vault_id: vault_id.into(),
            actor_id: actor_id.into(),
        })
    }

    pub fn load(&self) -> Result<MirrorCursor> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return MirrorCursor::empty(&self.vault_id, &self.actor_id);
            }
            Err(error) => return Err(error.into()),
        };
        let cursor: MirrorCursor = serde_json::from_slice(&bytes)?;
        if cursor.schema != "PHOENIX_VAULT_MIRROR_CURSOR_V1"
            || cursor.vault_id != self.vault_id
            || cursor.actor_id != self.actor_id
        {
            return Err("mirror cursor identity mismatch".into());
        }
        Ok(cursor)
    }

    fn save(&self, cursor: &MirrorCursor) -> Result<()> {
        let parent = self.path.parent().ok_or("mirror cursor has no parent")?;
        fs::create_dir_all(parent)?;
        let mut pending = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut pending, cursor)?;
        pending.flush()?;
        pending.as_file_mut().sync_all()?;
        pending.persist(&self.path)?;
        Ok(())
    }

    /// The outbox item is removed only after the revision map is durable.
    /// If removal fails, replay applies the same acknowledged mapping again.
    pub fn record_and_complete(
        &self,
        outbox: &MirrorOutbox,
        ack: &AcknowledgedCommit,
    ) -> Result<()> {
        let mut cursor = self.load()?;
        cursor.apply_ack(ack)?;
        self.save(&cursor)?;
        outbox.complete_ack(ack)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SavedSource;

    #[test]
    fn mapping_survives_restart_and_replay_without_double_advance() -> Result<()> {
        let root = tempfile::tempdir()?;
        let store = MirrorCursorStore::at(
            root.path().join("cursor.json"),
            "vault.test",
            "phoenix-service",
        )?;
        let outbox = MirrorOutbox::at(root.path().join("outbox"));
        let source = SavedSource::new("note.1".into(), 7, b"hello".to_vec())?;
        let path = outbox.enqueue(1, "vault.test", "phoenix-service", &source, 0)?;
        let item = outbox.read_item(&path)?;
        let ack = AcknowledgedCommit {
            path,
            vault_id: item.header.vault_id,
            actor_id: item.header.actor_id,
            source_id: item.header.source_id,
            local_revision: 7,
            remote_revision: 1,
            remote_epoch: 1,
            artifact_id: item.header.artifact_id,
            request_id: item.header.request_id,
        };
        let mut cursor = store.load()?;
        cursor.apply_ack(&ack)?;
        store.save(&cursor)?;
        // Simulate a crash before deleting the outbox item.
        store.record_and_complete(&outbox, &ack)?;
        let reopened = store.load()?;
        assert_eq!(reopened.source_epoch, 1);
        assert_eq!(reopened.remote_revision("note.1", 7), Some(1));
        assert!(outbox.pending()?.is_empty());
        Ok(())
    }
}
