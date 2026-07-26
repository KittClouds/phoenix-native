use super::{
    move_new_file, replace_file, temporary_path, write_synced, EntryId, EntryKind,
    WorkspaceDocument, WorkspaceError,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DOCUMENT_MAGIC: [u8; 16] = *b"PHXDOCPACKV1\0\0\0\0";
const DOCUMENT_VERSION: u32 = 1;
const HEADER_BYTES: usize = 80;
const DOCUMENT_DIRECTORY: &str = "documents-v1";
const DOCUMENT_EXTENSION: &str = "phxdoc";
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct DocumentRevision(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub fn of(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentLease {
    pub entry_id: EntryId,
    pub revision: DocumentRevision,
    pub content_hash: ContentHash,
    pub content: Arc<str>,
}

impl DocumentLease {
    fn empty(entry_id: EntryId) -> Self {
        let content: Arc<str> = Arc::from("");
        Self {
            entry_id,
            revision: DocumentRevision(0),
            content_hash: ContentHash::of(content.as_bytes()),
            content,
        }
    }

    pub fn token(&self) -> DocumentLeaseToken {
        DocumentLeaseToken {
            entry_id: self.entry_id,
            revision: self.revision,
            content_hash: self.content_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DocumentLeaseToken {
    pub entry_id: EntryId,
    pub revision: DocumentRevision,
    pub content_hash: ContentHash,
}

pub fn open_document(
    workspace_path: &Path,
    workspace: &WorkspaceDocument,
    entry_id: EntryId,
) -> Result<DocumentLease, WorkspaceError> {
    require_note(workspace, entry_id)?;
    let path = document_path(workspace_path, entry_id)?;
    if !path.exists() {
        return Ok(DocumentLease::empty(entry_id));
    }
    let stored_bytes = fs::metadata(&path)
        .map_err(|source| WorkspaceError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    if stored_bytes > (HEADER_BYTES + MAX_DOCUMENT_BYTES) as u64 {
        return Err(WorkspaceError::DocumentTooLarge {
            actual: usize::try_from(stored_bytes).unwrap_or(usize::MAX),
            maximum: HEADER_BYTES + MAX_DOCUMENT_BYTES,
        });
    }
    let bytes = fs::read(&path).map_err(|source| WorkspaceError::Io {
        path: path.clone(),
        source,
    })?;
    decode(entry_id, &bytes)
}

pub fn commit_document(
    workspace_path: &Path,
    workspace: &WorkspaceDocument,
    base: DocumentLeaseToken,
    content: &str,
) -> Result<DocumentLease, WorkspaceError> {
    require_note(workspace, base.entry_id)?;
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(WorkspaceError::DocumentTooLarge {
            actual: content.len(),
            maximum: MAX_DOCUMENT_BYTES,
        });
    }
    let current = open_document(workspace_path, workspace, base.entry_id)?;
    if current.revision != base.revision || current.content_hash != base.content_hash {
        return Err(WorkspaceError::StaleDocumentLease {
            entry: base.entry_id,
            expected: base.revision.0,
            actual: current.revision.0,
        });
    }
    let content_hash = ContentHash::of(content.as_bytes());
    if content_hash == current.content_hash && content.as_bytes() == current.content.as_bytes() {
        return Ok(current);
    }
    let revision =
        DocumentRevision(
            current.revision.0.checked_add(1).ok_or_else(|| {
                WorkspaceError::CorruptDocument(base.entry_id, "revision exhausted")
            })?,
        );
    let lease = DocumentLease {
        entry_id: base.entry_id,
        revision,
        content_hash,
        content: Arc::from(content),
    };
    let path = document_path(workspace_path, base.entry_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| WorkspaceError::CorruptDocument(base.entry_id, "path has no parent"))?;
    fs::create_dir_all(parent).map_err(|source| WorkspaceError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let bytes = encode(&lease)?;
    let temp = temporary_path(&path);
    if let Err(error) = write_synced(&temp, &bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let result = if path.exists() {
        replace_file(&path, &temp)
    } else {
        move_new_file(&path, &temp)
    };
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map(|()| lease)
}

fn require_note(workspace: &WorkspaceDocument, entry_id: EntryId) -> Result<(), WorkspaceError> {
    let entry = workspace
        .entry(entry_id)
        .ok_or(WorkspaceError::MissingEntry(entry_id))?;
    if entry.kind != EntryKind::Note {
        return Err(WorkspaceError::NotNote(entry_id));
    }
    Ok(())
}

fn document_path(workspace_path: &Path, entry_id: EntryId) -> Result<PathBuf, WorkspaceError> {
    let parent = workspace_path
        .parent()
        .ok_or_else(|| WorkspaceError::CorruptDocument(entry_id, "workspace path has no parent"))?;
    Ok(parent
        .join(DOCUMENT_DIRECTORY)
        .join(format!("{:016x}.{DOCUMENT_EXTENSION}", entry_id.0)))
}

fn encode(lease: &DocumentLease) -> Result<Vec<u8>, WorkspaceError> {
    let content_len =
        u64::try_from(lease.content.len()).map_err(|_| WorkspaceError::DocumentTooLarge {
            actual: lease.content.len(),
            maximum: MAX_DOCUMENT_BYTES,
        })?;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + lease.content.len());
    bytes.extend_from_slice(&DOCUMENT_MAGIC);
    bytes.extend_from_slice(&DOCUMENT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    bytes.extend_from_slice(&lease.entry_id.0.to_le_bytes());
    bytes.extend_from_slice(&lease.revision.0.to_le_bytes());
    bytes.extend_from_slice(&content_len.to_le_bytes());
    bytes.extend_from_slice(&lease.content_hash.0);
    bytes.extend_from_slice(lease.content.as_bytes());
    Ok(bytes)
}

fn decode(entry_id: EntryId, bytes: &[u8]) -> Result<DocumentLease, WorkspaceError> {
    if bytes.len() < HEADER_BYTES {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "truncated header",
        ));
    }
    if bytes[..16] != DOCUMENT_MAGIC {
        return Err(WorkspaceError::CorruptDocument(entry_id, "invalid magic"));
    }
    let version = read_u32(bytes, 16, entry_id)?;
    if version != DOCUMENT_VERSION {
        return Err(WorkspaceError::UnsupportedDocumentFormat(version));
    }
    if read_u32(bytes, 20, entry_id)? as usize != HEADER_BYTES {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "invalid header length",
        ));
    }
    if read_u64(bytes, 24, entry_id)? != entry_id.0 {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "entry identity mismatch",
        ));
    }
    let revision = DocumentRevision(read_u64(bytes, 32, entry_id)?);
    if revision.0 == 0 {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "stored revision is zero",
        ));
    }
    let content_len = usize::try_from(read_u64(bytes, 40, entry_id)?)
        .map_err(|_| WorkspaceError::CorruptDocument(entry_id, "content length overflows usize"))?;
    if content_len > MAX_DOCUMENT_BYTES {
        return Err(WorkspaceError::DocumentTooLarge {
            actual: content_len,
            maximum: MAX_DOCUMENT_BYTES,
        });
    }
    if bytes.len() != HEADER_BYTES + content_len {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "content length mismatch",
        ));
    }
    let mut stored_hash = [0u8; 32];
    stored_hash.copy_from_slice(&bytes[48..80]);
    let content_bytes = &bytes[HEADER_BYTES..];
    let content_hash = ContentHash::of(content_bytes);
    if stored_hash != content_hash.0 {
        return Err(WorkspaceError::CorruptDocument(
            entry_id,
            "content hash mismatch",
        ));
    }
    let content = std::str::from_utf8(content_bytes)
        .map_err(|_| WorkspaceError::CorruptDocument(entry_id, "content is not UTF-8"))?;
    Ok(DocumentLease {
        entry_id,
        revision,
        content_hash,
        content: Arc::from(content),
    })
}

fn read_u32(bytes: &[u8], offset: usize, entry_id: EntryId) -> Result<u32, WorkspaceError> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(WorkspaceError::CorruptDocument(entry_id, "truncated u32"))?
        .try_into()
        .map_err(|_| WorkspaceError::CorruptDocument(entry_id, "invalid u32"))?;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize, entry_id: EntryId) -> Result<u64, WorkspaceError> {
    let raw: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(WorkspaceError::CorruptDocument(entry_id, "truncated u64"))?
        .try_into()
        .map_err(|_| WorkspaceError::CorruptDocument(entry_id, "invalid u64"))?;
    Ok(u64::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (PathBuf, WorkspaceDocument, EntryId) {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!(
                "phoenix-native-document-test-{}-{sequence}",
                std::process::id()
            ))
            .join("workspace.json");
        (path, WorkspaceDocument::seeded(), EntryId(3))
    }

    #[test]
    fn document_commit_is_atomic_versioned_and_restart_durable(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (path, workspace, entry) = fixture();
        workspace.save_atomic(&path)?;
        let empty = open_document(&path, &workspace, entry)?;
        assert_eq!(empty.revision, DocumentRevision(0));
        let saved = commit_document(&path, &workspace, empty.token(), "# Phoenix\n\nDurable.")?;
        assert_eq!(saved.revision, DocumentRevision(1));
        let reopened = open_document(&path, &WorkspaceDocument::load(&path)?, entry)?;
        assert_eq!(reopened, saved);
        fs::remove_dir_all(path.parent().ok_or("missing test parent")?)?;
        Ok(())
    }

    #[test]
    fn stale_and_corrupt_document_envelopes_fail_closed() -> Result<(), Box<dyn std::error::Error>>
    {
        let (path, workspace, entry) = fixture();
        workspace.save_atomic(&path)?;
        let empty = open_document(&path, &workspace, entry)?;
        let saved = commit_document(&path, &workspace, empty.token(), "first")?;
        let stale = commit_document(&path, &workspace, empty.token(), "second");
        assert!(matches!(
            stale,
            Err(WorkspaceError::StaleDocumentLease { .. })
        ));

        let document_path = document_path(&path, entry)?;
        let mut bytes = fs::read(&document_path)?;
        let last = bytes.last_mut().ok_or("document fixture is empty")?;
        *last ^= 0x80;
        fs::write(&document_path, bytes)?;
        assert!(matches!(
            open_document(&path, &workspace, entry),
            Err(WorkspaceError::CorruptDocument(_, "content hash mismatch"))
        ));
        assert_eq!(saved.content.as_ref(), "first");
        fs::remove_dir_all(path.parent().ok_or("missing test parent")?)?;
        Ok(())
    }

    #[test]
    fn folders_and_oversized_content_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (path, workspace, entry) = fixture();
        assert!(matches!(
            open_document(&path, &workspace, EntryId(2)),
            Err(WorkspaceError::NotNote(EntryId(2)))
        ));
        let base = open_document(&path, &workspace, entry)?;
        let oversized = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert!(matches!(
            commit_document(&path, &workspace, base.token(), &oversized),
            Err(WorkspaceError::DocumentTooLarge { .. })
        ));
        Ok(())
    }
}
