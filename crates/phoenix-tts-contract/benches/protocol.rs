use criterion::{black_box, criterion_group, criterion_main, Criterion};
use phoenix_tts_contract::*;
fn bench(c: &mut Criterion) {
    c.bench_function("validate_64_pcm_blocks", |b| {
        b.iter(|| {
            let binding = Binding {
                request: 1,
                epoch: 1,
                plan: [1; 32],
                segment: 0,
                audio_key: [2; 32],
            };
            let mut v = StreamValidator::new(binding, [3; 32], 131072).unwrap();
            v.accept(Envelope {
                binding,
                sequence: 0,
                event: Event::Started {
                    provider: [3; 32],
                    format: AudioFormat::PCM24,
                },
            })
            .unwrap();
            for i in 0..64 {
                black_box(
                    v.accept(Envelope {
                        binding,
                        sequence: i + 1,
                        event: Event::AudioChunk {
                            first_frame: i * 2048,
                            frames: 2048,
                        },
                    })
                    .unwrap(),
                );
            }
        })
    });
}
criterion_group!(benches, bench);
criterion_main!(benches);
