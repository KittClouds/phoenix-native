use criterion::{black_box, criterion_group, criterion_main, Criterion};
use phoenix_reader_session::*;
fn bench(c: &mut Criterion) {
    let text = "A sentence.\n\n".repeat(10000);
    let spoken = text.clone();
    let range = ByteRange {
        start: 0,
        end: text.len() as u32,
    };
    let runs = [MappingRun {
        source: range,
        spoken: range,
        kind: MappingKind::Copy,
        rule: 0,
    }];
    c.bench_function("validate_130kb_copy_mapping", |b| {
        b.iter(|| validate_mapping(black_box(&text), black_box(&spoken), &runs, 0).unwrap())
    });
    c.bench_function("simd_paragraph_scan_130kb", |b| {
        b.iter(|| black_box(paragraph_breaks(&text).count()))
    });
}
criterion_group!(benches, bench);
criterion_main!(benches);
