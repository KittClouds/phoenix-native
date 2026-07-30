use crate::{
    compile_active_document, compile_graph_generation, NativeSceneCompilerError,
    NativeSceneCompilerInput, NATIVE_SCENE_COMPILER_CONTRACT,
};
use glam::Vec3;
use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisEntity, AnalysisEntityKind, AnalysisMention,
    AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord, AnalysisStageReceipt,
    DocumentAnalysisBinding, NliAdjudication, NliCandidate, NliCandidateKind, NliDecision,
    PhoenixDocumentAnalysisV1, PhoenixNerArtifactV1, PhoenixNliArtifactV1,
    PhoenixStructuralSubstrateV1, StructuralDialogueHint, StructuralSentenceQuality,
    StructuralSpanKind, ANALYSIS_CONTRACT, STRUCTURAL_SUBSTRATE_CONTRACT,
};
use phoenix_graph_generation::{
    write_graph_generation_new, AcceptedEdgeInput, CanonicalEntityInput, DurableDecisionInput,
    GraphGenerationInput, ProducerCapabilityInput, VerifiedGraphGeneration,
    ACCEPTED_EDGE_FLAG_PROMOTED, DECISION_FLAG_DURABLE_RECEIPT, DECISION_STATUS_ACCEPTED,
};
use phoenix_scene_archive::ArchiveManifold;
use phoenix_scene_contract::{
    AnchorCandidate, AnchorSource, CapsRole, DocumentId, EntityKind, HighlightPalette,
    RelationFamily, ReviewMask, VerifiedDocumentAnchors, CHUNK_NODE_KIND, DOCUMENT_NODE_KIND,
    EVIDENCE_NODE_KIND,
};
use phoenix_scene_publisher::ScenePublicationKind;
use phoenix_workspace::{
    commit_document, open_document, EntityRegistry, EntityTag, NerEntityRecord, WorkspaceDocument,
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
        verified_anchors: None,
        nli: None,
        palette: HighlightPalette::default(),
    })?;

    assert_eq!(
        NATIVE_SCENE_COMPILER_CONTRACT,
        "phoenix.native.active-document-scene-compiler/v2"
    );
    assert_eq!(compiled.publication.kind, ScenePublicationKind::Full);
    assert_eq!(compiled.receipt.node_count, 6);
    assert_eq!(compiled.receipt.edge_count, 6);
    assert_eq!(compiled.receipt.verified_mentions, 2);
    assert_eq!(
        compiled.publication.document_id,
        Some(fixture.lease.entry_id.0)
    );
    assert_eq!(
        std::array::from_fn::<_, 5, _>(|page| compiled.publication.positions[page].len()),
        [6; 5]
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
            .filter(|style| style.kind == CHUNK_NODE_KIND)
            .count(),
        1
    );
    let caps = &compiled.publication.positions[ArchiveManifold::Caps as usize];
    for (slot, style) in compiled.publication.styles.iter().enumerate() {
        let expected = match style.kind {
            DOCUMENT_NODE_KIND => CapsRole::Document.world_radius(),
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
        verified_anchors: None,
        nli: None,
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
fn verified_analysis_anchors_compile_without_registry_mention_copies(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("analysis-anchors", "Ryan met New Rome.")?;
    let ryan = fixture.content.find("Ryan").ok_or("Ryan")?;
    let rome = fixture.content.find("New Rome").ok_or("New Rome")?;
    let mut registry = EntityRegistry::empty();
    registry.publish_document_ner(
        fixture.lease.entry_id,
        1,
        &[
            NerEntityRecord {
                stable_id: 10,
                label: "Ryan".into(),
                kind: EntityKind::Character,
                custom_kind: None,
                mention_count: 1,
            },
            NerEntityRecord {
                stable_id: 20,
                label: "New Rome".into(),
                kind: EntityKind::Location,
                custom_kind: None,
                mention_count: 1,
            },
        ],
    )?;
    assert_eq!(registry.active_mentions_for(&fixture.lease).count(), 0);
    let anchors = VerifiedDocumentAnchors::verify(
        DocumentId(fixture.lease.entry_id.0),
        fixture.lease.revision.0,
        fixture.lease.content_hash.0,
        None,
        AnchorSource::VerifiedAnalysis,
        &fixture.content,
        vec![
            AnchorCandidate {
                start: ryan as u32,
                end: (ryan + "Ryan".len()) as u32,
                node_id: 10,
                entity_slot: 0,
                family: EntityKind::Character.family(),
                surface: "Ryan".into(),
            },
            AnchorCandidate {
                start: rome as u32,
                end: (rome + "New Rome".len()) as u32,
                node_id: 20,
                entity_slot: 1,
                family: EntityKind::Location.family(),
                surface: "New Rome".into(),
            },
        ],
    )?;
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: 2,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        verified_anchors: Some(&anchors),
        nli: None,
        palette: HighlightPalette::default(),
    })?;
    assert_eq!(compiled.receipt.verified_mentions, 2);
    assert_eq!(compiled.publication.entity_mappings.len(), 2);

    let wrong_document = VerifiedDocumentAnchors::verify(
        DocumentId(fixture.lease.entry_id.0 + 1),
        fixture.lease.revision.0,
        fixture.lease.content_hash.0,
        None,
        AnchorSource::VerifiedAnalysis,
        &fixture.content,
        vec![AnchorCandidate {
            start: ryan as u32,
            end: (ryan + "Ryan".len()) as u32,
            node_id: 10,
            entity_slot: 0,
            family: EntityKind::Character.family(),
            surface: "Ryan".into(),
        }],
    )?;
    assert_eq!(
        compile_active_document(NativeSceneCompilerInput {
            verified_anchors: Some(&wrong_document),
            ..NativeSceneCompilerInput {
                generation_id: 3,
                registry_revision: registry.revision(),
                document: &fixture.lease,
                registry: &registry,
                verified_anchors: None,
                nli: None,
                palette: HighlightPalette::default(),
            }
        })
        .expect_err("mismatched analysis authority"),
        NativeSceneCompilerError::AnchorAuthorityMismatch
    );
    fixture.cleanup();
    Ok(())
}

#[test]
fn candidate_only_nli_is_visible_as_proposed_without_promotion(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture("proposed-nli", "Ryan met New Rome. No entities here.")?;
    let ryan = fixture.content.find("Ryan").ok_or("Ryan")?;
    let rome = fixture.content.find("New Rome").ok_or("New Rome")?;
    let mut registry = EntityRegistry::empty();
    registry.publish_document_ner(
        fixture.lease.entry_id,
        1,
        &[
            NerEntityRecord {
                stable_id: 10,
                label: "Ryan".into(),
                kind: EntityKind::Character,
                custom_kind: None,
                mention_count: 1,
            },
            NerEntityRecord {
                stable_id: 20,
                label: "New Rome".into(),
                kind: EntityKind::Location,
                custom_kind: None,
                mention_count: 1,
            },
        ],
    )?;
    let anchors = VerifiedDocumentAnchors::verify(
        DocumentId(fixture.lease.entry_id.0),
        fixture.lease.revision.0,
        fixture.lease.content_hash.0,
        None,
        AnchorSource::VerifiedAnalysis,
        &fixture.content,
        vec![
            AnchorCandidate {
                start: ryan as u32,
                end: (ryan + "Ryan".len()) as u32,
                node_id: 10,
                entity_slot: 0,
                family: EntityKind::Character.family(),
                surface: "Ryan".into(),
            },
            AnchorCandidate {
                start: rome as u32,
                end: (rome + "New Rome".len()) as u32,
                node_id: 20,
                entity_slot: 1,
                family: EntityKind::Location.family(),
                surface: "New Rome".into(),
            },
        ],
    )?;
    let model = AnalysisModelIdentity {
        model_id: "fixture-model".into(),
        artifact_hash: [1; 32],
        config_hash: [2; 32],
        runtime_id: "fixture-runtime".into(),
    };
    let nli = PhoenixNliArtifactV1 {
        binding: DocumentAnalysisBinding {
            source_document_id: "fixture-document".into(),
            native_document_id: fixture.lease.entry_id.0,
            document_revision: fixture.lease.revision.0,
            content_hash: fixture.lease.content_hash.0,
            analysis_generation: 1,
            source_registry_revision: registry.revision() - 1,
            target_registry_revision: registry.revision(),
            producer_binary_hash: [3; 32],
            chunker: model.clone(),
            dynamic_ner: model.clone(),
            nli: model,
        },
        nli_candidates: vec![NliCandidate {
            candidate_id: [7; 32],
            kind: NliCandidateKind::Related,
            left_entity_id: 10,
            right_entity_id: 20,
            premise_start: 0,
            premise_end: fixture.content.len() as u32,
            premise: fixture.content.clone(),
            hypothesis: "Ryan is related to New Rome".into(),
        }],
        nli_adjudications: vec![NliAdjudication {
            candidate_id: [7; 32],
            decision: NliDecision::Supported,
            entailment_millis: 800,
            contradiction_millis: 50,
            neutral_millis: 150,
            confidence_millis: 800,
            needs_human_review: true,
        }],
        promotion_count: 0,
    };
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: 2,
        registry_revision: registry.revision(),
        document: &fixture.lease,
        registry: &registry,
        verified_anchors: Some(&anchors),
        nli: Some(&nli),
        palette: HighlightPalette::default(),
    })?;
    let proposed = compiled
        .publication
        .edge_products
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.review_mask == ReviewMask::PROPOSED.0)
        .collect::<Vec<_>>();
    assert_eq!(proposed.len(), 2);
    assert!(proposed.iter().all(|(index, _)| {
        compiled.publication.topology[*index]
            == phoenix_scene_archive::TopologyRecord {
                source_id: 10,
                target_id: 20,
            }
    }));
    assert!(compiled.publication.edge_products.iter().all(|edge| {
        edge.relation_mask & RelationFamily::CoOccurrence.mask().0 == 0
            || edge.review_mask == ReviewMask::PROPOSED.0
    }));
    assert_eq!(nli.promotion_count, 0);
    let receipt = AnalysisStageReceipt {
        chunk_count: 2,
        sentence_count: 2,
        mention_count: 2,
        entity_count: 2,
        nli_candidate_count: 1,
        nli_adjudication_count: 1,
        chunker_micros: 1,
        dynamic_ner_micros: 1,
        nli_load_micros: 1,
        nli_adjudication_micros: 1,
        promotion_count: 0,
    };
    let analysis = PhoenixDocumentAnalysisV1 {
        schema: ANALYSIS_CONTRACT.into(),
        ner: PhoenixNerArtifactV1 {
            binding: nli.binding.clone(),
            ner_revision: 1,
            entities: vec![
                AnalysisEntity {
                    stable_id: 10,
                    label: "Ryan".into(),
                    kind: AnalysisEntityKind::Character,
                    custom_kind: None,
                    mention_count: 1,
                },
                AnalysisEntity {
                    stable_id: 20,
                    label: "New Rome".into(),
                    kind: AnalysisEntityKind::Location,
                    custom_kind: None,
                    mention_count: 1,
                },
            ],
            mentions: vec![
                AnalysisMention {
                    mention_id: 1,
                    entity_id: 10,
                    start: ryan as u32,
                    end: (ryan + 4) as u32,
                    sentence_index: 0,
                    confidence: 0.9,
                    accepted: true,
                },
                AnalysisMention {
                    mention_id: 2,
                    entity_id: 20,
                    start: rome as u32,
                    end: (rome + 8) as u32,
                    sentence_index: 0,
                    confidence: 0.9,
                    accepted: true,
                },
            ],
            receipt,
        },
        nli: nli.clone(),
    };
    let structural = exact_two_chunks(&fixture.content, nli.binding.clone());
    let canonical = [
        CanonicalEntityInput {
            id: 10,
            label: "Ryan",
            custom_kind: None,
            mention_count: 1,
            kind: AnalysisEntityKind::Character as u16,
            source_mask: 1,
        },
        CanonicalEntityInput {
            id: 20,
            label: "New Rome",
            custom_kind: None,
            mention_count: 1,
            kind: AnalysisEntityKind::Location as u16,
            source_mask: 1,
        },
    ];
    let generation_path = fixture.root.join("exact-generation.phxgg");
    write_graph_generation_new(
        &generation_path,
        &GraphGenerationInput {
            text: &fixture.content,
            analysis: &analysis,
            structural: &structural,
            canonical_entities: &canonical,
            accepted_edges: &[],
            decisions: &[],
            capabilities: &[ProducerCapabilityInput {
                name: "exact-dynamic-chunks",
                producer: "test",
                supported: true,
                emitted: true,
                flags: 1,
            }],
        },
    )?;
    let generation = VerifiedGraphGeneration::open(&generation_path)?;
    let exact = compile_graph_generation(
        NativeSceneCompilerInput {
            generation_id: 3,
            registry_revision: registry.revision(),
            document: &fixture.lease,
            registry: &registry,
            verified_anchors: Some(&anchors),
            nli: Some(&nli),
            palette: HighlightPalette::default(),
        },
        &generation,
    )?;
    assert_eq!(exact.receipt.chunk_count, 2);
    assert!(generation.chunks().iter().all(|chunk| exact
        .publication
        .identities
        .iter()
        .any(|identity| identity.id == chunk.id)));
    let reviewed_path = fixture.root.join("reviewed-generation.phxgg");
    write_graph_generation_new(
        &reviewed_path,
        &GraphGenerationInput {
            text: &fixture.content,
            analysis: &analysis,
            structural: &structural,
            canonical_entities: &canonical,
            accepted_edges: &[AcceptedEdgeInput {
                id: phoenix_graph_generation::promoted_edge_id([7; 32]),
                source_id: 10,
                target_id: 20,
                evidence_id: 0,
                weight: 0.8,
                relation: 4,
                flags: ACCEPTED_EDGE_FLAG_PROMOTED,
            }],
            decisions: &[DurableDecisionInput {
                id: 92,
                candidate_id: [7; 32],
                reason: "test acceptance",
                decided_at_revision: fixture.lease.revision.0,
                status: DECISION_STATUS_ACCEPTED,
                flags: DECISION_FLAG_DURABLE_RECEIPT,
            }],
            capabilities: &[],
        },
    )?;
    let reviewed = VerifiedGraphGeneration::open(reviewed_path)?;
    let promoted = compile_graph_generation(
        NativeSceneCompilerInput {
            generation_id: 4,
            registry_revision: registry.revision(),
            document: &fixture.lease,
            registry: &registry,
            verified_anchors: Some(&anchors),
            nli: Some(&nli),
            palette: HighlightPalette::default(),
        },
        &reviewed,
    )?;
    let accepted_index = promoted
        .publication
        .edge_products
        .iter()
        .position(|edge| edge.edge_id == phoenix_graph_generation::promoted_edge_id([7; 32]))
        .ok_or("promoted edge")?;
    assert_eq!(
        promoted.publication.edge_products[accepted_index].review_mask,
        ReviewMask::ACCEPTED.0
    );
    assert_eq!(
        promoted.publication.topology[accepted_index],
        phoenix_scene_archive::TopologyRecord {
            source_id: 10,
            target_id: 20,
        }
    );
    fixture.cleanup();
    Ok(())
}

fn exact_two_chunks(
    content: &str,
    binding: DocumentAnalysisBinding,
) -> PhoenixStructuralSubstrateV1 {
    let end = content.len() as u32;
    let split = content.find('.').expect("fixture sentence boundary") as u32 + 1;
    PhoenixStructuralSubstrateV1 {
        schema: STRUCTURAL_SUBSTRATE_CONTRACT.into(),
        binding,
        source_len: end,
        chunks: vec![
            AnalysisChunkRecord {
                start: 0,
                end: split,
                sentence_start: 0,
                sentence_end: 1,
                paragraph_start: 0,
                paragraph_end: 1,
                chapter_index: 0,
                token_count: 4,
                content_hash: 17,
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisChunkRecord {
                start: split,
                end,
                sentence_start: 1,
                sentence_end: 2,
                paragraph_start: 0,
                paragraph_end: 1,
                chapter_index: 0,
                token_count: 3,
                content_hash: 18,
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
        sentences: vec![
            AnalysisSentenceRecord {
                start: 0,
                end: split,
                paragraph_index: 0,
                chapter_index: 0,
                token_count: 4,
                content_hash: 17,
                quality: StructuralSentenceQuality::Complete,
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisSentenceRecord {
                start: split,
                end,
                paragraph_index: 0,
                chapter_index: 0,
                token_count: 3,
                content_hash: 18,
                quality: StructuralSentenceQuality::Complete,
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
        spans: vec![
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Paragraph,
                start: 0,
                end,
                parent_index: 0,
                child_start: 0,
                child_end: 2,
                token_count: 7,
                content_hash: 17,
                label: String::new(),
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Chapter,
                start: 0,
                end,
                parent_index: u32::MAX,
                child_start: 0,
                child_end: 1,
                token_count: 7,
                content_hash: 17,
                label: "Document".into(),
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
    }
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
        verified_anchors: None,
        nli: None,
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
        6
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
        verified_anchors: None,
        nli: None,
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
        verified_anchors: None,
        nli: None,
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
