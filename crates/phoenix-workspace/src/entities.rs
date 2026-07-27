use super::{
    move_new_file, replace_file, temporary_path, write_synced, DocumentLease, EntryId,
    WorkspaceError,
};
use hashbrown::HashMap;
use phoenix_scene_contract::EntityKind;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_FORMAT: &str = "phoenix.native.entity-registry/v1";
const REGISTRY_FILE: &str = "entity-registry-v1.json";
pub const MAX_ENTITIES: usize = 100_000;
const MAX_MENTIONS: usize = 1_000_000;
const MAX_SURFACE_BYTES: usize = 512;
const MAX_CUSTOM_KIND_BYTES: usize = 64;
const CONTEXT_CHARS: usize = 32;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntitySourceMask {
    pub ner: bool,
    pub user_tagged: bool,
}

impl EntitySourceMask {
    pub const USER_TAGGED: Self = Self {
        ner: false,
        user_tagged: true,
    };

    pub const NER: Self = Self {
        ner: true,
        user_tagged: false,
    };

    pub const fn is_empty(self) -> bool {
        !self.ner && !self.user_tagged
    }
}

const fn legacy_user_source() -> EntitySourceMask {
    EntitySourceMask::USER_TAGGED
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryEntity {
    pub id: u64,
    pub label: String,
    pub kind: EntityKind,
    pub custom_kind: Option<String>,
    pub origin_document: Option<EntryId>,
    #[serde(default = "legacy_user_source")]
    pub sources: EntitySourceMask,
    #[serde(default)]
    pub ner_mention_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NerEntityRecord {
    pub stable_id: u64,
    pub label: String,
    pub kind: EntityKind,
    pub custom_kind: Option<String>,
    pub mention_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NerPublicationResult {
    pub ner_revision: u64,
    pub registry_revision: u64,
    pub ner_entities: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualEntityMention {
    pub entity_id: u64,
    pub document: EntryId,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub start: u32,
    pub end: u32,
    pub surface: String,
    pub prefix: String,
    pub suffix: String,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityRegistry {
    format: String,
    revision: u64,
    #[serde(default)]
    ner_revision: u64,
    entities: Vec<RegistryEntity>,
    mentions: Vec<ManualEntityMention>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityTag {
    pub kind: EntityKind,
    pub custom_kind: Option<String>,
    pub start: u32,
    pub end: u32,
    pub surface: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityTagResult {
    pub entity_id: u64,
    pub is_new: bool,
    pub registry_revision: u64,
}

impl EntityRegistry {
    pub fn empty() -> Self {
        Self {
            format: REGISTRY_FORMAT.into(),
            revision: 1,
            ner_revision: 0,
            entities: Vec::new(),
            mentions: Vec::new(),
        }
    }

    pub fn load_or_empty(workspace_path: &Path) -> Result<Self, WorkspaceError> {
        let path = registry_path(workspace_path)?;
        if !path.exists() {
            return Ok(Self::empty());
        }
        let bytes = fs::read(&path).map_err(|source| WorkspaceError::Io {
            path: path.clone(),
            source,
        })?;
        let registry = serde_json::from_slice::<Self>(&bytes)
            .map_err(|source| WorkspaceError::Json { path, source })?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn save_atomic(&self, workspace_path: &Path) -> Result<(), WorkspaceError> {
        self.validate()?;
        let path = registry_path(workspace_path)?;
        let parent = path
            .parent()
            .ok_or_else(|| WorkspaceError::InvalidEntityRegistry("path has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|source| WorkspaceError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let bytes = serde_json::to_vec(self).map_err(|source| WorkspaceError::Json {
            path: path.clone(),
            source,
        })?;
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
        result
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn ner_revision(&self) -> u64 {
        self.ner_revision
    }

    pub fn entities(&self) -> &[RegistryEntity] {
        &self.entities
    }

    pub fn mentions(&self) -> &[ManualEntityMention] {
        &self.mentions
    }

    pub fn active_mentions_for<'a>(
        &'a self,
        lease: &'a DocumentLease,
    ) -> impl Iterator<Item = (&'a ManualEntityMention, &'a RegistryEntity)> + 'a {
        self.mentions
            .iter()
            .filter(move |mention| {
                mention.active
                    && mention.document == lease.entry_id
                    && mention.document_revision == lease.revision.0
                    && mention.content_hash == lease.content_hash.0
            })
            .filter_map(|mention| {
                self.entities
                    .iter()
                    .find(|entity| entity.id == mention.entity_id)
                    .map(|entity| (mention, entity))
            })
    }

    pub fn tag(
        &mut self,
        lease: &DocumentLease,
        mut tag: EntityTag,
    ) -> Result<EntityTagResult, WorkspaceError> {
        normalize_tag(lease, &mut tag)?;
        let existing_index = self
            .mentions
            .iter()
            .find(|mention| {
                mention.active
                    && mention.document == lease.entry_id
                    && mention.start == tag.start
                    && mention.end == tag.end
            })
            .and_then(|mention| {
                self.entities
                    .iter()
                    .position(|entity| entity.id == mention.entity_id)
            });
        let (entity_id, is_new) = if let Some(index) = existing_index {
            let entity = &mut self.entities[index];
            entity.label.clone_from(&tag.surface);
            entity.kind = tag.kind;
            entity.custom_kind.clone_from(&tag.custom_kind);
            entity.sources.user_tagged = true;
            (entity.id, false)
        } else {
            if self.entities.len() >= MAX_ENTITIES {
                return Err(WorkspaceError::EntityLimit);
            }
            let id = stable_entity_id(
                lease.entry_id,
                tag.start,
                tag.end,
                self.revision,
                &self.entities,
            )?;
            self.entities.push(RegistryEntity {
                id,
                label: tag.surface.clone(),
                kind: tag.kind,
                custom_kind: tag.custom_kind.clone(),
                origin_document: Some(lease.entry_id),
                sources: EntitySourceMask::USER_TAGGED,
                ner_mention_count: 0,
            });
            (id, true)
        };

        self.mentions.retain(|mention| {
            if mention.document != lease.entry_id {
                return true;
            }
            let overlaps = mention.active && tag.start < mention.end && mention.start < tag.end;
            let duplicate = mention.entity_id == entity_id
                && mention.document == lease.entry_id
                && mention.start == tag.start
                && mention.end == tag.end;
            !overlaps && !duplicate
        });
        if self.mentions.len() >= MAX_MENTIONS {
            return Err(WorkspaceError::EntityMentionLimit);
        }
        let (prefix, suffix) = quote_context(&lease.content, tag.start as usize, tag.end as usize);
        self.mentions.push(ManualEntityMention {
            entity_id,
            document: lease.entry_id,
            document_revision: lease.revision.0,
            content_hash: lease.content_hash.0,
            start: tag.start,
            end: tag.end,
            surface: tag.surface,
            prefix,
            suffix,
            active: true,
        });
        self.bump_revision()?;
        self.sort();
        Ok(EntityTagResult {
            entity_id,
            is_new,
            registry_revision: self.revision,
        })
    }

    pub fn publish_ner(
        &mut self,
        ner_revision: u64,
        records: &[NerEntityRecord],
    ) -> Result<NerPublicationResult, WorkspaceError> {
        self.publish_ner_scoped(None, ner_revision, records)
    }

    pub fn publish_document_ner(
        &mut self,
        document: EntryId,
        ner_revision: u64,
        records: &[NerEntityRecord],
    ) -> Result<NerPublicationResult, WorkspaceError> {
        self.publish_ner_scoped(Some(document), ner_revision, records)
    }

    fn publish_ner_scoped(
        &mut self,
        document: Option<EntryId>,
        ner_revision: u64,
        records: &[NerEntityRecord],
    ) -> Result<NerPublicationResult, WorkspaceError> {
        if ner_revision <= self.ner_revision {
            return Err(WorkspaceError::StaleNerRevision {
                current: self.ner_revision,
                incoming: ner_revision,
            });
        }
        if records.len() > MAX_ENTITIES {
            return Err(WorkspaceError::NerBatchTooLarge {
                actual: records.len(),
                maximum: MAX_ENTITIES,
            });
        }

        let mut incoming = records.to_vec();
        incoming.sort_unstable_by_key(|record| record.stable_id);
        for pair in incoming.windows(2) {
            if pair[0].stable_id == pair[1].stable_id {
                return Err(WorkspaceError::DuplicateNerIdentity(pair[0].stable_id));
            }
        }
        let user_identities = self
            .entities
            .iter()
            .filter(|entity| entity.sources.user_tagged)
            .map(|entity| (entity.id, entity))
            .collect::<HashMap<_, _>>();
        for record in &incoming {
            validate_ner_record(record)?;
            if self.entities.iter().any(|entity| {
                entity.id == record.stable_id
                    && entity.sources.ner
                    && document.is_some()
                    && entity.origin_document != document
            }) {
                return Err(WorkspaceError::EntityIdentityConflict(record.stable_id));
            }
            if let Some(entity) = user_identities.get(&record.stable_id) {
                if entity.label != record.label
                    || entity.kind != record.kind
                    || entity.custom_kind != record.custom_kind
                {
                    return Err(WorkspaceError::EntityIdentityConflict(record.stable_id));
                }
            }
        }
        let user_entity_count = user_identities.len();
        let incoming_without_user_identity = incoming
            .iter()
            .filter(|record| !user_identities.contains_key(&record.stable_id))
            .count();
        if user_entity_count
            .checked_add(incoming_without_user_identity)
            .is_none_or(|count| count > MAX_ENTITIES)
        {
            return Err(WorkspaceError::EntityLimit);
        }
        let next_registry_revision = self
            .revision
            .checked_add(1)
            .ok_or(WorkspaceError::EntityRegistryRevisionExhausted)?;
        drop(user_identities);

        for entity in &mut self.entities {
            if document.is_none() || entity.origin_document == document {
                entity.sources.ner = false;
                entity.ner_mention_count = 0;
            }
        }
        for record in incoming {
            if let Some(entity) = self
                .entities
                .iter_mut()
                .find(|entity| entity.id == record.stable_id)
            {
                entity.label = record.label;
                entity.kind = record.kind;
                entity.custom_kind = record.custom_kind;
                entity.sources.ner = true;
                entity.ner_mention_count = record.mention_count;
            } else {
                self.entities.push(RegistryEntity {
                    id: record.stable_id,
                    label: record.label,
                    kind: record.kind,
                    custom_kind: record.custom_kind,
                    origin_document: document,
                    sources: EntitySourceMask::NER,
                    ner_mention_count: record.mention_count,
                });
            }
        }
        self.entities.retain(|entity| !entity.sources.is_empty());
        self.ner_revision = ner_revision;
        self.revision = next_registry_revision;
        self.sort();
        Ok(NerPublicationResult {
            ner_revision,
            registry_revision: self.revision,
            ner_entities: self
                .entities
                .iter()
                .filter(|entity| entity.sources.ner)
                .count(),
        })
    }

    pub fn reanchor_document(&mut self, lease: &DocumentLease) -> Result<bool, WorkspaceError> {
        let mut changed = false;
        for mention in self
            .mentions
            .iter_mut()
            .filter(|mention| mention.document == lease.entry_id)
        {
            let located = locate_mention(&lease.content, mention);
            let next_active = located.is_some();
            if let Some((start, end)) = located {
                let (prefix, suffix) = quote_context(&lease.content, start, end);
                changed |= mention.start != start as u32
                    || mention.end != end as u32
                    || mention.document_revision != lease.revision.0
                    || mention.content_hash != lease.content_hash.0
                    || !mention.active;
                mention.start = start as u32;
                mention.end = end as u32;
                mention.document_revision = lease.revision.0;
                mention.content_hash = lease.content_hash.0;
                mention.prefix = prefix;
                mention.suffix = suffix;
            } else {
                changed |= mention.active;
            }
            mention.active = next_active;
        }
        if changed {
            self.bump_revision()?;
            self.sort();
        }
        Ok(changed)
    }

    fn validate(&self) -> Result<(), WorkspaceError> {
        if self.format != REGISTRY_FORMAT {
            return Err(WorkspaceError::UnsupportedEntityRegistry(
                self.format.clone(),
            ));
        }
        if self.revision == 0
            || self.entities.len() > MAX_ENTITIES
            || self.mentions.len() > MAX_MENTIONS
        {
            return Err(WorkspaceError::InvalidEntityRegistry(
                "invalid revision or count".into(),
            ));
        }
        for entity in &self.entities {
            validate_kind(entity.kind, entity.custom_kind.as_deref())?;
            if entity.id == 0
                || entity.label.trim().is_empty()
                || entity.label.len() > MAX_SURFACE_BYTES
                || entity.sources.is_empty()
                || (!entity.sources.ner && entity.ner_mention_count != 0)
            {
                return Err(WorkspaceError::InvalidEntityRegistry(
                    "invalid entity record".into(),
                ));
            }
        }
        for mention in &self.mentions {
            let start = mention.start as usize;
            let end = mention.end as usize;
            if start >= end
                || mention.surface.is_empty()
                || mention.surface.len() > MAX_SURFACE_BYTES
                || !self
                    .entities
                    .iter()
                    .any(|entity| entity.id == mention.entity_id)
            {
                return Err(WorkspaceError::InvalidEntityRegistry(
                    "invalid mention record".into(),
                ));
            }
        }
        Ok(())
    }

    fn bump_revision(&mut self) -> Result<(), WorkspaceError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(WorkspaceError::EntityRegistryRevisionExhausted)?;
        Ok(())
    }

    fn sort(&mut self) {
        self.entities.sort_unstable_by_key(|entity| entity.id);
        self.mentions.sort_unstable_by_key(|mention| {
            (
                mention.document.0,
                mention.start,
                mention.end,
                mention.entity_id,
            )
        });
    }
}

fn normalize_tag(lease: &DocumentLease, tag: &mut EntityTag) -> Result<(), WorkspaceError> {
    validate_kind(tag.kind, tag.custom_kind.as_deref())?;
    let start = tag.start as usize;
    let end = tag.end as usize;
    if start >= end
        || end > lease.content.len()
        || !lease.content.is_char_boundary(start)
        || !lease.content.is_char_boundary(end)
        || lease.content[start..end] != tag.surface
        || tag.surface.len() > MAX_SURFACE_BYTES
        || tag.surface.trim().is_empty()
        || tag.surface != tag.surface.trim()
    {
        return Err(WorkspaceError::InvalidEntitySelection);
    }
    if tag.kind == EntityKind::Custom {
        tag.custom_kind = tag
            .custom_kind
            .take()
            .map(|value| value.trim().to_uppercase());
    } else {
        tag.custom_kind = None;
    }
    Ok(())
}

fn validate_kind(kind: EntityKind, custom: Option<&str>) -> Result<(), WorkspaceError> {
    match (
        kind,
        custom.map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (EntityKind::Custom, Some(value)) if value.len() <= MAX_CUSTOM_KIND_BYTES => Ok(()),
        (EntityKind::Custom, _) => Err(WorkspaceError::InvalidCustomEntityKind),
        (_, None) => Ok(()),
        (_, Some(_)) => Err(WorkspaceError::InvalidCustomEntityKind),
    }
}

fn stable_entity_id(
    document: EntryId,
    start: u32,
    end: u32,
    registry_revision: u64,
    entities: &[RegistryEntity],
) -> Result<u64, WorkspaceError> {
    for salt in 0u32..=u16::MAX as u32 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix.native.user-entity/v2\0");
        hasher.update(&document.0.to_le_bytes());
        hasher.update(&start.to_le_bytes());
        hasher.update(&end.to_le_bytes());
        hasher.update(&registry_revision.to_le_bytes());
        hasher.update(&salt.to_le_bytes());
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
        let id = u64::from_le_bytes(raw).max(1);
        if entities.iter().all(|entity| entity.id != id) {
            return Ok(id);
        }
    }
    Err(WorkspaceError::EntityIdExhausted)
}

fn validate_ner_record(record: &NerEntityRecord) -> Result<(), WorkspaceError> {
    validate_kind(record.kind, record.custom_kind.as_deref())?;
    if record.stable_id == 0
        || record.label.trim().is_empty()
        || record.label != record.label.trim()
        || record.label.len() > MAX_SURFACE_BYTES
        || record.mention_count == 0
    {
        return Err(WorkspaceError::InvalidNerEntity(record.stable_id));
    }
    Ok(())
}

fn quote_context(content: &str, start: usize, end: usize) -> (String, String) {
    let prefix = content[..start]
        .chars()
        .rev()
        .take(CONTEXT_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let suffix = content[end..].chars().take(CONTEXT_CHARS).collect();
    (prefix, suffix)
}

fn locate_mention(content: &str, mention: &ManualEntityMention) -> Option<(usize, usize)> {
    let start = mention.start as usize;
    let end = mention.end as usize;
    if end <= content.len()
        && content.is_char_boundary(start)
        && content.is_char_boundary(end)
        && content.get(start..end) == Some(mention.surface.as_str())
    {
        return Some((start, end));
    }
    let matches = content
        .match_indices(&mention.surface)
        .map(|(start, surface)| (start, start + surface.len()))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        return matches.first().copied();
    }
    let contextual = matches
        .into_iter()
        .filter(|(start, end)| {
            let (prefix, suffix) = quote_context(content, *start, *end);
            prefix.ends_with(&mention.prefix) && suffix.starts_with(&mention.suffix)
        })
        .collect::<Vec<_>>();
    (contextual.len() == 1).then(|| contextual[0])
}

fn registry_path(workspace_path: &Path) -> Result<PathBuf, WorkspaceError> {
    workspace_path
        .parent()
        .map(|parent| parent.join(REGISTRY_FILE))
        .ok_or_else(|| WorkspaceError::InvalidEntityRegistry("workspace path has no parent".into()))
}

#[cfg(test)]
#[path = "entities_migration_tests.rs"]
mod migration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commit_document, open_document, EntryKind, WorkspaceDocument, ROOT_ID};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> Result<(PathBuf, WorkspaceDocument, DocumentLease), WorkspaceError> {
        let path = std::env::temp_dir()
            .join(format!(
                "phoenix-entity-registry-test-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ))
            .join("workspace.json");
        let mut workspace = WorkspaceDocument::seeded();
        let note = workspace.create(ROOT_ID, EntryKind::Note, "Entities")?;
        workspace.save_atomic(&path)?;
        let lease = open_document(&path, &workspace, note)?;
        let lease = commit_document(&path, &workspace, lease.token(), "Ryan entered New Rome.")?;
        Ok((path, workspace, lease))
    }

    #[test]
    fn manual_tag_reuses_exact_selection_identity_and_is_restart_durable(
    ) -> Result<(), WorkspaceError> {
        let (path, _workspace, lease) = fixture()?;
        let mut registry = EntityRegistry::empty();
        let start = lease
            .content
            .find("Ryan")
            .ok_or(WorkspaceError::InvalidEntitySelection)?;
        let first = registry.tag(
            &lease,
            EntityTag {
                kind: EntityKind::Character,
                custom_kind: None,
                start: start as u32,
                end: (start + 4) as u32,
                surface: "Ryan".into(),
            },
        )?;
        let second = registry.tag(
            &lease,
            EntityTag {
                kind: EntityKind::Npc,
                custom_kind: None,
                start: start as u32,
                end: (start + 4) as u32,
                surface: "Ryan".into(),
            },
        )?;
        assert_eq!(first.entity_id, second.entity_id);
        assert!(!second.is_new);
        registry.save_atomic(&path)?;
        let reopened = EntityRegistry::load_or_empty(&path)?;
        assert_eq!(reopened.entities()[0].kind, EntityKind::Npc);
        assert_eq!(reopened.active_mentions_for(&lease).count(), 1);
        let _ = fs::remove_dir_all(path.parent().expect("fixture parent"));
        Ok(())
    }

    #[test]
    fn ner_publication_is_revisioned_deterministic_and_transactional() -> Result<(), WorkspaceError>
    {
        let mut registry = EntityRegistry::empty();
        let records = [
            NerEntityRecord {
                stable_id: 22,
                label: "New Rome".into(),
                kind: EntityKind::Location,
                custom_kind: None,
                mention_count: 4,
            },
            NerEntityRecord {
                stable_id: 11,
                label: "Ryan".into(),
                kind: EntityKind::Character,
                custom_kind: None,
                mention_count: 7,
            },
        ];
        let result = registry.publish_ner(3, &records)?;
        assert_eq!(result.ner_entities, 2);
        assert_eq!(
            registry
                .entities()
                .iter()
                .map(|entity| entity.id)
                .collect::<Vec<_>>(),
            vec![11, 22]
        );
        let before = registry.clone();
        let duplicate = [
            records[0].clone(),
            NerEntityRecord {
                stable_id: records[0].stable_id,
                ..records[1].clone()
            },
        ];
        assert!(matches!(
            registry.publish_ner(4, &duplicate),
            Err(WorkspaceError::DuplicateNerIdentity(22))
        ));
        assert_eq!(registry.revision(), before.revision());
        assert_eq!(registry.entities(), before.entities());
        assert!(matches!(
            registry.publish_ner(3, &records),
            Err(WorkspaceError::StaleNerRevision {
                current: 3,
                incoming: 3
            })
        ));
        Ok(())
    }

    #[test]
    fn document_scoped_ner_publication_preserves_other_documents() -> Result<(), WorkspaceError> {
        let mut registry = EntityRegistry::empty();
        let document_a = EntryId(41);
        let document_b = EntryId(42);
        let record_a = NerEntityRecord {
            stable_id: 101,
            label: "Ryan".into(),
            kind: EntityKind::Character,
            custom_kind: None,
            mention_count: 7,
        };
        let record_b = NerEntityRecord {
            stable_id: 202,
            label: "New Rome".into(),
            kind: EntityKind::Location,
            custom_kind: None,
            mention_count: 4,
        };

        registry.publish_document_ner(document_a, 2, std::slice::from_ref(&record_a))?;
        registry.publish_document_ner(document_b, 3, std::slice::from_ref(&record_b))?;
        assert_eq!(registry.entities().len(), 2);
        assert!(registry.entities().iter().any(|entity| {
            entity.id == record_a.stable_id && entity.origin_document == Some(document_a)
        }));
        assert!(registry.entities().iter().any(|entity| {
            entity.id == record_b.stable_id && entity.origin_document == Some(document_b)
        }));

        let updated_a = NerEntityRecord {
            mention_count: 9,
            ..record_a
        };
        registry.publish_document_ner(document_a, 4, std::slice::from_ref(&updated_a))?;
        assert_eq!(registry.entities().len(), 2);
        assert!(registry.entities().iter().any(|entity| {
            entity.id == updated_a.stable_id
                && entity.origin_document == Some(document_a)
                && entity.ner_mention_count == 9
        }));
        assert!(registry.entities().iter().any(|entity| {
            entity.id == record_b.stable_id
                && entity.origin_document == Some(document_b)
                && entity.ner_mention_count == 4
        }));
        Ok(())
    }

    #[test]
    fn reanchor_is_contextual_and_ambiguous_matches_fail_closed() -> Result<(), WorkspaceError> {
        let (path, workspace, lease) = fixture()?;
        let start = lease
            .content
            .find("New Rome")
            .ok_or(WorkspaceError::InvalidEntitySelection)?;
        let mut registry = EntityRegistry::empty();
        registry.tag(
            &lease,
            EntityTag {
                kind: EntityKind::Location,
                custom_kind: None,
                start: start as u32,
                end: (start + 8) as u32,
                surface: "New Rome".into(),
            },
        )?;
        let moved = commit_document(
            &path,
            &workspace,
            lease.token(),
            "Yesterday, Ryan entered New Rome.",
        )?;
        assert!(registry.reanchor_document(&moved)?);
        assert_eq!(registry.active_mentions_for(&moved).count(), 1);
        let ambiguous = commit_document(&path, &workspace, moved.token(), "New Rome and New Rome")?;
        assert!(registry.reanchor_document(&ambiguous)?);
        assert_eq!(registry.active_mentions_for(&ambiguous).count(), 0);
        let _ = fs::remove_dir_all(path.parent().expect("fixture parent"));
        Ok(())
    }
}
