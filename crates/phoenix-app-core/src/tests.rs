use super::*;
use phoenix_analysis_contract::{
    AnalysisEntity, AnalysisEntityKind, AnalysisMention, AnalysisModelIdentity,
    AnalysisStageReceipt, DocumentAnalysisBinding, PhoenixNerArtifactV1,
};
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PageKey, PageKind,
    PhoenixSceneArchiveBuilderV1, PhoenixSceneArchiveV1, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{
    AnchorCandidate, AnchorSource, EntityFamily, EntityKind, GraphAction, GraphLens, GraphScope,
    GraphSurface, HighlightMode, RelationFamily, ReviewMask, SceneAuthority, SceneContractError,
    VerifiedDocumentAnchors,
};
use phoenix_scene_product_index::{
    PhoenixSceneProductIndexBuilderV1, PhoenixSceneProductIndexV1, ProductIndexBinding, ReviewState,
};
use phoenix_workspace::{EntityTag, NerPublicationResult};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

mod graph_view_tests;
mod selection_tests;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[test]
fn bounded_command_envelope_stays_compact() {
    assert!(
        std::mem::size_of::<KernelCommand>() <= 96,
        "KernelCommand grew to {} bytes",
        std::mem::size_of::<KernelCommand>()
    );
}

#[test]
fn command_queue_pressure_fails_closed_at_the_named_capacity(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let hold_kernel = Arc::clone(&kernel);
    let hold_entered = Arc::clone(&entered);
    let hold_release = Arc::clone(&release);
    let hold = std::thread::spawn(move || {
        hold_kernel.execute(KernelCommand::TestHoldCoordinator(Box::new((
            hold_entered,
            hold_release,
        ))))
    });
    entered.wait();

    let mut queued = Vec::with_capacity(COMMAND_CAPACITY);
    for index in 0..COMMAND_CAPACITY {
        let queued_kernel = Arc::clone(&kernel);
        queued.push(std::thread::spawn(move || {
            queued_kernel.execute(KernelCommand::SetManifold(
                Manifold::ALL[index % Manifold::ALL.len()],
            ))
        }));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while kernel.metrics().commands_pending < COMMAND_CAPACITY as u64 {
        if Instant::now() >= deadline {
            release.wait();
            return Err("command queue did not reach its bounded capacity".into());
        }
        std::thread::yield_now();
    }
    assert!(matches!(
        kernel.execute(KernelCommand::SetManifold(Manifold::Hybrid)),
        Err(KernelError::CommandQueueFull)
    ));
    assert_eq!(
        kernel.metrics().command_queue_high_water,
        COMMAND_CAPACITY as u64 + 1
    );
    release.wait();
    hold.join().map_err(|_| "hold command panicked")??;
    for command in queued {
        command.join().map_err(|_| "queued command panicked")??;
    }
    assert_eq!(kernel.metrics().commands_pending, 0);
    assert!(kernel.metrics().commands_rejected >= 1);
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
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

#[test]
fn analysis_generation_never_regresses_behind_persisted_authority() {
    assert_eq!(
        scene_rebuild::next_analysis_generation(2, 2, None).unwrap(),
        3
    );
    assert_eq!(
        scene_rebuild::next_analysis_generation(7, 2, Some(11)).unwrap(),
        12
    );
    assert_eq!(
        scene_rebuild::next_analysis_generation(19, 2, Some(11)).unwrap(),
        19
    );
    assert!(scene_rebuild::next_analysis_generation(1, u64::MAX, None).is_err());
}

fn test_ner_batch(
    kernel: &PhoenixKernel,
    revision: u64,
    records: Vec<NerEntityRecord>,
) -> Result<NerEntityBatch, Box<dyn std::error::Error>> {
    let mut snapshot = kernel.snapshot()?;
    let mut lease = snapshot
        .active_document_lease
        .ok_or("active document lease missing")?;
    if lease.revision.0 == 0 {
        kernel.execute(KernelCommand::SaveDocument {
            lease: lease.token(),
            content: Arc::from("analysis test document"),
        })?;
        snapshot = kernel.snapshot()?;
        lease = snapshot
            .active_document_lease
            .ok_or("saved document lease missing")?;
    }
    let model = |model_id: &str, seed: u8| AnalysisModelIdentity {
        model_id: model_id.into(),
        artifact_hash: [seed; 32],
        config_hash: [seed.wrapping_add(1); 32],
        runtime_id: "test".into(),
    };
    let source_registry_revision = snapshot.entity_registry.revision();
    let entities = records
        .into_iter()
        .map(|record| AnalysisEntity {
            stable_id: record.stable_id,
            label: record.label,
            kind: match record.kind {
                EntityKind::Character => AnalysisEntityKind::Character,
                EntityKind::Location => AnalysisEntityKind::Location,
                EntityKind::Npc => AnalysisEntityKind::Npc,
                EntityKind::Faction => AnalysisEntityKind::Faction,
                EntityKind::Event => AnalysisEntityKind::Event,
                EntityKind::Concept => AnalysisEntityKind::Concept,
                EntityKind::Custom => AnalysisEntityKind::Custom,
            },
            custom_kind: record.custom_kind,
            mention_count: record.mention_count,
        })
        .collect::<Vec<_>>();
    let entity_count = entities.len().try_into()?;
    Ok(NerEntityBatch::test_fixture(PhoenixNerArtifactV1 {
        binding: DocumentAnalysisBinding {
            source_document_id: "test-document".into(),
            native_document_id: lease.entry_id.0,
            document_revision: lease.revision.0,
            content_hash: lease.content_hash.0,
            analysis_generation: revision,
            source_registry_revision,
            target_registry_revision: source_registry_revision + 1,
            producer_binary_hash: [1; 32],
            chunker: model("chunker", 2),
            dynamic_ner: model("dynamic-ner", 4),
            nli: model("nli", 6),
        },
        ner_revision: revision,
        entities,
        mentions: Vec::new(),
        receipt: AnalysisStageReceipt {
            chunk_count: 1,
            sentence_count: 1,
            mention_count: 0,
            entity_count,
            nli_candidate_count: 0,
            nli_adjudication_count: 0,
            chunker_micros: 1,
            dynamic_ner_micros: 1,
            nli_load_micros: 1,
            nli_adjudication_micros: 1,
            promotion_count: 0,
        },
    }))
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
        entity_mappings: vec![phoenix_scene_product_index::EntityNodeMappingRecord {
            entity_id: 9001,
            node_id: 501,
        }],
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
    kernel.execute(KernelCommand::SelectEntry(id))?;
    assert!(renamed.sequence > created.sequence);
    let metrics = kernel.metrics();
    assert_eq!(metrics.commands_submitted, 3);
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
    assert_eq!(reopened.snapshot()?.active_entry, id);
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
    assert_eq!(
        snapshot
            .document_anchors
            .as_ref()
            .and_then(|anchors| anchors.anchors().first())
            .map(|anchor| anchor.entity_slot),
        Some(0)
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
fn verified_analysis_highlights_exact_mentions_with_manual_precedence(
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
            kind: EntityKind::Npc,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let manual_id = match tagged.outcome {
        KernelOutcome::EntityTagged(result) => result.entity_id,
        other => return Err(format!("unexpected tag outcome: {other:?}").into()),
    };
    let records = vec![
        NerEntityRecord {
            stable_id: 20,
            label: "Ryan".into(),
            kind: EntityKind::Character,
            custom_kind: None,
            mention_count: 1,
        },
        NerEntityRecord {
            stable_id: 30,
            label: "New Rome".into(),
            kind: EntityKind::Location,
            custom_kind: None,
            mention_count: 1,
        },
        NerEntityRecord {
            stable_id: 40,
            label: "entered".into(),
            kind: EntityKind::Custom,
            custom_kind: Some("ACTION".into()),
            mention_count: 1,
        },
    ];
    let mut batch = test_ner_batch(&kernel, 1, records)?;
    let artifact = Arc::make_mut(&mut batch.artifact);
    artifact.mentions = vec![
        AnalysisMention {
            mention_id: 1,
            entity_id: 20,
            start: 0,
            end: 4,
            sentence_index: 0,
            confidence: 0.98,
            accepted: true,
        },
        AnalysisMention {
            mention_id: 2,
            entity_id: 30,
            start: 13,
            end: 21,
            sentence_index: 0,
            confidence: 0.97,
            accepted: true,
        },
        AnalysisMention {
            mention_id: 3,
            entity_id: 40,
            start: 5,
            end: 12,
            sentence_index: 0,
            confidence: 0.87,
            accepted: false,
        },
    ];
    artifact.receipt.mention_count = 3;
    let artifact = Arc::clone(&batch.artifact);
    kernel.execute(KernelCommand::PublishNerEntities(batch))?;
    let snapshot = kernel.snapshot()?;
    let lease = snapshot
        .active_document_lease
        .as_deref()
        .ok_or("active document lease missing")?;
    let anchors =
        super::analysis::verified_analysis_anchors(&artifact, lease, &snapshot.entity_registry)?;
    assert_eq!(anchors.source(), AnchorSource::CanonicalRegistry);
    assert_eq!(anchors.anchors().len(), 3);
    assert_eq!(anchors.anchors()[0].node_id, manual_id);
    assert_eq!(anchors.anchors()[0].family, EntityFamily::Character);
    assert_eq!(anchors.anchors()[1].node_id, 40);
    assert_eq!(anchors.anchors()[1].family, EntityFamily::Other);
    assert_eq!(anchors.anchors()[2].node_id, 30);
    assert_eq!(anchors.anchors()[2].family, EntityFamily::Location);
    assert!(anchors.anchors()[0].entity_slot < 4);
    assert!(anchors.anchors()[1].entity_slot < 4);
    assert!(anchors.anchors()[2].entity_slot < 4);
    kernel.shutdown()?;
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
    let published = kernel.execute(KernelCommand::PublishNerEntities(test_ner_batch(
        &kernel,
        1,
        vec![
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
        ],
    )?))?;
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
    let duplicate = vec![
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
    ];
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNerEntities(test_ner_batch(
            &kernel, 1, duplicate,
        )?)),
        Err(KernelError::AnalysisAuthorityMismatch)
    ));
    kernel.execute(KernelCommand::PublishNerEntities(test_ner_batch(
        &kernel,
        1,
        vec![NerEntityRecord {
            stable_id: 8,
            label: "Concept".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 2,
        }],
    )?))?;
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNerEntities(test_ner_batch(
            &kernel,
            1,
            Vec::new(),
        )?)),
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
fn test_only_backend_publication_cannot_become_restart_authority(
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
        NativeScenePublishCommand::backend(full_publication(
            full_generation,
            initial.atlas_registry.registry_revision,
        )),
    )))?;
    let full_receipt = match published.outcome {
        KernelOutcome::SceneGenerationPublished(receipt) => receipt,
        other => return Err(format!("unexpected publication outcome: {other:?}").into()),
    };
    assert_eq!(full_receipt.kind, ScenePublicationKind::Full);
    assert_eq!(full_receipt.node_count, 1);
    assert!(matches!(
        kernel.execute(KernelCommand::PublishNativeScene(Box::new(
            NativeScenePublishCommand::backend(full_publication(
                full_generation,
                initial.atlas_registry.registry_revision,
            ))
        ))),
        Err(KernelError::ScenePublication(
            ScenePublicationError::StaleGeneration { .. }
        ))
    ));
    kernel.execute(KernelCommand::PublishNerEntities(test_ner_batch(
        &kernel,
        1,
        vec![NerEntityRecord {
            stable_id: 901,
            label: "Atlas arrived later".into(),
            kind: EntityKind::Concept,
            custom_kind: None,
            mention_count: 1,
        }],
    )?))?;
    assert_eq!(kernel.snapshot()?.scene_publication, Some(full_receipt));
    assert_eq!(kernel.events_after(0)?.len(), 3);

    let empty_note = match kernel
        .execute(KernelCommand::CreateEntry {
            kind: EntryKind::Note,
            name: "Empty note".into(),
        })?
        .outcome
    {
        KernelOutcome::EntryCreated(id) => id,
        other => return Err(format!("unexpected create outcome: {other:?}").into()),
    };
    kernel.execute(KernelCommand::SelectEntry(empty_note))?;
    let empty_note_snapshot = kernel.snapshot()?;
    assert_eq!(
        empty_note_snapshot.scene_publication,
        Some(full_receipt),
        "changing the active note must not discard the resident graph generation"
    );
    assert_eq!(
        empty_note_snapshot
            .resident_scene
            .as_ref()
            .map(|scene| scene.generation().0),
        Some(full_receipt.generation_id)
    );
    kernel.shutdown()?;
    drop(kernel);

    assert!(matches!(
        PhoenixKernel::start_production(path.clone()),
        Err(KernelError::MissingV2CompilerAuthority)
    ));
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn native_rebuild_compiles_active_evidence_and_reopens_exact_generation(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let initial = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    let content: Arc<str> = Arc::from("Ryan entered New Rome.");
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
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
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("tagged lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content,
        tag: EntityTag {
            kind: EntityKind::Location,
            custom_kind: None,
            start: 13,
            end: 21,
            surface: "New Rome".into(),
        },
    })))?;

    let rebuilt = kernel.rebuild_active_scene()?;
    let receipt = match rebuilt.outcome {
        KernelOutcome::GraphRebuilt(receipt) => receipt,
        other => return Err(format!("unexpected rebuild outcome: {other:?}").into()),
    };
    assert_eq!(receipt.compile.node_count, 9);
    assert_eq!(receipt.compile.edge_count, 8);
    assert_eq!(receipt.compile.verified_mentions, 2);
    assert_eq!(receipt.publication.kind, ScenePublicationKind::Full);
    assert_eq!(receipt.publication.node_count, 9);
    assert_eq!(receipt.publication.edge_count, 8);

    let snapshot = kernel.snapshot()?;
    assert_eq!(snapshot.graph_view.surface, GraphSurface::Atlas);
    assert_eq!(snapshot.graph_view.lens, GraphLens::Structure);
    let generation_hash = snapshot
        .graph_generation_v2
        .as_deref()
        .ok_or("V2 graph generation missing")?
        .header()
        .generation_hash;
    let review_authority = snapshot
        .review_catalog_v2
        .as_deref()
        .ok_or("V2 review catalog missing")?
        .authority();
    assert_eq!(review_authority.source_generation_hash, generation_hash);
    let scene = snapshot.resident_scene.as_ref().ok_or("scene missing")?;
    assert_eq!(scene.source(), phoenix_scene_contract::SceneSource::Backend);
    assert_eq!(scene.inventory().node_count, 9);
    assert_eq!(scene.inventory().edge_count, 8);
    let anchors = snapshot
        .document_anchors
        .as_ref()
        .ok_or("resident anchors missing")?;
    assert_eq!(anchors.source(), AnchorSource::ResidentGraph);
    assert_eq!(
        anchors.graph_generation(),
        Some(GraphGeneration(receipt.publication.generation_id))
    );
    assert_eq!(anchors.anchors().len(), 2);
    assert!(anchors
        .anchors()
        .iter()
        .all(|anchor| anchor.entity_slot < 2));
    assert_eq!(
        kernel.events_after(0)?.last().map(|event| &event.kind),
        Some(&KernelEventKind::GraphRebuilt { receipt })
    );
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production(path.clone())?;
    let reopened_snapshot = reopened.snapshot()?;
    assert_eq!(reopened_snapshot.graph_view.surface, GraphSurface::Atlas);
    assert_eq!(reopened_snapshot.graph_view.lens, GraphLens::Structure);
    assert_eq!(
        reopened_snapshot.scene_publication,
        Some(receipt.publication)
    );
    assert_eq!(
        reopened_snapshot
            .resident_scene
            .as_ref()
            .map(|scene| scene.source()),
        Some(phoenix_scene_contract::SceneSource::Backend)
    );
    assert_eq!(
        reopened_snapshot
            .graph_generation_v2
            .as_deref()
            .map(|generation| generation.header().generation_hash),
        Some(generation_hash)
    );
    assert_eq!(
        reopened_snapshot
            .review_catalog_v2
            .as_deref()
            .map(|catalog| catalog.authority()),
        Some(review_authority)
    );
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn atlas_control_guides_one_valid_action_through_publication(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let waiting = kernel.atlas_control_snapshot()?;
    assert_eq!(waiting.build_state, AtlasBuildState::Ready);
    assert_eq!(waiting.primary_action, AtlasPrimaryAction::RunPipeline);

    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content: Arc::from("Ryan entered New Rome."),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let ready = kernel.atlas_control_snapshot()?;
    assert_eq!(ready.build_state, AtlasBuildState::Ready);
    assert_eq!(ready.primary_action, AtlasPrimaryAction::RunPipeline);
    assert_eq!(ready.resident_anchor_count, 1);

    kernel.rebuild_active_scene()?;
    let published = kernel.atlas_control_snapshot()?;
    assert_eq!(published.build_state, AtlasBuildState::Published);
    assert_eq!(published.primary_action, AtlasPrimaryAction::RunPipeline);
    assert_eq!(
        published.generation_id,
        published
            .last_run
            .as_ref()
            .map(|receipt| receipt.authority.published_generation)
    );
    assert!(published.node_count > 0);
    assert!(published.graph_reviews.accepted_edges > 0);
    assert_eq!(published.graph_reviews.proposed_edges, 0);
    assert_eq!(
        published.decisions.accepted.state,
        AtlasCapabilityState::Unsupported
    );
    let run = published.last_run.as_ref().ok_or("durable run missing")?;
    run.validate()?;
    assert_eq!(run.resources.documents, 1);
    assert_eq!(
        run.resources.canonical_entities,
        published.canonical_entities
    );
    assert_eq!(run.resources.graph_nodes, published.node_count);
    assert_eq!(run.resources.graph_edges, published.edge_count);
    assert_eq!(
        run.resources.analysis_mentions.state,
        AtlasCapabilityState::Unsupported
    );
    assert_eq!(
        run.resources.analysis_mentions.count, None,
        "an unsupported analysis producer must not be rendered as zero output"
    );
    assert_eq!(
        run.graph_reviews.accepted_edges,
        published.graph_reviews.accepted_edges
    );
    assert_eq!(run.graph_reviews.proposed_edges, 0);
    assert_eq!(run.authority.previous_generation, Some(2));
    assert!(published.last_run_hash.is_some());
    assert!(!published.last_run_restored);

    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn native_release_manifest_freezes_exact_authority_and_fails_closed_on_corruption(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    let content: Arc<str> = Arc::from("Ryan entered New Rome.");
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content: Arc::clone(&content),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("tagged lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content,
        tag: EntityTag {
            kind: EntityKind::Location,
            custom_kind: None,
            start: 13,
            end: 21,
            surface: "New Rome".into(),
        },
    })))?;
    kernel.rebuild_active_scene()?;

    let snapshot = kernel.snapshot()?;
    let lease = snapshot
        .active_document_lease
        .as_deref()
        .ok_or("release lease missing")?;
    let control = kernel.atlas_control_snapshot()?;
    let run = control.last_run.ok_or("release run missing")?;
    let scene = snapshot.resident_scene.as_deref().ok_or("scene missing")?;
    assert!(matches!(
        kernel.release_manifest(),
        Err(ReleaseLockError::Missing("production analysis generation"))
    ));
    let digest = ReleaseCohortDigestsV1 {
        document_structure: [1; 32],
        entity_mentions_evidence: [2; 32],
        accepted_topology: [3; 32],
        candidate_semantics: [4; 32],
        durable_decisions: [5; 32],
        producer_capabilities: [6; 32],
        shared_scene_pages: [7; 32],
        manifold_positions: [[8; 32]; 5],
        manifold_guides: [[9; 32]; 5],
        manifold_paths: [[10; 32]; 5],
        product_families_reviews_scopes: [11; 32],
        product_labels: [12; 32],
        entity_node_mappings: [13; 32],
        inspector_provenance: [14; 32],
    };
    let manifest = PhoenixReleaseManifestV1 {
        contract: RELEASE_MANIFEST_CONTRACT.to_owned(),
        authority: ReleaseCohortAuthorityV1 {
            document_id: lease.entry_id.0,
            document_revision: lease.revision.0,
            document_hash: lease.content_hash.0,
            document_bytes: lease.content.len() as u64,
            registry_revision: run.authority.registry_revision,
            analysis_generation: run.authority.analysis_generation.unwrap_or_default(),
            graph_generation_hash: [15; 32],
            scene_generation: scene.generation().0,
            archive_cohort_hash: run.authority.archive_cohort_hash,
            product_index_hash: run.authority.product_index_hash,
            runtime_binary_hash: [16; 32],
            atlas_run_hash: [17; 32],
            decision_ledger_hash: [18; 32],
        },
        counts: ReleaseCohortCountsV1 {
            chunks: 1,
            sentences: 1,
            spans: 1,
            canonical_entities: run.resources.canonical_entities,
            mentions: 2,
            evidence: 2,
            accepted_edges: run.graph_reviews.accepted_edges,
            candidate_edges: run.graph_reviews.proposed_edges,
            adjudications: 0,
            durable_decisions: 0,
            scene_nodes: run.resources.graph_nodes,
            scene_edges: run.resources.graph_edges,
            entity_node_mappings: run.resources.canonical_entities,
            receipt_backed_promotions: 0,
            unreceipted_promotions: 0,
        },
        digests: digest,
        run,
        gates: ReleaseGateTargetsV1::default(),
        json_graph_freight: 0,
        fallback_count: 0,
        resident_generation_count: 1,
    };
    manifest.validate()?;
    assert_eq!(manifest.contract, RELEASE_MANIFEST_CONTRACT);
    assert_eq!(manifest.counts.unreceipted_promotions, 0);
    assert_eq!(manifest.json_graph_freight, 0);
    assert_eq!(manifest.fallback_count, 0);
    assert!(manifest.exact_semantic_mismatches(&manifest).is_empty());
    let mut drifted = manifest.clone();
    drifted.digests.manifold_positions[2] = [0xAA; 32];
    assert_eq!(
        manifest.exact_semantic_mismatches(&drifted),
        vec!["semantic products"]
    );
    let parent = path.parent().ok_or("test path has no parent")?;
    let manifest_path = parent.join("exact-cohort.phxrl");
    manifest.write_new(&manifest_path)?;
    assert_eq!(PhoenixReleaseManifestV1::open(&manifest_path)?, manifest);

    let mut corrupted = std::fs::read(&manifest_path)?;
    let last = corrupted.last_mut().ok_or("manifest is empty")?;
    *last ^= 0x80;
    let corrupt_path = parent.join("corrupt-cohort.phxrl");
    std::fs::write(&corrupt_path, corrupted)?;
    assert!(matches!(
        PhoenixReleaseManifestV1::open(&corrupt_path),
        Err(ReleaseLockError::HashMismatch)
    ));
    let oversized_path = parent.join("oversized-cohort.phxrl");
    std::fs::File::create(&oversized_path)?.set_len(2 * 1024 * 1024 + 65)?;
    assert!(matches!(
        PhoenixReleaseManifestV1::open(&oversized_path),
        Err(ReleaseLockError::Oversized(_))
    ));

    kernel.shutdown()?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn production_graph_architecture_excludes_legacy_and_json_freight() {
    let root_manifest = include_str!("../../../Cargo.toml");
    let app_core_manifest = include_str!("../Cargo.toml");
    let compiler_manifest = include_str!("../../phoenix-scene-compiler/Cargo.toml");
    let shell_manifest = include_str!("../../../apps/phoenix-shell-proof/Cargo.toml");
    let shell_main = include_str!("../../../apps/phoenix-shell-proof/src/main.rs");
    let legacy_manifest = include_str!("../../../apps/phoenix-legacy-bridge/Cargo.toml");

    let default_members = manifest_section(root_manifest, "default-members");
    assert!(!default_members.contains("phoenix-graph-generation\""));
    assert!(!default_members.contains("phoenix-legacy-bridge"));
    assert!(root_manifest.contains("exclude = [\"apps/phoenix-legacy-bridge\"]"));
    assert!(!app_core_manifest.contains("phoenix-graph-generation ="));
    assert!(!app_core_manifest.contains("serde_json"));
    assert!(compiler_manifest.contains("default = []"));
    assert!(compiler_manifest.contains("legacy-v1-fixture = [\"dep:phoenix-graph-generation\"]"));
    assert!(shell_manifest.contains("legacy-graph-adapter = []"));
    assert!(shell_main.contains("PHOENIX_LEGACY_GRAPH_ADAPTER_FORBIDDEN"));
    assert!(legacy_manifest.contains("[workspace]"));
    assert_eq!(PRODUCTION_GRAPH_AUTHORITY, "PhoenixGraphGenerationV2");
    assert_eq!(PRODUCTION_FALLBACK_COUNT, 0);
    assert_eq!(PRODUCTION_JSON_GRAPH_FREIGHT, 0);
    assert_eq!(PRODUCTION_RESIDENT_GENERATION_COUNT, 1);
}

fn manifest_section<'a>(manifest: &'a str, name: &str) -> &'a str {
    let start = manifest
        .find(&format!("{name} = ["))
        .expect("manifest section must exist");
    let tail = &manifest[start..];
    let end = tail.find("]\n").expect("manifest section must terminate");
    &tail[..end + 2]
}

#[test]
fn explicit_publication_root_remains_a_writable_production_authority(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let parent = path.parent().ok_or("test path has no parent")?;
    let publication_root = parent.join("packaged-scene-publications-v1");
    let kernel = PhoenixKernel::start_production_at_root(path.clone(), publication_root.clone())?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content: Arc::from("Ryan entered New Rome."),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let rebuilt = kernel.rebuild_active_scene()?;
    let generation = match rebuilt.outcome {
        KernelOutcome::GraphRebuilt(receipt) => receipt.publication.generation_id,
        other => return Err(format!("unexpected rebuild outcome: {other:?}").into()),
    };
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production_at_root(path.clone(), publication_root)?;
    assert_eq!(
        reopened
            .snapshot()?
            .scene_publication
            .map(|receipt| receipt.generation_id),
        Some(generation)
    );
    let restored = reopened.atlas_control_snapshot()?;
    assert_eq!(restored.build_state, AtlasBuildState::Published);
    assert!(restored.last_run_restored);
    assert_eq!(
        restored
            .last_run
            .as_ref()
            .map(|receipt| receipt.authority.published_generation),
        Some(generation)
    );
    reopened.shutdown()?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn corrupt_v2_compiler_authority_fails_closed_on_restart() -> Result<(), Box<dyn std::error::Error>>
{
    let path = path();
    let parent = path.parent().ok_or("test path has no parent")?;
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content: Arc::from("Ryan entered New Rome."),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    let generation = match kernel.rebuild_active_scene()?.outcome {
        KernelOutcome::GraphRebuilt(receipt) => receipt.publication.generation_id,
        other => return Err(format!("unexpected rebuild outcome: {other:?}").into()),
    };
    kernel.shutdown()?;
    drop(kernel);

    let marker = parent
        .join("scene-publications-v1")
        .join(format!("generation-{generation:020}.phxcav2"));
    let mut bytes = std::fs::read(&marker)?;
    bytes[40] ^= 0x80;
    std::fs::write(&marker, bytes)?;
    assert!(matches!(
        PhoenixKernel::start_production(path.clone()),
        Err(KernelError::InvalidV2CompilerAuthority)
    ));
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn stale_durable_run_never_claims_a_changed_document() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("initial lease missing")?;
    kernel.execute(KernelCommand::TagSelection(Box::new(EntityTagCommand {
        lease: lease.token(),
        content: Arc::from("Ryan entered New Rome."),
        tag: EntityTag {
            kind: EntityKind::Character,
            custom_kind: None,
            start: 0,
            end: 4,
            surface: "Ryan".into(),
        },
    })))?;
    kernel.rebuild_active_scene()?;
    let built_lease = kernel
        .snapshot()?
        .active_document_lease
        .ok_or("built lease missing")?;
    kernel.execute(KernelCommand::SaveDocument {
        lease: built_lease.token(),
        content: Arc::from("Ryan entered New Rome, then left."),
    })?;
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production(path.clone())?;
    let control = reopened.atlas_control_snapshot()?;
    assert_ne!(control.build_state, AtlasBuildState::Published);
    assert!(
        control.last_run.is_none(),
        "a source-mismatched durable receipt must not enter restored UI state"
    );
    reopened.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn native_rebuild_without_verified_mentions_preserves_current_generation(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start_production(path.clone())?;
    let initial = kernel
        .snapshot()?
        .scene_publication
        .ok_or("initial publication missing")?;
    assert!(matches!(
        kernel.rebuild_active_scene(),
        Err(KernelError::DocumentAnchorsNotActive)
    ));
    assert_eq!(kernel.snapshot()?.scene_publication, Some(initial));
    let control = kernel.atlas_control_snapshot()?;
    assert_eq!(control.build_state, AtlasBuildState::Failed);
    assert_eq!(control.primary_action, AtlasPrimaryAction::RunPipeline);
    assert!(control.last_error.is_some());
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start_production(path.clone())?;
    assert_eq!(reopened.snapshot()?.scene_publication, Some(initial));
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
fn semantic_producer_cancellation_is_explicit_and_bounded() -> Result<(), Box<dyn std::error::Error>>
{
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    assert!(!kernel.shared.producer_cancel.load(Ordering::Acquire));
    kernel.cancel_active_producers();
    assert!(kernel.shared.producer_cancel.load(Ordering::Acquire));
    kernel.shutdown()?;
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
