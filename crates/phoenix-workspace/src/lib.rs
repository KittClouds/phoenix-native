//! Durable workspace authority for the native Phoenix application.

mod documents;
mod entities;
mod palette;

pub use documents::{
    commit_document, open_document, ContentHash, DocumentLease, DocumentLeaseToken,
    DocumentRevision, MAX_DOCUMENT_BYTES,
};
pub use entities::{
    EntityRegistry, EntitySourceMask, EntityTag, EntityTagResult, ManualEntityMention,
    NerEntityRecord, NerPublicationResult, RegistryEntity, RegistryEntityDraft,
    RegistryEntityEditResult, MAX_ENTITIES,
};
pub use palette::{load_highlight_palette_or_default, save_highlight_palette_atomic};

use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
};

const FORMAT: &str = "phoenix.native.workspace/v1";
const MAX_ENTRIES: usize = 16_384;
const MAX_NAME_BYTES: usize = 96;
pub const ROOT_ID: EntryId = EntryId(1);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Folder,
    Note,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Note => "note",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceEntry {
    pub id: EntryId,
    pub parent: Option<EntryId>,
    pub kind: EntryKind,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceDocument {
    format: String,
    revision: u64,
    next_id: u64,
    #[serde(default)]
    active_entry: Option<EntryId>,
    entries: Vec<WorkspaceEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRow {
    pub id: EntryId,
    pub depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceCounts {
    pub folders: usize,
    pub notes: usize,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("LOCALAPPDATA is unavailable; refusing an implicit workspace location")]
    LocalDataUnavailable,
    #[error("workspace entry {0:?} does not exist")]
    MissingEntry(EntryId),
    #[error("workspace entry {0:?} is not a folder")]
    NotFolder(EntryId),
    #[error("workspace entry {0:?} is not a note")]
    NotNote(EntryId),
    #[error("the workspace root cannot be renamed or deleted")]
    RootIsImmutable,
    #[error("workspace name is empty")]
    EmptyName,
    #[error("workspace name exceeds {MAX_NAME_BYTES} UTF-8 bytes")]
    NameTooLong,
    #[error("workspace name contains a reserved filesystem character")]
    ReservedNameCharacter,
    #[error("a sibling named '{0}' already exists")]
    DuplicateName(String),
    #[error("workspace entry limit of {MAX_ENTRIES} reached")]
    EntryLimit,
    #[error("workspace manifest format '{0}' is unsupported")]
    UnsupportedFormat(String),
    #[error("workspace manifest invariant failed: {0}")]
    InvalidManifest(String),
    #[error("document content is {actual} bytes; maximum is {maximum}")]
    DocumentTooLarge { actual: usize, maximum: usize },
    #[error("document envelope for {0:?} is corrupt: {1}")]
    CorruptDocument(EntryId, &'static str),
    #[error("document envelope format version {0} is unsupported")]
    UnsupportedDocumentFormat(u32),
    #[error("entity registry format '{0}' is unsupported")]
    UnsupportedEntityRegistry(String),
    #[error("entity registry invariant failed: {0}")]
    InvalidEntityRegistry(String),
    #[error("entity registry limit reached")]
    EntityLimit,
    #[error("entity {0} is not present in the registry")]
    EntityNotFound(u64),
    #[error("manual entity identity space is exhausted")]
    EntityIdentityExhausted,
    #[error("entity mention registry limit reached")]
    EntityMentionLimit,
    #[error("entity registry revision exhausted")]
    EntityRegistryRevisionExhausted,
    #[error("no collision-free entity identity is available")]
    EntityIdExhausted,
    #[error("NER publication revision {incoming} is not newer than {current}")]
    StaleNerRevision { current: u64, incoming: u64 },
    #[error("NER publication contains {actual} entities; maximum is {maximum}")]
    NerBatchTooLarge { actual: usize, maximum: usize },
    #[error("NER publication repeats stable entity identity {0}")]
    DuplicateNerIdentity(u64),
    #[error("stable entity identity {0} conflicts with its user-tagged record")]
    EntityIdentityConflict(u64),
    #[error("NER entity {0} is invalid")]
    InvalidNerEntity(u64),
    #[error("entity selection is empty, stale, non-UTF-8-aligned, or mismatched")]
    InvalidEntitySelection,
    #[error("custom entity kinds must contain 1 to 64 UTF-8 bytes")]
    InvalidCustomEntityKind,
    #[error(
        "document lease for {entry:?} is stale: expected revision {expected}, current revision {actual}"
    )]
    StaleDocumentLease {
        entry: EntryId,
        expected: u64,
        actual: u64,
    },
    #[error("workspace I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("workspace JSON failed at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("atomic workspace replacement failed at {path}: {source}")]
    AtomicReplace {
        path: PathBuf,
        #[source]
        source: windows::core::Error,
    },
}

impl WorkspaceDocument {
    pub fn seeded() -> Self {
        Self {
            format: FORMAT.into(),
            revision: 1,
            next_id: 6,
            active_entry: Some(EntryId(3)),
            entries: vec![
                WorkspaceEntry {
                    id: ROOT_ID,
                    parent: None,
                    kind: EntryKind::Folder,
                    name: "Phoenix".into(),
                },
                WorkspaceEntry {
                    id: EntryId(2),
                    parent: Some(ROOT_ID),
                    kind: EntryKind::Folder,
                    name: "Notes".into(),
                },
                WorkspaceEntry {
                    id: EntryId(3),
                    parent: Some(EntryId(2)),
                    kind: EntryKind::Note,
                    name: "Welcome".into(),
                },
                WorkspaceEntry {
                    id: EntryId(4),
                    parent: Some(ROOT_ID),
                    kind: EntryKind::Folder,
                    name: "Research".into(),
                },
                WorkspaceEntry {
                    id: EntryId(5),
                    parent: Some(EntryId(4)),
                    kind: EntryKind::Note,
                    name: "Native graph renderer".into(),
                },
            ],
        }
    }

    pub fn load_or_seed(path: &Path) -> Result<Self, WorkspaceError> {
        if path.exists() {
            return Self::load(path);
        }
        let workspace = Self::seeded();
        workspace.save_atomic(path)?;
        Ok(workspace)
    }

    pub fn load(path: &Path) -> Result<Self, WorkspaceError> {
        let bytes = fs::read(path).map_err(|source| WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let workspace: Self =
            serde_json::from_slice(&bytes).map_err(|source| WorkspaceError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        workspace.validate()?;
        Ok(workspace)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), WorkspaceError> {
        self.validate()?;
        let parent = path.parent().ok_or_else(|| {
            WorkspaceError::InvalidManifest("workspace path has no parent".into())
        })?;
        fs::create_dir_all(parent).map_err(|source| WorkspaceError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| WorkspaceError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        let temp = temporary_path(path);
        let write_result = write_synced(&temp, &bytes);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        let replace_result = if path.exists() {
            replace_file(path, &temp)
        } else {
            move_new_file(path, &temp)
        };
        if replace_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        replace_result
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entry(&self, id: EntryId) -> Option<&WorkspaceEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn entries(&self) -> &[WorkspaceEntry] {
        &self.entries
    }

    pub fn active_entry(&self) -> Option<EntryId> {
        self.active_entry
    }

    pub fn remember_active_entry(&mut self, id: EntryId) -> Result<(), WorkspaceError> {
        if self.entry(id).is_none() {
            return Err(WorkspaceError::MissingEntry(id));
        }
        self.active_entry = Some(id);
        Ok(())
    }

    pub fn counts(&self) -> WorkspaceCounts {
        self.entries.iter().fold(
            WorkspaceCounts {
                folders: 0,
                notes: 0,
            },
            |mut counts, entry| {
                match entry.kind {
                    EntryKind::Folder => counts.folders += 1,
                    EntryKind::Note => counts.notes += 1,
                }
                counts
            },
        )
    }

    pub fn first_note(&self) -> Option<EntryId> {
        self.entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Note)
            .map(|entry| entry.id)
    }

    pub fn path_for(&self, id: EntryId) -> Result<String, WorkspaceError> {
        let mut names = Vec::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            let entry = self
                .entry(current)
                .ok_or(WorkspaceError::MissingEntry(current))?;
            names.push(entry.name.as_str());
            cursor = entry.parent;
        }
        names.reverse();
        Ok(names.join(" / "))
    }

    pub fn visible_rows(&self, expanded: &HashSet<EntryId>) -> Vec<WorkspaceRow> {
        let mut rows = Vec::with_capacity(self.entries.len());
        self.append_rows(ROOT_ID, 0, expanded, &mut rows);
        rows
    }

    pub fn create(
        &mut self,
        parent: EntryId,
        kind: EntryKind,
        name: &str,
    ) -> Result<EntryId, WorkspaceError> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(WorkspaceError::EntryLimit);
        }
        let parent_entry = self
            .entry(parent)
            .ok_or(WorkspaceError::MissingEntry(parent))?;
        if parent_entry.kind != EntryKind::Folder {
            return Err(WorkspaceError::NotFolder(parent));
        }
        let clean = validated_name(name)?;
        self.ensure_unique_name(parent, clean)?;
        let id = EntryId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| WorkspaceError::InvalidManifest("entry ID exhausted".into()))?;
        self.entries.push(WorkspaceEntry {
            id,
            parent: Some(parent),
            kind,
            name: clean.into(),
        });
        self.bump_revision()?;
        Ok(id)
    }

    pub fn rename(&mut self, id: EntryId, name: &str) -> Result<(), WorkspaceError> {
        if id == ROOT_ID {
            return Err(WorkspaceError::RootIsImmutable);
        }
        let clean = validated_name(name)?;
        let parent = self
            .entry(id)
            .ok_or(WorkspaceError::MissingEntry(id))?
            .parent
            .ok_or_else(|| {
                WorkspaceError::InvalidManifest("non-root entry has no parent".into())
            })?;
        self.ensure_unique_name_except(parent, clean, id)?;
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(WorkspaceError::MissingEntry(id))?;
        entry.name.clear();
        entry.name.push_str(clean);
        self.bump_revision()
    }

    pub fn delete(&mut self, id: EntryId) -> Result<usize, WorkspaceError> {
        if id == ROOT_ID {
            return Err(WorkspaceError::RootIsImmutable);
        }
        if self.entry(id).is_none() {
            return Err(WorkspaceError::MissingEntry(id));
        }
        let mut removed = HashSet::new();
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            if !removed.insert(current) {
                continue;
            }
            pending.extend(
                self.entries
                    .iter()
                    .filter(|entry| entry.parent == Some(current))
                    .map(|entry| entry.id),
            );
        }
        self.entries.retain(|entry| !removed.contains(&entry.id));
        if self
            .active_entry
            .is_some_and(|active| removed.contains(&active))
        {
            self.active_entry = None;
        }
        self.bump_revision()?;
        Ok(removed.len())
    }

    pub fn parent_for_create(&self, selected: EntryId) -> Result<EntryId, WorkspaceError> {
        let entry = self
            .entry(selected)
            .ok_or(WorkspaceError::MissingEntry(selected))?;
        match entry.kind {
            EntryKind::Folder => Ok(entry.id),
            EntryKind::Note => entry
                .parent
                .ok_or_else(|| WorkspaceError::InvalidManifest("note has no parent folder".into())),
        }
    }

    pub fn validate(&self) -> Result<(), WorkspaceError> {
        if self.format != FORMAT {
            return Err(WorkspaceError::UnsupportedFormat(self.format.clone()));
        }
        if self.entries.is_empty() || self.entries.len() > MAX_ENTRIES {
            return Err(WorkspaceError::InvalidManifest(
                "entry count is outside the supported range".into(),
            ));
        }
        let mut by_id = HashMap::with_capacity(self.entries.len());
        for entry in &self.entries {
            if by_id.insert(entry.id, entry).is_some() {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "duplicate entry ID {:?}",
                    entry.id
                )));
            }
            validated_name(&entry.name)?;
        }
        let root = by_id
            .get(&ROOT_ID)
            .ok_or_else(|| WorkspaceError::InvalidManifest("root entry is missing".into()))?;
        if root.parent.is_some() || root.kind != EntryKind::Folder {
            return Err(WorkspaceError::InvalidManifest(
                "root must be a parentless folder".into(),
            ));
        }
        let mut sibling_names: HashSet<(EntryId, String)> =
            HashSet::with_capacity(self.entries.len());
        for entry in &self.entries {
            if entry.id == ROOT_ID {
                continue;
            }
            let parent = entry.parent.ok_or_else(|| {
                WorkspaceError::InvalidManifest(format!("entry {:?} has no parent", entry.id))
            })?;
            let parent_entry = by_id.get(&parent).ok_or_else(|| {
                WorkspaceError::InvalidManifest(format!(
                    "entry {:?} refers to missing parent {:?}",
                    entry.id, parent
                ))
            })?;
            if parent_entry.kind != EntryKind::Folder {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "entry {:?} has non-folder parent {:?}",
                    entry.id, parent
                )));
            }
            if !sibling_names.insert((parent, entry.name.to_lowercase())) {
                return Err(WorkspaceError::DuplicateName(entry.name.clone()));
            }
            self.validate_ancestry(entry.id, &by_id)?;
        }
        let max_id = self
            .entries
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0);
        if self.next_id <= max_id {
            return Err(WorkspaceError::InvalidManifest(
                "next_id does not exceed every stored ID".into(),
            ));
        }
        if self
            .active_entry
            .is_some_and(|active| !by_id.contains_key(&active))
        {
            return Err(WorkspaceError::InvalidManifest(
                "active entry does not exist".into(),
            ));
        }
        Ok(())
    }

    fn append_rows(
        &self,
        id: EntryId,
        depth: usize,
        expanded: &HashSet<EntryId>,
        rows: &mut Vec<WorkspaceRow>,
    ) {
        rows.push(WorkspaceRow { id, depth });
        if !expanded.contains(&id) {
            return;
        }
        let mut children: Vec<&WorkspaceEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.parent == Some(id))
            .collect();
        children.sort_unstable_by(|left, right| {
            kind_rank(left.kind)
                .cmp(&kind_rank(right.kind))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        for child in children {
            self.append_rows(child.id, depth + 1, expanded, rows);
        }
    }

    fn ensure_unique_name(&self, parent: EntryId, name: &str) -> Result<(), WorkspaceError> {
        self.ensure_unique_name_except(parent, name, EntryId(0))
    }

    fn ensure_unique_name_except(
        &self,
        parent: EntryId,
        name: &str,
        except: EntryId,
    ) -> Result<(), WorkspaceError> {
        if self.entries.iter().any(|entry| {
            entry.id != except
                && entry.parent == Some(parent)
                && entry.name.eq_ignore_ascii_case(name)
        }) {
            return Err(WorkspaceError::DuplicateName(name.into()));
        }
        Ok(())
    }

    fn validate_ancestry(
        &self,
        id: EntryId,
        by_id: &HashMap<EntryId, &WorkspaceEntry>,
    ) -> Result<(), WorkspaceError> {
        let mut seen = HashSet::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            if !seen.insert(current) {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "cycle detected at entry {:?}",
                    current
                )));
            }
            cursor = by_id.get(&current).and_then(|entry| entry.parent);
        }
        if !seen.contains(&ROOT_ID) {
            return Err(WorkspaceError::InvalidManifest(format!(
                "entry {:?} is not rooted at {:?}",
                id, ROOT_ID
            )));
        }
        Ok(())
    }

    fn bump_revision(&mut self) -> Result<(), WorkspaceError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| WorkspaceError::InvalidManifest("revision exhausted".into()))?;
        Ok(())
    }
}

pub fn default_workspace_path() -> Result<PathBuf, WorkspaceError> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or(WorkspaceError::LocalDataUnavailable)?;
    Ok(PathBuf::from(local)
        .join("Phoenix")
        .join("NativeShell")
        .join("workspace-v1.json"))
}

fn validated_name(name: &str) -> Result<&str, WorkspaceError> {
    let clean = name.trim();
    if clean.is_empty() {
        return Err(WorkspaceError::EmptyName);
    }
    if clean.len() > MAX_NAME_BYTES {
        return Err(WorkspaceError::NameTooLong);
    }
    if clean
        .chars()
        .any(|character| character.is_control() || r#"/\<>:"|?*"#.contains(character))
    {
        return Err(WorkspaceError::ReservedNameCharacter);
    }
    Ok(clean)
}

fn kind_rank(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Folder => 0,
        EntryKind::Note => 1,
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(temp)
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), WorkspaceError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|source| WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|source| WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn replace_file(destination: &Path, replacement: &Path) -> Result<(), WorkspaceError> {
    let destination_wide = wide_null(destination.as_os_str());
    let replacement_wide = wide_null(replacement.as_os_str());
    // SAFETY: Both UTF-16 buffers are NUL terminated and live for the duration of the call.
    unsafe {
        ReplaceFileW(
            PCWSTR(destination_wide.as_ptr()),
            PCWSTR(replacement_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|source| WorkspaceError::AtomicReplace {
        path: destination.to_path_buf(),
        source,
    })
}

fn move_new_file(destination: &Path, replacement: &Path) -> Result<(), WorkspaceError> {
    let destination_wide = wide_null(destination.as_os_str());
    let replacement_wide = wide_null(replacement.as_os_str());
    // SAFETY: Both UTF-16 buffers are NUL terminated and live for the duration of the call.
    unsafe {
        MoveFileExW(
            PCWSTR(replacement_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|source| WorkspaceError::AtomicReplace {
        path: destination.to_path_buf(),
        source,
    })
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temp_manifest() -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "phoenix-native-workspace-test-{}-{sequence}",
                std::process::id()
            ))
            .join("workspace.json")
    }

    #[test]
    fn crud_roundtrip_is_durable_and_recursive() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_manifest();
        let mut workspace = WorkspaceDocument::seeded();
        let folder = workspace.create(ROOT_ID, EntryKind::Folder, "Drafts")?;
        let note = workspace.create(folder, EntryKind::Note, "Chapter 01")?;
        workspace.rename(note, "Chapter 1")?;
        workspace.remember_active_entry(note)?;
        workspace.save_atomic(&path)?;

        let mut reopened = WorkspaceDocument::load(&path)?;
        assert_eq!(reopened.active_entry(), Some(note));
        assert_eq!(reopened.path_for(note)?, "Phoenix / Drafts / Chapter 1");
        assert_eq!(reopened.delete(folder)?, 2);
        assert_eq!(reopened.active_entry(), None);
        reopened.save_atomic(&path)?;

        let final_state = WorkspaceDocument::load(&path)?;
        assert!(final_state.entry(folder).is_none());
        assert!(final_state.entry(note).is_none());
        let _ = fs::remove_dir_all(path.parent().ok_or("test path has no parent")?);
        Ok(())
    }

    #[test]
    fn rejects_duplicates_cycles_and_root_mutation() -> Result<(), Box<dyn std::error::Error>> {
        let mut workspace = WorkspaceDocument::seeded();
        let duplicate = workspace.create(ROOT_ID, EntryKind::Folder, "notes");
        assert!(matches!(duplicate, Err(WorkspaceError::DuplicateName(_))));
        assert!(matches!(
            workspace.rename(ROOT_ID, "Elsewhere"),
            Err(WorkspaceError::RootIsImmutable)
        ));
        assert!(matches!(
            workspace.delete(ROOT_ID),
            Err(WorkspaceError::RootIsImmutable)
        ));

        workspace.entries[1].parent = Some(workspace.entries[1].id);
        assert!(matches!(
            workspace.validate(),
            Err(WorkspaceError::InvalidManifest(_))
        ));
        Ok(())
    }

    #[test]
    fn visible_rows_obey_expansion_without_losing_identity() {
        let workspace = WorkspaceDocument::seeded();
        let mut expanded = HashSet::new();
        expanded.insert(ROOT_ID);
        expanded.insert(EntryId(2));

        let rows = workspace.visible_rows(&expanded);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![ROOT_ID, EntryId(2), EntryId(3), EntryId(4)]
        );
    }
}
