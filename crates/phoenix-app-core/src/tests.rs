use super::*;
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PageKey, PageKind,
    PhoenixSceneArchiveBuilderV1, PhoenixSceneArchiveV1, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{
    AnchorCandidate, AnchorSource, EntityFamily, EntityKind, FamilyMask, GraphScope, HighlightMode,
    RelationMask, ReviewMask, SceneAuthority, SceneContractError, VerifiedDocumentAnchors,
};
use phoenix_scene_product_index::{
    PhoenixSceneProductIndexBuilderV1, PhoenixSceneProductIndexV1, ProductIndexBinding, ReviewState,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[test]
fn bounded_command_envelope_stays_compact() {
    assert!(
        std::mem::size_of::<KernelCommand>() <= 96,
        "KernelCommand grew to {} bytes",
        std::mem::size_of::<KernelCommand>()
    );
}

fn path() -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!(
            "phoenix-kernel-test-{}-{sequence}",
            std::process::id()
        ))
        .join("workspace.json")
}

fn scene(generation: u64) -> Result<Arc<ResidentScene>, SceneContractError> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "phoenix-kernel-scene-test-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).map_err(phoenix_scene_archive::ArchiveError::Io)?;
    let archive_path = directory.join("scene.phxscene");
    let mut builder = PhoenixSceneArchiveBuilderV1::new(generation)?;
    builder
        .add_records(
            PageKey::shared(PageKind::NodeIdentity),
            &[] as &[NodeIdentityRecord],
        )?
        .add_records(
            PageKey::shared(PageKind::NodeStyle),
            &[] as &[NodeStyleRecord],
        )?
        .add_records(
            PageKey::shared(PageKind::Topology),
            &[] as &[TopologyRecord],
        )?
        .add_records(PageKey::shared(PageKind::Edge), &[] as &[EdgeRecord])?;
    for manifold in ArchiveManifold::ALL {
        builder.add_records(
            PageKey::manifold(PageKind::Positions, manifold),
            &[] as &[PositionRecord],
        )?;
    }
    builder.write_to_path(&archive_path)?;
    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    Ok(Arc::new(ResidentScene::from_archive(
        Arc::new(archive),
        None,
    )?))
}

fn product_index(
    scene: &ResidentScene,
) -> Result<Arc<PhoenixSceneProductIndexV1>, Box<dyn std::error::Error>> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "phoenix-kernel-product-index-{}-{sequence}.pspi",
        std::process::id()
    ));
    PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding {
        archive_generation: scene.generation().0,
        archive_cohort_hash: scene.archive_identity().cohort_hash,
    })
    .write_to_path(&path)?;
    Ok(Arc::new(PhoenixSceneProductIndexV1::open(path)?))
}

fn full_publication(generation_id: u64, registry_revision: u64) -> NativeScenePublication {
    let positions = std::array::from_fn(|manifold| {
        vec![PositionRecord {
            position: [manifold as f32, 0.0, 0.0],
        }]
    });
    NativeScenePublication {
        generation_id,
        kind: ScenePublicationKind::Full,
        registry_revision,
        document_id: None,
        identities: vec![NodeIdentityRecord { id: 501 }],
        styles: vec![NodeStyleRecord {
            color: [0.2, 0.8, 0.6, 1.0],
            radius: 4.0,
            kind: EntityKind::Concept as u16,
            flags: 0,
        }],
        topology: Vec::new(),
        edges: Vec::new(),
        positions,
        node_products: vec![SceneNodeProduct {
            node_id: 501,
            family_mask: 1,
            scope_mask: 1,
            review_mask: ReviewState::Accepted as u32,
            label: Arc::from("Published graph node"),
            inspector_ref: u32::MAX,
            provenance_ref: u32::MAX,
        }],
        edge_products: Vec::new(),
        entity_mappings: Vec::new(),
        references: Vec::new(),
    }
}

#[test]
fn commands_are_sequenced_and_workspace_reopens() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let created = kernel.execute(KernelCommand::CreateEntry {
        kind: EntryKind::Note,
        name: "Kernel note".into(),
    })?;
    let id = match created.outcome {
        KernelOutcome::EntryCreated(id) => id,
        other => return Err(format!("unexpected create outcome: {other:?}").into()),
    };
    let renamed = kernel.execute(KernelCommand::RenameEntry {
        id,
        name: "Resident note".into(),
    })?;
    assert!(renamed.sequence > created.sequence);
    let metrics = kernel.metrics();
    assert_eq!(metrics.commands_submitted, 2);
    assert_eq!(metrics.commands_pending, 0);
    assert_eq!(metrics.command_queue_high_water, 1);
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start(path.clone(), None)?;
    assert_eq!(
        reopened
            .snapshot()?
            .workspace
            .entry(id)
            .map(|entry| entry.name.as_str()),
        Some("Resident note")
    );
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn document_save_is_kernel_owned_and_restart_durable() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial document lease missing")?;
    assert_eq!(initial.revision, DocumentRevision(0));
    let receipt = kernel.execute(KernelCommand::SaveDocument {
        lease: initial.token(),
        content: Arc::from("# Durable\n\nKernel-owned."),
    })?;
    assert_eq!(
        receipt.outcome,
        KernelOutcome::DocumentSaved(DocumentRevision(1))
    );
    let stale = kernel.execute(KernelCommand::SaveDocument {
        lease: initial.token(),
        content: Arc::from("stale"),
    });
    assert!(matches!(stale, Err(KernelError::DocumentLeaseNotActive)));
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start(path.clone(), None)?;
    let lease = reopened
        .snapshot()?
        .active_document_lease
        .ok_or("reopened document lease missing")?;
    assert_eq!(lease.revision, DocumentRevision(1));
    assert_eq!(lease.content.as_ref(), "# Durable\n\nKernel-owned.");
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn manual_entity_tag_is_verified_reused_and_restart_durable(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial document lease missing")?;
    let content: Arc<str> = Arc::from("Ryan entered New Rome.");
    let tagged = kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: initial.token(),
        content: Arc::clone(&content),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let first_id = match tagged.outcome {
        KernelOutcome::EntityTagged(result) => {
            assert!(result.is_new);
            result.entity_id
        }
        other => return Err(format!("unexpected tag outcome: {other:?}").into()),
    };
    let snapshot = kernel.snapshot()?;
    assert_eq!(snapshot.entity_registry.entities().len(), 1);
    assert_eq!(
        snapshot
            .document_anchors
            .as_ref()
            .map(|anchors| anchors.anchors().len()),
        Some(1)
    );
    let lease = snapshot
        .active_document_lease
        .ok_or("tagged document lease missing")?;
    let retagged = kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content,
        tag: EntityTag {
            kind: EntityKind::Npc,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    match retagged.outcome {
        KernelOutcome::EntityTagged(result) => {
            assert!(!result.is_new);
            assert_eq!(result.entity_id, first_id);
        }
        other => return Err(format!("unexpected retag outcome: {other:?}").into()),
    }
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start(path.clone(), None)?;
    let snapshot = reopened.snapshot()?;
    assert_eq!(snapshot.entity_registry.entities()[0].kind, EntityKind::Npc);
    assert_eq!(
        snapshot
            .document_anchors
            .as_ref()
            .map(|anchors| anchors.anchors().len()),
        Some(1)
    );
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn canonical_atlas_merges_only_stable_identity_and_preserves_sources(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial document lease missing")?;
    let tagged = kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: initial.token(),
        content: Arc::from("Ryan entered New Rome."),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let user_id = match tagged.outcome {
        KernelOutcome::EntityTagged(result) => result.entity_id,
        other => return Err(format!("unexpected tag outcome: {other:?}").into()),
    };
    let published = kernel.execute(KernelCommand::PublishNerEntities(NerEntityBatch {
        revision: 1,
        entities: Arc::from([
            NerEntityRecord {
                stable_id: user_id,
                label: "Ryan".into(),
                kind: EntityKind::Character,
                custom_kind: None,
                mention_count: 5,
            },
            NerEntityRecord {
                stable_id: 9001,
                label: "New Rome".into(),
                kind: EntityKind::Location,
                custom_kind: None,
                mention_count: 3,
            },
        ]),
    }))?;
    assert!(matches!(
        published.outcome,
        KernelOutcome::NerEntitiesPublished(NerPublicationResult {
            ner_revision: 1,
            ner_entities: 2,
            ..
        })
    ));
    let atlas = kernel.snapshot()?.atlas_registry;
    assert_eq!(atlas.entities.len(), 2);
    assert_eq!(atlas.ner_source_count, 2);
    assert_eq!(atlas.user_tagged_source_count, 1);
    let ryan = atlas
        .entities
        .iter()
        .find(|entity| entity.stable_id == user_id)
        .ok_or("merged Ryan missing")?;
    assert_eq!(
        ryan.sources,
        EntitySourceMask {
            ner: true,
            user_tagged: true
        }
    );
    assert_eq!(ryan.mention_count, 6);
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start(path.clone(), None)?;
    let atlas = reopened.snapshot()?.atlas_registry;
    assert_eq!(atlas.entities.len(), 2);
    assert_eq!(atlas.ner_source_count, 2);
    assert_eq!(atlas.user_tagged_source_count, 1);
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn ner_publication_rejects_duplicate_and_stale_batches() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let duplicate = Arc::from([
        NerEntityRecord {
            stable_id: 7,
            label: "A".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 1,
        },
        NerEntityRecord {
            stable_id: 7,
            label: "B".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 1,
        },
    ]);
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNerEntities(NerEntityBatch {
            revision: 1,
            entities: duplicate,
        })),
        Err(KernelError::Workspace(
            WorkspaceError::DuplicateNerIdentity(7)
        ))
    ));
    kernel.execute(KernelCommand::PublishNerEntities(NerEntityBatch {
        revision: 1,
        entities: Arc::from([NerEntityRecord {
            stable_id: 8,
            label: "Concept".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 2,
        }]),
    }))?;
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNerEntities(NerEntityBatch {
            revision: 1,
            entities: Arc::from([]),
        })),
        Err(KernelError::Workspace(WorkspaceError::StaleNerRevision {
            current: 1,
            incoming: 1
        }))
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn production_publication_is_atomic_monotonic_and_restart_durable(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let initial = kernel.snapshot()?;
    let registry_receipt = initial
        .scene_publication
        .ok_or("registry publication missing")?;
    assert_eq!(registry_receipt.kind, ScenePublicationKind::RegistryOnly);
    assert_eq!(registry_receipt.node_count, 0);
    let full_generation = registry_receipt.generation_id + 1;
    let published = kernel.execute(KernelCommand::PublishNativeScene(Box::new(
        full_publication(full_generation, initial.atlas_registry.registry_revision),
    )))?;
    let full_receipt = match published.outcome {
        KernelOutcome::SceneGenerationPublished(receipt) => receipt,
        other => return Err(format!("unexpected publication outcome: {other:?}").into()),
    };
    assert_eq!(full_receipt.kind, ScenePublicationKind::Full);
    assert_eq!(full_receipt.node_count, 1);
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNativeScene(Box::new(
            full_publication(full_generation, initial.atlas_registry.registry_revision,)
        ))),
        Err(KernelError::ScenePublication(
            ScenePublicationError::StaleGeneration { .. }
        ))
    ));
    kernel.execute(KernelCommand::PublishNerEntities(NerEntityBatch {
        revision: 1,
        entities: Arc::from([NerEntityRecord {
            stable_id: 901,
            label: "Atlas arrived later".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 1,
        }]),
    }))?;
    assert_eq!(kernel.snapshot()?.scene_publication, Some(full_receipt));
    assert_eq!(kernel.events_after(0)?.len(), 2);
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production(path.clone())?;
    let reopened_snapshot = reopened.snapshot()?;
    assert_eq!(reopened_snapshot.scene_publication, Some(full_receipt));
    assert_eq!(
        reopened_snapshot
            .resident_scene
            .as_ref()
            .map(|scene| scene.source()),
        Some(phoenix_scene_contract::SceneSource::Backend)
    );
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn registry_only_scene_uses_canonical_ids_and_reopens_before_full_graph(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    let content: Arc<str> = Arc::from("Ryan entered New Rome.");
    let tagged = kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: initial.token(),
        content,
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let entity_id = match tagged.outcome {
        KernelOutcome::EntityTagged(result) => result.entity_id,
        other => return Err(format!("unexpected tag outcome: {other:?}").into()),
    };
    let snapshot = kernel.snapshot()?;
    let publication = snapshot
        .scene_publication
        .ok_or("registry scene publication missing")?;
    assert_eq!(publication.kind, ScenePublicationKind::RegistryOnly);
    assert_eq!(
        publication.registry_revision,
        snapshot.atlas_registry.registry_revision
    );
    assert_eq!(publication.entity_count, 1);
    assert_eq!(
        snapshot.scene_product_index.as_ref().and_then(
            |index| index.node_for_entity(phoenix_scene_product_index::EntityId(entity_id))
        ),
        Some(phoenix_scene_product_index::NodeId(entity_id))
    );
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production(path.clone())?;
    assert_eq!(reopened.snapshot()?.scene_publication, Some(publication));
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn graph_view_is_kernel_owned_and_preserves_resident_arrays(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let scene = scene(71)?;
    let index = product_index(&scene)?;
    let kernel = PhoenixKernel::start_with_product_index(
        path.clone(),
        Some(Arc::clone(&scene)),
        Some(Arc::clone(&index)),
    )?;
    let initial = kernel.snapshot()?;
    assert!(Arc::ptr_eq(
        initial.resident_scene.as_ref().ok_or("scene missing")?,
        &scene
    ));
    assert!(Arc::ptr_eq(
        initial
            .scene_product_index
            .as_ref()
            .ok_or("product index missing")?,
        &index
    ));
    let mut view = initial.graph_view;
    view.scope = GraphScope(0b10);
    view.families = FamilyMask(0b100);
    view.reviews = ReviewMask::ACCEPTED;
    view.relations = RelationMask(0b1000);
    kernel.execute(KernelCommand::SetGraphView(Box::new(view)))?;
    let updated = kernel.snapshot()?;
    assert_eq!(updated.graph_view, view);
    assert!(Arc::ptr_eq(
        updated.resident_scene.as_ref().ok_or("scene missing")?,
        &scene
    ));
    assert!(Arc::ptr_eq(
        updated
            .scene_product_index
            .as_ref()
            .ok_or("product index missing")?,
        &index
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn graph_view_rejects_missing_or_stale_product_authority() -> Result<(), Box<dyn std::error::Error>>
{
    let path = path();
    let scene = scene(72)?;
    let kernel = PhoenixKernel::start(path.clone(), Some(scene))?;
    let mut filtered = kernel.snapshot()?.graph_view;
    filtered.families = FamilyMask(1);
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(filtered))),
        Err(KernelError::ProductIndexRequiredForFilteredView)
    ));
    let mut stale = kernel.snapshot()?.graph_view;
    stale.authority = SceneAuthority::Archive {
        generation: GraphGeneration(999),
        cohort_hash: [0; 32],
        product_index_hash: None,
    };
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(stale))),
        Err(KernelError::StaleGraphViewAuthority)
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn shutdown_is_idempotent_and_worker_exits() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    kernel.shutdown()?;
    kernel.shutdown()?;
    assert!(kernel.metrics().worker_exited);
    assert!(matches!(
        kernel.execute(KernelCommand::SetManifold(Manifold::Transit)),
        Err(KernelError::ShuttingDown)
    ));
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn verified_anchors_are_kernel_owned_and_invalidated_by_save(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    kernel.execute(KernelCommand::SaveDocument {
        lease: initial.token(),
        content: Arc::from("Ryan entered New Rome."),
    })?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("saved lease missing")?;
    let anchors = Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        None,
        AnchorSource::VerificationFixture,
        &lease.content,
        vec![AnchorCandidate {
            start: 0,
            end: 4,
            node_id: 41,
            entity_slot: 7,
            family: EntityFamily::Character,
            surface: "Ryan".into(),
        }],
    )?);
    let receipt = kernel.execute(KernelCommand::PublishDocumentAnchors(Arc::clone(&anchors)))?;
    assert_eq!(receipt.outcome, KernelOutcome::DocumentAnchorsPublished(1));
    let resident = kernel
        .snapshot()?
        .document_anchors
        .ok_or("kernel anchor snapshot missing")?;
    assert!(Arc::ptr_eq(&resident, &anchors));

    kernel.execute(KernelCommand::SaveDocument {
        lease: lease.token(),
        content: Arc::from("Ryan left New Rome."),
    })?;
    assert!(kernel.snapshot()?.document_anchors.is_none());
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn highlight_mode_is_a_kernel_style_revision() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let mut style = kernel.snapshot()?.style;
    style.revision += 1;
    style.highlight_mode = HighlightMode::Vivid;
    kernel.execute(KernelCommand::SetStyle(style))?;
    let snapshot = kernel.snapshot()?;
    assert_eq!(snapshot.style.highlight_mode, HighlightMode::Vivid);
    assert_eq!(snapshot.style.revision, style.revision);
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}
