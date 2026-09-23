//! Real reference-conditioned requests through the supervised provider and cache.
use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request, VoiceAsset};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

fn wav(path: &Path, pcm: &[u8]) -> std::io::Result<()> {
    let mut file = std::io::BufWriter::new(fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + pcm.len() as u32).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    for value in [1u16, 1u16] {
        file.write_all(&value.to_le_bytes())?;
    }
    for value in [24000u32, 48000u32] {
        file.write_all(&value.to_le_bytes())?;
    }
    for value in [2u16, 16u16] {
        file.write_all(&value.to_le_bytes())?;
    }
    file.write_all(b"data")?;
    file.write_all(&(pcm.len() as u32).to_le_bytes())?;
    file.write_all(pcm)?;
    file.flush()
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 6, "WORKER MODEL DLL_DIR VOICE OUTPUT");
    let root = Path::new(&args[5]);
    fs::create_dir(root)?;
    let bundle = Bundle::open(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    )?;
    let base = bundle.identity("", 42, 1_440_000)?;
    let path = Path::new(&args[4]);
    anyhow::ensure!(
        fs::metadata(path)?.len() <= phoenix_tts_native::MAX_VOICE_BYTES,
        "voice bounds"
    );
    let hash = *blake3::hash(&fs::read(path)?).as_bytes();
    let voice = VoiceAsset::open(path, hash, base.model, base.codec)?;
    let identity = voice.identity(&bundle, "", 42, 1_440_000)?;
    let mut provider =
        NativeProvider::new(bundle, Duration::from_secs(240), Duration::from_secs(120))?;
    let mut cache = AudioCache::open(root.join("cache"), 32 * 1024 * 1024)?;
    let mut receipts = Vec::new();
    for (segment, text) in [
        "The room was quiet. Outside the window, rain fell softly on the garden.",
        "At last, the sun appeared above the trees. He opened the door and stepped outside.",
    ]
    .into_iter()
    .enumerate()
    {
        let start = Instant::now();
        let mut first = None;
        let mut frames = 0;
        let key = provider.generate_voiced_streamed(
            Request {
                epoch: 1,
                plan: [92; 32],
                segment: segment as u32,
                text,
                instruction: "",
                seed: 42,
                max_frames: 1_440_000,
            },
            Some(&voice),
            &mut cache,
            &Cancellation::default(),
            |chunk| {
                assert_eq!(frames, chunk.first_frame);
                frames += chunk.pcm.len() as u64 / 2;
                first.get_or_insert(start.elapsed());
                Ok(())
            },
        )?;
        assert_eq!(key, identity.audio_key(text)?);
        let audio = cache.get(key)?;
        assert_eq!(audio.manifest().frames, frames);
        assert!(frames > 0);
        wav(&root.join(format!("passage-{segment}.wav")), audio.pcm())?;
        receipts.push(
            serde_json::json!({"segment":segment,"text":text,"frames":frames,
            "first_pcm_ms":first.unwrap().as_millis(),"total_ms":start.elapsed().as_millis(),
            "voice":hash,"key":key,"completion":"normal_eos_and_quiet"}),
        );
    }
    provider.stop()?;
    fs::write(
        root.join("passed.json"),
        serde_json::to_vec_pretty(&receipts)?,
    )?;
    Ok(())
}
