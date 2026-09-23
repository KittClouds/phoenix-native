use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{
    supertonic::{SupertonicBundle, SupertonicProvider, STYLES},
    Cancellation, Request,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn request(text: &str) -> Request<'_> {
    Request {
        epoch: 1,
        plan: [1; 32],
        segment: 0,
        text,
        instruction: "",
        seed: 0,
        max_frames: 24_000,
    }
}
fn bundle(root: &Path) -> SupertonicBundle {
    std::fs::create_dir_all(root.join("onnx")).unwrap();
    std::fs::create_dir_all(root.join("voice_styles")).unwrap();
    for name in [
        "duration_predictor.onnx",
        "text_encoder.onnx",
        "vector_estimator.onnx",
        "vocoder.onnx",
        "tts.json",
        "unicode_indexer.json",
    ] {
        std::fs::write(root.join("onnx").join(name), name).unwrap();
    }
    for style in STYLES {
        std::fs::write(
            root.join("voice_styles").join(format!("{style}.json")),
            style,
        )
        .unwrap();
    }
    SupertonicBundle::open(
        Path::new(env!("CARGO_BIN_EXE_supertonic-provider-fixture")),
        root,
        &Cancellation::default(),
    )
    .unwrap()
}
#[test]
fn only_complete_quiescent_output_publishes_and_style_changes_key() {
    let root = tempfile::tempdir().unwrap();
    let bundle = bundle(root.path());
    assert_ne!(
        bundle.identity("F1", 24_000).unwrap(),
        bundle.identity("M1", 24_000).unwrap()
    );
    let identity = bundle.identity("F1", 24_000).unwrap();
    let mut provider = SupertonicProvider::new(bundle, root.path().join("jobs")).unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    for text in ["failed", "truncated", "duplicate"] {
        assert!(provider
            .generate(
                request(text),
                "F1",
                &mut cache,
                &Cancellation::default(),
                |_| Ok(())
            )
            .is_err());
        assert!(!cache.contains(identity.audio_key(text).unwrap()));
    }
    let key = provider
        .generate(
            request("valid"),
            "F1",
            &mut cache,
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(cache.get(key).unwrap().manifest().frames, 4000);
    provider
        .generate(
            request("valid"),
            "F1",
            &mut cache,
            &Cancellation::default(),
            |_| panic!("cache hit synthesized"),
        )
        .unwrap();
}
#[test]
fn cancellation_at_final_pcm_block_and_sink_failure_never_commit() {
    let root = tempfile::tempdir().unwrap();
    let bundle = bundle(root.path());
    let identity = bundle.identity("F1", 24_000).unwrap();
    let mut provider = SupertonicProvider::new(bundle, root.path().join("jobs")).unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let cancel = Cancellation::default();
    assert!(provider
        .generate(request("cancel-last"), "F1", &mut cache, &cancel, |chunk| {
            if chunk.first_frame > 0 {
                cancel.cancel();
            }
            Ok(())
        })
        .is_err());
    assert!(!cache.contains(identity.audio_key("cancel-last").unwrap()));
    assert!(provider
        .generate(
            request("sink-failure"),
            "F1",
            &mut cache,
            &Cancellation::default(),
            |_| Err(phoenix_tts_native::Error::Invalid("sink failed"))
        )
        .is_err());
    assert!(!cache.contains(identity.audio_key("sink-failure").unwrap()));
    drop(cache);
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    assert_eq!(cache.bytes(), 0);
    provider
        .generate(
            request("recovered"),
            "F1",
            &mut cache,
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
}
#[test]
fn cancel_kills_owned_process_and_allows_recovery() {
    let root = tempfile::tempdir().unwrap();
    let bundle = bundle(root.path());
    let mut provider = SupertonicProvider::new(bundle, root.path().join("jobs")).unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let cancel = Cancellation::default();
    let token = cancel.clone();
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        token.cancel();
    });
    let start = Instant::now();
    assert!(provider
        .generate(request("slow"), "F1", &mut cache, &cancel, |_| Ok(()))
        .is_err());
    trigger.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(cache.bytes(), 0);
    provider
        .generate(
            request("recovered"),
            "F1",
            &mut cache,
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
}
