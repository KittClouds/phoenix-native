use phoenix_reader_session::AudioCache;
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request};
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
        instruction: "Clear",
        seed: 42,
        max_frames: 24000,
    }
}
fn setup(root: &Path) -> NativeProvider {
    let model = root.join("model");
    std::fs::write(&model, b"fixture model").unwrap();
    let dll = root.join("dll");
    std::fs::create_dir(&dll).unwrap();
    let bundle = Bundle::open(
        Path::new(env!("CARGO_BIN_EXE_native-provider-fixture")),
        &model,
        &dll,
    )
    .unwrap();
    NativeProvider::new(bundle, Duration::from_secs(5), Duration::from_secs(2)).unwrap()
}
#[test]
fn valid_cache_hit_and_resident_worker() {
    let root = tempfile::tempdir().unwrap();
    let mut provider = setup(root.path());
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let cancel = Cancellation::default();
    let key = provider
        .generate(request("valid"), &mut cache, &cancel)
        .unwrap();
    assert_eq!(cache.get(key).unwrap().pcm(), [1, 0, 2, 0, 3, 0, 4, 0]);
    let pid = provider.pid();
    assert_eq!(
        provider
            .generate(request("valid"), &mut cache, &cancel)
            .unwrap(),
        key
    );
    provider
        .generate(request("second"), &mut cache, &cancel)
        .unwrap();
    assert_eq!(provider.pid(), pid);
    provider.stop().unwrap();
    assert_eq!(provider.pid(), None);
}
#[test]
fn faults_never_publish_and_next_request_restarts() {
    let root = tempfile::tempdir().unwrap();
    let mut provider = setup(root.path());
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    for mode in [
        "reject", "crash", "stale", "oversize", "partial", "eof", "limit", "count", "late",
    ] {
        let key = provider
            .bundle()
            .identity("Clear", 42, 24000)
            .unwrap()
            .audio_key(mode)
            .unwrap();
        assert!(
            provider
                .generate(request(mode), &mut cache, &Cancellation::default())
                .is_err(),
            "{mode}"
        );
        assert!(!cache.contains(key), "{mode}");
        assert_eq!(provider.pid(), None);
        assert!(!std::fs::read_dir(root.path().join("cache"))
            .unwrap()
            .any(|x| x
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".writing")));
    }
    provider
        .generate(request("recovered"), &mut cache, &Cancellation::default())
        .unwrap();
}
#[test]
fn cancel_during_prefill_or_audio_and_timeout_kills_owned_child() {
    let root = tempfile::tempdir().unwrap();
    let mut provider = setup(root.path());
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    for text in ["prefill", "stall"] {
        let cancel = Cancellation::default();
        let trigger = cancel.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.cancel();
            Instant::now()
        });
        assert!(provider
            .generate(request(text), &mut cache, &cancel)
            .is_err());
        let fired = handle.join().unwrap();
        assert!(fired.elapsed() < Duration::from_millis(250));
        assert_eq!(provider.pid(), None);
        assert_eq!(cache.bytes(), 0);
    }
    assert!(provider
        .generate(request("stall"), &mut cache, &Cancellation::default())
        .is_err());
    assert_eq!(provider.pid(), None);
    assert_eq!(cache.bytes(), 0);
    let cancel = Cancellation::default();
    let trigger = cancel.clone();
    assert!(provider
        .generate_streamed(request("valid"), &mut cache, &cancel, move |_| {
            trigger.cancel();
            Ok(())
        })
        .is_err());
    assert_eq!(cache.bytes(), 0);
}

#[test]
fn cancelled_startup_reaps_child_and_pinned_bundle_denies_writes() {
    let root = tempfile::tempdir().unwrap();
    let model = root.path().join("model");
    std::fs::write(&model, b"startup stall").unwrap();
    let dll = root.path().join("dll");
    std::fs::create_dir(&dll).unwrap();
    let bundle = Bundle::open(
        Path::new(env!("CARGO_BIN_EXE_native-provider-fixture")),
        &model,
        &dll,
    )
    .unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&model)
        .is_err());
    let mut provider =
        NativeProvider::new(bundle, Duration::from_secs(5), Duration::from_secs(2)).unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let cancel = Cancellation::default();
    let trigger = cancel.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });
    assert!(matches!(
        provider.generate(request("valid"), &mut cache, &cancel),
        Err(phoenix_tts_native::Error::Cancelled)
    ));
    handle.join().unwrap();
    assert_eq!(provider.pid(), None);
    assert_eq!(cache.bytes(), 0);
}

#[test]
fn sink_failure_and_stale_epoch_cannot_publish() {
    let root = tempfile::tempdir().unwrap();
    let mut provider = setup(root.path());
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let mut r = request("valid");
    r.epoch = 2;
    assert!(provider
        .generate_streamed(r, &mut cache, &Cancellation::default(), |chunk| {
            assert_eq!(chunk.binding.epoch, 2);
            assert_eq!(chunk.first_frame, 0);
            assert_eq!(chunk.pcm.len(), 8);
            Err(phoenix_tts_native::Error::Invalid("playback unavailable"))
        })
        .is_err());
    assert_eq!(provider.pid(), None);
    assert_eq!(cache.bytes(), 0);
    assert!(provider
        .generate(request("valid"), &mut cache, &Cancellation::default())
        .is_err());
    drop(cache);
    let reopened = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    assert_eq!(reopened.bytes(), 0);
}

#[test]
fn explicit_voice_has_distinct_cache_identity_and_faults_never_publish() {
    let root = tempfile::tempdir().unwrap();
    let mut provider = setup(root.path());
    let mut bytes = b"BRZV".to_vec();
    for n in [1u32, 24000, 16, 1, 2] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(b"Hi");
    bytes.extend([0; 64]);
    let path = root.path().join("voice.breeze");
    std::fs::write(&path, &bytes).unwrap();
    let base = provider.bundle().identity("Clear", 42, 24000).unwrap();
    let voice = phoenix_tts_native::VoiceAsset::open(
        &path,
        *blake3::hash(&bytes).as_bytes(),
        base.model,
        base.codec,
    )
    .unwrap();
    let mut cache = AudioCache::open(root.path().join("cache"), 4 * 1024 * 1024).unwrap();
    let key = provider
        .generate_voiced_streamed(
            request("valid"),
            Some(&voice),
            &mut cache,
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
    assert_ne!(key, base.audio_key("valid").unwrap());
    assert_eq!(
        cache.get(key).unwrap().manifest().identity.voice,
        voice.hash()
    );
    for mode in ["partial", "limit", "late", "eof"] {
        let identity = voice
            .identity(provider.bundle(), "Clear", 42, 24000)
            .unwrap();
        let key = identity.audio_key(mode).unwrap();
        assert!(provider
            .generate_voiced_streamed(
                request(mode),
                Some(&voice),
                &mut cache,
                &Cancellation::default(),
                |_| Ok(())
            )
            .is_err());
        assert!(!cache.contains(key));
    }
    let wrong = phoenix_tts_native::VoiceAsset::open(
        &path,
        *blake3::hash(&bytes).as_bytes(),
        [99; 32],
        base.codec,
    )
    .unwrap();
    assert!(wrong
        .identity(provider.bundle(), "Clear", 42, 24000)
        .is_err());
}
