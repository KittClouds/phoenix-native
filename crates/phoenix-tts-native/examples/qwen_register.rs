//! Registers an already encoded Qwen clone in a Reader voice library, exactly
//! as in-app enrollment does after `phoenix-qwen-worker --enroll`:
//! qwen_register <storage> <worker> <talker> <codec> <voice.qwen> <original.wav> <name>
use phoenix_reader_session::{VoiceLibrary, VoiceProfile, VoiceReference};
use phoenix_tts_native::{Bundle, Cancellation, Engine, VoiceAsset};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        a.len() == 8,
        "usage: qwen_register <storage> <worker> <talker> <codec> <voice.qwen> <original.wav> <name>"
    );
    let bundle = Bundle::open_qwen(Path::new(&a[2]), Path::new(&a[3]), Path::new(&a[4]), &Cancellation::default())?;
    let identity = bundle.identity("", 42, 1_440_000)?;
    drop(bundle);
    let encoded_bytes = std::fs::read(&a[5])?;
    let encoded = *blake3::hash(&encoded_bytes).as_bytes();
    let asset = VoiceAsset::open(Path::new(&a[5]), encoded, identity.model, identity.codec)?;
    anyhow::ensure!(asset.engine() == Engine::Qwen, "not a Qwen voice");
    let transcript = asset.transcript().to_owned();
    drop(asset);
    let root = Path::new(&a[1]).join("voices");
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join(format!("{}.qwen", blake3::Hash::from(encoded).to_hex())), &encoded_bytes)?;
    let original = std::fs::read(&a[6])?;
    let original_hash = *blake3::hash(&original).as_bytes();
    std::fs::write(root.join(format!("{}.wav", blake3::Hash::from(original_hash).to_hex())), &original)?;
    let mut id = blake3::Hasher::new();
    id.update(b"phoenix.user-qwen-reference-profile/v1\0");
    id.update(a[7].as_bytes());
    id.update(&encoded);
    let profile = VoiceProfile {
        id: *id.finalize().as_bytes(),
        revision: 1,
        name: a[7].clone(),
        description: "Qwen reference voice created from your recording.".into(),
        reference: Some(VoiceReference {
            encoded,
            original_audio: original_hash,
            transcript: *blake3::hash(transcript.as_bytes()).as_bytes(),
            model: identity.model,
            codec: identity.codec,
        }),
        default_delivery: String::new(),
        seed: 42,
    };
    let choice = VoiceLibrary::open(&root)?.save(&profile)?;
    println!("registered {} ({:?})", a[7], choice);
    Ok(())
}
