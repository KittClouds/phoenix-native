use super::*;
use crate::{commit_document, open_document, EntryKind, WorkspaceDocument, ROOT_ID};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[test]
fn v1_registry_without_source_fields_migrates_in_memory() -> Result<(), WorkspaceError> {
    let path = std::env::temp_dir()
        .join(format!(
            "phoenix-entity-migration-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
        .join("workspace.json");
    let mut workspace = WorkspaceDocument::seeded();
    let note = workspace.create(ROOT_ID, EntryKind::Note, "Migration")?;
    workspace.save_atomic(&path)?;
    let lease = open_document(&path, &workspace, note)?;
    let lease = commit_document(&path, &workspace, lease.token(), "Ryan")?;
    let registry_file = registry_path(&path)?;
    let legacy = serde_json::json!({
        "format": REGISTRY_FORMAT,
        "revision": 9,
        "entities": [{
            "id": 42,
            "label": "Ryan",
            "kind": "character",
            "custom_kind": null,
            "origin_document": lease.entry_id,
        }],
        "mentions": [],
    });
    fs::write(
        &registry_file,
        serde_json::to_vec(&legacy).map_err(|source| WorkspaceError::Json {
            path: registry_file.clone(),
            source,
        })?,
    )
    .map_err(|source| WorkspaceError::Io {
        path: registry_file,
        source,
    })?;
    let migrated = EntityRegistry::load_or_empty(&path)?;
    assert_eq!(migrated.ner_revision(), 0);
    assert_eq!(
        migrated.entities()[0].sources,
        EntitySourceMask::USER_TAGGED
    );
    assert_eq!(migrated.entities()[0].origin_document, Some(lease.entry_id));
    let _ = fs::remove_dir_all(path.parent().expect("fixture parent"));
    Ok(())
}

#[test]
fn equal_labels_at_different_selections_never_merge() -> Result<(), WorkspaceError> {
    let path = std::env::temp_dir()
        .join(format!(
            "phoenix-entity-identity-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
        .join("workspace.json");
    let mut workspace = WorkspaceDocument::seeded();
    let note = workspace.create(ROOT_ID, EntryKind::Note, "Identity")?;
    workspace.save_atomic(&path)?;
    let lease = open_document(&path, &workspace, note)?;
    let lease = commit_document(&path, &workspace, lease.token(), "Ryan met Ryan.")?;
    let mut registry = EntityRegistry::empty();
    let first = registry.tag(
        &lease,
        EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    )?;
    let second = registry.tag(
        &lease,
        EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 9,
            end: 13,
            surface: "Ryan".into(),
        },
    )?;
    assert_ne!(first.entity_id, second.entity_id);
    assert_eq!(registry.entities().len(), 2);
    let _ = fs::remove_dir_all(path.parent().expect("fixture parent"));
    Ok(())
}
