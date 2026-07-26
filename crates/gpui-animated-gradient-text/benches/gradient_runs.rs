use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use gpui::TextStyle;
use gpui_animated_gradient_text::GradientText;

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
}

criterion_group!(benches, gradient_runs);
criterion_main!(benches);
