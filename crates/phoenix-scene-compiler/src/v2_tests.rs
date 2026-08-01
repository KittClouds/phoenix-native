use crate::{compile_graph_generation_v2, NativeSceneCompilerError, NativeSceneCompilerV2Input};
use phoenix_graph_generation_v2::{
    write_generation_new, CandidateId, CandidateStatus, ChapterRecord, ChunkRecord, DocumentRecord,
    EntityRecord, EvidenceRecord, GenerationPages, GenerationWriteAuthority, ParagraphRecord,
    SemanticFamily, SentenceRecord, StringRef, StructuralEdgeRecord,
    TypedRelationshipCandidateRecord,
};
use phoenix_scene_archive::{ArchiveManifold, PageKey, PageKind};
use phoenix_scene_contract::{
    EntityKind, FamilyMask, GraphGeneration, HighlightPalette, Manifold, ReviewMask,
    RELATIONSHIP_FACT_NODE_KIND,
};
use phoenix_scene_publisher::ScenePublicationStore;
use phoenix_semantic_lens::{
    CandidateOrigin, CoreSemanticClass, EndpointKind, LensNeutralReviewBinding, SemanticEndpointRef,
};
use phoenix_semantic_review::{
    ReviewAuthority, ReviewCandidate, ReviewCandidateLocation, ReviewCatalog, ReviewPage,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn v2_source_truth_publishes_without_synthetic_episodes() {
    let root = temporary_root("source-truth");
    fs::create_dir_all(&root).expect("create test root");
    let generation_path = root.join("source.pgg2");
    let mut strings = StringSlab::default();
    let source = strings.push("Shortrun");
    let chapter_title = strings.push("Chapter 1");
    let entity_label = strings.push("Ryan");
    let document = DocumentRecord {
        id: 10,
        source_id: source,
        source_len: 128,
        chapter_count: 1,
        paragraph_count: 1,
        sentence_count: 1,
        chunk_count: 1,
        span_count: 0,
        entity_count: 1,
        mention_count: 0,
        evidence_count: 1,
        structural_edge_count: 4,
        flags: 0,
        reserved: [0; 3],
    };
    let chapters = [ChapterRecord {
        id: 20,
        document_id: 10,
        title: chapter_title,
        start: 0,
        end: 128,
        paragraph_start: 0,
        paragraph_end: 1,
        ordinal: 0,
        flags: 0,
        reserved: [0; 2],
    }];
    let paragraphs = [ParagraphRecord {
        id: 30,
        document_id: 10,
        chapter_id: 20,
        start: 0,
        end: 128,
        sentence_start: 0,
        sentence_end: 1,
        ordinal: 0,
        flags: 0,
        reserved: [0; 2],
    }];
    let sentences = [SentenceRecord {
        id: 40,
        document_id: 10,
        paragraph_id: 30,
        content_hash: 4,
        start: 0,
        end: 128,
        ordinal: 0,
        token_count: 20,
        quality: 1,
        dialogue_hint: 0,
        flags: 0,
        reserved_u16: 0,
        reserved: 0,
    }];
    let chunks = [ChunkRecord {
        id: 50,
        document_id: 10,
        content_hash: 5,
        start: 0,
        end: 128,
        sentence_start: 0,
        sentence_end: 1,
        paragraph_start: 0,
        paragraph_end: 1,
        chapter_index: 0,
        token_count: 20,
        flags: 0,
        reserved: 0,
    }];
    let entities = [EntityRecord {
        id: 60,
        label: entity_label,
        custom_kind: StringRef::default(),
        mention_count: 1,
        kind: EntityKind::Character as u16,
        source_mask: 1,
        flags: 0,
        reserved: 0,
    }];
    let evidence = [EvidenceRecord {
        id: 70,
        entity_id: 60,
        mention_id: 80,
        chunk_id: 50,
        start: 12,
        end: 16,
        role: 1,
        flags: 0,
        reserved: 0,
    }];
    let structural_edges = [
        structural_edge(101, 10, 20),
        structural_edge(102, 20, 30),
        structural_edge(103, 30, 40),
        structural_edge(104, 10, 50),
    ];
    let source_generation = write_generation_new(
        &generation_path,
        GenerationWriteAuthority {
            source_document_id_hash: [1; 32],
            content_hash: [2; 32],
            cohort_hash: [3; 32],
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
            published_generation: 12,
        },
        GenerationPages {
            strings: &strings.bytes,
            documents: &[document],
            chapters: &chapters,
            paragraphs: &paragraphs,
            sentences: &sentences,
            chunks: &chunks,
            entities: &entities,
            evidence: &evidence,
            structural_edges: &structural_edges,
            ..GenerationPages::default()
        },
    )
    .expect("write source generation");
    let catalog = ReviewCatalog::new(
        ReviewAuthority {
            source_generation_hash: source_generation.header().generation_hash,
            document_hash: source_generation.header().content_hash,
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
        },
        Vec::new(),
    )
    .expect("empty review catalog");

    let compiled = compile_graph_generation_v2(NativeSceneCompilerV2Input {
        scene_generation_id: 41,
        generation: &source_generation,
        review_catalog: &catalog,
        palette: HighlightPalette::default(),
    })
    .expect("compile V2 scene");
    assert_eq!(compiled.receipt.episode_count, 0);
    assert_eq!(
        compiled.receipt.source_generation_hash,
        source_generation.header().generation_hash
    );
    assert_eq!(compiled.publication.identities[0].id, 10);
    assert!(compiled.publication.edges.iter().any(|edge| edge.id == 104));
    assert!(!compiled
        .publication
        .identities
        .iter()
        .any(|node| matches!(node.id, 20 | 30 | 40)));
    assert!(!compiled
        .publication
        .node_products
        .iter()
        .any(|node| node.label.as_ref() == "Episode 1"));

    let store_root = root.join("published");
    let published = ScenePublicationStore::at_root(&store_root)
        .publish(compiled.publication)
        .expect("publish V2 scene");
    assert_eq!(published.scene.generation(), GraphGeneration(41));
    assert_eq!(published.scene.inventory().node_count, 4);
    assert_eq!(
        published
            .product_index
            .node_for_entity(phoenix_scene_product_index::EntityId(60)),
        Some(phoenix_scene_product_index::NodeId(60))
    );
    assert!(published
        .product_index
        .references()
        .iter()
        .any(|reference| reference.stable_ref == 70
            && reference.source_offset == 12
            && reference.source_len == 4));
    for (archive_manifold, manifold) in ArchiveManifold::ALL.into_iter().zip([
        Manifold::Hybrid,
        Manifold::Hopf,
        Manifold::Caps,
        Manifold::Transit,
        Manifold::Siegel,
    ]) {
        let active = published
            .scene
            .activate_manifold(manifold)
            .expect("activate prepared manifold");
        assert_eq!(active.pages.positions.len(), 4);
        assert!(active.guides.is_some());
        assert!(active.prepared_paths.is_some());
        assert!(published
            .scene
            .archive()
            .has_page(PageKey::manifold(PageKind::Positions, archive_manifold)));
    }
    drop(published);
    drop(source_generation);
    fs::remove_dir_all(root).expect("remove test root");
}

#[test]
fn accepted_status_without_receipt_fails_closed() {
    let root = temporary_root("missing-decision");
    fs::create_dir_all(&root).expect("create test root");
    let mut strings = StringSlab::default();
    let source = strings.push("receipt-check");
    let label = strings.push("Ryan");
    let candidate_id = CandidateId([7; 32]);
    let relationship = [TypedRelationshipCandidateRecord {
        candidate_id,
        source_entity_id: 60,
        target_entity_id: 60,
        evidence_start: 0,
        evidence_count: 0,
        premise_start: 0,
        premise_end: 1,
        relation: 1,
        family: SemanticFamily::Relationship as u16,
        status: CandidateStatus::Accepted as u16,
        flags_u16: 0,
        confidence_bits: 0.8_f32.to_bits(),
        flags: 0,
    }];
    let generation = write_generation_new(
        root.join("accepted-without-receipt.pgg2"),
        GenerationWriteAuthority {
            source_document_id_hash: [1; 32],
            content_hash: [2; 32],
            cohort_hash: [3; 32],
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
            published_generation: 12,
        },
        GenerationPages {
            strings: &strings.bytes,
            documents: &[DocumentRecord {
                id: 10,
                source_id: source,
                source_len: 1,
                chapter_count: 0,
                paragraph_count: 0,
                sentence_count: 0,
                chunk_count: 0,
                span_count: 0,
                entity_count: 1,
                mention_count: 0,
                evidence_count: 0,
                structural_edge_count: 0,
                flags: 0,
                reserved: [0; 3],
            }],
            entities: &[EntityRecord {
                id: 60,
                label,
                custom_kind: StringRef::default(),
                mention_count: 0,
                kind: EntityKind::Character as u16,
                source_mask: 1,
                flags: 0,
                reserved: 0,
            }],
            typed_relationship_candidates: &relationship,
            ..GenerationPages::default()
        },
    )
    .expect("write accepted generation");
    let candidate = ReviewCandidate {
        binding: LensNeutralReviewBinding {
            origin: CandidateOrigin {
                candidate_id,
                lens_id: [4; 32],
                vocabulary_hash: [5; 32],
                semantic_code: 1,
                semantic_class: CoreSemanticClass::Relation as u16,
                flags_u16: 0,
                reserved: 0,
            },
            origin_alignment_padding: 0,
            source: SemanticEndpointRef::new(EndpointKind::Entity, 60),
            target: SemanticEndpointRef::new(EndpointKind::Entity, 60),
            document_hash: [2; 32],
            candidate_hash: [6; 32],
            evidence_hash: [8; 32],
            producer_generation: 11,
            registry_revision: 9,
            flags: 0,
            reserved: 0,
        },
        location: ReviewCandidateLocation::new(ReviewPage::TypedRelationship, 0),
    };
    let catalog = ReviewCatalog::new(
        ReviewAuthority {
            source_generation_hash: generation.header().generation_hash,
            document_hash: [2; 32],
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
        },
        vec![candidate],
    )
    .expect("review catalog");
    let error = compile_graph_generation_v2(NativeSceneCompilerV2Input {
        scene_generation_id: 42,
        generation: &generation,
        review_catalog: &catalog,
        palette: HighlightPalette::default(),
    })
    .err()
    .expect("missing decision must fail");
    assert_eq!(
        error,
        NativeSceneCompilerError::V2DecisionReceiptMismatch(candidate_id)
    );
    let proposed_relationship = [TypedRelationshipCandidateRecord {
        status: CandidateStatus::Proposed as u16,
        ..relationship[0]
    }];
    let proposed = write_generation_new(
        root.join("proposed.pgg2"),
        GenerationWriteAuthority {
            source_document_id_hash: [1; 32],
            content_hash: [2; 32],
            cohort_hash: [3; 32],
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
            published_generation: 13,
        },
        GenerationPages {
            strings: &strings.bytes,
            documents: &[DocumentRecord {
                id: 10,
                source_id: source,
                source_len: 1,
                chapter_count: 0,
                paragraph_count: 0,
                sentence_count: 0,
                chunk_count: 0,
                span_count: 0,
                entity_count: 1,
                mention_count: 0,
                evidence_count: 0,
                structural_edge_count: 0,
                flags: 0,
                reserved: [0; 3],
            }],
            entities: &[EntityRecord {
                id: 60,
                label,
                custom_kind: StringRef::default(),
                mention_count: 0,
                kind: EntityKind::Character as u16,
                source_mask: 1,
                flags: 0,
                reserved: 0,
            }],
            typed_relationship_candidates: &proposed_relationship,
            ..GenerationPages::default()
        },
    )
    .expect("write proposed generation");
    let proposed_catalog = ReviewCatalog::new(
        ReviewAuthority {
            source_generation_hash: proposed.header().generation_hash,
            document_hash: [2; 32],
            native_document_id: 10,
            document_revision: 7,
            registry_revision: 9,
            producer_generation: 11,
        },
        vec![candidate],
    )
    .expect("proposed review catalog");
    let compiled = compile_graph_generation_v2(NativeSceneCompilerV2Input {
        scene_generation_id: 43,
        generation: &proposed,
        review_catalog: &proposed_catalog,
        palette: HighlightPalette::default(),
    })
    .expect("proposed candidate compiles as overlay");
    assert_eq!(compiled.receipt.accepted_semantic_edges, 0);
    assert_eq!(compiled.receipt.candidate_overlay_edges, 1);
    assert_eq!(compiled.receipt.relationship_candidate_count, 1);
    assert_eq!(compiled.receipt.identity_candidate_count, 0);
    assert_eq!(
        compiled
            .publication
            .edge_products
            .iter()
            .filter(|edge| edge.review_mask == ReviewMask::PROPOSED.0)
            .count(),
        2,
        "one relationship fact projects through one midpoint and two typed edges"
    );
    let fact_slot = compiled
        .publication
        .styles
        .iter()
        .position(|style| style.kind == RELATIONSHIP_FACT_NODE_KIND)
        .expect("relationship fact midpoint");
    let fact = &compiled.publication.node_products[fact_slot];
    assert_eq!(fact.family_mask, FamilyMask::FACTS.0);
    assert_eq!(fact.review_mask, ReviewMask::PROPOSED.0);
    drop(proposed);
    drop(generation);
    fs::remove_dir_all(root).expect("remove test root");
}

fn structural_edge(id: u64, source_id: u64, target_id: u64) -> StructuralEdgeRecord {
    StructuralEdgeRecord {
        id,
        source_id,
        target_id,
        evidence_id: 0,
        weight_bits: 1.0_f32.to_bits(),
        relation: 1,
        flags: 0,
    }
}

#[derive(Default)]
struct StringSlab {
    bytes: Vec<u8>,
}

impl StringSlab {
    fn push(&mut self, value: &str) -> StringRef {
        let reference = StringRef {
            offset: self.bytes.len() as u64,
            length: value.len() as u32,
            reserved: 0,
        };
        self.bytes.extend_from_slice(value.as_bytes());
        reference
    }
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phoenix-scene-compiler-v2-{label}-{}-{nonce}",
        std::process::id()
    ))
}
