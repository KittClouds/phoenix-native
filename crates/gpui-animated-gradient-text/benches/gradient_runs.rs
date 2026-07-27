use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use gpui::TextStyle;
use gpui_animated_gradient_text::{ColorSpace, GradientPalette, GradientText};

fn gradient_runs(criterion: &mut Criterion) {
    let style = TextStyle::default();
    let mut group = criterion.benchmark_group("gradient_runs");

    for graphemes in [16_usize, 64, 256, 4_096] {
        let source = "G".repeat(graphemes);
        let text = GradientText::phoenix(source).max_color_runs(64);
        group.throughput(Throughput::Elements(graphemes as u64));
        group.bench_with_input(
            BenchmarkId::new("bounded_64", graphemes),
            &text,
            |bencher, text| {
                bencher.iter(|| black_box(text.color_runs(black_box(&style))));
            },
        );
    }

    group.finish();

    let palette = GradientPalette::phoenix().with_color_space(ColorSpace::Oklab);
    criterion.bench_function("palette_sample_oklab", |bencher| {
        let mut phase = 0.0_f32;
        bencher.iter(|| {
            phase = (phase + 0.001).fract();
            black_box(palette.sample(black_box(phase)))
        });
    });

    let words = GradientText::phoenix("26198 words").max_color_runs(12);
    let chars = GradientText::phoenix("152000 chars").max_color_runs(12);
    criterion.bench_function("footer_pair_animated_frame", |bencher| {
        let mut phase = 0.0_f32;
        bencher.iter(|| {
            phase = (phase + (1.0 / 300.0)).fract();
            black_box(words.clone().phase(phase).color_runs(black_box(&style)));
            black_box(chars.clone().phase(phase).color_runs(black_box(&style)));
        });
    });
}

criterion_group!(benches, gradient_runs);
criterion_main!(benches);
