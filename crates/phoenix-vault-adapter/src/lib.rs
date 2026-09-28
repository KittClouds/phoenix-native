//! Opt-in Phoenix product adapter for the accepted Kammi Library vault API.
//!
//! This crate does not select an authority mode or alter Phoenix saves. It
//! snapshots already committed product bytes and offers a bounded mirror pass
//! for isolated fixtures. A later kernel hook must populate the ordered outbox
//! and pass the cutover gates before use with a live workspace.

use kammi_client::KammiClient;
use phoenix_reader_session::{NarrationPlan, ReaderSession};
use phoenix_workspace::{open_document, EntryId, EntryKind, WorkspaceDocument};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;

mod cursor;
mod outbox;
mod scene;
pub use cursor::{MirrorCursor, MirrorCursorStore, RevisionMapping};
pub use outbox::{AcknowledgedCommit, MirrorOutbox, QueuedCommit, QueuedCommitHeader};
pub use scene::{stage_verified_scene, SceneAsset, SceneAssetPlan, SceneVaultTransport};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The exact bytes already committed by Phoenix. `local_revision` is a
/// Phoenix revision; Library revisions are tracked separately in its view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedSource {
    pub source_id: String,
    pub local_revision: u64,
    pub bytes: Vec<u8>,
    pub artifact_id: String,
}

impl SavedSource {
    pub fn new(source_id: String, local_revision: u64, bytes: Vec<u8>) -> Result<Self> {
        if !safe_component(&source_id) || bytes.len() > 16 * 1024 * 1024 {
            return Err("invalid Phoenix vault source".into());
        }
        let artifact_id = sha256_id(&bytes);
        Ok(Self {
            source_id,
            local_revision,
            bytes,
            artifact_id,
        })
    }
}

/// Read the durable workspace tree and note leases. Directory order never
/// becomes vault identity; entry IDs stay stable across rename.
pub fn saved_sources(workspace_path: &Path) -> Result<Vec<SavedSource>> {
    let workspace = WorkspaceDocument::load(workspace_path)?;
    let manifest = fs::read(workspace_path)?;
    let mut sources = vec![SavedSource::new(
        "workspace".into(),
        workspace.revision(),
        manifest,
    )?];
    for entry in workspace.entries() {
        if entry.kind != EntryKind::Note {
            continue;
        }
        let lease = open_document(workspace_path, &workspace, entry.id)?;
        if lease.revision.0 == 0 {
            continue;
        }
        sources.push(SavedSource::new(
            source_id(entry.id),
            lease.revision.0,
            lease.content.as_bytes().to_vec(),
        )?);
    }
    sources[1..].sort_unstable_by(|a, b| a.source_id.cmp(&b.source_id));
    Ok(sources)
}

#[must_use]
pub fn source_id(entry: EntryId) -> String {
    format!("note.{:016x}", entry.0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReaderLocator {
    pub source_id: String,
    /// Phoenix document revision. Resolve its Library source revision from
    /// the acknowledged mirror mapping before calling the vault Reader API.
    pub local_revision: u64,
    pub offset: u64,
    pub session_id: [u8; 32],
    pub checkpoint_sequence: u64,
}

impl ReaderLocator {
    /// Resolve the Library revision only from a durable mirror mapping.
    /// Callers must submit positions in session order; this builder never
    /// treats the Phoenix revision number as a Library revision.
    pub fn vault_request(
        &self,
        cursor: &MirrorCursor,
        vault_id: &str,
        actor_id: &str,
    ) -> Result<Value> {
        if cursor.vault_id != vault_id || cursor.actor_id != actor_id {
            return Err("reader vault identity mismatch".into());
        }
        let revision = cursor
            .remote_revision(&self.source_id, self.local_revision)
            .ok_or("reader source revision is not mirrored")?;
        let mut hasher = Sha256::new();
        for field in [
            vault_id.as_bytes(),
            self.source_id.as_bytes(),
            self.session_id.as_slice(),
            &self.checkpoint_sequence.to_le_bytes(),
            &self.offset.to_le_bytes(),
        ] {
            hasher.update((field.len() as u64).to_le_bytes());
            hasher.update(field);
        }
        Ok(serde_json::json!({
            "actor_id": actor_id,
            "vault_id": vault_id,
            "source_id": self.source_id,
            "revision": revision,
            "offset": self.offset,
            "request_id": format!("phoenix-reader-{:x}", hasher.finalize()),
        }))
    }
}

/// The vault's locator is deliberately a segment boundary. Audio frames do
/// not imply a defensible source byte offset within synthesized speech.
/// Call only for a book session; selection audition never updates this locator.
pub fn reader_locator_from_book_session(
    plan: &NarrationPlan,
    session: &ReaderSession,
) -> Result<ReaderLocator> {
    session.validate(plan)?;
    let binding = session.document();
    let segment = plan.segment(session.position().segment)?;
    Ok(ReaderLocator {
        source_id: source_id(EntryId(binding.entry)),
        local_revision: binding.revision,
        offset: u64::from(segment.source.start),
        session_id: session.id(),
        checkpoint_sequence: session.sequence(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorDecision {
    AlreadyCurrent {
        remote_revision: u64,
    },
    Commit {
        base_revision: u64,
        request_id: String,
    },
}

/// Compare exact CAS identity, not filenames, labels, or BLAKE3 document
/// hashes. A matching remote source is a no-op even after a lost response.
pub fn decide(vault_id: &str, source: &SavedSource, view: &Value) -> Result<MirrorDecision> {
    if !safe_component(vault_id) || view.get("vault_id").and_then(Value::as_str) != Some(vault_id) {
        return Err("vault identity mismatch".into());
    }
    let remote = view
        .get("sources")
        .and_then(Value::as_object)
        .and_then(|sources| sources.get(&source.source_id));
    let base_revision = match remote {
        Some(value) => {
            let revision = value
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or("remote source revision missing")?;
            let artifact = value
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or("remote source artifact missing")?;
            let byte_count = value
                .get("byte_count")
                .and_then(Value::as_u64)
                .ok_or("remote source byte count missing")?;
            if artifact == source.artifact_id && byte_count == source.bytes.len() as u64 {
                return Ok(MirrorDecision::AlreadyCurrent {
                    remote_revision: revision,
                });
            }
            return Err("remote source differs; use the ordered outbox".into());
        }
        None => 0,
    };
    Ok(MirrorDecision::Commit {
        base_revision,
        request_id: mirror_request_id(vault_id, source),
    })
}

/// The product will supply a durable outbox before calling this on live
/// commits. The transport boundary is narrow enough for fixture tests.
pub trait VaultTransport {
    fn view(&self, vault_id: &str, actor_id: &str) -> Result<Value>;
    fn commit_source_file(
        &self,
        path: &Path,
        vault_id: &str,
        source_id: &str,
        base_revision: u64,
        actor_id: &str,
        request_id: &str,
    ) -> Result<Value>;
}

impl VaultTransport for KammiClient {
    fn view(&self, vault_id: &str, actor_id: &str) -> Result<Value> {
        self.vault_view(vault_id, actor_id)
    }

    fn commit_source_file(
        &self,
        path: &Path,
        vault_id: &str,
        source_id: &str,
        base_revision: u64,
        actor_id: &str,
        request_id: &str,
    ) -> Result<Value> {
        self.vault_commit_source_file(
            path,
            vault_id,
            source_id,
            base_revision,
            actor_id,
            request_id,
        )
    }
}

/// One source only, with a fresh service view. Never report success from a
/// local save alone; inspect the service's returned revision and CAS identity.
pub fn mirror_saved_source<T: VaultTransport>(
    transport: &T,
    vault_id: &str,
    actor_id: &str,
    source: &SavedSource,
) -> Result<u64> {
    if !safe_component(actor_id) {
        return Err("invalid Phoenix service actor".into());
    }
    let view = transport.view(vault_id, actor_id)?;
    if view.get("owner_actor").and_then(Value::as_str) != Some(actor_id) {
        return Err("vault owner mismatch".into());
    }
    let (base_revision, request_id) = match decide(vault_id, source, &view)? {
        MirrorDecision::AlreadyCurrent { remote_revision } => return Ok(remote_revision),
        MirrorDecision::Commit {
            base_revision,
            request_id,
        } => (base_revision, request_id),
    };
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(&source.bytes)?;
    file.as_file_mut().sync_all()?;
    let response = transport.commit_source_file(
        file.path(),
        vault_id,
        &source.source_id,
        base_revision,
        actor_id,
        &request_id,
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
    if revision != base_revision + 1 || artifact != source.artifact_id {
        return Err("source commit receipt does not match saved bytes".into());
    }
    Ok(revision)
}

fn mirror_request_id(vault_id: &str, source: &SavedSource) -> String {
    let mut hash = Sha256::new();
    for part in [
        vault_id.as_bytes(),
        source.source_id.as_bytes(),
        &source.local_revision.to_le_bytes(),
        source.artifact_id.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    format!("phoenix-source-{:x}", hash.finalize())
}

fn sha256_id(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_reader_session::{plan_markdown, PlannerConfig, ReaderSession};
    use phoenix_workspace::{commit_document, EntryKind, WorkspaceDocument, ROOT_ID};
    use serde_json::json;
    use std::cell::RefCell;

    struct Fake {
        state: RefCell<Value>,
    }

    impl VaultTransport for Fake {
        fn view(&self, _: &str, _: &str) -> Result<Value> {
            Ok(self.state.borrow().clone())
        }
        fn commit_source_file(
            &self,
            path: &Path,
            vault_id: &str,
            source_id: &str,
            base: u64,
            _: &str,
            _: &str,
        ) -> Result<Value> {
            let bytes = fs::read(path)?;
            let artifact_id = sha256_id(&bytes);
            let revision = base + 1;
            self.state.borrow_mut()["sources"][source_id] = json!({
                "revision": revision, "artifact_id": artifact_id,
                "byte_count": bytes.len(),
            });
            assert_eq!(vault_id, "vault.test");
            Ok(json!({"source": {"revision": revision, "artifact_id": artifact_id}}))
        }
    }

    #[test]
    fn idempotent_fixture_mirror_and_stable_note_identity() -> Result<()> {
        let fake = Fake {
            state: RefCell::new(json!({
                "vault_id":"vault.test", "owner_actor":"phoenix-service", "sources":{}
            })),
        };
        let source = SavedSource::new(source_id(EntryId(42)), 7, b"saved note".to_vec())?;
        assert_eq!(source.source_id, "note.000000000000002a");
        assert_eq!(
            mirror_saved_source(&fake, "vault.test", "phoenix-service", &source)?,
            1
        );
        assert_eq!(
            mirror_saved_source(&fake, "vault.test", "phoenix-service", &source)?,
            1
        );
        assert_eq!(fake.state.borrow()["sources"].as_object().unwrap().len(), 1);
        Ok(())
    }

    #[test]
    fn rejects_wrong_owner_and_remote_identity() -> Result<()> {
        let fake = Fake {
            state: RefCell::new(json!({
                "vault_id":"vault.test", "owner_actor":"other", "sources":{}
            })),
        };
        let source = SavedSource::new("workspace".into(), 1, b"{}".to_vec())?;
        assert!(mirror_saved_source(&fake, "vault.test", "phoenix-service", &source).is_err());
        assert!(decide("another", &source, &fake.state.borrow()).is_err());
        let divergent = json!({"vault_id":"vault.test", "sources": {
            "workspace": {"revision": 2, "artifact_id":"sha256:other", "byte_count": 2}
        }});
        assert!(decide("vault.test", &source, &divergent).is_err());
        Ok(())
    }

    #[test]
    fn snapshots_only_committed_note_bytes_and_survives_rename() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("workspace.json");
        let mut workspace = WorkspaceDocument::seeded();
        let note = workspace.create(ROOT_ID, EntryKind::Note, "Before")?;
        workspace.save_atomic(&path)?;
        let empty = open_document(&path, &workspace, note)?;
        commit_document(&path, &workspace, empty.token(), "A saved note")?;
        let before = saved_sources(&path)?;
        let note_before = before
            .iter()
            .find(|item| item.source_id == source_id(note))
            .unwrap();
        assert_eq!(note_before.bytes, b"A saved note");
        workspace.rename(note, "After")?;
        workspace.save_atomic(&path)?;
        let after = saved_sources(&path)?;
        let note_after = after
            .iter()
            .find(|item| item.source_id == source_id(note))
            .unwrap();
        assert_eq!(note_before, note_after);
        assert_ne!(before[0].artifact_id, after[0].artifact_id);
        Ok(())
    }

    #[test]
    fn source_size_gate_matches_phoenix_document_limit() {
        assert!(SavedSource::new("note.x".into(), 1, vec![0; 16 * 1024 * 1024]).is_ok());
        assert!(SavedSource::new("note.x".into(), 1, vec![0; 16 * 1024 * 1024 + 1]).is_err());
    }

    #[test]
    fn vendored_sdk_archive_matches_accepted_handoff() -> Result<()> {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor");
        let handoff: Value =
            serde_json::from_slice(&fs::read(vendor.join("SDK-HANDOFF-v1.json"))?)?;
        assert_eq!(handoff["api_major"], "v1");
        assert_eq!(handoff["package"], "kammi-client-0.1.0.crate");
        assert_eq!(
            handoff["package_sha256"],
            sha256_id(&fs::read(vendor.join("kammi-client-0.1.0.crate"))?)
        );
        Ok(())
    }

    #[test]
    fn reader_locator_uses_bound_source_segment_start() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("workspace.json");
        let mut workspace = WorkspaceDocument::seeded();
        let note = workspace.create(ROOT_ID, EntryKind::Note, "Read")?;
        workspace.save_atomic(&path)?;
        let empty = open_document(&path, &workspace, note)?;
        let lease = commit_document(
            &path,
            &workspace,
            empty.token(),
            "One sentence. Two sentences.",
        )?;
        let plan = plan_markdown([1; 32], &lease, PlannerConfig::default())?.plan;
        let session = ReaderSession::new([2; 32], &plan, [3; 32])?;
        let locator = reader_locator_from_book_session(&plan, &session)?;
        assert_eq!(locator.source_id, source_id(note));
        assert_eq!(locator.local_revision, lease.revision.0);
        assert_eq!(locator.offset, u64::from(plan.segment(0)?.source.start));
        let cursor = MirrorCursor::empty("vault.test", "phoenix-service")?;
        assert!(locator
            .vault_request(&cursor, "vault.test", "phoenix-service")
            .is_err());
        Ok(())
    }
}
