use phoenix_scene_compiler::{compile_active_document, NativeSceneCompilerInput};
use phoenix_scene_contract::{EntityKind, HighlightPalette};
use phoenix_workspace::{
    ContentHash, DocumentLease, DocumentRevision, EntityRegistry, EntityTag, EntryId,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ENTITIES: usize = 1_000;
const ENTITIES_PER_PARAGRAPH: usize = 16;
const TRIALS: usize = 30;

fn main() {
    let (document, registry) = fixture();
    let mut timings = Vec::with_capacity(TRIALS);
    for _ in 0..TRIALS {
        let started = Instant::now();
        let compiled = compile_active_document(NativeSceneCompilerInput {
            generation_id: 2,
            registry_revision: registry.revision(),
            document: &document,
            registry: &registry,
            palette: HighlightPalette::default(),
        })
        .unwrap_or_else(|error| panic!("compile benchmark fixture: {error}"));
        std::hint::black_box((
            compiled.receipt.node_count,
            compiled.receipt.edge_count,
            compiled.publication.positions[0].as_ptr(),
        ));
        timings.push(started.elapsed());
    }
    timings.sort_unstable();
    let median = percentile(&timings, 50);
    let p95 = percentile(&timings, 95);
    let max = timings.last().copied().unwrap_or(Duration::ZERO);
    println!(
        "active-document native scene compile: entities={ENTITIES} \
         paragraphs={} trials={TRIALS} median={median:?} p95={p95:?} max={max:?}",
        ENTITIES.div_ceil(ENTITIES_PER_PARAGRAPH)
    );
}

fn fixture() -> (DocumentLease, EntityRegistry) {
    let mut content = String::with_capacity(ENTITIES * 16);
    let mut tags = Vec::with_capacity(ENTITIES);
    for ordinal in 0..ENTITIES {
        if ordinal > 0 {
            if ordinal % ENTITIES_PER_PARAGRAPH == 0 {
                content.push_str("\n\n");
            } else {
                content.push(' ');
            }
        }
        let start = u32::try_from(content.len()).expect("fixture offset fits u32");
        let label = format!("Entity{ordinal:04}");
        content.push_str(&label);
        let end = u32::try_from(content.len()).expect("fixture offset fits u32");
        tags.push((start, end, label));
    }
    let content: Arc<str> = Arc::from(content);
    let document = DocumentLease {
        entry_id: EntryId(42),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(content.as_bytes()),
        content,
    };
    let mut registry = EntityRegistry::empty();
    for (ordinal, (start, end, surface)) in tags.into_iter().enumerate() {
        registry
            .tag(
                &document,
                EntityTag {
                    kind: if ordinal % 2 == 0 {
                        EntityKind::Character
                    } else {
                        EntityKind::Location
                    },
                    custom_kind: None,
                    start,
                    end,
                    surface,
                },
            )
            .unwrap_or_else(|error| panic!("tag benchmark entity {ordinal}: {error}"));
    }
    (document, registry)
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let index = sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(index).copied().unwrap_or(Duration::ZERO)
}
