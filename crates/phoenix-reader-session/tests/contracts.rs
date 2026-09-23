mod support;
use phoenix_reader_session::*;
use phoenix_tts_contract::{AudioFormat, Event, FinishReason};
use support::*;

#[test]
fn revision_snapshot_cache_checkpoint_restart_smoke() {
    let root = tempfile::tempdir().unwrap();
    let old = lease("A narrator reads.", 1);
    let plan = plan(&old);
    let snapshots = SnapshotStore::open(root.path().join("snapshots")).unwrap();
    snapshots.retain(&old).unwrap();
    snapshots.retain_plan(&plan).unwrap();
    let edited = lease("An edited story.", 2);
    assert!(!plan.spec().document.matches_editor([1; 32], &edited));
    let mut cache = AudioCache::open(root.path().join("audio"), 2 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, &old.content);
    let mut session = ReaderSession::new([7; 32], &plan, [6; 32]).unwrap();
    session
        .set_position(&plan, 0, 3, &cache.get(key).unwrap())
        .unwrap();
    session.bookmark(1).unwrap();
    let mut store = SessionStore::open(root.path().join("sessions")).unwrap();
    store.checkpoint(&session, &plan, 1000, true).unwrap();
    let stale = session.clone();
    session.set_speed(1300).unwrap();
    assert!(!store.checkpoint(&session, &plan, 1500, false).unwrap());
    store.checkpoint(&session, &plan, 1500, true).unwrap();
    assert!(store.checkpoint(&stale, &plan, 1600, true).is_err());
    drop((store, cache, snapshots));
    let snapshots = SnapshotStore::open(root.path().join("snapshots")).unwrap();
    let restored = snapshots.load_plan(plan.id(), old.content_hash.0).unwrap();
    let store = SessionStore::open(root.path().join("sessions")).unwrap();
    let restored_session = store.load([7; 32], &restored).unwrap();
    assert_eq!(restored_session.position().source_frame, 3);
    assert_eq!(restored_session.speed_milli(), 1300);
    assert_eq!(restored_session.bookmarks()[&1].source_frame, 3);
    let mut cache = AudioCache::open(root.path().join("audio"), 2 * 1024 * 1024).unwrap();
    assert_eq!(cache.get(key).unwrap().pcm(), &[1, 0, 2, 0, 3, 0, 4, 0]);
    assert_eq!(
        restored_session
            .resume_position(&restored, &cache.get(key).unwrap())
            .unwrap()
            .source_frame,
        3
    );
}

#[test]
fn unicode_pronunciation_and_markup_accounting() {
    let source = "**Dr.** Éva";
    let spoken = "Doctor Éva";
    let r = |a, b, c, d, kind, rule| MappingRun {
        source: ByteRange { start: a, end: b },
        spoken: ByteRange { start: c, end: d },
        kind,
        rule,
    };
    let runs = [
        r(0, 2, 0, 0, MappingKind::Omit, 1),
        r(2, 5, 0, 6, MappingKind::Replace, 2),
        r(5, 7, 6, 6, MappingKind::Omit, 1),
        r(7, 12, 6, 11, MappingKind::Copy, 0),
    ];
    validate_mapping(source, spoken, &runs, 2).unwrap();
    let mut paint = Vec::new();
    SourceMap::new(source, spoken, &runs, 2)
        .unwrap()
        .project(ByteRange { start: 1, end: 3 }, |r| paint.push(r))
        .unwrap();
    assert_eq!(paint, vec![ByteRange { start: 2, end: 5 }]);
    let mut bad = runs;
    bad[3].source.end = 9; // splits É
    assert!(validate_mapping(source, spoken, &bad, 2).is_err());
    let mut bad = runs;
    bad[1].spoken.start = 1;
    assert!(validate_mapping(source, spoken, &bad, 2).is_err());
}

#[test]
fn interrupted_and_truncated_outputs_never_become_hits() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let b = binding("Hello.");
    for reason in [FinishReason::TransportEof, FinishReason::TokenLimit] {
        let mut w = cache.begin(b, identity(), "Hello.", 10).unwrap();
        w.push(
            event(
                b,
                0,
                Event::Started {
                    provider: identity().provider,
                    format: AudioFormat::PCM24,
                },
            ),
            &[],
        )
        .unwrap();
        w.push(
            event(
                b,
                1,
                Event::AudioChunk {
                    first_frame: 0,
                    frames: 1,
                },
            ),
            &[0, 0],
        )
        .unwrap();
        assert!(w
            .finish(event(b, 2, Event::Completed { frames: 1, reason }), None)
            .is_err());
        assert!(!cache.contains(b.audio_key));
    }
    {
        let _unfinished = cache.begin(b, identity(), "Hello.", 10).unwrap();
    }
    assert!(!cache.contains(b.audio_key));
    drop(cache);
    // Simulate crash after writing payload/commit but before directory publication.
    let unfinished = root
        .path()
        .join(format!("{}.writing", uuid::Uuid::new_v4()));
    std::fs::create_dir(&unfinished).unwrap();
    std::fs::write(unfinished.join("audio.pcm"), [0, 0]).unwrap();
    std::fs::write(unfinished.join("commit"), b"unpublished").unwrap();
    let cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    assert!(!cache.contains(b.audio_key));
    assert!(!unfinished.exists());
}

#[test]
fn mmap_lease_prevents_eviction_and_corruption_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path(), 1024 * 1024 + 200).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let lease = cache.get(key).unwrap();
    let b = binding("Other.");
    assert!(cache.begin(b, identity(), "Other.", 100).is_err());
    drop(lease);
    let other = write_cache(&mut cache, "Other.");
    assert!(!cache.contains(key));
    drop(cache);
    let path = root
        .path()
        .join(blake3::Hash::from(other).to_hex().to_string())
        .join("audio.pcm");
    std::fs::write(path, [9, 0, 2, 0, 3, 0, 4, 0]).unwrap();
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    assert!(cache.get(other).is_err());
}

#[test]
fn audio_lease_keeps_store_ownership_after_cache_handle_is_dropped() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let key = write_cache(&mut cache, "Hello.");
    let audio = cache.get(key).unwrap();
    drop(cache);
    assert!(AudioCache::open(root.path(), 2 * 1024 * 1024).is_err());
    assert_eq!(audio.pcm().len(), 8);
    drop(audio);
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    assert_eq!(cache.get(key).unwrap().pcm().len(), 8);
}

#[test]
fn audio_identity_covers_voice_seed_and_runtime_but_not_document_offset() {
    let original = identity();
    let key = original.audio_key("Hello.").unwrap();
    let mut changed = original.clone();
    changed.seed += 1;
    assert_ne!(key, changed.audio_key("Hello.").unwrap());
    changed = original.clone();
    changed.runtime[0] ^= 1;
    assert_ne!(key, changed.audio_key("Hello.").unwrap());
    changed = original.clone();
    changed.voice[0] ^= 1;
    assert_ne!(key, changed.audio_key("Hello.").unwrap());
    assert_eq!(key, original.audio_key("Hello.").unwrap());
}

#[test]
fn streamed_alignment_is_bound_to_realized_audio_and_source_text() {
    use phoenix_tts_contract::{AlignmentHint, AlignmentLevel};
    let root = tempfile::tempdir().unwrap();
    let mut cache = AudioCache::open(root.path(), 2 * 1024 * 1024).unwrap();
    let b = binding("Hi.");
    let mut w = cache.begin(b, identity(), "Hi.", 10).unwrap();
    w.push(
        event(
            b,
            0,
            Event::Started {
                provider: identity().provider,
                format: AudioFormat::PCM24,
            },
        ),
        &[],
    )
    .unwrap();
    w.push(
        event(
            b,
            1,
            Event::AudioChunk {
                first_frame: 0,
                frames: 4,
            },
        ),
        &[0; 8],
    )
    .unwrap();
    w.push(
        event(
            b,
            2,
            Event::AlignmentChunk(AlignmentHint {
                level: AlignmentLevel::Word,
                provenance: [5; 32],
                spoken_start: 0,
                spoken_end: 3,
                first_frame: 0,
                end_frame: 4,
            }),
        ),
        &[],
    )
    .unwrap();
    w.finish(
        event(
            b,
            3,
            Event::Completed {
                frames: 4,
                reason: FinishReason::Normal,
            },
        ),
        None,
    )
    .unwrap();
    let audio = cache.get(b.audio_key).unwrap();
    assert_eq!(audio.manifest().alignment.level, AlignmentLevel::Word);
    assert_eq!(
        audio.manifest().alignment.audio_hash,
        audio.manifest().audio_hash
    );
}

#[test]
fn request_admission_requires_full_prompt_budget_and_completion_capability() {
    use phoenix_tts_contract::{AlignmentLevel, Capabilities, SynthesisRequest};
    let identity = identity();
    let b = binding("Hi.");
    let caps = Capabilities {
        provider: identity.provider,
        format: AudioFormat::PCM24,
        max_context_tokens: 100,
        max_output_frames: 100,
        cancellation: false,
        normal_finish_reason: true,
        concurrency: 1,
        alignment: AlignmentLevel::Segment,
    };
    SynthesisRequest::new(b, &identity, "Hi.", caps, 100, 100).unwrap();
    assert!(SynthesisRequest::new(b, &identity, "Hi.", caps, 101, 100).is_err());
    assert!(SynthesisRequest::new(
        b,
        &identity,
        "Hi.",
        Capabilities {
            normal_finish_reason: false,
            ..caps
        },
        10,
        100
    )
    .is_err());
}
