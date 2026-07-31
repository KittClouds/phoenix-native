use super::*;
use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord,
    DocumentAnalysisBinding, PhoenixStructuralSubstrateV1, StructuralDialogueHint,
    StructuralSentenceQuality, StructuralSpanKind, NO_STRUCTURAL_PARENT,
    STRUCTURAL_SUBSTRATE_CONTRACT,
};
use phoenix_memory_contract::{
    CandidateEndpointBindingRecordV3, CandidateStatus, ContentUnitKind, ContentUnitRecord,
    PageKindV3, ProducerCapabilityRecordV3, ProducerStateV3, SemanticCandidateRecordV3, TurnRecord,
    VerifiedGraphGenerationV3, VocabularyPackRecordV3,
};
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;
use tempfile::TempDir;

#[derive(Default)]
struct FixtureProducer {
    document_calls: AtomicU64,
    turn_calls: AtomicU64,
}

impl DualFaceProducer for FixtureProducer {
    fn analyze_document(
        &self,
        request: &IngestDocumentRevision,
        cancellation: &CancellationProbe,
    ) -> Result<DocumentProduction, CoordinatorError> {
        cancellation.check()?;
        self.document_calls.fetch_add(1, Ordering::AcqRel);
        Ok(DocumentProduction {
            structural: structural_fixture(request),
            common: common_fixture(&request.lease.content),
        })
    }

    fn analyze_turn(
        &self,
        _conversation_external_id: &[u8],
        turn: &CommittedTurn,
        cancellation: &CancellationProbe,
    ) -> Result<TurnProduction, CoordinatorError> {
        cancellation.check()?;
        self.turn_calls.fetch_add(1, Ordering::AcqRel);
        Ok(TurnProduction {
            common: common_fixture(&turn.content),
        })
    }
}

#[test]
fn one_coordinator_preserves_exact_document_chunks_and_turn_boundaries() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let coordinator = coordinator(temp.path(), producer);
    let document = document_request(7, "Alice remembers Rome.");

    let first = coordinator
        .try_ingest_document(document.clone())
        .expect("submit document")
        .wait()
        .expect("publish document");
    assert_eq!(first.document_count, 1);
    assert_eq!(first.conversation_count, 0);

    let conversation = conversation();
    let user = committed_turn(0, b"turn/user", "Alice prefers green.", None);
    coordinator
        .try_ingest_turn(IngestTurn {
            conversation: conversation.clone(),
            committed_turn: user,
        })
        .expect("submit user turn")
        .wait()
        .expect("publish user turn");
    let assistant = committed_turn(
        1,
        b"turn/assistant",
        "I will remember that preference.",
        Some(0),
    );
    let published = coordinator
        .try_ingest_turn(IngestTurn {
            conversation,
            committed_turn: assistant,
        })
        .expect("submit assistant turn")
        .wait()
        .expect("publish assistant turn");

    let generation =
        VerifiedGraphGenerationV3::open(&published.path).expect("open mixed generation");
    assert_eq!(generation.header().document_revision_count, 1);
    assert_eq!(generation.header().conversation_count, 1);
    assert_eq!(generation.header().turn_count, 2);
    let chunks = generation
        .typed_page::<phoenix_memory_contract::ChunkRecord>(PageKindV3::Chunks)
        .expect("chunk page");
    assert_eq!(chunks.len(), 1);
    assert_eq!((chunks[0].start, chunks[0].end), (0, 21));
    let turns = generation
        .typed_page::<TurnRecord>(PageKindV3::Turns)
        .expect("turn page");
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].ordinal, 0);
    assert_eq!(turns[1].ordinal, 1);
    assert_eq!(
        generation
            .resolve_source_text(turns[0].content)
            .expect("user source"),
        "Alice prefers green."
    );
    assert_eq!(
        generation
            .resolve_source_text(turns[1].content)
            .expect("assistant source"),
        "I will remember that preference."
    );
    let turn_units = generation
        .typed_page::<ContentUnitRecord>(PageKindV3::ContentUnits)
        .expect("content unit page")
        .iter()
        .filter(|unit| unit.kind == ContentUnitKind::Turn as u16)
        .count();
    assert_eq!(turn_units, 2);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn recall_reads_committed_history_without_ingesting_pending_answer() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let coordinator = coordinator(temp.path(), producer);
    let conversation = conversation();
    coordinator
        .try_ingest_turn(IngestTurn {
            conversation: conversation.clone(),
            committed_turn: committed_turn(0, b"turn/user", "Alice prefers green tea.", None),
        })
        .expect("submit history")
        .wait()
        .expect("publish history");
    let before = coordinator
        .try_verify_current()
        .expect("submit verify")
        .wait()
        .expect("verify current")
        .expect("current generation");

    let pending_text: Arc<str> = Arc::from("The pending answer says green tea.");
    let packet = coordinator
        .try_recall(RecallTurn {
            conversation,
            pending_turn: PendingTurn {
                external_id: Arc::from(&b"turn/pending"[..]),
                ordinal: 1,
                role: phoenix_memory_contract::ParticipantRole::Assistant,
                event_time_millis: 1_010,
                reply_to_ordinal: Some(0),
                content: pending_text.clone(),
                origin: IngestionOrigin::ExternalConversation,
            },
            scope: Default::default(),
        })
        .expect("submit recall")
        .wait()
        .expect("recall history");
    assert_eq!(packet.committed_history_count, 1);
    assert_eq!(packet.items.len(), 1);
    assert_eq!(packet.items[0].content.as_ref(), "Alice prefers green tea.");
    assert_eq!(
        packet.items[0].source_kind,
        phoenix_memory_contract::SourceKind::Conversation
    );
    assert_eq!(packet.lexical.status, LexicalRecallStatus::Ready);
    assert_eq!(
        packet.lexical.path_id.as_str(),
        "phoenix.lexical.positional/v1"
    );
    assert_eq!(packet.lexical.returned_items, 1);
    assert_eq!(packet.lexical.generation_hash, Some(before));
    assert_eq!(packet.lexical.query_hash, packet.pending_turn_hash);
    assert_eq!(packet.proposed_candidates.len(), 2);
    assert!(packet.proposed_candidates.iter().all(|candidate| {
        candidate.status == CandidateStatus::Proposed
            && candidate.producer_identity_hash == [9; 32]
            && !candidate.evidence.is_empty()
            && candidate
                .evidence
                .iter()
                .all(|evidence| evidence.content.as_ref() == "Alice")
    }));
    assert!(packet
        .items
        .iter()
        .all(|item| item.content.as_ref() != pending_text.as_ref()));
    assert_eq!(
        packet.pending_turn_hash,
        *blake3::hash(pending_text.as_bytes()).as_bytes()
    );
    let after = coordinator
        .try_verify_current()
        .expect("submit second verify")
        .wait()
        .expect("verify after recall")
        .expect("current generation");
    assert_eq!(before, after);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn production_recall_searches_exact_document_chunks_and_conversation_turns() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let coordinator = coordinator(temp.path(), producer);
    coordinator
        .try_ingest_document(document_request(
            9,
            "Alice archived the cobalt map in Rome.",
        ))
        .expect("submit document")
        .wait()
        .expect("publish document");
    let conversation = conversation();
    for (ordinal, external_id, content) in [
        (
            0,
            &b"mixed/0"[..],
            "Alice asked for the cobalt map yesterday.",
        ),
        (1, &b"mixed/1"[..], "Bob repaired an engine."),
    ] {
        coordinator
            .try_ingest_turn(IngestTurn {
                conversation: conversation.clone(),
                committed_turn: committed_turn(
                    ordinal,
                    external_id,
                    content,
                    ordinal.checked_sub(1),
                ),
            })
            .expect("submit conversation")
            .wait()
            .expect("publish conversation");
    }

    let request = RecallTurn {
        conversation,
        pending_turn: PendingTurn {
            external_id: Arc::from(&b"mixed/pending"[..]),
            ordinal: 2,
            role: phoenix_memory_contract::ParticipantRole::User,
            event_time_millis: 1_020,
            reply_to_ordinal: Some(1),
            content: Arc::from("Where is Alice's cobalt map?"),
            origin: IngestionOrigin::ExternalConversation,
        },
        scope: Default::default(),
    };
    let packet = coordinator
        .try_recall(request.clone())
        .expect("submit mixed recall")
        .wait()
        .expect("mixed recall");
    let warm = coordinator
        .try_recall(request.clone())
        .expect("submit warm mixed recall")
        .wait()
        .expect("warm mixed recall");

    assert_eq!(packet.lexical.status, LexicalRecallStatus::Ready);
    assert_eq!(packet.items, warm.items);
    assert_eq!(packet.proposed_candidates, warm.proposed_candidates);
    assert!(!warm.lexical.search.allocations_grew);
    assert_eq!(packet.lexical.indexed_items, 3);
    assert_eq!(packet.items.len(), 2);
    assert!(packet.items.iter().any(|item| item.source_kind
        == phoenix_memory_contract::SourceKind::WorkspaceDocument
        && item.content.as_ref() == "Alice archived the cobalt map in Rome."
        && item.locator
            == crate::MemorySourceLocator::Document {
                entry_id: 9,
                revision: phoenix_workspace::DocumentRevision(3),
            }));
    assert!(packet.items.iter().any(|item| item.source_kind
        == phoenix_memory_contract::SourceKind::Conversation
        && item.content.as_ref() == "Alice asked for the cobalt map yesterday."
        && item.locator
            == crate::MemorySourceLocator::ConversationTurn {
                conversation_external_id: request.conversation.external_id.clone(),
                turn_ordinal: 0,
            }));
    assert!(packet
        .items
        .iter()
        .all(|item| item.content.as_ref() != "Bob repaired an engine."));
    assert!(packet
        .proposed_candidates
        .iter()
        .all(|candidate| candidate.status == CandidateStatus::Proposed));
    let document_only = coordinator
        .try_recall(RecallTurn {
            scope: crate::MemoryScope::Document(9),
            ..request.clone()
        })
        .expect("submit document-scoped recall")
        .wait()
        .expect("document-scoped recall");
    assert_eq!(document_only.items.len(), 1);
    assert_eq!(
        document_only.items[0].source_kind,
        phoenix_memory_contract::SourceKind::WorkspaceDocument
    );
    let conversation_only = coordinator
        .try_recall(RecallTurn {
            scope: crate::MemoryScope::Conversation(request.conversation.external_id.clone()),
            ..request
        })
        .expect("submit conversation-scoped recall")
        .wait()
        .expect("conversation-scoped recall");
    assert_eq!(conversation_only.items.len(), 1);
    assert_eq!(
        conversation_only.items[0].source_kind,
        phoenix_memory_contract::SourceKind::Conversation
    );
    assert_ne!(document_only.scope_hash, conversation_only.scope_hash);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn lexical_index_failure_rejects_mutation_without_changing_generation() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let mut config = test_config(temp.path());
    config.lexical_recall.maximum_items = 1;
    let coordinator =
        DualFaceIngestionCoordinator::new(config, producer).expect("bounded coordinator");
    let conversation = conversation();
    coordinator
        .try_ingest_turn(IngestTurn {
            conversation: conversation.clone(),
            committed_turn: committed_turn(0, b"bounded/0", "first committed memory", None),
        })
        .expect("submit first")
        .wait()
        .expect("publish first");
    let before = coordinator
        .try_verify_current()
        .expect("submit verify")
        .wait()
        .expect("verify")
        .expect("generation");
    let error = coordinator
        .try_ingest_turn(IngestTurn {
            conversation: conversation.clone(),
            committed_turn: committed_turn(1, b"bounded/1", "second forbidden memory", Some(0)),
        })
        .expect("submit second")
        .wait()
        .expect_err("oversized lexical corpus must reject publication");
    assert!(matches!(error, CoordinatorError::Oversized));
    let after = coordinator
        .try_verify_current()
        .expect("submit verify")
        .wait()
        .expect("verify")
        .expect("generation");
    assert_eq!(before, after);

    let packet = coordinator
        .try_recall(RecallTurn {
            conversation,
            pending_turn: PendingTurn {
                external_id: Arc::from(&b"bounded/pending"[..]),
                ordinal: 1,
                role: phoenix_memory_contract::ParticipantRole::User,
                event_time_millis: 1_010,
                reply_to_ordinal: Some(0),
                content: Arc::from("second forbidden"),
                origin: IngestionOrigin::ExternalConversation,
            },
            scope: Default::default(),
        })
        .expect("submit recall")
        .wait()
        .expect("recall retained generation");
    assert!(packet.items.is_empty());
    assert_eq!(packet.committed_history_count, 1);
    assert_eq!(packet.lexical.generation_hash, Some(before));
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn qps_v2_01_shadow_is_explicit_bounded_and_never_changes_authority_items() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let mut config = test_config(temp.path());
    config.qps_shadow = QpsShadowConfig::v2_01_shadow();
    let coordinator =
        DualFaceIngestionCoordinator::new(config, producer).expect("shadow coordinator");
    let conversation = conversation();
    for (ordinal, external_id, content) in [
        (
            0,
            &b"shadow/0"[..],
            "Mira prefers jasmine green tea in Rome.",
        ),
        (1, &b"shadow/1"[..], "Bob repairs engines after work."),
        (2, &b"shadow/2"[..], "Mira orders green tea every morning."),
    ] {
        coordinator
            .try_ingest_turn(IngestTurn {
                conversation: conversation.clone(),
                committed_turn: committed_turn(
                    ordinal,
                    external_id,
                    content,
                    ordinal.checked_sub(1),
                ),
            })
            .expect("submit shadow history")
            .wait()
            .expect("publish shadow history");
    }
    let request = RecallTurn {
        conversation,
        pending_turn: PendingTurn {
            external_id: Arc::from(&b"shadow/pending"[..]),
            ordinal: 3,
            role: phoenix_memory_contract::ParticipantRole::User,
            event_time_millis: 1_030,
            reply_to_ordinal: Some(2),
            content: Arc::from("Which jasmine green tea does Mira prefer?"),
            origin: IngestionOrigin::ExternalConversation,
        },
        scope: Default::default(),
    };
    let first = coordinator
        .try_recall(request.clone())
        .expect("submit first shadow recall")
        .wait()
        .expect("first shadow recall");
    let second = coordinator
        .try_recall(request)
        .expect("submit warm shadow recall")
        .wait()
        .expect("warm shadow recall");

    assert_eq!(first.items, second.items);
    assert!(second.qps_shadow.authority_unchanged);
    assert_eq!(second.qps_shadow.path_id.as_str(), "qps/v2.01/shadow");
    assert_eq!(second.qps_shadow.status, QpsShadowStatus::Ready);
    assert_eq!(second.qps_shadow.documents, 3);
    assert_eq!(second.qps_shadow.shadow_ordinals.first(), Some(&0));
    assert!(!second.qps_shadow.search.allocations_grew);
    assert!(second.qps_shadow.search.stages.total > 0);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn oversized_shadow_corpus_reports_failure_without_blocking_authority_recall() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let mut config = test_config(temp.path());
    config.qps_shadow = QpsShadowConfig {
        maximum_documents: 1,
        ..QpsShadowConfig::v2_01_shadow()
    };
    let coordinator =
        DualFaceIngestionCoordinator::new(config, producer).expect("shadow coordinator");
    let conversation = conversation();
    for (ordinal, external_id, content) in [
        (0, &b"oversized/0"[..], "first committed turn"),
        (1, &b"oversized/1"[..], "second committed turn"),
    ] {
        coordinator
            .try_ingest_turn(IngestTurn {
                conversation: conversation.clone(),
                committed_turn: committed_turn(
                    ordinal,
                    external_id,
                    content,
                    ordinal.checked_sub(1),
                ),
            })
            .expect("submit history")
            .wait()
            .expect("publish authority history");
    }
    let packet = coordinator
        .try_recall(RecallTurn {
            conversation,
            pending_turn: PendingTurn {
                external_id: Arc::from(&b"oversized/pending"[..]),
                ordinal: 2,
                role: phoenix_memory_contract::ParticipantRole::User,
                event_time_millis: 1_020,
                reply_to_ordinal: Some(1),
                content: Arc::from("committed turn"),
                origin: IngestionOrigin::ExternalConversation,
            },
            scope: Default::default(),
        })
        .expect("submit recall")
        .wait()
        .expect("authority recall survives shadow rejection");
    assert_eq!(packet.items.len(), 2);
    assert_eq!(packet.qps_shadow.status, QpsShadowStatus::OversizedCorpus);
    assert!(packet.qps_shadow.authority_unchanged);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn ingestion_is_idempotent_and_semantics_remain_candidate_only() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let coordinator = coordinator(temp.path(), producer.clone());
    let request = document_request(9, "Alice remembers Rome.");
    let first = coordinator
        .try_ingest_document(request.clone())
        .expect("submit first")
        .wait()
        .expect("publish first");
    let repeated = coordinator
        .try_ingest_document(request)
        .expect("submit repeat")
        .wait()
        .expect("repeat is idempotent");
    assert_eq!(first.generation_hash, repeated.generation_hash);
    assert_eq!(first.path, repeated.path);
    assert_eq!(producer.document_calls.load(Ordering::Acquire), 1);

    let generation =
        VerifiedGraphGenerationV3::open(&first.path).expect("open candidate generation");
    let candidates = generation
        .typed_page::<SemanticCandidateRecordV3>(PageKindV3::SemanticCandidates)
        .expect("semantic candidates");
    assert_eq!(candidates.len(), 2);
    assert!(candidates
        .iter()
        .all(|candidate| candidate.status == CandidateStatus::Proposed as u16));
    let endpoints = generation
        .typed_page::<CandidateEndpointBindingRecordV3>(PageKindV3::CandidateEndpointBindings)
        .expect("candidate endpoints");
    let packs = generation
        .typed_page::<VocabularyPackRecordV3>(PageKindV3::VocabularyPacks)
        .expect("vocabulary packs");
    assert_eq!(endpoints.len(), 2);
    assert_eq!(packs.len(), 1);
    assert!(candidates.iter().all(|candidate| {
        candidate.vocabulary_pack_id == packs[0].id
            && candidate.producer_identity_hash == packs[0].producer_identity_hash
            && candidate.endpoint_count == 1
            && candidate.evidence_count == 1
    }));
    assert!(generation.page_bytes(PageKindV3::Decisions).is_empty());
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn fresh_coordinator_replay_reuses_the_exact_immutable_generation() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let document = document_request(10, "Alice remembers Rome.");
    let conversation = conversation();
    let turn = committed_turn(0, b"turn/replay", "Alice prefers green.", None);

    let first = coordinator(temp.path(), Arc::new(FixtureProducer::default()));
    first
        .try_ingest_document(document.clone())
        .expect("submit initial document")
        .wait()
        .expect("publish initial document");
    let initial = first
        .try_ingest_turn(IngestTurn {
            conversation: conversation.clone(),
            committed_turn: turn.clone(),
        })
        .expect("submit initial turn")
        .wait()
        .expect("publish initial mixed generation");
    first.shutdown().expect("first shutdown");

    let replay = coordinator(temp.path(), Arc::new(FixtureProducer::default()));
    let document_replay = replay
        .try_ingest_document(document)
        .expect("submit replay document")
        .wait()
        .expect("reuse document generation");
    assert!(document_replay.reused);
    let replayed = replay
        .try_ingest_turn(IngestTurn {
            conversation,
            committed_turn: turn,
        })
        .expect("submit replay turn")
        .wait()
        .expect("reuse mixed generation");
    assert!(replayed.reused);
    assert_eq!(replayed.generation_hash, initial.generation_hash);
    assert_eq!(replayed.path, initial.path);
    replay.shutdown().expect("replay shutdown");
}

#[test]
fn unsupported_products_are_explicit_capabilities_not_zero_claims() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let coordinator = coordinator(temp.path(), Arc::new(FixtureProducer::default()));
    let published = coordinator
        .try_ingest_document(document_request(3, "Alice remembers Rome."))
        .expect("submit document")
        .wait()
        .expect("publish document");
    let generation =
        VerifiedGraphGenerationV3::open(&published.path).expect("open capability generation");
    let capabilities = generation
        .typed_page::<ProducerCapabilityRecordV3>(PageKindV3::ProducerCapabilitiesV3)
        .expect("capability page");
    assert_eq!(capabilities.len(), ProducerProductV3::ALL.len());
    let causality = &capabilities[(ProducerProductV3::Causality as usize) - 1];
    assert_eq!(causality.state, ProducerStateV3::Unsupported as u16);
    assert_eq!(causality.output_count, 0);
    let claims = &capabilities[(ProducerProductV3::ClaimsAttributes as usize) - 1];
    assert_eq!(claims.state, ProducerStateV3::Produced as u16);
    assert_eq!(claims.output_count, 2);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn gold_answers_and_gold_sessions_are_rejected_before_production() {
    let temp = TempDir::new().expect("temporary artifact directory");
    let producer = Arc::new(FixtureProducer::default());
    let coordinator = coordinator(temp.path(), producer.clone());
    let mut document = document_request(12, "Alice remembers Rome.");
    document.origin = IngestionOrigin::LongMemEvalGoldSession;
    let error = coordinator
        .try_ingest_document(document)
        .expect("queue accepts command")
        .wait()
        .expect_err("gold session must be rejected");
    assert!(matches!(error, CoordinatorError::GoldDataRejected));
    assert_eq!(producer.document_calls.load(Ordering::Acquire), 0);

    let error = coordinator
        .try_recall(RecallTurn {
            conversation: conversation(),
            pending_turn: PendingTurn {
                external_id: Arc::from(&b"gold"[..]),
                ordinal: 0,
                role: phoenix_memory_contract::ParticipantRole::Assistant,
                event_time_millis: 1_000,
                reply_to_ordinal: None,
                content: Arc::from("forbidden answer"),
                origin: IngestionOrigin::LongMemEvalGoldAnswer,
            },
            scope: Default::default(),
        })
        .expect("queue accepts recall")
        .wait()
        .expect_err("gold answer must be rejected");
    assert!(matches!(error, CoordinatorError::GoldDataRejected));
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn bounded_queue_rejects_overflow_and_cancellation_reaches_queued_work() {
    struct BlockingProducer {
        started: Mutex<Option<mpsc::SyncSender<()>>>,
        release: (Mutex<bool>, Condvar),
    }

    impl DualFaceProducer for BlockingProducer {
        fn analyze_document(
            &self,
            request: &IngestDocumentRevision,
            cancellation: &CancellationProbe,
        ) -> Result<DocumentProduction, CoordinatorError> {
            if let Some(sender) = self
                .started
                .lock()
                .map_err(|_| CoordinatorError::Shutdown)?
                .take()
            {
                let _ = sender.send(());
            }
            let mut released = self
                .release
                .0
                .lock()
                .map_err(|_| CoordinatorError::Shutdown)?;
            while !*released {
                released = self
                    .release
                    .1
                    .wait(released)
                    .map_err(|_| CoordinatorError::Shutdown)?;
            }
            cancellation.check()?;
            Ok(DocumentProduction {
                structural: structural_fixture(request),
                common: common_fixture(&request.lease.content),
            })
        }

        fn analyze_turn(
            &self,
            _conversation_external_id: &[u8],
            _turn: &CommittedTurn,
            _cancellation: &CancellationProbe,
        ) -> Result<TurnProduction, CoordinatorError> {
            Ok(TurnProduction::default())
        }
    }

    let temp = TempDir::new().expect("temporary artifact directory");
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let producer = Arc::new(BlockingProducer {
        started: Mutex::new(Some(started_sender)),
        release: (Mutex::new(false), Condvar::new()),
    });
    let mut config = test_config(temp.path());
    config.queue_capacity = 1;
    let coordinator =
        DualFaceIngestionCoordinator::new(config, producer.clone()).expect("coordinator");
    let first = coordinator
        .try_ingest_document(document_request(20, "Alice remembers Rome."))
        .expect("submit active document");
    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker entered producer");
    let second = coordinator
        .try_ingest_document(document_request(21, "Alice remembers Paris."))
        .expect("fill bounded queue");
    assert!(matches!(
        coordinator.try_ingest_document(document_request(22, "Alice remembers Cairo.")),
        Err(CoordinatorError::QueueFull)
    ));
    coordinator.cancel();
    {
        let mut released = producer.release.0.lock().expect("release lock");
        *released = true;
        producer.release.1.notify_all();
    }
    assert!(matches!(first.wait(), Err(CoordinatorError::Cancelled)));
    assert!(matches!(second.wait(), Err(CoordinatorError::Cancelled)));
    let metrics = coordinator.metrics();
    assert_eq!(metrics.queue_capacity, 1);
    assert_eq!(metrics.queue_high_water, 1);
    coordinator.shutdown().expect("clean shutdown");
}

#[test]
fn a_producer_cannot_emit_a_family_registered_as_unsupported() {
    struct UnsupportedCandidateProducer;

    impl DualFaceProducer for UnsupportedCandidateProducer {
        fn analyze_document(
            &self,
            request: &IngestDocumentRevision,
            _cancellation: &CancellationProbe,
        ) -> Result<DocumentProduction, CoordinatorError> {
            let mut common = common_fixture(&request.lease.content);
            common.candidates[0].family = SemanticCandidateFamilyV3::Causal;
            Ok(DocumentProduction {
                structural: structural_fixture(request),
                common,
            })
        }

        fn analyze_turn(
            &self,
            _conversation_external_id: &[u8],
            _turn: &CommittedTurn,
            _cancellation: &CancellationProbe,
        ) -> Result<TurnProduction, CoordinatorError> {
            Ok(TurnProduction::default())
        }
    }

    let temp = TempDir::new().expect("temporary artifact directory");
    let coordinator = DualFaceIngestionCoordinator::new(
        test_config(temp.path()),
        Arc::new(UnsupportedCandidateProducer),
    )
    .expect("coordinator");
    let error = coordinator
        .try_ingest_document(document_request(30, "Alice remembers Rome."))
        .expect("submit document")
        .wait()
        .expect_err("unsupported candidate must fail closed");
    assert!(matches!(
        error,
        CoordinatorError::ProducerAuthority("producer emitted a product registered as unsupported")
    ));
    coordinator.shutdown().expect("clean shutdown");
}

fn coordinator(
    artifact_dir: &std::path::Path,
    producer: Arc<FixtureProducer>,
) -> DualFaceIngestionCoordinator<FixtureProducer> {
    DualFaceIngestionCoordinator::new(test_config(artifact_dir), producer).expect("coordinator")
}

fn test_config(artifact_dir: &std::path::Path) -> CoordinatorConfig {
    CoordinatorConfig::new(
        Arc::<[u8]>::from(&b"phoenix/test-workspace"[..]),
        artifact_dir,
        registrations(),
        [ModelIdentityInputV3 {
            name: Arc::from("fixture-model"),
            runtime: Arc::from("native-test"),
            artifact_uri: Arc::from("fixture://model"),
            artifact_hash: [0x31; 32],
            config_hash: [0x32; 32],
        }],
    )
}

fn registrations() -> Arc<[ProducerRegistrationV3]> {
    ProducerProductV3::ALL
        .into_iter()
        .map(|product| ProducerRegistrationV3 {
            product,
            producer: Arc::from("fixture-producer"),
            producer_binary_hash: [0x41; 32],
            config_hash: [0x42; 32],
            model_identity_index: Some(0),
            support: if matches!(
                product,
                ProducerProductV3::SourceStructure
                    | ProducerProductV3::ContentUnitsAndChunks
                    | ProducerProductV3::MentionsAndEvidence
                    | ProducerProductV3::CanonicalEntityBindings
                    | ProducerProductV3::ClaimsAttributes
            ) {
                RegistrationSupport::Supported
            } else {
                RegistrationSupport::Unsupported
            },
        })
        .collect::<Vec<_>>()
        .into()
}

fn document_request(id: u64, content: &str) -> IngestDocumentRevision {
    let content: Arc<str> = Arc::from(content);
    let hash = ContentHash::of(content.as_bytes());
    let revision = DocumentRevision(3);
    IngestDocumentRevision {
        lease: Arc::new(DocumentLease {
            entry_id: EntryId(id),
            revision,
            content_hash: hash,
            content,
        }),
        revision,
        hash,
        origin: IngestionOrigin::Workspace,
    }
}

fn conversation() -> ConversationKey {
    ConversationKey {
        external_id: Arc::from(&b"chat/fixture"[..]),
        started_at_millis: 1_000,
    }
}

fn committed_turn(
    ordinal: u32,
    external_id: &'static [u8],
    content: &str,
    reply_to_ordinal: Option<u32>,
) -> CommittedTurn {
    CommittedTurn {
        external_id: Arc::from(external_id),
        ordinal,
        role: if ordinal % 2 == 0 {
            phoenix_memory_contract::ParticipantRole::User
        } else {
            phoenix_memory_contract::ParticipantRole::Assistant
        },
        event_time_millis: 1_000 + i64::from(ordinal) * 10,
        reply_to_ordinal,
        actor_entity_id: 0,
        model_identity_index: (ordinal % 2 == 1).then_some(0),
        content: Arc::from(content),
        origin: IngestionOrigin::ExternalConversation,
    }
}

fn structural_fixture(request: &IngestDocumentRevision) -> PhoenixStructuralSubstrateV1 {
    let len = request.lease.content.len() as u32;
    let content_hash = u64::from_le_bytes(request.hash.0[..8].try_into().unwrap_or([1; 8]));
    PhoenixStructuralSubstrateV1 {
        schema: STRUCTURAL_SUBSTRATE_CONTRACT.to_owned(),
        binding: DocumentAnalysisBinding {
            source_document_id: format!("native:{}", request.lease.entry_id.0),
            native_document_id: request.lease.entry_id.0,
            document_revision: request.revision.0,
            content_hash: request.hash.0,
            analysis_generation: 1,
            source_registry_revision: 4,
            target_registry_revision: 5,
            producer_binary_hash: [0x51; 32],
            chunker: analysis_model("chunker", 0x61),
            dynamic_ner: analysis_model("ner", 0x62),
            nli: analysis_model("nli", 0x63),
        },
        source_len: len,
        chunks: vec![AnalysisChunkRecord {
            start: 0,
            end: len,
            sentence_start: 0,
            sentence_end: 1,
            paragraph_start: 0,
            paragraph_end: 1,
            chapter_index: 0,
            token_count: 3,
            content_hash,
            dialogue_hint: StructuralDialogueHint::None,
        }],
        sentences: vec![AnalysisSentenceRecord {
            start: 0,
            end: len,
            paragraph_index: 0,
            chapter_index: 0,
            token_count: 3,
            content_hash,
            quality: StructuralSentenceQuality::Complete,
            dialogue_hint: StructuralDialogueHint::None,
        }],
        spans: vec![
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Chapter,
                start: 0,
                end: len,
                parent_index: NO_STRUCTURAL_PARENT,
                child_start: 0,
                child_end: 1,
                token_count: 3,
                content_hash,
                label: "Chapter 1".to_owned(),
                dialogue_hint: StructuralDialogueHint::None,
            },
            AnalysisSpanRecord {
                kind: StructuralSpanKind::Paragraph,
                start: 0,
                end: len,
                parent_index: 0,
                child_start: 0,
                child_end: 1,
                token_count: 3,
                content_hash,
                label: String::new(),
                dialogue_hint: StructuralDialogueHint::None,
            },
        ],
    }
}

fn analysis_model(name: &str, byte: u8) -> AnalysisModelIdentity {
    AnalysisModelIdentity {
        model_id: name.to_owned(),
        artifact_hash: [byte; 32],
        config_hash: [byte.wrapping_add(1); 32],
        runtime_id: "native-test".to_owned(),
    }
}

fn common_fixture(content: &str) -> CommonProducts {
    let Some(start) = content.find("Alice") else {
        return CommonProducts::default();
    };
    let source_hash = blake3::hash(content.as_bytes());
    let mention_id = nonzero_u64(b"mention", source_hash.as_bytes());
    let evidence_id = nonzero_u64(b"evidence", source_hash.as_bytes());
    let candidate_id = candidate_hash(b"candidate/claim", source_hash.as_bytes());
    let second_candidate_id = candidate_hash(b"candidate/attribute", source_hash.as_bytes());
    let mut candidates = vec![
        SemanticCandidateDraft {
            candidate_id,
            vocabulary_pack_id: 77,
            relation_kind: Arc::from("core.preference"),
            value: Arc::from("green"),
            endpoints: Arc::from([crate::CandidateEndpointDraft {
                endpoint_id: 100,
                role: phoenix_memory_contract::CandidateEndpointRoleV3::Subject,
                flags: 0,
            }]),
            evidence_ids: Arc::from([evidence_id]),
            valid_time_from_millis: i64::MIN,
            valid_time_to_millis: i64::MAX,
            family: SemanticCandidateFamilyV3::Claim,
            confidence: 0.75,
            model_identity_index: Some(0),
            producer_identity_hash: [9; 32],
            status: CandidateStatus::Proposed,
            flags: 0,
        },
        SemanticCandidateDraft {
            candidate_id: second_candidate_id,
            vocabulary_pack_id: 77,
            relation_kind: Arc::from("core.attribute"),
            value: Arc::from("Alice"),
            endpoints: Arc::from([crate::CandidateEndpointDraft {
                endpoint_id: 100,
                role: phoenix_memory_contract::CandidateEndpointRoleV3::Subject,
                flags: 0,
            }]),
            evidence_ids: Arc::from([evidence_id]),
            valid_time_from_millis: i64::MIN,
            valid_time_to_millis: i64::MAX,
            family: SemanticCandidateFamilyV3::Attribute,
            confidence: 0.9,
            model_identity_index: Some(0),
            producer_identity_hash: [9; 32],
            status: CandidateStatus::Proposed,
            flags: 0,
        },
    ];
    candidates.sort_unstable_by_key(|candidate| std::cmp::Reverse(candidate.candidate_id));
    CommonProducts {
        entities: vec![EntityDraft {
            stable_id: 100,
            label: Arc::from("Alice"),
            custom_kind: None,
            mention_count: 1,
            kind: 1,
            source_mask: 1,
        }],
        mentions: vec![MentionDraft {
            stable_id: mention_id,
            entity_id: 100,
            evidence_id,
            start: start as u32,
            end: (start + 5) as u32,
            confidence: 0.99,
            flags: 0,
        }],
        canonical_bindings: vec![CanonicalBindingDraft {
            source_entity_id: 100,
            canonical_entity_id: 100,
            decision_id: 0,
            source_mask: 1,
            kind: phoenix_memory_contract::CanonicalBindingKind::Direct,
        }],
        vocabulary_packs: vec![crate::VocabularyPackDraft {
            id: 77,
            name: Arc::from("phoenix.core.memory"),
            version: Arc::from("1"),
            schema_hash: [8; 32],
            producer_identity_hash: [9; 32],
            kind: phoenix_memory_contract::VocabularyPackKindV3::Core,
            flags: 0,
        }],
        candidates,
    }
}

fn nonzero_u64(domain: &[u8], value: &[u8]) -> u64 {
    let hash = blake3::hash(&[domain, value].concat());
    u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap_or([1; 8])).max(1)
}

fn candidate_hash(domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(value);
    *hasher.finalize().as_bytes()
}
