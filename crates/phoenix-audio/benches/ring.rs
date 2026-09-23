use criterion::{black_box, criterion_group, criterion_main, Criterion};
use phoenix_audio::pcm_ring;
use phoenix_tts_contract::BLOCK_FRAMES;
fn bench(c: &mut Criterion) {
    let (producer, mut consumer, _control) = pcm_ring();
    let mut out = [0; BLOCK_FRAMES];
    c.bench_function("pooled_2048_frame_handoff", |b| {
        b.iter(|| {
            let mut block = producer.acquire().unwrap();
            block.prepare(1, 0, 0, BLOCK_FRAMES).unwrap();
            assert!(producer.submit(block).is_ok());
            black_box(consumer.render(&mut out, 0).unwrap());
        })
    });
}
criterion_group!(benches, bench);
criterion_main!(benches);
