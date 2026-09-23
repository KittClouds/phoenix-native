#[path = "../../../apps/phoenix-shell-proof/src/shell/reader/worker.rs"]
#[allow(dead_code)]
mod worker;
use phoenix_reader_session::*;
use phoenix_tts_native::Bundle;
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::{path::Path, sync::Arc};
use worker::voices;

#[test]
fn shell_cast_persists_and_changes_only_assigned_passage() {
    let root = tempfile::tempdir().unwrap();
    let source = "First passage.\n\nSecond passage.";
    let lease = DocumentLease {
        entry_id: EntryId(1),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(source.as_bytes()),
        content: Arc::from(source),
    };
    let plan = plan_markdown([1; 32], &lease, Default::default())
        .unwrap()
        .plan;
    assert_eq!(plan.spec().segments.len(), 2);
    std::fs::write(root.path().join("model"), b"fake model").unwrap();
    std::fs::create_dir(root.path().join("dll")).unwrap();
    let bundle = Bundle::open(
        Path::new(env!("CARGO_BIN_EXE_native-provider-fixture")),
        &root.path().join("model"),
        &root.path().join("dll"),
    )
    .unwrap();
    let first = voices::VoiceSpec::default();
    let bundle = worker::engines::Bundles {
        breeze: Some(bundle),
        cpu: None,
    };
    let mut second = first.clone();
    second.profile.id = [44; 32];
    second.profile.name = "Character".into();
    second.profile.description = "A firm English speaker.".into();
    let narrator = VoiceChoice::of(&first.profile).unwrap();
    let character = VoiceChoice::of(&second.profile).unwrap();
    let mut specs = vec![first, second];
    voices::load_library(root.path(), &mut specs).unwrap();
    let before = voices::prepare(&specs, Some(narrator), None, source, &plan, &bundle).unwrap();
    voices::assign_passage(
        root.path(),
        None,
        source,
        &plan,
        1,
        "Kai",
        character,
        narrator,
    )
    .unwrap();
    let library = VoiceLibrary::open(root.path().join("voices")).unwrap();
    let cast = library.load_cast(source, &plan).unwrap().unwrap();
    drop(library);
    let after =
        voices::prepare(&specs, Some(narrator), Some(&cast), source, &plan, &bundle).unwrap();
    assert_eq!(
        before.table.identity(0).unwrap(),
        after.table.identity(0).unwrap()
    );
    assert_ne!(
        before.table.identity(1).unwrap(),
        after.table.identity(1).unwrap()
    );
    assert_eq!(after.voices[usize::from(after.slots[1])].name, "Character");
    specs.pop();
    assert!(voices::prepare(&specs, Some(narrator), Some(&cast), source, &plan, &bundle).is_err());
}

#[test]
fn cpu_only_book_enrolls_without_any_breeze_paths() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("onnx")).unwrap();
    std::fs::create_dir_all(root.path().join("voice_styles")).unwrap();
    for name in [
        "duration_predictor.onnx",
        "text_encoder.onnx",
        "vector_estimator.onnx",
        "vocoder.onnx",
        "tts.json",
        "unicode_indexer.json",
    ] {
        std::fs::write(root.path().join("onnx").join(name), name).unwrap();
    }
    for style in phoenix_tts_native::supertonic::STYLES {
        std::fs::write(
            root.path()
                .join("voice_styles")
                .join(format!("{style}.json")),
            style,
        )
        .unwrap();
    }
    let mut config: worker::Config = serde_json::from_value(serde_json::json!({
        "storage": root.path().join("reader"),
        "supertonic": {"runner": env!("CARGO_BIN_EXE_supertonic-provider-fixture"), "models": root.path()}
    })).unwrap();
    config.load_voices().unwrap();
    let source = "A CPU-only book.";
    let lease = DocumentLease {
        entry_id: EntryId(1),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(source.as_bytes()),
        content: Arc::from(source),
    };
    let plan = plan_markdown([2; 32], &lease, Default::default())
        .unwrap()
        .plan;
    let narrator = VoiceChoice::of(
        &config
            .voices
            .iter()
            .find(|v| v.supertonic_style.as_deref() == Some("F1"))
            .unwrap()
            .profile,
    )
    .unwrap();
    let bundles =
        worker::engines::Bundles::for_plan(&config, &plan, Some(narrator), &Default::default())
            .unwrap();
    assert!(bundles.breeze.is_none());
    assert!(bundles.cpu.is_some());
    let prepared = voices::prepare(
        &config.voices,
        Some(narrator),
        None,
        source,
        &plan,
        &bundles,
    )
    .unwrap();
    assert_eq!(prepared.voices[0].supertonic_style.as_deref(), Some("F1"));
    assert_eq!(
        prepared.table.identity(0).unwrap().provider,
        *blake3::hash(b"phoenix.supertonic-cpu/v1").as_bytes()
    );
}
