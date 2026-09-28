//! End-to-end smoke of the Qwen PBN1 worker through the Rust supervisor:
//! qwen_smoke <worker.exe> <talker.gguf> <codec.gguf> <voice.qwen> <text> <out.pcm>
use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request, VoiceAsset};
use std::{path::Path, time::Duration, time::Instant};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() == 7, "usage: qwen_smoke <worker> <talker> <codec> <voice.qwen> <text> <out.pcm>");
    let cancel = Cancellation::default();
    let started = Instant::now();
    let bundle = Bundle::open_qwen(Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]), &cancel)?;
    let voice_bytes = std::fs::read(&a[4])?;
    let hash = *blake3::hash(&voice_bytes).as_bytes();
    let probe = bundle.identity("", 42, 1_440_000)?;
    let voice = VoiceAsset::open(Path::new(&a[4]), hash, probe.model, probe.codec)?;
    println!("bundle+voice {:?}", started.elapsed());
    let mut provider = NativeProvider::new(bundle, Duration::from_secs(240), Duration::from_secs(120))?;
    let root = tempfile::tempdir()?;
    let mut cache = AudioCache::open(root.path(), 64 * 1024 * 1024)?;
    for (epoch, text) in [(1u64, a[5].as_str()), (2, "A second request reuses the resident model.")] {
        let t = Instant::now();
        let key = provider.generate_voiced_streamed(
            Request {
                epoch,
                plan: [7; 32],
                segment: epoch as u32,
                text,
                instruction: "",
                seed: 42,
                max_frames: 1_440_000,
            },
            Some(&voice),
            &mut cache,
            &cancel,
            |_| Ok(()),
        )?;
        let audio = cache.get(key)?;
        let seconds = audio.manifest().frames as f64 / 24000.0;
        println!(
            "request {epoch}: {:.2}s audio in {:?} (pid {:?})",
            seconds,
            t.elapsed(),
            provider.pid()
        );
        if epoch == 1 {
            std::fs::write(&a[6], audio.pcm())?;
        }
    }
    provider.stop()?;
    Ok(())
}
