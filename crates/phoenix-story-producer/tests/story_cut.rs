use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisEntity, AnalysisEntityKind, AnalysisMention,
    AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord, AnalysisStageReceipt,
    DocumentAnalysisBinding, PhoenixNerArtifactV1, PhoenixStructuralSubstrateV1,
    StructuralDialogueHint, StructuralSentenceQuality, StructuralSpanKind, NO_STRUCTURAL_PARENT,
    STRUCTURAL_SUBSTRATE_CONTRACT,
};
use phoenix_document_producer::{publish_or_reuse_structural_generation, StructuralProducerInput};
use phoenix_entity_producer::{publish_entity_generation_new, EntityProducerInput};
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateId, CandidateStatus, CapabilityRecord,
    CapabilityState, CausalCandidateRecord, ContextualEvidenceRecord, DecisionAction,
    DecisionRecord, EntityId, EpisodeMembershipRecord, EpisodeRecord, EventRecord, EvidenceId,
    EvidenceRecord, MemoryStateCandidateRecord, MentionRecord, PageKind, ProducerProduct,
    TemporalCandidateRecord, TypedRelationshipCandidateRecord,
};
use phoenix_semantic_lens::{write_semantic_lens_pack_new, CandidateKeyBuilder, CoreSemanticClass};
use phoenix_semantic_review::{
    append_authority_record, publish_reviewed_generation_new, rollback_authority, DecisionCommand,
    DecisionLedger,
};
use phoenix_story_producer::{
    derive_causal_candidate_id, derive_episode_id, derive_event_id, derive_memory_candidate_id,
    derive_relationship_candidate_id, derive_temporal_candidate_id,
    publish_deterministic_story_generation_new, publish_story_generation_new, story_review_catalog,
    CausalCandidateInput, CausalRelation, DeterministicStoryProducerInput, EpisodeCandidateInput,
    EpisodeFamily, EpisodeMember, EpisodeMembershipInput, EventCandidateInput, EventKind,
    MemoryStateCandidateInput, MemoryStateKind, ModelIdentityInput, ModelRankingBatch,
    ModelScoreInput, ProducerRegistration, RelationshipCandidateInput, RelationshipKind,
    SemanticEndpoint, StoryProducerError, StoryProducerInput, StoryRegistrations,
    TemporalCandidateInput, TemporalRelation,
};

const RELATIONSHIP_PRODUCER: &str = "deterministic/relationship-v1";
const EVENT_PRODUCER: &str = "deterministic/event-v1";
const EPISODE_PRODUCER: &str = "deterministic/episode-v1";
const TEMPORAL_PRODUCER: &str = "deterministic/temporal-v1";
const CAUSAL_PRODUCER: &str = "deterministic/causal-v1";
const MEMORY_PRODUCER: &str = "deterministic/memory-v1";

#[test]
fn every_story_output_is_proposed_and_evidence_bound() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let published = publish_full_story(
        story_dir.path().join("story.phxgg2"),
        &fixture,
        entities.generation(),
        true,
    )
    .unwrap();
    let generation = published.generation();

    let relationships: &[TypedRelationshipCandidateRecord] = generation
        .typed_page(PageKind::TypedRelationshipCandidates)
        .unwrap();
    let events: &[EventRecord] = generation.typed_page(PageKind::Events).unwrap();
    let episodes: &[EpisodeRecord] = generation.typed_page(PageKind::Episodes).unwrap();
    let memberships: &[EpisodeMembershipRecord] =
        generation.typed_page(PageKind::EpisodeMemberships).unwrap();
    let temporal: &[TemporalCandidateRecord] =
        generation.typed_page(PageKind::TemporalCandidates).unwrap();
    let causal: &[CausalCandidateRecord] =
        generation.typed_page(PageKind::CausalCandidates).unwrap();
    let memory: &[MemoryStateCandidateRecord] = generation
        .typed_page(PageKind::MemoryStateCandidates)
        .unwrap();
    let bindings: &[CandidateEvidenceBindingRecord] = generation
        .typed_page(PageKind::CandidateEvidenceBindings)
        .unwrap();
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence).unwrap();
    let evidence_ids = evidence
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        (
            relationships.len(),
            events.len(),
            episodes.len(),
            memberships.len(),
            temporal.len(),
            causal.len(),
            memory.len()
        ),
        (1, 2, 1, 2, 1, 1, 1)
    );
    assert!(relationships
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count >= 2));
    assert_eq!(f32::from_bits(relationships[0].confidence_bits), 0.93);
    assert_ne!(relationships[0].flags, 0);
    assert!(events
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count > 0));
    assert!(episodes
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.membership_count == 2));
    assert_eq!(
        generation.resolve_string(episodes[0].label).unwrap(),
        "Mara warned Ivo"
    );
    assert!(memberships
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count > 0));
    assert!(temporal
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count > 0));
    assert!(causal
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count > 0));
    assert!(memory
        .iter()
        .all(|row| row.status == CandidateStatus::Proposed as u16 && row.evidence_count > 0));
    assert!(bindings
        .iter()
        .all(|binding| evidence_ids.contains(&binding.evidence_id)));
    assert_eq!(
        generation
            .typed_page::<DecisionRecord>(PageKind::Decisions)
            .unwrap()
            .len(),
        0,
        "story production must not create promotion receipts"
    );
    assert_eq!(
        generation.descriptor(PageKind::StructuralEdges).hash,
        entities
            .generation()
            .descriptor(PageKind::StructuralEdges)
            .hash,
        "candidate production must not mutate accepted topology"
    );
    assert_eq!(published.receipt().model_ranked_count, 1);
    assert_eq!(published.receipt().unsupported_mask, 0);
    let lens_pack = write_semantic_lens_pack_new(
        &story_dir.path().join("story.pslp"),
        &phoenix_story_producer::story_lens_definition(),
        generation.header().generation_hash,
    )
    .unwrap();
    assert_eq!(
        lens_pack.header().bound_generation_hash,
        generation.header().generation_hash
    );
    assert_eq!(
        lens_pack.identity(),
        phoenix_story_producer::story_lens_identity().unwrap()
    );
}

#[test]
fn deterministic_coordinator_populates_every_supported_semantic_lane() {
    let mut fixture = fixture();
    let first_start = fixture.text.find("Mara warned Ivo.").unwrap() as u32;
    let first_end = first_start + "Mara warned Ivo.".len() as u32;
    let second_start = fixture.text.find("Ivo fled because").unwrap() as u32;
    let second_end = fixture.text.len() as u32;
    fixture.structural.sentences = vec![
        AnalysisSentenceRecord {
            start: first_start,
            end: first_end,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 3,
            content_hash: fnv64(&fixture.text[first_start as usize..first_end as usize]),
            quality: StructuralSentenceQuality::Complete,
            dialogue_hint: StructuralDialogueHint::None,
        },
        AnalysisSentenceRecord {
            start: second_start,
            end: second_end,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 8,
            content_hash: fnv64(&fixture.text[second_start as usize..]),
            quality: StructuralSentenceQuality::Complete,
            dialogue_hint: StructuralDialogueHint::None,
        },
    ];
    fixture.structural.chunks[0].sentence_end = 2;
    for mention in &mut fixture.ner.mentions {
        mention.sentence_index = u32::from(mention.start >= second_start);
    }

    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let mentions: &[MentionRecord] = entities
        .generation()
        .typed_page(PageKind::Mentions)
        .unwrap();
    let pair = mentions
        .windows(2)
        .find(|pair| pair[0].entity_id != pair[1].entity_id)
        .unwrap();
    let contextual = [ContextualEvidenceRecord {
        source_entity_id: pair[0].entity_id,
        target_entity_id: pair[1].entity_id,
        source_mention_id: pair[0].id,
        target_mention_id: pair[1].id,
        chunk_id: pair[0].chunk_id,
        weight_bits: 0.75_f32.to_bits(),
        byte_distance: pair[1].start.saturating_sub(pair[0].end),
        flags: 0,
        reserved: 0,
    }];

    let published = publish_deterministic_story_generation_new(
        story_dir.path().join("deterministic.phxgg2"),
        DeterministicStoryProducerInput {
            text: &fixture.text,
            source: entities.generation(),
            contextual_evidence: &contextual,
            producer_binary_hash: [89; 32],
            published_generation: 21,
        },
    )
    .unwrap();
    let generation = published.generation();

    for page in [
        PageKind::TypedRelationshipCandidates,
        PageKind::Events,
        PageKind::Episodes,
        PageKind::EpisodeMemberships,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
        PageKind::ContextualEvidence,
    ] {
        assert!(generation.descriptor(page).count > 0, "{page:?} is empty");
    }
    assert_eq!(
        generation.descriptor(PageKind::Decisions).count,
        0,
        "deterministic production cannot promote its own candidates"
    );
    let episodes: &[EpisodeRecord] = generation.typed_page(PageKind::Episodes).unwrap();
    assert_ne!(
        generation.resolve_string(episodes[0].label).unwrap(),
        "Episode 1",
        "episode identity must remain source-bound"
    );
    let capabilities: &[CapabilityRecord] = generation.typed_page(PageKind::Capabilities).unwrap();
    for product in [
        ProducerProduct::Relationships,
        ProducerProduct::Events,
        ProducerProduct::Episodes,
        ProducerProduct::Temporal,
        ProducerProduct::Causal,
        ProducerProduct::MemoryState,
        ProducerProduct::ContextualEvidence,
    ] {
        assert!(capabilities.iter().any(|row| {
            row.product == product as u16 && row.state == CapabilityState::Produced as u16
        }));
    }
}

#[test]
fn durable_review_is_idempotent_restart_safe_and_atomically_rollbackable() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let authority_dir = tempfile::tempdir().unwrap();
    let ledger_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let source_path = authority_dir.path().join("story.phxgg2");
    let published =
        publish_full_story(source_path.clone(), &fixture, entities.generation(), true).unwrap();
    let source = published.generation();
    let catalog = story_review_catalog(source).unwrap();
    assert_eq!(catalog.candidates().len(), 9);

    let candidate = catalog
        .candidates()
        .iter()
        .find(|candidate| {
            candidate.location.page == phoenix_semantic_review::ReviewPage::TypedRelationship as u16
        })
        .copied()
        .unwrap();
    let command = DecisionCommand {
        candidate_id: candidate.binding.origin.candidate_id,
        expected_source_generation_hash: source.header().generation_hash,
        expected_candidate_hash: candidate.binding.candidate_hash,
        expected_evidence_hash: candidate.binding.evidence_hash,
        action: DecisionAction::Accept,
        reason: "Exact evidence supports this relationship.".to_owned(),
        decided_at_unix_millis: 1_753_776_000_000,
    };
    let mut ledger = DecisionLedger::open(ledger_dir.path()).unwrap();
    let first = ledger.decide(&catalog, &command).unwrap();
    let repeated = ledger.decide(&catalog, &command).unwrap();
    assert!(!first.reused);
    assert!(repeated.reused);
    assert_eq!(first.receipt_id, repeated.receipt_id);
    assert_eq!(ledger.receipts().len(), 1);
    drop(ledger);

    let ledger = DecisionLedger::open(ledger_dir.path()).unwrap();
    assert_eq!(ledger.receipts().len(), 1);
    assert_eq!(
        ledger.head(command.candidate_id).unwrap().reason(),
        command.reason
    );

    let output_path = authority_dir.path().join("reviewed.phxgg2");
    let (reviewed, receipt) =
        publish_reviewed_generation_new(&output_path, source, &catalog, &ledger, 1_753_776_000_100)
            .unwrap();
    assert_eq!(receipt.accepted_count, 1);
    assert_eq!(receipt.superseded_decision_count, 0);
    assert_eq!(
        reviewed
            .typed_page::<TypedRelationshipCandidateRecord>(PageKind::TypedRelationshipCandidates)
            .unwrap()[0]
            .status,
        CandidateStatus::Accepted as u16
    );
    assert_eq!(
        reviewed.descriptor(PageKind::StructuralEdges).hash,
        source.descriptor(PageKind::StructuralEdges).hash
    );
    let decisions: &[DecisionRecord] = reviewed.typed_page(PageKind::Decisions).unwrap();
    assert!(decisions.iter().any(|decision| {
        decision.candidate_id == command.candidate_id
            && decision.status == CandidateStatus::Accepted as u16
            && decision.evidence_hash == command.expected_evidence_hash
    }));

    let changed_source_path = authority_dir.path().join("story-reranked.phxgg2");
    let changed_story =
        publish_full_story(changed_source_path, &fixture, entities.generation(), false).unwrap();
    let changed_catalog = story_review_catalog(changed_story.generation()).unwrap();
    let superseded_path = authority_dir.path().join("superseded.phxgg2");
    let (superseded_generation, superseded_receipt) = publish_reviewed_generation_new(
        superseded_path,
        changed_story.generation(),
        &changed_catalog,
        &ledger,
        1_753_776_000_200,
    )
    .unwrap();
    assert_eq!(superseded_receipt.accepted_count, 0);
    assert_eq!(superseded_receipt.superseded_decision_count, 1);
    assert_eq!(
        superseded_generation
            .typed_page::<TypedRelationshipCandidateRecord>(PageKind::TypedRelationshipCandidates)
            .unwrap()[0]
            .status,
        CandidateStatus::Proposed as u16
    );
    assert!(superseded_generation
        .typed_page::<DecisionRecord>(PageKind::Decisions)
        .unwrap()
        .iter()
        .any(|decision| {
            decision.candidate_id == command.candidate_id
                && decision.status == CandidateStatus::Superseded as u16
        }));

    let initial = append_authority_record(authority_dir.path(), &source_path).unwrap();
    assert_eq!(
        initial.generation().header().generation_hash,
        source.header().generation_hash
    );
    let current = append_authority_record(authority_dir.path(), &output_path).unwrap();
    assert_eq!(
        current.generation().header().generation_hash,
        reviewed.header().generation_hash
    );
    let rolled_back = rollback_authority(authority_dir.path()).unwrap();
    assert_eq!(
        rolled_back.generation().header().generation_hash,
        source.header().generation_hash
    );
    assert_eq!(rolled_back.header().sequence, 3);
    assert!(source_path.exists());
    assert!(output_path.exists());
}

#[test]
fn changed_candidate_binding_is_rejected_as_stale() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let ledger_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let published = publish_full_story(
        story_dir.path().join("story.phxgg2"),
        &fixture,
        entities.generation(),
        true,
    )
    .unwrap();
    let catalog = story_review_catalog(published.generation()).unwrap();
    let candidate = catalog.candidates()[0];
    let stale = DecisionCommand {
        candidate_id: candidate.binding.origin.candidate_id,
        expected_source_generation_hash: catalog.authority().source_generation_hash,
        expected_candidate_hash: *blake3::hash(b"changed binding").as_bytes(),
        expected_evidence_hash: candidate.binding.evidence_hash,
        action: DecisionAction::Accept,
        reason: "stale".to_owned(),
        decided_at_unix_millis: 1,
    };
    let mut ledger = DecisionLedger::open(ledger_dir.path()).unwrap();
    assert!(matches!(
        ledger.decide(&catalog, &stale),
        Err(phoenix_semantic_review::SemanticReviewError::StaleCandidate)
    ));
    assert!(ledger.receipts().is_empty());
}

#[test]
fn unsupported_and_supported_empty_capabilities_are_distinct() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let empty_relationships = [];
    let published = publish_story_generation_new(
        story_dir.path().join("capabilities.phxgg2"),
        StoryProducerInput {
            text: &fixture.text,
            source: entities.generation(),
            registrations: StoryRegistrations {
                relationships: ProducerRegistration::Deterministic {
                    producer_id: RELATIONSHIP_PRODUCER,
                    rules: &empty_relationships,
                },
                events: unsupported("events/unavailable"),
                episodes: unsupported("episodes/unavailable"),
                temporal: unsupported("temporal/unavailable"),
                causal: unsupported("causal/unavailable"),
                memory_state: unsupported("memory/unavailable"),
            },
            contextual_evidence: &[],
            model_ranking: None,
            producer_binary_hash: [61; 32],
            published_generation: 15,
        },
    )
    .unwrap();
    let capabilities: &[CapabilityRecord] = published
        .generation()
        .typed_page(PageKind::Capabilities)
        .unwrap();
    let relationship = capabilities
        .iter()
        .rfind(|row| row.product == ProducerProduct::Relationships as u16)
        .unwrap();
    let episodes = capabilities
        .iter()
        .rfind(|row| row.product == ProducerProduct::Episodes as u16)
        .unwrap();

    assert_eq!(relationship.state, CapabilityState::Produced as u16);
    assert_eq!(relationship.output_count, 0);
    assert_eq!(episodes.state, CapabilityState::Unsupported as u16);
    assert_eq!(episodes.output_count, 0);
    assert_eq!(published.receipt().unsupported_mask, 0b11_1110);
}

#[test]
fn synthetic_episode_label_and_unknown_model_score_fail_closed() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let evidence = evidence_for(entities.generation(), EntityId(101));
    let chunk = entities
        .generation()
        .typed_page::<phoenix_graph_generation_v2::ChunkRecord>(PageKind::Chunks)
        .unwrap()[0];
    let memberships = [EpisodeMembershipInput {
        member: EpisodeMember::Chunk(phoenix_graph_generation_v2::ChunkId(chunk.id)),
        evidence_ids: &evidence,
        confidence: 1.0,
    }];
    let episodes = [EpisodeCandidateInput {
        episode_id: phoenix_graph_generation_v2::EpisodeId(7),
        label: "Episode 1",
        label_start: 0,
        label_end: 9,
        evidence_ids: &evidence,
        memberships: &memberships,
        ordinal: 0,
        family: EpisodeFamily::Scene,
        confidence: 1.0,
    }];
    let error = publish_story_generation_new(
        story_dir.path().join("synthetic.phxgg2"),
        StoryProducerInput {
            text: &fixture.text,
            source: entities.generation(),
            registrations: StoryRegistrations {
                relationships: unsupported("relationships/unavailable"),
                events: unsupported("events/unavailable"),
                episodes: ProducerRegistration::Deterministic {
                    producer_id: EPISODE_PRODUCER,
                    rules: &episodes,
                },
                temporal: unsupported("temporal/unavailable"),
                causal: unsupported("causal/unavailable"),
                memory_state: unsupported("memory/unavailable"),
            },
            contextual_evidence: &[],
            model_ranking: None,
            producer_binary_hash: [61; 32],
            published_generation: 15,
        },
    )
    .err()
    .unwrap();
    assert!(matches!(error, StoryProducerError::InvalidSourceLabel));

    let unknown_scores = [ModelScoreInput {
        candidate_id: CandidateId([99; 32]),
        confidence: 0.9,
    }];
    let error = publish_empty_with_ranking(
        story_dir.path().join("unknown-rank.phxgg2"),
        &fixture,
        entities.generation(),
        &unknown_scores,
    )
    .err()
    .unwrap();
    assert!(matches!(error, StoryProducerError::InvalidModelRanking));
}

#[test]
fn identical_rules_produce_byte_identical_candidate_pages() {
    let fixture = fixture();
    let structural_dir = tempfile::tempdir().unwrap();
    let entity_dir = tempfile::tempdir().unwrap();
    let story_dir = tempfile::tempdir().unwrap();
    let structural = publish_structure(structural_dir.path(), &fixture);
    let entities = publish_entities(
        entity_dir.path().join("entity.phxgg2"),
        &fixture,
        structural.generation(),
    );
    let left = publish_full_story(
        story_dir.path().join("left.phxgg2"),
        &fixture,
        entities.generation(),
        false,
    )
    .unwrap();
    let right = publish_full_story(
        story_dir.path().join("right.phxgg2"),
        &fixture,
        entities.generation(),
        false,
    )
    .unwrap();

    assert_eq!(
        left.receipt().generation_hash,
        right.receipt().generation_hash
    );
    assert_eq!(
        left.receipt().candidate_authority_hash,
        right.receipt().candidate_authority_hash
    );
    assert_eq!(
        std::fs::read(left.receipt().path.as_path()).unwrap(),
        std::fs::read(right.receipt().path.as_path()).unwrap()
    );
}

fn publish_full_story(
    path: std::path::PathBuf,
    fixture: &Fixture,
    source: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
    rank_relationship: bool,
) -> Result<phoenix_story_producer::VerifiedStoryGeneration, StoryProducerError> {
    let content_hash = source.header().content_hash;
    let mara_evidence = evidence_for(source, EntityId(101));
    let ivo_evidence = evidence_for(source, EntityId(202));
    let fire_evidence = evidence_for(source, EntityId(303));

    let mut relationship = RelationshipCandidateInput {
        candidate_id: CandidateId::ZERO,
        source_entity_id: EntityId(101),
        target_entity_id: EntityId(202),
        source_evidence_id: mara_evidence[0],
        target_evidence_id: ivo_evidence[0],
        additional_evidence_ids: &[],
        relation: RelationshipKind::CommunicatesWith,
        confidence: 0.78,
    };
    relationship.candidate_id =
        derive_relationship_candidate_id(&content_hash, RELATIONSHIP_PRODUCER, &relationship);
    let relationships = [relationship];

    let warning_start = fixture.text.find("Mara warned Ivo").unwrap() as u32;
    let warning_end = warning_start + "Mara warned Ivo".len() as u32;
    let warning_evidence = sorted(&[mara_evidence[0], ivo_evidence[0]]);
    let mut warning = EventCandidateInput {
        event_id: phoenix_graph_generation_v2::EventId(0),
        label: "Mara warned Ivo",
        label_start: warning_start,
        label_end: warning_end,
        evidence_ids: &warning_evidence,
        kind: EventKind::Encounter,
        confidence: 0.88,
    };
    warning.event_id = derive_event_id(&content_hash, EVENT_PRODUCER, &warning);

    let flight_start = fixture.text.find("Ivo fled").unwrap() as u32;
    let flight_end = flight_start + "Ivo fled".len() as u32;
    let flight_evidence = sorted(&[ivo_evidence[1], mara_evidence[1]]);
    let mut flight = EventCandidateInput {
        event_id: phoenix_graph_generation_v2::EventId(0),
        label: "Ivo fled",
        label_start: flight_start,
        label_end: flight_end,
        evidence_ids: &flight_evidence,
        kind: EventKind::Action,
        confidence: 0.91,
    };
    flight.event_id = derive_event_id(&content_hash, EVENT_PRODUCER, &flight);
    let events = [warning, flight];

    let episode_evidence = sorted(&[
        mara_evidence[0],
        ivo_evidence[0],
        ivo_evidence[1],
        mara_evidence[1],
    ]);
    let memberships = [
        EpisodeMembershipInput {
            member: EpisodeMember::Event(warning.event_id),
            evidence_ids: &warning_evidence,
            confidence: 0.9,
        },
        EpisodeMembershipInput {
            member: EpisodeMember::Event(flight.event_id),
            evidence_ids: &flight_evidence,
            confidence: 0.9,
        },
    ];
    let mut episode = EpisodeCandidateInput {
        episode_id: phoenix_graph_generation_v2::EpisodeId(0),
        label: "Mara warned Ivo",
        label_start: warning_start,
        label_end: warning_end,
        evidence_ids: &episode_evidence,
        memberships: &memberships,
        ordinal: 0,
        family: EpisodeFamily::Conflict,
        confidence: 0.84,
    };
    episode.episode_id = derive_episode_id(&content_hash, EPISODE_PRODUCER, &episode);
    let episodes = [episode];

    let sequence_evidence = sorted(&[mara_evidence[0], ivo_evidence[1]]);
    let mut temporal = TemporalCandidateInput {
        candidate_id: CandidateId::ZERO,
        source: SemanticEndpoint::Event(warning.event_id),
        target: SemanticEndpoint::Event(flight.event_id),
        evidence_ids: &sequence_evidence,
        relation: TemporalRelation::Before,
        confidence: 0.92,
    };
    temporal.candidate_id =
        derive_temporal_candidate_id(&content_hash, TEMPORAL_PRODUCER, &temporal);
    let temporal_rules = [temporal];

    let mut causal = CausalCandidateInput {
        candidate_id: CandidateId::ZERO,
        cause: SemanticEndpoint::Event(warning.event_id),
        effect: SemanticEndpoint::Event(flight.event_id),
        evidence_ids: &sequence_evidence,
        relation: CausalRelation::Motivates,
        confidence: 0.73,
    };
    causal.candidate_id = derive_causal_candidate_id(&content_hash, CAUSAL_PRODUCER, &causal);
    let causal_rules = [causal];

    let memory_evidence = sorted(&[mara_evidence[1], fire_evidence[0]]);
    let mut memory = MemoryStateCandidateInput {
        candidate_id: CandidateId::ZERO,
        subject_entity_id: EntityId(101),
        context: SemanticEndpoint::Event(flight.event_id),
        key: "remembers",
        value: "the fire",
        evidence_ids: &memory_evidence,
        kind: MemoryStateKind::Remembers,
        confidence: 0.95,
    };
    memory.candidate_id = derive_memory_candidate_id(&content_hash, MEMORY_PRODUCER, &memory);
    let memory_rules = [memory];

    let scores = [ModelScoreInput {
        candidate_id: relationship.candidate_id,
        confidence: 0.93,
    }];
    let ranking = rank_relationship.then_some(ModelRankingBatch {
        model: ModelIdentityInput {
            name: "fixture/story-ranker",
            runtime: "test-only",
            artifact_uri: "",
            artifact_hash: [71; 32],
            config_hash: [72; 32],
        },
        scores: &scores,
    });
    publish_story_generation_new(
        path,
        StoryProducerInput {
            text: &fixture.text,
            source,
            registrations: StoryRegistrations {
                relationships: ProducerRegistration::Deterministic {
                    producer_id: RELATIONSHIP_PRODUCER,
                    rules: &relationships,
                },
                events: ProducerRegistration::Deterministic {
                    producer_id: EVENT_PRODUCER,
                    rules: &events,
                },
                episodes: ProducerRegistration::Deterministic {
                    producer_id: EPISODE_PRODUCER,
                    rules: &episodes,
                },
                temporal: ProducerRegistration::Deterministic {
                    producer_id: TEMPORAL_PRODUCER,
                    rules: &temporal_rules,
                },
                causal: ProducerRegistration::Deterministic {
                    producer_id: CAUSAL_PRODUCER,
                    rules: &causal_rules,
                },
                memory_state: ProducerRegistration::Deterministic {
                    producer_id: MEMORY_PRODUCER,
                    rules: &memory_rules,
                },
            },
            contextual_evidence: &[],
            model_ranking: ranking,
            producer_binary_hash: [61; 32],
            published_generation: 15,
        },
    )
}

fn publish_empty_with_ranking(
    path: std::path::PathBuf,
    fixture: &Fixture,
    source: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
    scores: &[ModelScoreInput],
) -> Result<phoenix_story_producer::VerifiedStoryGeneration, StoryProducerError> {
    let empty_relationships = [];
    publish_story_generation_new(
        path,
        StoryProducerInput {
            text: &fixture.text,
            source,
            registrations: StoryRegistrations {
                relationships: ProducerRegistration::Deterministic {
                    producer_id: RELATIONSHIP_PRODUCER,
                    rules: &empty_relationships,
                },
                events: unsupported("events/unavailable"),
                episodes: unsupported("episodes/unavailable"),
                temporal: unsupported("temporal/unavailable"),
                causal: unsupported("causal/unavailable"),
                memory_state: unsupported("memory/unavailable"),
            },
            contextual_evidence: &[],
            model_ranking: Some(ModelRankingBatch {
                model: ModelIdentityInput {
                    name: "fixture/story-ranker",
                    runtime: "test-only",
                    artifact_uri: "",
                    artifact_hash: [71; 32],
                    config_hash: [72; 32],
                },
                scores,
            }),
            producer_binary_hash: [61; 32],
            published_generation: 15,
        },
    )
}

fn unsupported<T>(producer_id: &'static str) -> ProducerRegistration<'static, T> {
    ProducerRegistration::Unsupported { producer_id }
}

#[test]
fn story_v1_lens_formalization_preserves_existing_candidate_identity() {
    let content_hash = *blake3::hash(b"story identity fixture").as_bytes();
    let relationship = RelationshipCandidateInput {
        candidate_id: CandidateId::ZERO,
        source_entity_id: EntityId(11),
        target_entity_id: EntityId(12),
        source_evidence_id: EvidenceId(101),
        target_evidence_id: EvidenceId(102),
        additional_evidence_ids: &[EvidenceId(103)],
        relation: RelationshipKind::Supports,
        confidence: 0.75,
    };
    let legacy =
        derive_relationship_candidate_id(&content_hash, RELATIONSHIP_PRODUCER, &relationship);
    let mut generic = CandidateKeyBuilder::producer_scoped(
        phoenix_story_producer::STORY_CANDIDATE_NAMESPACE,
        b"relationship",
        &content_hash,
        RELATIONSHIP_PRODUCER,
    )
    .unwrap();
    generic
        .update_u64(relationship.source_entity_id.0)
        .update_u64(relationship.target_entity_id.0)
        .update_u16(relationship.relation as u16)
        .update_u64(relationship.source_evidence_id.0)
        .update_u64(relationship.target_evidence_id.0)
        .update_u64(relationship.additional_evidence_ids[0].0);
    assert_eq!(legacy, generic.finish());

    let identity = phoenix_story_producer::story_lens_identity().unwrap();
    let origin = phoenix_story_producer::story_candidate_origin(
        legacy,
        CoreSemanticClass::Relation,
        relationship.relation as u16,
    )
    .unwrap();
    assert_eq!(origin.lens_id, identity.lens_id);
    assert_eq!(origin.vocabulary_hash, identity.vocabulary_hash);
    assert_eq!(
        origin.semantic_code,
        phoenix_story_producer::story_semantic_code(
            CoreSemanticClass::Relation,
            RelationshipKind::Supports as u16
        )
    );
}

fn evidence_for(
    generation: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
    entity: EntityId,
) -> Vec<EvidenceId> {
    let mut rows = generation
        .typed_page::<EvidenceRecord>(PageKind::Evidence)
        .unwrap()
        .iter()
        .filter(|record| record.entity_id == entity.0)
        .map(|record| (record.start, EvidenceId(record.id)))
        .collect::<Vec<_>>();
    rows.sort_unstable_by_key(|(start, _)| *start);
    rows.into_iter().map(|(_, id)| id).collect()
}

fn sorted(values: &[EvidenceId]) -> Vec<EvidenceId> {
    let mut values = values.to_vec();
    values.sort_unstable();
    values.dedup();
    values
}

fn publish_entities(
    path: std::path::PathBuf,
    fixture: &Fixture,
    structural: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
) -> phoenix_entity_producer::VerifiedEntityGeneration {
    publish_entity_generation_new(
        path,
        EntityProducerInput {
            text: &fixture.text,
            structural,
            ner: &fixture.ner,
            user_entities: &[],
            user_mentions: &[],
            merge_decisions: &[],
            identity_candidates: &[],
            published_generation: 14,
        },
    )
    .unwrap()
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
    let text =
        "## Chapter 1\nMara warned Ivo. Ivo fled because Mara remembered the fire.".to_owned();
    let body_start = text.find("Mara").unwrap();
    let binding = DocumentAnalysisBinding {
        source_document_id: "fixture:story-candidates".to_owned(),
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
            token_count: 12,
            content_hash: fnv64(&text),
            dialogue_hint: StructuralDialogueHint::None,
        }],
        sentences: vec![AnalysisSentenceRecord {
            start: body_start as u32,
            end: text.len() as u32,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 11,
            content_hash: fnv64(&text[body_start..]),
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
                token_count: 12,
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
                token_count: 12,
                content_hash: fnv64(&text),
                label: "Chapter 1".to_owned(),
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
    };
    let mara_starts = occurrences(&text, "Mara");
    let ivo_starts = occurrences(&text, "Ivo");
    let fire_start = text.find("fire").unwrap() as u32;
    let ner = PhoenixNerArtifactV1 {
        binding,
        ner_revision: 1,
        entities: vec![
            entity(101, "Mara", AnalysisEntityKind::Character, 2),
            entity(202, "Ivo", AnalysisEntityKind::Character, 2),
            entity(303, "fire", AnalysisEntityKind::Concept, 1),
        ],
        mentions: vec![
            mention(1_001, 101, mara_starts[0], "Mara".len()),
            mention(1_002, 202, ivo_starts[0], "Ivo".len()),
            mention(1_003, 202, ivo_starts[1], "Ivo".len()),
            mention(1_004, 101, mara_starts[1], "Mara".len()),
            mention(1_005, 303, fire_start, "fire".len()),
        ],
        receipt: AnalysisStageReceipt {
            chunk_count: 1,
            sentence_count: 1,
            mention_count: 5,
            entity_count: 3,
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

fn entity(
    stable_id: u64,
    label: &str,
    kind: AnalysisEntityKind,
    mention_count: u32,
) -> AnalysisEntity {
    AnalysisEntity {
        stable_id,
        label: label.to_owned(),
        kind,
        custom_kind: None,
        mention_count,
    }
}

fn mention(id: u64, entity_id: u64, start: u32, len: usize) -> AnalysisMention {
    AnalysisMention {
        mention_id: id,
        entity_id,
        start,
        end: start + len as u32,
        sentence_index: 0,
        confidence: 0.98,
        accepted: true,
    }
}

fn occurrences(text: &str, needle: &str) -> Vec<u32> {
    text.match_indices(needle)
        .map(|(offset, _)| offset as u32)
        .collect()
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
