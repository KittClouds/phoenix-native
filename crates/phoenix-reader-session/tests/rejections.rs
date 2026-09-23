use phoenix_reader_session::*;
use phoenix_tts_contract::AlignmentLevel;
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::sync::Arc;

fn fixture() -> (String, PlanSpec) {
    let source = "One. Two.".to_owned();
    let lease = DocumentLease {
        entry_id: EntryId(3),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(source.as_bytes()),
        content: Arc::from(source.as_str()),
    };
    let all = ByteRange { start: 0, end: 9 };
    let spec = PlanSpec {
        document: DocumentBinding::from_lease([1; 32], &lease).unwrap(),
        planner: [2; 32],
        pronunciation: [3; 32],
        rules: Box::new([]),
        spoken: source.clone().into_boxed_str(),
        mappings: vec![MappingRun {
            source: all,
            spoken: all,
            kind: MappingKind::Copy,
            rule: 0,
        }]
        .into_boxed_slice(),
        chapters: vec![
            Chapter {
                source: ByteRange { start: 0, end: 5 },
            },
            Chapter {
                source: ByteRange { start: 5, end: 9 },
            },
        ]
        .into_boxed_slice(),
        segments: vec![
            Segment {
                chapter: 0,
                sentence: 0,
                source: ByteRange { start: 0, end: 5 },
                spoken: ByteRange { start: 0, end: 5 },
            },
            Segment {
                chapter: 1,
                sentence: 1,
                source: ByteRange { start: 5, end: 9 },
                spoken: ByteRange { start: 5, end: 9 },
            },
        ]
        .into_boxed_slice(),
    };
    (source, spec)
}
#[test]
fn plan_rejects_chapter_crossing_gaps_stale_hashes_and_invalid_projection() {
    let (source, spec) = fixture();
    let valid = NarrationPlan::new(&source, spec.clone()).unwrap();
    let encoded = valid.encode().unwrap();
    NarrationPlan::decode(&source, &encoded, valid.id()).unwrap();
    assert!(NarrationPlan::decode("New. Two.", &encoded, valid.id()).is_err());
    let mut bad = spec.clone();
    bad.segments[0].source.end = 6;
    assert!(NarrationPlan::new(&source, bad).is_err());
    let mut bad = spec.clone();
    bad.segments[1].spoken.start = 6;
    assert!(NarrationPlan::new(&source, bad).is_err());
    let mut bad = spec;
    bad.document.content = [9; 32];
    assert!(NarrationPlan::new(&source, bad).is_err());
    assert!(valid
        .project(ByteRange { start: 0, end: 10 }, |_| panic!(
            "invalid range painted"
        ))
        .is_err());
}
#[test]
fn alignment_rejects_wrong_audio_overlap_and_unaccounted_speech() {
    let text = "one two";
    let alignment = Alignment {
        level: AlignmentLevel::Word,
        provenance: [1; 32],
        audio_hash: [2; 32],
        spoken_hash: *blake3::hash(text.as_bytes()).as_bytes(),
        ranges: vec![
            AlignedRange {
                spoken: ByteRange { start: 0, end: 3 },
                first_frame: 0,
                end_frame: 30,
            },
            AlignedRange {
                spoken: ByteRange { start: 4, end: 7 },
                first_frame: 40,
                end_frame: 70,
            },
        ]
        .into_boxed_slice(),
    };
    alignment.validate(text, [2; 32], 80).unwrap();
    assert!(alignment.validate(text, [3; 32], 80).is_err());
    let mut bad = alignment.clone();
    bad.ranges[1].first_frame = 20;
    assert!(bad.validate(text, [2; 32], 80).is_err());
    let mut bad = alignment;
    bad.ranges[1].spoken.start = 5;
    assert!(bad.validate(text, [2; 32], 80).is_err());
}
#[test]
fn stores_reject_a_second_writer_and_release_ownership_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let first = SessionStore::open(root.path()).unwrap();
    assert!(SessionStore::open(root.path()).is_err());
    drop(first);
    SessionStore::open(root.path()).unwrap();
}

#[test]
fn empty_spoken_query_cannot_paint_a_replacement() {
    let source = "Dr.";
    let spoken = "Doctor";
    let runs = [MappingRun {
        source: ByteRange { start: 0, end: 3 },
        spoken: ByteRange { start: 0, end: 6 },
        kind: MappingKind::Replace,
        rule: 1,
    }];
    let map = SourceMap::new(source, spoken, &runs, 1).unwrap();
    map.project(ByteRange { start: 2, end: 2 }, |_| {
        panic!("empty query painted")
    })
    .unwrap();
}
