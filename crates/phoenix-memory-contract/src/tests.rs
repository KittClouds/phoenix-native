use crate::{
    AuthoritySubjectKind, ConversationInput, DocumentChunkInput, DocumentInput,
    MemoryContractError, MixedSourceBuilder, OpenExpectation, PageKindV3, ParticipantRole,
    SupersessionRecord, TurnInput, ValidityIntervalRecord, VerifiedGraphGenerationV3,
    MAX_GENERATION_BYTES,
};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mixed_generation_round_trips_document_and_conversation_source_truth() {
    let path = temp_path("roundtrip");
    let generation = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path))
        .expect("mixed generation should publish");

    assert_eq!(generation.header().source_count, 4);
    assert_eq!(generation.header().document_revision_count, 2);
    assert_eq!(generation.header().conversation_count, 2);
    assert_eq!(generation.header().turn_count, 4);

    let documents = generation
        .typed_page::<crate::DocumentRevisionRecord>(PageKindV3::DocumentRevisions)
        .expect("document page");
    let document_texts = documents
        .iter()
        .map(|record| {
            generation
                .resolve_source_text(record.content)
                .expect("document text")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        document_texts,
        ["Alpha met Beta.\nGamma stayed.", "Research note."]
    );
    assert_eq!(
        generation
            .typed_page::<crate::ChunkRecord>(PageKindV3::Chunks)
            .expect("chunk page")
            .len(),
        3
    );

    let turns = generation
        .typed_page::<crate::TurnRecord>(PageKindV3::Turns)
        .expect("turn page");
    let first_reply = turns
        .iter()
        .find(|turn| turn.ordinal == 1 && turn.role == ParticipantRole::Assistant as u16)
        .expect("assistant reply");
    assert_ne!(first_reply.reply_to_turn_id, 0);
    assert_eq!(first_reply.event_time_millis, 1_010);
    assert_eq!(first_reply.model_identity_index, 7);
    assert_eq!(
        generation
            .resolve_source_text(first_reply.content)
            .expect("turn text"),
        "You prefer blue."
    );

    drop(generation);
    fs::remove_file(path).expect("remove test generation");
}

#[test]
fn input_order_does_not_change_ids_page_hashes_or_generation_hash() {
    let path_a = temp_path("order-a");
    let path_b = temp_path("order-b");
    let first = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path_a))
        .expect("first generation");
    let second = sample_builder(true)
        .prepare()
        .and_then(|prepared| prepared.write(&path_b))
        .expect("second generation");

    assert_eq!(
        first.header().source_set_hash,
        second.header().source_set_hash
    );
    assert_eq!(
        first.header().generation_hash,
        second.header().generation_hash
    );
    assert_eq!(
        first
            .directory()
            .iter()
            .map(|descriptor| descriptor.hash)
            .collect::<Vec<_>>(),
        second
            .directory()
            .iter()
            .map(|descriptor| descriptor.hash)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        first
            .typed_page::<crate::SourceRecord>(PageKindV3::Sources)
            .expect("sources")
            .iter()
            .map(|source| source.id)
            .collect::<Vec<_>>(),
        second
            .typed_page::<crate::SourceRecord>(PageKindV3::Sources)
            .expect("sources")
            .iter()
            .map(|source| source.id)
            .collect::<Vec<_>>()
    );

    drop((first, second));
    fs::remove_file(path_a).expect("remove first");
    fs::remove_file(path_b).expect("remove second");
}

#[test]
fn corrupt_page_fails_closed() {
    let path = temp_path("corrupt");
    let generation = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path))
        .expect("generation");
    let source_text_offset = generation.descriptor(PageKindV3::SourceText).offset;
    drop(generation);

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open generation");
    file.seek(SeekFrom::Start(source_text_offset))
        .expect("seek source text");
    file.write_all(&[0xff]).expect("corrupt byte");
    file.sync_all().expect("sync corruption");
    drop(file);

    assert!(matches!(
        VerifiedGraphGenerationV3::open(&path),
        Err(MemoryContractError::PageHashMismatch {
            page: PageKindV3::SourceText
        })
    ));
    fs::remove_file(path).expect("remove corrupt generation");
}

#[test]
fn namespace_source_set_and_stale_expectations_fail_closed() {
    let path = temp_path("expectation");
    let generation = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path))
        .expect("generation");
    let namespace_hash = generation.header().namespace_hash;
    let source_set_hash = generation.header().source_set_hash;
    drop(generation);

    assert!(matches!(
        VerifiedGraphGenerationV3::open_expected(
            &path,
            OpenExpectation {
                namespace_hash: Some([0x55; 32]),
                ..OpenExpectation::default()
            }
        ),
        Err(MemoryContractError::NamespaceMismatch)
    ));
    assert!(matches!(
        VerifiedGraphGenerationV3::open_expected(
            &path,
            OpenExpectation {
                namespace_hash: Some(namespace_hash),
                source_set_hash: Some([0x33; 32]),
                minimum_published_generation: None,
            }
        ),
        Err(MemoryContractError::ExpectedSourceSetMismatch)
    ));
    assert!(matches!(
        VerifiedGraphGenerationV3::open_expected(
            &path,
            OpenExpectation {
                namespace_hash: Some(namespace_hash),
                source_set_hash: Some(source_set_hash),
                minimum_published_generation: Some(6),
            }
        ),
        Err(MemoryContractError::StaleGeneration {
            actual: 5,
            minimum: 6
        })
    ));
    fs::remove_file(path).expect("remove expectation generation");
}

#[test]
fn oversized_declared_generation_fails_before_mapping_pages() {
    let path = temp_path("oversized");
    let generation = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path))
        .expect("generation");
    let mut header = *generation.header();
    drop(generation);
    header.total_len = MAX_GENERATION_BYTES + 1;

    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open generation");
    file.seek(SeekFrom::Start(0)).expect("seek header");
    file.write_all(bytemuck::bytes_of(&header))
        .expect("write oversized header");
    file.sync_all().expect("sync oversized header");
    drop(file);

    assert!(matches!(
        VerifiedGraphGenerationV3::open(&path),
        Err(MemoryContractError::OversizedGeneration { .. })
    ));
    fs::remove_file(path).expect("remove oversized generation");
}

#[test]
fn duplicate_source_and_invalid_chunk_are_rejected_before_write() {
    let document = primary_document();
    assert!(matches!(
        MixedSourceBuilder::new(b"workspace")
            .add_document(document.clone())
            .add_document(document)
            .prepare(),
        Err(MemoryContractError::DuplicateSourceIdentity)
    ));

    let mut invalid = primary_document();
    invalid.chunks[0].end = u32::MAX;
    assert!(matches!(
        MixedSourceBuilder::new(b"workspace")
            .add_document(invalid)
            .prepare(),
        Err(MemoryContractError::InvalidChunkRange)
    ));
}

#[test]
fn semantic_strings_do_not_change_the_source_set_identity() {
    let path_a = temp_path("source-set-a");
    let path_b = temp_path("source-set-b");
    let first = sample_builder(false)
        .prepare()
        .and_then(|prepared| prepared.write(&path_a))
        .expect("first generation");
    let mut prepared = sample_builder(false)
        .prepare()
        .expect("prepared generation");
    prepared
        .pages
        .strings
        .extend_from_slice(b"future semantic label");
    let second = prepared.write(&path_b).expect("second generation");

    assert_eq!(
        first.header().source_set_hash,
        second.header().source_set_hash
    );
    assert_ne!(
        first.header().generation_hash,
        second.header().generation_hash
    );
    assert_ne!(
        first.descriptor(PageKindV3::Strings).hash,
        second.descriptor(PageKindV3::Strings).hash
    );

    drop((first, second));
    fs::remove_file(path_a).expect("remove first");
    fs::remove_file(path_b).expect("remove second");
}

#[test]
fn validity_and_supersession_are_explicit_and_decision_bound() {
    let valid_path = temp_path("valid-time");
    let mut prepared = sample_builder(false)
        .prepare()
        .expect("prepared generation");
    prepared
        .pages
        .validity_intervals
        .push(ValidityIntervalRecord {
            subject_id: [1; 32],
            valid_time_from_millis: 100,
            valid_time_to_millis: 200,
            system_generation_from: 5,
            system_generation_to: u64::MAX,
            subject_kind: AuthoritySubjectKind::AcceptedFact as u16,
            flags: 0,
            reserved: 0,
        });
    prepared.pages.supersessions.push(SupersessionRecord {
        subject_id: [2; 32],
        replacement_id: [3; 32],
        evidence_id: 9,
        decision_id: 11,
        system_generation: 5,
        status: 1,
        flags_u16: 0,
        flags: 0,
    });
    let generation = prepared
        .write(&valid_path)
        .expect("valid temporal authority");
    assert_eq!(
        generation
            .typed_page::<ValidityIntervalRecord>(PageKindV3::ValidityIntervals)
            .expect("validity page")
            .len(),
        1
    );
    drop(generation);
    fs::remove_file(valid_path).expect("remove valid generation");

    let invalid_path = temp_path("invalid-supersession");
    let mut invalid = sample_builder(false)
        .prepare()
        .expect("prepared generation");
    invalid.pages.supersessions.push(SupersessionRecord {
        subject_id: [4; 32],
        replacement_id: [5; 32],
        evidence_id: 9,
        decision_id: 0,
        system_generation: 5,
        status: 1,
        flags_u16: 0,
        flags: 0,
    });
    assert!(matches!(
        invalid.write(&invalid_path),
        Err(MemoryContractError::InvalidSourceModel(
            "supersession is not decision-bound"
        ))
    ));
    assert!(!invalid_path.exists());
}

fn sample_builder(reverse: bool) -> MixedSourceBuilder {
    let documents = [primary_document(), research_document()];
    let conversations = [primary_conversation(), second_conversation()];
    let mut builder = MixedSourceBuilder::new(b"phoenix-workspace")
        .generations(4, 9, 5)
        .cohort_hash([0x91; 32]);
    if reverse {
        for conversation in conversations.into_iter().rev() {
            builder = builder.add_conversation(conversation);
        }
        for document in documents.into_iter().rev() {
            builder = builder.add_document(document);
        }
    } else {
        for document in documents {
            builder = builder.add_document(document);
        }
        for conversation in conversations {
            builder = builder.add_conversation(conversation);
        }
    }
    builder
}

fn primary_document() -> DocumentInput {
    DocumentInput::current(
        b"note/short-a".to_vec(),
        3,
        "Phoenix / Notes / Short A",
        "Alpha met Beta.\nGamma stayed.",
        vec![
            DocumentChunkInput {
                start: 0,
                end: 15,
                sentence_start: 0,
                sentence_end: 1,
                paragraph_start: 0,
                paragraph_end: 1,
                chapter_index: 0,
                token_count: 3,
                flags: 1,
            },
            DocumentChunkInput {
                start: 16,
                end: 29,
                sentence_start: 1,
                sentence_end: 2,
                paragraph_start: 1,
                paragraph_end: 2,
                chapter_index: 0,
                token_count: 2,
                flags: 1,
            },
        ],
        9,
    )
}

fn research_document() -> DocumentInput {
    DocumentInput::current(
        b"note/research".to_vec(),
        2,
        "Phoenix / Research",
        "Research note.",
        vec![DocumentChunkInput {
            start: 0,
            end: 14,
            sentence_start: 0,
            sentence_end: 1,
            paragraph_start: 0,
            paragraph_end: 1,
            chapter_index: 0,
            token_count: 2,
            flags: 1,
        }],
        9,
    )
}

fn primary_conversation() -> ConversationInput {
    ConversationInput {
        external_id: b"chat/alpha".to_vec(),
        started_at_millis: 1_000,
        ended_at_millis: 1_010,
        turns: vec![
            TurnInput {
                external_id: b"turn/answer".to_vec(),
                ordinal: 1,
                role: ParticipantRole::Assistant,
                event_time_millis: 1_010,
                reply_to_ordinal: Some(0),
                actor_entity_id: 0,
                model_identity_index: Some(7),
                content: "You prefer blue.".to_owned(),
                flags: 0,
            },
            TurnInput {
                external_id: b"turn/question".to_vec(),
                ordinal: 0,
                role: ParticipantRole::User,
                event_time_millis: 1_000,
                reply_to_ordinal: None,
                actor_entity_id: 0,
                model_identity_index: None,
                content: "Remember that I prefer blue.".to_owned(),
                flags: 0,
            },
        ],
    }
}

fn second_conversation() -> ConversationInput {
    ConversationInput {
        external_id: b"chat/beta".to_vec(),
        started_at_millis: 2_000,
        ended_at_millis: 2_000,
        turns: vec![
            TurnInput {
                external_id: b"turn/system".to_vec(),
                ordinal: 0,
                role: ParticipantRole::System,
                event_time_millis: 2_000,
                reply_to_ordinal: None,
                actor_entity_id: 0,
                model_identity_index: None,
                content: "Be concise.".to_owned(),
                flags: 0,
            },
            TurnInput {
                external_id: b"turn/tool".to_vec(),
                ordinal: 1,
                role: ParticipantRole::Tool,
                event_time_millis: 2_000,
                reply_to_ordinal: Some(0),
                actor_entity_id: 0,
                model_identity_index: None,
                content: "Tool result.".to_owned(),
                flags: 0,
            },
        ],
    }
}

fn temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phoenix-memory-contract-{label}-{}-{nonce}.phxgg3",
        std::process::id()
    ))
}
