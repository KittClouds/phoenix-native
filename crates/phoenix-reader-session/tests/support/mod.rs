use phoenix_reader_session::*;
use phoenix_tts_contract::*;
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::sync::Arc;
pub fn lease(text: &str, revision: u64) -> DocumentLease {
    DocumentLease {
        entry_id: EntryId(3),
        revision: DocumentRevision(revision),
        content_hash: ContentHash::of(text.as_bytes()),
        content: Arc::from(text),
    }
}
pub fn plan(lease: &DocumentLease) -> NarrationPlan {
    let range = ByteRange {
        start: 0,
        end: lease.content.len() as u32,
    };
    NarrationPlan::new(
        &lease.content,
        PlanSpec {
            document: DocumentBinding::from_lease([1; 32], lease).unwrap(),
            planner: [2; 32],
            pronunciation: [3; 32],
            rules: Box::new([]),
            spoken: lease.content.to_string().into_boxed_str(),
            mappings: vec![MappingRun {
                source: range,
                spoken: range,
                kind: MappingKind::Copy,
                rule: 0,
            }]
            .into_boxed_slice(),
            chapters: vec![Chapter { source: range }].into_boxed_slice(),
            segments: vec![Segment {
                chapter: 0,
                sentence: 0,
                source: range,
                spoken: range,
            }]
            .into_boxed_slice(),
        },
    )
    .unwrap()
}
pub fn identity() -> SynthesisIdentity {
    SynthesisIdentity {
        provider: [1; 32],
        runtime: [2; 32],
        model: [3; 32],
        tokenizer: [4; 32],
        codec: [5; 32],
        voice: [6; 32],
        reference_audio: None,
        reference_transcript: None,
        direction: [7; 32],
        generation_config: [8; 32],
        transformations: [9; 32],
        postprocessing: [10; 32],
        seed: 42,
        format: AudioFormat::PCM24,
    }
}
pub fn binding(text: &str) -> Binding {
    Binding {
        request: 1,
        epoch: 1,
        plan: [8; 32],
        segment: 0,
        audio_key: identity().audio_key(text).unwrap(),
    }
}
pub fn event(binding: Binding, sequence: u64, event: Event) -> Envelope {
    Envelope {
        binding,
        sequence,
        event,
    }
}
pub fn write_cache(cache: &mut AudioCache, text: &str) -> Digest {
    let b = binding(text);
    let mut writer = cache.begin(b, identity(), text, 100).unwrap();
    writer
        .push(
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
    writer
        .push(
            event(
                b,
                1,
                Event::AudioChunk {
                    first_frame: 0,
                    frames: 4,
                },
            ),
            &[1, 0, 2, 0, 3, 0, 4, 0],
        )
        .unwrap();
    writer
        .finish(
            event(
                b,
                2,
                Event::Completed {
                    frames: 4,
                    reason: FinishReason::Normal,
                },
            ),
            None,
        )
        .unwrap();
    b.audio_key
}
