//! Import encoded Breeze references and direction variants into one Reader library.
//! Usage: enroll_reader_voices WORKER MODEL DLL_DIR STORAGE MANIFEST.json
use anyhow::{Context, Result};
use phoenix_reader_session::{VoiceLibrary, VoiceProfile, VoiceReference};
use phoenix_tts_native::{Bundle, VoiceAsset, MAX_VOICE_BYTES};
use serde::Deserialize;
use std::{fs, io::Write, path::Path};

#[derive(Deserialize)]
struct Entry {
    name: String,
    description: String,
    original_audio: String,
    encoded_voice: String,
    transcript: String,
    #[serde(default)]
    direction: String,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 6,
        "WORKER MODEL DLL_DIR STORAGE MANIFEST.json"
    );
    let bundle = Bundle::open(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    )?;
    let base = bundle.identity("", 42, 1_440_000)?;
    let root = Path::new(&args[4]).join("voices");
    let entries: Vec<Entry> = serde_json::from_slice(&fs::read(&args[5])?)?;
    anyhow::ensure!(
        !entries.is_empty() && entries.len() <= 257,
        "manifest bounds"
    );
    let library = VoiceLibrary::open(&root)?;
    for entry in entries {
        anyhow::ensure!(
            fs::metadata(&entry.encoded_voice)?.len() <= MAX_VOICE_BYTES,
            "encoded voice bounds"
        );
        let bytes = fs::read(&entry.encoded_voice)?;
        let encoded = *blake3::hash(&bytes).as_bytes();
        let voice = VoiceAsset::open(
            Path::new(&entry.encoded_voice),
            encoded,
            base.model,
            base.codec,
        )?;
        anyhow::ensure!(
            voice.transcript() == entry.transcript,
            "reference transcript mismatch"
        );
        let original = fs::read(&entry.original_audio)?;
        anyhow::ensure!(original.len() <= 8 * 1024 * 1024, "original audio bounds");
        let destination = root.join(format!("{}.breeze", blake3::Hash::from(encoded).to_hex()));
        if destination.exists() {
            anyhow::ensure!(
                *blake3::hash(&fs::read(&destination)?).as_bytes() == encoded,
                "installed voice hash mismatch"
            );
        } else {
            let mut temporary = tempfile::NamedTempFile::new_in(&root)?;
            temporary.write_all(&bytes)?;
            temporary.as_file_mut().sync_all()?;
            temporary
                .persist(&destination)
                .context("publish encoded voice")?;
        }
        let mut id = blake3::Hasher::new();
        id.update(b"phoenix.user-reference-profile/v1\0");
        id.update(entry.name.as_bytes());
        id.update(&encoded);
        id.update(entry.direction.as_bytes());
        let profile = VoiceProfile {
            id: *id.finalize().as_bytes(),
            revision: 1,
            name: entry.name,
            description: entry.description,
            reference: Some(VoiceReference {
                encoded,
                original_audio: *blake3::hash(&original).as_bytes(),
                transcript: *blake3::hash(entry.transcript.as_bytes()).as_bytes(),
                model: base.model,
                codec: base.codec,
            }),
            default_delivery: entry.direction,
            seed: 42,
        };
        let choice = library.save(&profile)?;
        println!(
            "{} {}",
            profile.name,
            blake3::Hash::from(choice.fingerprint).to_hex()
        );
    }
    Ok(())
}
