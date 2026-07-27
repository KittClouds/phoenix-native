use crate::{
    compile_active_document, NativeSceneCompilerError, NativeSceneCompilerInput,
    NATIVE_SCENE_COMPILER_CONTRACT,
};
use glam::Vec3;
use phoenix_scene_archive::ArchiveManifold;
use phoenix_scene_contract::{
    CapsRole, EntityKind, HighlightPalette, RelationFamily, CHUNK_NODE_KIND, DOCUMENT_NODE_KIND,
    EPISODE_NODE_KIND, EVIDENCE_NODE_KIND,
};
use phoenix_scene_publisher::ScenePublicationKind;
use phoenix_workspace::{
    commit_document, open_document, EntityRegistry, EntityTag, WorkspaceDocument,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn compiles_canonical_mentions_into_one_deterministic_full_scene(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("deterministic", "Ryan met New Rome.")?;
    let ryan = fixture.content.find("Ryan").ok_or("Ryan")?;
    let rome = fixture.content.find("New Rome").ok_or("New Rome")?;
    let mut registry = EntityRegistry::empty();
    let ryan_result = registry.tag(&fixture.lease, tag(EntityKind::Character, ryan, "Ryan"))?;
    let rome_result = registry.tag(&fixture.lease, tag(EntityKind::Location, rome, "New Rome"))?;
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: 7,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    })?;

    assert_eq!(
        NATIVE_SCENE_COMPILER_CONTRACT,
        "phoenix.native.active-document-scene-compiler/v2"
    );
    assert_eq!(compiled.publication.kind, ScenePublicationKind::Full);
    assert_eq!(compiled.receipt.node_count, 7);
    assert_eq!(compiled.receipt.edge_count, 7);
    assert_eq!(compiled.receipt.verified_mentions, 2);
    assert_eq!(
        compiled.publication.document_id,
        Some(fixture.lease.entry_id.0)
    );
    assert_eq!(
        std::array::from_fn::<_, 5, _>(|page| compiled.publication.positions[page].len()),
        [7; 5]
    );
    assert_eq!(compiled.anchors.len(), 2);
    compiled.publication.validate()?;

    let expected_ids = {
        let mut ids = [ryan_result.entity_id, rome_result.entity_id];
        ids.sort_unstable();
        ids
    };
    assert_eq!(
        compiled
            .publication
            .identities
            .iter()
            .take(2)
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(
        compiled
            .publication
            .entity_mappings
            .iter()
            .map(|mapping| (mapping.entity_id, mapping.node_id))
            .collect::<Vec<_>>(),
        expected_ids.map(|id| (id, id))
    );
    let co_occurrence = compiled
        .publication
        .edge_products
        .iter()
        .position(|edge| edge.relation_mask == RelationFamily::CoOccurrence.mask().0)
        .ok_or("co-occurrence edge")?;
    let edge = compiled.publication.topology[co_occurrence];
    assert_eq!([edge.source_id, edge.target_id], expected_ids);
    assert_eq!(
        compiled
            .publication
            .styles
            .iter()
            .filter(|style| style.kind == EPISODE_NODE_KIND || style.kind == CHUNK_NODE_KIND)
            .count(),
        2
    );
    let caps = &compiled.publication.positions[ArchiveManifold::Caps as usize];
    for (slot, style) in compiled.publication.styles.iter().enumerate() {
        let expected = match style.kind {
            DOCUMENT_NODE_KIND => CapsRole::Document.world_radius(),
            EPISODE_NODE_KIND => CapsRole::Episode.world_radius(),
            CHUNK_NODE_KIND => CapsRole::Chunk.world_radius(),
            EVIDENCE_NODE_KIND => CapsRole::Evidence.world_radius(),
            _ => CapsRole::Entity.world_radius(),
        };
        assert!((Vec3::from_array(caps[slot].position).length() - expected).abs() < 0.001);
    }
    let chunk_slot = compiled
        .publication
        .styles
        .iter()
        .position(|style| style.kind == CHUNK_NODE_KIND)
        .ok_or("CAPS chunk")?;
    let chunk_direction = Vec3::from_array(caps[chunk_slot].position).normalize();
    for entity in caps.iter().take(2) {
        assert!(
            chunk_direction.dot(Vec3::from_array(entity.position).normalize()) > 0.93,
            "entities must remain inside the chunk/evidence cap chain"
        );
    }

    let repeated = compile_active_document(NativeSceneCompilerInput {
        generation_id: 8,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    })?;
    assert_eq!(
        compiled.publication.identities,
        repeated.publication.identities
    );
    assert_eq!(compiled.publication.topology, repeated.publication.topology);
    assert_eq!(compiled.publication.edges, repeated.publication.edges);
    assert_eq!(
        compiled.publication.positions,
        repeated.publication.positions
    );
    fixture.cleanup();
    Ok(())
}

#[test]
fn paragraph_boundaries_do_not_invent_cross_paragraph_edges(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("paragraphs", "Ryan left.\n\nNew Rome waited.")?;
    let ryan = fixture.content.find("Ryan").ok_or("Ryan")?;
    let rome = fixture.content.find("New Rome").ok_or("New Rome")?;
    let mut registry = EntityRegistry::empty();
    registry.tag(&fixture.lease, tag(EntityKind::Character, ryan, "Ryan"))?;
    registry.tag(&fixture.lease, tag(EntityKind::Location, rome, "New Rome"))?;
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: 1,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    })?;
    assert_eq!(compiled.receipt.chunk_count, 2);
    assert!(!compiled
        .publication
        .edge_products
        .iter()
        .any(|edge| edge.relation_mask == RelationFamily::CoOccurrence.mask().0));
    assert_eq!(
        compiled
            .publication
            .edge_products
            .iter()
            .filter(|edge| edge.relation_mask == RelationFamily::Structural.mask().0)
            .count(),
        7
    );
    fixture.cleanup();
    Ok(())
}

#[test]
fn explicit_event_entities_occupy_the_event_shell() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("event-shell", "The Launch began.")?;
    let start = fixture.content.find("Launch").ok_or("Launch")?;
    let mut registry = EntityRegistry::empty();
    registry.tag(&fixture.lease, tag(EntityKind::Event, start, "Launch"))?;
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: 1,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    })?;
    let position = compiled.publication.positions[ArchiveManifold::Caps as usize][0].position;
    assert!((Vec3::from_array(position).length() - CapsRole::Event.world_radius()).abs() < 0.001);
    fixture.cleanup();
    Ok(())
}

#[test]
fn no_verified_mentions_and_revision_drift_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("negative", "No graph evidence.")?;
    let registry = EntityRegistry::empty();
    let base = NativeSceneCompilerInput {
        generation_id: 1,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    };
    assert_eq!(
        compile_active_document(base).expect_err("no mentions"),
        NativeSceneCompilerError::NoVerifiedMentions
    );
    assert_eq!(
        compile_active_document(NativeSceneCompilerInput {
            registry_revision: registry.revision() + 1,
            ..base
        })
        .expect_err("revision mismatch"),
        NativeSceneCompilerError::RegistryRevisionMismatch {
            provided: registry.revision() + 1,
            canonical: registry.revision(),
        }
    );
    fixture.cleanup();
    Ok(())
}

struct Fixture {
    root: PathBuf,
    lease: phoenix_workspace::DocumentLease,
    content: String,
}

impl Fixture {
    fn cleanup(self) {
        let _ = std::fs::remove_dir_all(self.root);
    }
}

fn fixture(label: &str, content: &str) -> Result<Fixture, Box<dyn std::error::Error>> {
    let root = unique_temp(label);
    std::fs::create_dir_all(&root)?;
    let workspace_path = root.join("workspace.json");
    let workspace = WorkspaceDocument::seeded();
    workspace.save_atomic(&workspace_path)?;
    let base = open_document(
        &workspace_path,
        &workspace,
        workspace.first_note().ok_or("first note")?,
    )?;
    let lease = commit_document(&workspace_path, &workspace, base.token(), content)?;
    Ok(Fixture {
        root,
        lease,
        content: content.into(),
    })
}

fn tag(kind: EntityKind, start: usize, surface: &str) -> EntityTag {
    EntityTag {
        kind,
        custom_kind: None,
        start: start as u32,
        end: (start + surface.len()) as u32,
        surface: surface.into(),
    }
}

fn unique_temp(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "phoenix-native-scene-compiler-{label}-{}-{nanos}",
        std::process::id()
    ))
}
