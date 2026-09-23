//! Real GPU worker and durable cache qualification; no device/UI side effects.
use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{Bundle, Cancellation, Error, NativeProvider, Request};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn request(text: &str, max_frames: u64) -> Request<'_> {
    Request {
        epoch: 1,
        plan: [7; 32],
        segment: 0,
        text,
        instruction: "A calm, clear English narrator.",
        seed: 42,
        max_frames,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: cache_smoke WORKER MODEL DLL_DIRECTORY CACHE_ROOT".into());
    }
    let start = Instant::now();
    let bundle = Bundle::open(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    )?;
    println!("bundle_hash_ms={}", start.elapsed().as_millis());
    let mut provider =
        NativeProvider::new(bundle, Duration::from_secs(240), Duration::from_secs(120))?;
    let mut cache = AudioCache::open(&args[4], 32 * 1024 * 1024)?;
    let text = "The room was quiet. Outside the window, rain fell softly on the garden.";
    let cancel = Cancellation::default();
    let start = Instant::now();
    let mut first = None;
    let mut frames = 0;
    let key =
        provider.generate_streamed(request(text, 24000 * 20), &mut cache, &cancel, |chunk| {
            assert_eq!(chunk.binding.epoch, 1);
            assert_eq!(chunk.first_frame, frames);
            frames += chunk.pcm.len() as u64 / 2;
            first.get_or_insert(start.elapsed());
            Ok(())
        })?;
    let audio = cache.get(key)?;
    assert!(
        frames > 0,
        "use a fresh cache root for real generation proof"
    );
    assert_eq!(audio.manifest().frames, frames);
    assert_eq!(audio.pcm().len() as u64, frames * 2);
    assert!(audio.pcm().iter().any(|b| *b != 0));
    println!(
        "normal_complete=true frames={frames} first_pcm_ms={} total_ms={}",
        first.unwrap().as_millis(),
        start.elapsed().as_millis()
    );
    drop(audio);
    let pid = provider.pid();
    let hit = Instant::now();
    assert_eq!(
        provider.generate(request(text, 24000 * 20), &mut cache, &cancel)?,
        key
    );
    assert_eq!(provider.pid(), pid);
    println!("cache_hit_ms={}", hit.elapsed().as_millis());
    let bytes = cache.bytes();
    let limited = provider.generate(
        request(
            "The journey was only beginning, and there were many miles still ahead of them.",
            1920,
        ),
        &mut cache,
        &cancel,
    );
    assert!(limited.is_err());
    assert_eq!(cache.bytes(), bytes);
    assert_eq!(provider.pid(), None);
    println!("token_limit_rejected=true worker_exit_confirmed=true");
    let trigger = cancel.clone();
    let mut fired = None;
    let cancelled = provider.generate_streamed(
        request(
            "She opened the door and listened to the distant sound of bells.",
            24000 * 20,
        ),
        &mut cache,
        &cancel,
        |_| {
            fired = Some(Instant::now());
            trigger.cancel();
            Ok(())
        },
    );
    assert!(matches!(cancelled, Err(Error::Cancelled)));
    assert_eq!(cache.bytes(), bytes);
    assert_eq!(provider.pid(), None);
    println!(
        "cancel_after_pcm_ms={} partial_not_published=true",
        fired.unwrap().elapsed().as_millis()
    );
    let recovered = provider.generate(
        request("At last, the sun appeared above the trees.", 24000 * 20),
        &mut cache,
        &Cancellation::default(),
    )?;
    assert!(cache.get(recovered)?.manifest().frames > 0);
    provider.stop()?;
    drop(cache);
    let mut reopened = AudioCache::open(&args[4], 32 * 1024 * 1024)?;
    assert_eq!(reopened.get(key)?.manifest().frames, frames);
    assert!(reopened.contains(recovered));
    println!("restart_recovery=true durable_reopen=true");
    Ok(())
}
