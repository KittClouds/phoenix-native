//! Adversarial CLI fixture, never a production inference engine.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let value = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    // Match upstream's string-based forward-slash join. This fails for verbatim
    // Windows paths and protects the real-model path regression.
    assert!(
        std::path::Path::new(&format!("{}/duration_predictor.onnx", value("--onnx-dir"))).is_file()
    );
    let text = value("--text");
    if text == "slow" {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    let path = std::path::PathBuf::from(value("--save-dir"));
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(8036u32.to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(24000u32.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend(8000u32.to_le_bytes());
    for i in 0..4000 {
        bytes.extend((i as i16).to_le_bytes());
    }
    if text == "truncated" {
        bytes.truncate(100);
    }
    std::fs::write(path.join("output.wav"), &bytes).unwrap();
    if text == "duplicate" {
        std::fs::write(path.join("extra.wav"), &bytes).unwrap();
    }
    if text == "failed" {
        std::process::exit(2);
    }
}
