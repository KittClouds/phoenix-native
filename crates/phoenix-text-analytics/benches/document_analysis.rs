use phoenix_text_analytics::analyze;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let paragraph = "Ryan noticed the thermal signal, but Ryan never trusted the golden network. The city answered with a long and luminous mechanical whisper. ";
    let source = paragraph.repeat(2_000);
    let started = Instant::now();
    let iterations = 20;
    for _ in 0..iterations {
        black_box(analyze(black_box(&source)));
    }
    let elapsed = started.elapsed();
    println!(
        "phoenix_text_analytics bytes={} iterations={} average_ms={:.3}",
        source.len(),
        iterations,
        elapsed.as_secs_f64() * 1_000.0 / iterations as f64
    );
}
