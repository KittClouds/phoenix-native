//! Real CPU runner -> validated/resampled cache, including warm cache reuse.
use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{
    supertonic::{SupertonicBundle, SupertonicProvider, STYLES},
    Cancellation, Request,
};
use std::{path::Path, time::Instant};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "RUNNER MODEL_ROOT OUTPUT_ROOT");
    let root = Path::new(&args[3]);
    std::fs::create_dir(root)?;
    let start = Instant::now();
    let bundle = SupertonicBundle::open(
        Path::new(&args[1]),
        Path::new(&args[2]),
        &Cancellation::default(),
    )?;
    let enrollment_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut provider = SupertonicProvider::new(bundle, root.join("jobs"))?;
    let mut cache = AudioCache::open(root.join("cache"), 64 * 1024 * 1024)?;
    let text="The rain had stopped. She opened the old book and smiled. At last, the journey could begin.";
    let request = || Request {
        epoch: 1,
        plan: [7; 32],
        segment: 0,
        text,
        instruction: "",
        seed: 0,
        max_frames: 1_440_000,
    };
    let mut receipts = Vec::new();
    for style in STYLES {
        let start = Instant::now();
        let mut frames = 0;
        let key = provider.generate(
            request(),
            style,
            &mut cache,
            &Cancellation::default(),
            |chunk| {
                assert_eq!(frames, chunk.first_frame);
                frames += chunk.pcm.len() as u64 / 2;
                Ok(())
            },
        )?;
        let seconds = start.elapsed().as_secs_f64();
        let audio = cache.get(key)?;
        anyhow::ensure!(
            frames == audio.manifest().frames && frames > 0,
            "frame receipt mismatch"
        );
        let hit = Instant::now();
        let again = provider.generate(
            request(),
            style,
            &mut cache,
            &Cancellation::default(),
            |_| panic!("cached request generated"),
        )?;
        anyhow::ensure!(key == again, "cache key changed");
        receipts.push(serde_json::json!({"style":style,"seconds":seconds,"audio_seconds":frames as f64/24000.,"rtf":seconds/(frames as f64/24000.),"cache_hit_ms":hit.elapsed().as_secs_f64()*1000.,"key":blake3::Hash::from(key).to_hex().to_string()}));
    }
    let receipt = serde_json::json!({"scope":"CPU on current host; transport/cache proof, not listening or lower-end hardware qualification", "enrollment_ms":enrollment_ms,"voices":receipts});
    std::fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
