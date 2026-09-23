//! Controlled short-dialogue reproduction; separate caches force every repeat.
use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request};
use serde_json::json;
use std::{fs, path::Path, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 6 {
        return Err("usage: dialogue_probe WORKER MODEL DLL_DIR CHAPTER_PLAN OUTPUT".into());
    }
    let plan: serde_json::Value = serde_json::from_slice(&fs::read(&a[4])?)?;
    let text = plan["spec"]["spoken"].as_str().ok_or("spoken")?;
    let ranges = plan["spec"]["segments"].as_array().ok_or("segments")?;
    let slice = |first: usize, last: usize| -> &str {
        let start = ranges[first]["spoken"]["start"].as_u64().unwrap() as usize;
        let end = ranges[last]["spoken"]["end"].as_u64().unwrap() as usize;
        &text[start..end]
    };
    let exact = slice(76, 76);
    let cases = [
        ("exact-repeat-1", exact, 42),
        ("exact-repeat-2", exact, 42),
        ("trimmed", exact.trim(), 42),
        ("ascii-quotes", "\"Drop your weapons!\"", 42),
        ("no-quotes", "Drop your weapons!", 42),
        ("previous-context", slice(75, 76), 42),
        ("next-context", slice(76, 77), 42),
        ("both-context", slice(75, 77), 42),
        ("exact-seed-43", exact, 43),
        ("exact-seed-44", exact, 44),
        ("control-short", slice(64, 64), 42),
        ("control-context", slice(63, 65), 42),
    ];
    let root = Path::new(&a[5]);
    fs::create_dir(root)?;
    let bundle = Bundle::open(Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]))?;
    let mut provider =
        NativeProvider::new(bundle, Duration::from_secs(240), Duration::from_secs(120))?;
    let mut receipts = Vec::new();
    for (i, (name, text, seed)) in cases.iter().enumerate() {
        let mut cache = AudioCache::open(root.join(name), 8 * 1024 * 1024)?;
        let result = provider.generate(
            Request {
                epoch: 1,
                plan: [8; 32],
                segment: i as u32,
                text,
                instruction: "A calm, clear English narrator.",
                seed: *seed,
                max_frames: 1_440_000,
            },
            &mut cache,
            &Cancellation::default(),
        );
        let row = match result {
            Ok(key) => {
                let audio = cache.get(key)?;
                fs::write(root.join(format!("{name}.pcm")), audio.pcm())?;
                json!({"case":name,"text":text,"seed":seed,"key":key,"frames":audio.manifest().frames,"audio_hash":audio.manifest().audio_hash,"status":"normal_eos"})
            }
            Err(e) => {
                json!({"case":name,"text":text,"seed":seed,"status":"failed","error":e.to_string()})
            }
        };
        println!("{row}");
        receipts.push(row);
        fs::write(
            root.join("cases.json"),
            serde_json::to_vec_pretty(&receipts)?,
        )?;
    }
    provider.stop()?;
    Ok(())
}
