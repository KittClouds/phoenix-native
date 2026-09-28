use super::{mirror_request_id, safe_component, sha256_id, Result, SavedSource, VaultTransport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"PHXVOB1\0";
const MAX_HEADER_BYTES: usize = 4096;
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueuedCommitHeader {
    pub sequence: u64,
    pub vault_id: String,
    pub actor_id: String,
    pub source_id: String,
    pub local_revision: u64,
    pub base_revision: u64,
    pub request_id: String,
    pub artifact_id: String,
    pub byte_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedCommit {
    pub header: QueuedCommitHeader,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcknowledgedCommit {
    pub path: PathBuf,
    pub vault_id: String,
    pub actor_id: String,
    pub source_id: String,
    pub local_revision: u64,
    pub remote_revision: u64,
    pub remote_epoch: u64,
    pub artifact_id: String,
    pub request_id: String,
}

/// Product synchronization state for LEGACY_MIRROR. A complete immutable item
/// is published with one rename. It is not a Library journal or authority.
pub struct MirrorOutbox {
    root: PathBuf,
}

impl MirrorOutbox {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn enqueue(
        &self,
        sequence: u64,
        vault_id: &str,
        actor_id: &str,
        source: &SavedSource,
        base_revision: u64,
    ) -> Result<PathBuf> {
        if sequence == 0 || !safe_component(vault_id) || !safe_component(actor_id) {
            return Err("invalid vault outbox identity".into());
        }
        fs::create_dir_all(&self.root)?;
        let request_id = mirror_request_id(vault_id, source);
        let final_path = self.root.join(format!("{sequence:020}-{request_id}.item"));
        let item = QueuedCommit {
            header: QueuedCommitHeader {
                sequence,
                vault_id: vault_id.into(),
                actor_id: actor_id.into(),
                source_id: source.source_id.clone(),
                local_revision: source.local_revision,
                base_revision,
                request_id,
                artifact_id: source.artifact_id.clone(),
                byte_count: source.bytes.len() as u64,
            },
            bytes: source.bytes.clone(),
        };
        if final_path.exists() {
            if self.read_item(&final_path)? == item {
                return Ok(final_path);
            }
            return Err("vault outbox request collision".into());
        }
        let header = serde_json::to_vec(&item.header)?;
        if header.len() > MAX_HEADER_BYTES {
            return Err("vault outbox header too large".into());
        }
        let mut pending = tempfile::NamedTempFile::new_in(&self.root)?;
        pending.write_all(MAGIC)?;
        pending.write_all(&(header.len() as u32).to_le_bytes())?;
        pending.write_all(&header)?;
        pending.write_all(&item.bytes)?;
        pending.as_file_mut().sync_all()?;
        match pending.persist_noclobber(&final_path) {
            Ok(_) => Ok(final_path),
            Err(_error) if final_path.exists() && self.read_item(&final_path)? == item => {
                Ok(final_path)
            }
            Err(error) => Err(error.error.into()),
        }
    }

    pub fn pending(&self) -> Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "item")
            {
                paths.push(path);
            }
        }
        paths.sort_unstable();
        Ok(paths)
    }

    pub fn read_item(&self, path: &Path) -> Result<QueuedCommit> {
        if path.parent() != Some(self.root.as_path()) || path.extension() != Some("item".as_ref()) {
            return Err("outbox item outside expected directory".into());
        }
        let mut file = fs::File::open(path)?;
        let mut prefix = [0_u8; 12];
        file.read_exact(&mut prefix)?;
        if &prefix[..8] != MAGIC {
            return Err("invalid outbox magic".into());
        }
        let header_len = u32::from_le_bytes(prefix[8..12].try_into()?) as usize;
        if header_len > MAX_HEADER_BYTES {
            return Err("outbox header too large".into());
        }
        let mut header_bytes = vec![0_u8; header_len];
        file.read_exact(&mut header_bytes)?;
        let header: QueuedCommitHeader = serde_json::from_slice(&header_bytes)?;
        if header.sequence == 0
            || !safe_component(&header.vault_id)
            || !safe_component(&header.actor_id)
            || !safe_component(&header.source_id)
            || header.byte_count > MAX_SOURCE_BYTES
        {
            return Err("invalid outbox item identity or size".into());
        }
        let mut bytes = Vec::with_capacity(header.byte_count as usize);
        file.take(header.byte_count + 1).read_to_end(&mut bytes)?;
        let expected_name = format!("{:020}-{}.item", header.sequence, header.request_id);
        let bound = SavedSource {
            source_id: header.source_id.clone(),
            local_revision: header.local_revision,
            artifact_id: header.artifact_id.clone(),
            bytes: Vec::new(),
        };
        if bytes.len() as u64 != header.byte_count
            || sha256_id(&bytes) != header.artifact_id
            || mirror_request_id(&header.vault_id, &bound) != header.request_id
            || path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
        {
            return Err("outbox item failed content binding".into());
        }
        Ok(QueuedCommit { header, bytes })
    }

    /// Retry the original request ID and base revision. A lost response is
    /// idempotent at the Library. Divergence is an error, never a new base.
    pub fn flush_next<T: VaultTransport>(
        &self,
        transport: &T,
    ) -> Result<Option<AcknowledgedCommit>> {
        let Some(path) = self.pending()?.into_iter().next() else {
            return Ok(None);
        };
        let item = self.read_item(&path)?;
        let mut payload = tempfile::NamedTempFile::new()?;
        payload.write_all(&item.bytes)?;
        payload.as_file_mut().sync_all()?;
        let header = &item.header;
        let response = transport.commit_source_file(
            payload.path(),
            &header.vault_id,
            &header.source_id,
            header.base_revision,
            &header.actor_id,
            &header.request_id,
        )?;
        let receipt = response.get("source").ok_or("source receipt missing")?;
        let revision = receipt
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or("source revision missing")?;
        let artifact = receipt
            .get("artifact_id")
            .and_then(Value::as_str)
            .ok_or("source artifact missing")?;
        let epoch = receipt
            .get("epoch")
            .and_then(Value::as_u64)
            .ok_or("source epoch missing")?;
        if revision != header.base_revision + 1 || artifact != header.artifact_id {
            return Err("source commit receipt does not match queued bytes".into());
        }
        Ok(Some(AcknowledgedCommit {
            path,
            vault_id: header.vault_id.clone(),
            actor_id: header.actor_id.clone(),
            source_id: header.source_id.clone(),
            local_revision: header.local_revision,
            remote_revision: revision,
            remote_epoch: epoch,
            artifact_id: header.artifact_id.clone(),
            request_id: header.request_id.clone(),
        }))
    }

    /// Call only after the Phoenix local-revision to Library-revision mapping
    /// has been durably saved. If that save fails, retry the same outbox item.
    pub fn complete_ack(&self, ack: &AcknowledgedCommit) -> Result<()> {
        let item = self.read_item(&ack.path)?;
        if item.header.vault_id != ack.vault_id
            || item.header.actor_id != ack.actor_id
            || item.header.source_id != ack.source_id
            || item.header.local_revision != ack.local_revision
            || item.header.base_revision + 1 != ack.remote_revision
            || item.header.artifact_id != ack.artifact_id
            || item.header.request_id != ack.request_id
        {
            return Err("outbox acknowledgement does not match queued item".into());
        }
        fs::remove_file(&ack.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    struct Fake {
        calls: RefCell<Vec<String>>,
        epoch: RefCell<u64>,
    }
    impl VaultTransport for Fake {
        fn view(&self, _: &str, _: &str) -> Result<Value> {
            unreachable!()
        }
        fn commit_source_file(
            &self,
            path: &Path,
            _: &str,
            _: &str,
            base: u64,
            _: &str,
            request_id: &str,
        ) -> Result<Value> {
            self.calls.borrow_mut().push(request_id.to_owned());
            *self.epoch.borrow_mut() += 1;
            Ok(json!({"source": {"revision": base + 1,
                "epoch": *self.epoch.borrow(),
                "artifact_id": sha256_id(&fs::read(path)?)} }))
        }
    }

    #[test]
    fn queue_is_atomic_bounded_and_drains_in_sequence() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outbox = MirrorOutbox::at(root.path());
        let first = SavedSource::new("note.1".into(), 1, b"one".to_vec())?;
        let second = SavedSource::new("note.1".into(), 2, b"two".to_vec())?;
        let one = outbox.enqueue(1, "vault.test", "phoenix-service", &first, 0)?;
        outbox.enqueue(2, "vault.test", "phoenix-service", &second, 1)?;
        assert_eq!(
            outbox.enqueue(1, "vault.test", "phoenix-service", &first, 0)?,
            one
        );
        assert_eq!(outbox.pending()?.len(), 2);
        let fake = Fake {
            calls: RefCell::new(Vec::new()),
            epoch: RefCell::new(0),
        };
        let ack1 = outbox.flush_next(&fake)?.unwrap();
        assert_eq!(ack1.remote_revision, 1);
        assert_eq!(outbox.pending()?.len(), 2);
        outbox.complete_ack(&ack1)?;
        let ack2 = outbox.flush_next(&fake)?.unwrap();
        assert_eq!(ack2.remote_revision, 2);
        outbox.complete_ack(&ack2)?;
        assert_eq!(outbox.flush_next(&fake)?, None);
        assert_eq!(fake.calls.borrow().len(), 2);
        Ok(())
    }

    #[test]
    fn tamper_and_false_receipt_never_discard_queued_bytes() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outbox = MirrorOutbox::at(root.path());
        let source = SavedSource::new("workspace".into(), 1, b"{}".to_vec())?;
        let path = outbox.enqueue(1, "vault.test", "phoenix-service", &source, 0)?;
        let mut bytes = fs::read(&path)?;
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&path, bytes)?;
        assert!(outbox
            .flush_next(&Fake {
                calls: RefCell::new(Vec::new()),
                epoch: RefCell::new(0),
            })
            .is_err());
        assert!(path.exists());
        Ok(())
    }

    struct LostResponse {
        seen: RefCell<Vec<String>>,
    }

    impl VaultTransport for LostResponse {
        fn view(&self, _: &str, _: &str) -> Result<Value> {
            unreachable!()
        }

        fn commit_source_file(
            &self,
            path: &Path,
            _: &str,
            _: &str,
            base: u64,
            _: &str,
            request_id: &str,
        ) -> Result<Value> {
            let mut seen = self.seen.borrow_mut();
            seen.push(request_id.to_owned());
            if seen.len() == 1 {
                return Err("response lost after server commit".into());
            }
            Ok(json!({"source": {"revision": base + 1,
                "epoch": 1,
                "artifact_id": sha256_id(&fs::read(path)?)} }))
        }
    }

    #[test]
    fn lost_response_retries_exact_request_and_bytes() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outbox = MirrorOutbox::at(root.path());
        let source = SavedSource::new("note.1".into(), 3, b"stable".to_vec())?;
        let path = outbox.enqueue(10, "vault.test", "phoenix-service", &source, 2)?;
        let fake = LostResponse {
            seen: RefCell::new(Vec::new()),
        };
        assert!(outbox.flush_next(&fake).is_err());
        assert!(path.exists());
        let ack = outbox.flush_next(&fake)?.unwrap();
        assert_eq!(ack.remote_revision, 3);
        outbox.complete_ack(&ack)?;
        let seen = fake.seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], seen[1]);
        Ok(())
    }
}
