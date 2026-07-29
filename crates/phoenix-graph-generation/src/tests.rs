use crate::{
    write_graph_generation_new, AcceptedEdgeInput, CanonicalEntityInput, DurableDecisionInput,
    GraphGenerationError, GraphGenerationInput, ProducerCapabilityInput, VerifiedGraphGeneration,
    ACCEPTED_EDGE_FLAG_PROMOTED, DECISION_FLAG_DURABLE_RECEIPT, DECISION_STATUS_ACCEPTED,
    DECISION_STATUS_REJECTED,
};
use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisEntity, AnalysisEntityKind, AnalysisMention,
    AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord, AnalysisStageReceipt,
    DocumentAnalysisBinding, NliAdjudication, NliCandidate, NliCandidateKind, NliDecision,
    PhoenixDocumentAnalysisV1, PhoenixNerArtifactV1, PhoenixNliArtifactV1,
    PhoenixStructuralSubstrateV1, StructuralDialogueHint, StructuralSentenceQuality,
    StructuralSpanKind, ANALYSIS_CONTRACT, STRUCTURAL_SUBSTRATE_CONTRACT,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const TEXT: &str = "Alpha sees Beta.";

#[test]
fn exact_structural_records_round_trip_through_mmap() {
    let fixture = fixture();
    let path = unique_path("round-trip");
    let receipt = write(&path, &fixture);
    let opened = VerifiedGraphGeneration::open(&path).expect("open verified generation");
    assert_eq!(opened.generation_hash(), receipt.generation_hash);
    assert_eq!(opened.chunks().len(), 1);
    assert_eq!(opened.chunks()[0].start, fixture.structural.chunks[0].start);
    assert_eq!(opened.chunks()[0].end, fixture.structural.chunks[0].end);
    assert_eq!(
        opened.chunks()[0].content_hash,
        fixture.structural.chunks[0].content_hash
    );
    assert_eq!(opened.mentions().len(), 2);
    assert_eq!(opened.candidate_edges().len(), 1);
    assert_eq!(opened.string(opened.entities()[0].label).unwrap(), "Alpha");
    fs::remove_file(path).ok();
}

#[test]
fn exact_source_produces_exact_generation_hash_and_ids() {
    let fixture = fixture();
    let left_path = unique_path("determinism-left");
    let right_path = unique_path("determinism-right");
    let left = write(&left_path, &fixture);
    let right = write(&right_path, &fixture);
    assert_eq!(left.generation_hash, right.generation_hash);
    let left = VerifiedGraphGeneration::open(&left_path).unwrap();
    let right = VerifiedGraphGeneration::open(&right_path).unwrap();
    assert_eq!(left.chunks()[0].id, right.chunks()[0].id);
    assert_eq!(left.evidence()[0].id, right.evidence()[0].id);
    fs::remove_file(left_path).ok();
    fs::remove_file(right_path).ok();
}

#[test]
fn corruption_and_stale_binding_fail_closed() {
    let fixture = fixture();
    let path = unique_path("corrupt");
    write(&path, &fixture);
    let opened = VerifiedGraphGeneration::open(&path).unwrap();
    assert!(matches!(
        opened.verify_binding(7, 99, [7; 32], 2),
        Err(GraphGenerationError::Binding(_))
    ));
    drop(opened);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x5a;
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        VerifiedGraphGeneration::open(&path),
        Err(GraphGenerationError::Corrupt(_))
    ));
    fs::remove_file(path).ok();
}

#[test]
fn mismatched_structural_authority_is_never_written() {
    let mut fixture = fixture();
    fixture.structural.binding.document_revision += 1;
    let path = unique_path("mismatched");
    let canonical = fixture
        .analysis
        .ner
        .entities
        .iter()
        .map(|entity| CanonicalEntityInput {
            id: entity.stable_id,
            label: &entity.label,
            custom_kind: entity.custom_kind.as_deref(),
            mention_count: entity.mention_count,
            kind: entity.kind as u16,
            source_mask: 1,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        write_graph_generation_new(
            &path,
            &GraphGenerationInput {
                text: TEXT,
                analysis: &fixture.analysis,
                structural: &fixture.structural,
                canonical_entities: &canonical,
                accepted_edges: &[],
                decisions: &[],
                capabilities: &[],
            },
        ),
        Err(GraphGenerationError::Binding(_))
    ));
    assert!(!path.exists());
}

#[test]
fn promoted_edges_require_a_matching_durable_acceptance() {
    let fixture = fixture();
    let path = unique_path("promotion-without-receipt");
    let canonical = canonical_entities(&fixture);
    let promoted = promoted_edge();
    assert!(matches!(
        write_graph_generation_new(
            &path,
            &GraphGenerationInput {
                text: TEXT,
                analysis: &fixture.analysis,
                structural: &fixture.structural,
                canonical_entities: &canonical,
                accepted_edges: &[promoted],
                decisions: &[],
                capabilities: &[],
            },
        ),
        Err(GraphGenerationError::Binding(_))
    ));
    assert!(!path.exists());
}

#[test]
fn accepted_decisions_require_exactly_one_promoted_edge() {
    let fixture = fixture();
    let path = unique_path("receipt-without-promotion");
    let canonical = canonical_entities(&fixture);
    let accepted = decision(DECISION_STATUS_ACCEPTED);
    assert!(matches!(
        write_graph_generation_new(
            &path,
            &GraphGenerationInput {
                text: TEXT,
                analysis: &fixture.analysis,
                structural: &fixture.structural,
                canonical_entities: &canonical,
                accepted_edges: &[],
                decisions: &[accepted],
                capabilities: &[],
            },
        ),
        Err(GraphGenerationError::Binding(_))
    ));
    assert!(!path.exists());
}

#[test]
fn receipt_backed_promotion_round_trips_and_rejection_cannot_promote() {
    let fixture = fixture();
    let accepted_path = unique_path("receipt-backed-promotion");
    let canonical = canonical_entities(&fixture);
    write_graph_generation_new(
        &accepted_path,
        &GraphGenerationInput {
            text: TEXT,
            analysis: &fixture.analysis,
            structural: &fixture.structural,
            canonical_entities: &canonical,
            accepted_edges: &[promoted_edge()],
            decisions: &[decision(DECISION_STATUS_ACCEPTED)],
            capabilities: &[],
        },
    )
    .unwrap();
    let opened = VerifiedGraphGeneration::open(&accepted_path).unwrap();
    assert_eq!(
        opened
            .accepted_edges()
            .iter()
            .filter(|edge| edge.flags & ACCEPTED_EDGE_FLAG_PROMOTED != 0)
            .count(),
        1
    );
    assert_eq!(opened.decisions().len(), 1);

    let rejected_path = unique_path("rejected-cannot-promote");
    assert!(matches!(
        write_graph_generation_new(
            &rejected_path,
            &GraphGenerationInput {
                text: TEXT,
                analysis: &fixture.analysis,
                structural: &fixture.structural,
                canonical_entities: &canonical,
                accepted_edges: &[promoted_edge()],
                decisions: &[decision(DECISION_STATUS_REJECTED)],
                capabilities: &[],
            },
        ),
        Err(GraphGenerationError::Binding(_))
    ));
    fs::remove_file(accepted_path).ok();
}

struct Fixture {
    analysis: PhoenixDocumentAnalysisV1,
    structural: PhoenixStructuralSubstrateV1,
}

fn write(path: &PathBuf, fixture: &Fixture) -> crate::GraphGenerationWriteReceipt {
    let canonical = canonical_entities(fixture);
    write_graph_generation_new(
        path,
        &GraphGenerationInput {
            text: TEXT,
            analysis: &fixture.analysis,
            structural: &fixture.structural,
            canonical_entities: &canonical,
            accepted_edges: &[],
            decisions: &[],
            capabilities: &[ProducerCapabilityInput {
                name: "candidate-nli",
                producer: "test",
                supported: true,
                emitted: true,
                flags: 0,
            }],
        },
    )
    .expect("write generation")
}

fn canonical_entities(fixture: &Fixture) -> Vec<CanonicalEntityInput<'_>> {
    fixture
        .analysis
        .ner
        .entities
        .iter()
        .map(|entity| CanonicalEntityInput {
            id: entity.stable_id,
            label: &entity.label,
            custom_kind: entity.custom_kind.as_deref(),
            mention_count: entity.mention_count,
            kind: entity.kind as u16,
            source_mask: 1,
        })
        .collect()
}

fn promoted_edge() -> AcceptedEdgeInput {
    AcceptedEdgeInput {
        id: crate::promoted_edge_id([9; 32]),
        source_id: 101,
        target_id: 102,
        evidence_id: 0,
        weight: 0.9,
        relation: 4,
        flags: ACCEPTED_EDGE_FLAG_PROMOTED,
    }
}

fn decision(status: u16) -> DurableDecisionInput<'static> {
    DurableDecisionInput {
        id: 801,
        candidate_id: [9; 32],
        reason: "reviewed in test",
        decided_at_revision: 11,
        status,
        flags: DECISION_FLAG_DURABLE_RECEIPT,
    }
}

fn fixture() -> Fixture {
    let content_hash = *blake3::hash(TEXT.as_bytes()).as_bytes();
    let identity = AnalysisModelIdentity {
        model_id: "fixture-model".to_owned(),
        artifact_hash: [2; 32],
        config_hash: [3; 32],
        runtime_id: "fixture-runtime".to_owned(),
    };
    let binding = DocumentAnalysisBinding {
        source_document_id: "fixture-note".to_owned(),
        native_document_id: 7,
        document_revision: 11,
        content_hash,
        analysis_generation: 13,
        source_registry_revision: 1,
        target_registry_revision: 2,
        producer_binary_hash: [1; 32],
        chunker: identity.clone(),
        dynamic_ner: identity.clone(),
        nli: identity,
    };
    let receipt = AnalysisStageReceipt {
        chunk_count: 1,
        sentence_count: 1,
        mention_count: 2,
        entity_count: 2,
        nli_candidate_count: 1,
        nli_adjudication_count: 1,
        chunker_micros: 10,
        dynamic_ner_micros: 20,
        nli_load_micros: 30,
        nli_adjudication_micros: 40,
        promotion_count: 0,
    };
    let candidate_id = [9; 32];
    let analysis = PhoenixDocumentAnalysisV1 {
        schema: ANALYSIS_CONTRACT.to_owned(),
        ner: PhoenixNerArtifactV1 {
            binding: binding.clone(),
            ner_revision: binding.analysis_generation,
            entities: vec![
                AnalysisEntity {
                    stable_id: 101,
                    label: "Alpha".to_owned(),
                    kind: AnalysisEntityKind::Character,
                    custom_kind: None,
                    mention_count: 1,
                },
                AnalysisEntity {
                    stable_id: 102,
                    label: "Beta".to_owned(),
                    kind: AnalysisEntityKind::Character,
                    custom_kind: None,
                    mention_count: 1,
                },
            ],
            mentions: vec![
                AnalysisMention {
                    mention_id: 201,
                    entity_id: 101,
                    start: 0,
                    end: 5,
                    sentence_index: 0,
                    confidence: 0.9,
                    accepted: true,
                },
                AnalysisMention {
                    mention_id: 202,
                    entity_id: 102,
                    start: 11,
                    end: 15,
                    sentence_index: 0,
                    confidence: 0.8,
                    accepted: true,
                },
            ],
            receipt,
        },
        nli: PhoenixNliArtifactV1 {
            binding: binding.clone(),
            nli_candidates: vec![NliCandidate {
                candidate_id,
                kind: NliCandidateKind::Related,
                left_entity_id: 101,
                right_entity_id: 102,
                premise_start: 0,
                premise_end: TEXT.len() as u32,
                premise: TEXT.to_owned(),
                hypothesis: "Alpha is related to Beta.".to_owned(),
            }],
            nli_adjudications: vec![NliAdjudication {
                candidate_id,
                decision: NliDecision::Supported,
                entailment_millis: 800,
                contradiction_millis: 100,
                neutral_millis: 100,
                confidence_millis: 800,
                needs_human_review: true,
            }],
            promotion_count: 0,
        },
    };
    let structural = PhoenixStructuralSubstrateV1 {
        schema: STRUCTURAL_SUBSTRATE_CONTRACT.to_owned(),
        binding,
        source_len: TEXT.len() as u32,
        chunks: vec![AnalysisChunkRecord {
            start: 0,
            end: TEXT.len() as u32,
            sentence_start: 0,
            sentence_end: 1,
            paragraph_start: 0,
            paragraph_end: 1,
            chapter_index: 0,
            token_count: 3,
            content_hash: 77,
            dialogue_hint: StructuralDialogueHint::None,
        }],
        sentences: vec![AnalysisSentenceRecord {
            start: 0,
            end: TEXT.len() as u32,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 3,
            content_hash: 77,
            quality: StructuralSentenceQuality::Complete,
            dialogue_hint: StructuralDialogueHint::None,
        }],
        spans: vec![
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Paragraph,
                start: 0,
                end: TEXT.len() as u32,
                parent_index: 0,
                child_start: 0,
                child_end: 1,
                token_count: 3,
                content_hash: 77,
                label: String::new(),
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Chapter,
                start: 0,
                end: TEXT.len() as u32,
                parent_index: u32::MAX,
                child_start: 0,
                child_end: 1,
                token_count: 3,
                content_hash: 77,
                label: "Document".to_owned(),
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
    };
    Fixture {
        analysis,
        structural,
    }
}

fn unique_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phoenix-graph-generation-{label}-{}-{nonce}.phxgg",
        std::process::id()
    ))
}
