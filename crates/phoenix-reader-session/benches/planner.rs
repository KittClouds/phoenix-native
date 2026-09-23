use criterion::{black_box, criterion_group, criterion_main, Criterion};
use phoenix_reader_session::{plan_markdown, PlannerConfig};
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::sync::Arc;

fn bench(c: &mut Criterion) {
    let source = "# Chapter\n\n".to_owned()
        + &"A sentence with a stable source mapping. Another sentence follows.\n\n".repeat(256);
    let lease = DocumentLease {
        entry_id: EntryId(1),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(source.as_bytes()),
        content: Arc::from(source),
    };
    c.bench_function("markdown_plan_256_paragraphs", |b| {
        b.iter(|| black_box(plan_markdown([1; 32], &lease, PlannerConfig::default()).unwrap()))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
