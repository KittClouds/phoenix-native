use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisEntity, AnalysisEntityKind, AnalysisMention,
    AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord, AnalysisStageReceipt,
    DocumentAnalysisBinding, PhoenixNerArtifactV1, PhoenixStructuralSubstrateV1,
    StructuralDialogueHint, StructuralSentenceQuality, StructuralSpanKind, NO_STRUCTURAL_PARENT,
    STRUCTURAL_SUBSTRATE_CONTRACT,
};
use phoenix_document_producer::{publish_or_reuse_structural_generation, StructuralProducerInput};
use phoenix_entity_producer::{
    publish_entity_generation_new, EntityProducerError, EntityProducerInput,
    IdentityCandidateInput, IdentityCandidateKind, IdentityMergeDecision, UserTaggedEntityInput,
    UserTaggedMentionInput,
};
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateId, CandidateStatus, CanonicalBindingKind,
    CanonicalEntityBindingRecord, EntityId, EntityRecord, EvidenceRecord, IdentityCandidateRecord,
    MentionId, MentionRecord, PageKind,
};
use phoenix_scene_contract::EntityKind;

#[test]
fn overlapping_mentions_survive_authority_while_paint_is_non_overlapping() {
    let fixture = fixture();
    let structural_directory = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_directory.path(), &fixture);
    let output_directory = tempfile::tempdir().unwrap();
    let published = publish_entities(
        output_directory.path().join("entity.psg2"),
        &fixture,
        structural.generation(),
        &[],
    )
    .unwrap();
    let generation = published.generation();
    let entities: &[EntityRecord] = generation.typed_page(PageKind::Entities).unwrap();
    let mentions: &[MentionRecord] = generation.typed_page(PageKind::Mentions).unwrap();
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence).unwrap();
    let candidates: &[IdentityCandidateRecord] =
        generation.typed_page(PageKind::IdentityCandidates).unwrap();
    let bindings: &[CandidateEvidenceBindingRecord] = generation
        .typed_page(PageKind::CandidateEvidenceBindings)
        .unwrap();
    let canonical_bindings: &[CanonicalEntityBindingRecord] = generation
        .typed_page(PageKind::CanonicalEntityBindings)
        .unwrap();

    assert_eq!(entities.len(), 3, "same labels must not merge identities");
    assert_eq!(mentions.len(), 3, "all overlapping mentions are authority");
    assert_eq!(evidence.len(), 3, "paint cannot delete graph evidence");
    assert_eq!(published.paint().spans.len(), 1);
    assert_eq!(published.paint().spans[0].entity_id, EntityId(303));
    assert_eq!(published.paint().spans[0].source_mask, 2);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].status, CandidateStatus::Proposed as u16);
    assert_eq!(
        (candidates[0].evidence_start, candidates[0].evidence_count),
        (0, 2)
    );
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].candidate_id, candidates[0].candidate_id);
    assert_eq!(bindings[1].candidate_id, candidates[0].candidate_id);
    assert_ne!(bindings[0].evidence_id, bindings[1].evidence_id);
    assert_eq!(canonical_bindings.len(), 3);
    assert!(canonical_bindings.iter().all(|binding| {
        binding.source_entity_id == binding.canonical_entity_id
            && binding.decision_id == 0
            && binding.kind == CanonicalBindingKind::Direct as u16
    }));

    let chunks = structural.chunks().unwrap();
    assert!(mentions
        .iter()
        .all(|mention| mention.chunk_id == chunks[0].id));
    assert!(mentions.iter().all(|mention| mention.sentence_index == 0));
    assert_eq!(published.receipt().entity_count, 3);
    assert_eq!(published.receipt().mention_count, 3);
    assert_eq!(published.receipt().evidence_count, 3);
    assert_eq!(published.receipt().paint_span_count, 1);
}

#[test]
fn only_an_explicit_coordinator_decision_merges_a_user_identity() {
    let fixture = fixture();
    let structural_directory = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_directory.path(), &fixture);
    let output_directory = tempfile::tempdir().unwrap();
    let decision = IdentityMergeDecision {
        decision_id: 501,
        source_user_entity_id: EntityId(303),
        canonical_entity_id: EntityId(101),
    };
    let published = publish_entities(
        output_directory.path().join("merged.psg2"),
        &fixture,
        structural.generation(),
        &[decision],
    )
    .unwrap();
    let entities: &[EntityRecord] = published
        .generation()
        .typed_page(PageKind::Entities)
        .unwrap();
    let bindings: &[CanonicalEntityBindingRecord] = published
        .generation()
        .typed_page(PageKind::CanonicalEntityBindings)
        .unwrap();

    assert_eq!(entities.len(), 2);
    let merged = entities.iter().find(|entity| entity.id == 101).unwrap();
    assert_eq!(merged.source_mask, 3);
    assert_eq!(merged.mention_count, 2);
    assert!(entities.iter().all(|entity| entity.id != 303));
    assert_eq!(published.paint().spans[0].entity_id, EntityId(101));
    let merged_binding = bindings
        .iter()
        .find(|binding| binding.source_entity_id == 303)
        .unwrap();
    assert_eq!(merged_binding.canonical_entity_id, 101);
    assert_eq!(merged_binding.decision_id, 501);
    assert_eq!(
        merged_binding.kind,
        CanonicalBindingKind::CoordinatorDecision as u16
    );
}

#[test]
fn identical_authority_produces_identical_entity_and_evidence_pages() {
    let fixture = fixture();
    let structural_directory = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_directory.path(), &fixture);
    let output_directory = tempfile::tempdir().unwrap();
    let left = publish_entities(
        output_directory.path().join("left.psg2"),
        &fixture,
        structural.generation(),
        &[],
    )
    .unwrap();
    let right = publish_entities(
        output_directory.path().join("right.psg2"),
        &fixture,
        structural.generation(),
        &[],
    )
    .unwrap();

    assert_eq!(
        left.receipt().generation_hash,
        right.receipt().generation_hash
    );
    assert_eq!(
        left.receipt().graph_evidence_hash,
        right.receipt().graph_evidence_hash
    );
    assert_eq!(
        left.receipt().paint_projection_hash,
        right.receipt().paint_projection_hash
    );
    for kind in [
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::StructuralEdges,
        PageKind::Entities,
        PageKind::Mentions,
        PageKind::Evidence,
        PageKind::IdentityCandidates,
        PageKind::CandidateEvidenceBindings,
        PageKind::CanonicalEntityBindings,
    ] {
        assert_eq!(
            left.generation().descriptor(kind).hash,
            right.generation().descriptor(kind).hash,
            "{kind:?} drifted"
        );
    }
}

#[test]
fn stale_source_and_implicit_ner_identity_rewrites_fail_closed() {
    let fixture = fixture();
    let structural_directory = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_directory.path(), &fixture);
    let output_directory = tempfile::tempdir().unwrap();
    let invalid_decision = IdentityMergeDecision {
        decision_id: 501,
        source_user_entity_id: EntityId(101),
        canonical_entity_id: EntityId(202),
    };
    let error = publish_entities(
        output_directory.path().join("invalid.psg2"),
        &fixture,
        structural.generation(),
        &[invalid_decision],
    )
    .err()
    .expect("an NER canonical ID cannot be used as a user merge source");
    assert!(matches!(error, EntityProducerError::InvalidMergeDecision));

    let mut stale = fixture;
    stale.text.push('x');
    let error = publish_entities(
        output_directory.path().join("stale.psg2"),
        &stale,
        structural.generation(),
        &[],
    )
    .err()
    .expect("stale source must fail");
    assert!(matches!(
        error,
        EntityProducerError::AuthorityMismatch | EntityProducerError::SourceBindingMismatch
    ));
}

#[test]
fn overlapping_dynamic_chunks_choose_the_smallest_exact_container() {
    let mut fixture = fixture();
    let sentence = fixture.structural.sentences[0];
    fixture.structural.chunks.push(AnalysisChunkRecord {
        start: sentence.start,
        end: sentence.end,
        sentence_start: 0,
        sentence_end: 1,
        paragraph_start: 0,
        paragraph_end: 1,
        chapter_index: 0,
        token_count: sentence.token_count,
        content_hash: fnv64(&fixture.text[sentence.start as usize..sentence.end as usize]),
        dialogue_hint: StructuralDialogueHint::None,
    });
    fixture.ner.receipt.chunk_count = 2;

    let structural_directory = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_directory.path(), &fixture);
    let output_directory = tempfile::tempdir().unwrap();
    let published = publish_entities(
        output_directory.path().join("overlapping-chunks.psg2"),
        &fixture,
        structural.generation(),
        &[],
    )
    .unwrap();
    let mentions: &[MentionRecord] = published
        .generation()
        .typed_page(PageKind::Mentions)
        .unwrap();
    let chunks = structural.chunks().unwrap();

    assert_eq!(chunks.len(), 2);
    assert!(mentions
        .iter()
        .all(|mention| mention.chunk_id == chunks[1].id));
}

fn publish_entities<'a>(
    path: std::path::PathBuf,
    fixture: &'a Fixture,
    structural: &'a phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
    decisions: &'a [IdentityMergeDecision],
) -> Result<phoenix_entity_producer::VerifiedEntityGeneration, EntityProducerError> {
    let new_rome_start = fixture.text.find("New Rome").unwrap() as u32;
    let new_rome_end = new_rome_start + "New Rome".len() as u32;
    let user_entities = [UserTaggedEntityInput {
        stable_id: EntityId(303),
        label: "New Rome",
        kind: EntityKind::Location,
        custom_kind: None,
    }];
    let user_mentions = [UserTaggedMentionInput {
        source_entity_id: EntityId(303),
        start: new_rome_start,
        end: new_rome_end,
        surface: "New Rome",
    }];
    let identity_candidates = [IdentityCandidateInput {
        candidate_id: CandidateId([9; 32]),
        left_entity_id: EntityId(101),
        right_entity_id: EntityId(202),
        left_mention_id: MentionId(1_001),
        right_mention_id: MentionId(1_002),
        kind: IdentityCandidateKind::SameSurface,
        confidence: 0.72,
    }];
    publish_entity_generation_new(
        path,
        EntityProducerInput {
            text: &fixture.text,
            structural,
            ner: &fixture.ner,
            user_entities: &user_entities,
            user_mentions: &user_mentions,
            merge_decisions: decisions,
            identity_candidates: &identity_candidates,
            published_generation: 14,
        },
    )
}

fn publish_structure(
    directory: &std::path::Path,
    fixture: &Fixture,
) -> phoenix_document_producer::VerifiedStructuralGeneration {
    publish_or_reuse_structural_generation(
        directory,
        StructuralProducerInput {
            text: &fixture.text,
            structural: &fixture.structural,
        },
    )
    .unwrap()
}

struct Fixture {
    text: String,
    structural: PhoenixStructuralSubstrateV1,
    ner: PhoenixNerArtifactV1,
}

fn fixture() -> Fixture {
    let text = "## Chapter 1\nNew Rome met Rome.".to_owned();
    let sentence_start = text.find("New Rome").unwrap();
    let sentence_end = text.len();
    let binding = DocumentAnalysisBinding {
        source_document_id: "fixture:entity-evidence".to_owned(),
        native_document_id: 7,
        document_revision: 11,
        content_hash: *blake3::hash(text.as_bytes()).as_bytes(),
        analysis_generation: 13,
        source_registry_revision: 17,
        target_registry_revision: 18,
        producer_binary_hash: [23; 32],
        chunker: model("phoenix-chunker/structural-v1", 29),
        dynamic_ner: model("phoenix-dynamic-ner/v1", 31),
        nli: model("candidate-only", 37),
    };
    let structural = PhoenixStructuralSubstrateV1 {
        schema: STRUCTURAL_SUBSTRATE_CONTRACT.to_owned(),
        binding: binding.clone(),
        source_len: text.len() as u32,
        chunks: vec![AnalysisChunkRecord {
            start: 0,
            end: text.len() as u32,
            sentence_start: 0,
            sentence_end: 1,
            paragraph_start: 0,
            paragraph_end: 1,
            chapter_index: 0,
            token_count: 6,
            content_hash: fnv64(&text),
            dialogue_hint: StructuralDialogueHint::None,
        }],
        sentences: vec![AnalysisSentenceRecord {
            start: sentence_start as u32,
            end: sentence_end as u32,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 4,
            content_hash: fnv64(&text[sentence_start..sentence_end]),
            quality: StructuralSentenceQuality::Complete,
            dialogue_hint: StructuralDialogueHint::None,
        }],
        spans: vec![
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Paragraph,
                start: 0,
                end: text.len() as u32,
                parent_index: 0,
                child_start: 0,
                child_end: 1,
                token_count: 6,
                content_hash: fnv64(&text),
                label: String::new(),
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Chapter,
                start: 0,
                end: text.len() as u32,
                parent_index: NO_STRUCTURAL_PARENT,
                child_start: 0,
                child_end: 1,
                token_count: 6,
                content_hash: fnv64(&text),
                label: "Chapter 1".to_owned(),
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
    };
    let new_rome_start = text.find("New Rome").unwrap() as u32;
    let new_rome_end = new_rome_start + "New Rome".len() as u32;
    let nested_rome_start = new_rome_start + "New ".len() as u32;
    let nested_rome_end = new_rome_end;
    let ner = PhoenixNerArtifactV1 {
        binding,
        ner_revision: 1,
        entities: vec![
            AnalysisEntity {
                stable_id: 101,
                label: "New Rome".to_owned(),
                kind: AnalysisEntityKind::Location,
                custom_kind: None,
                mention_count: 1,
            },
            AnalysisEntity {
                stable_id: 202,
                label: "Rome".to_owned(),
                kind: AnalysisEntityKind::Location,
                custom_kind: None,
                mention_count: 1,
            },
        ],
        mentions: vec![
            AnalysisMention {
                mention_id: 1_001,
                entity_id: 101,
                start: new_rome_start,
                end: new_rome_end,
                sentence_index: 0,
                confidence: 0.97,
                accepted: true,
            },
            AnalysisMention {
                mention_id: 1_002,
                entity_id: 202,
                start: nested_rome_start,
                end: nested_rome_end,
                sentence_index: 0,
                confidence: 0.91,
                accepted: true,
            },
        ],
        receipt: AnalysisStageReceipt {
            chunk_count: 1,
            sentence_count: 1,
            mention_count: 2,
            entity_count: 2,
            nli_candidate_count: 0,
            nli_adjudication_count: 0,
            chunker_micros: 0,
            dynamic_ner_micros: 0,
            nli_load_micros: 0,
            nli_adjudication_micros: 0,
            promotion_count: 0,
        },
    };
    Fixture {
        text,
        structural,
        ner,
    }
}

fn model(name: &str, seed: u8) -> AnalysisModelIdentity {
    AnalysisModelIdentity {
        model_id: name.to_owned(),
        artifact_hash: [seed; 32],
        config_hash: [seed.wrapping_add(1); 32],
        runtime_id: "rust-native".to_owned(),
    }
}

fn fnv64(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
