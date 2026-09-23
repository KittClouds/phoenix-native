//! Exercises the exact shell controller without GPUI; default-device audible smoke.
#[path = "../../../apps/phoenix-shell-proof/src/shell/reader/worker.rs"]
#[allow(dead_code)] // UI-only casting actions are covered by cast contract tests.
mod worker;
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use worker::{Command, Status};
fn wait(b: &worker::Bridge, predicate: impl Fn(&Status) -> bool) -> Status {
    // Cold multi-gigabyte file verification can dominate on the model drive;
    // this harness deadline does not change provider inference/cancel deadlines.
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let s = b.status.lock().unwrap().clone();
        if s.finished {
            panic!("{}", s.message);
        }
        if predicate(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "timeout: {}", s.message);
        thread::sleep(Duration::from_millis(20));
    }
}
fn main() -> anyhow::Result<()> {
    let root = std::env::args().nth(1).expect("fresh output directory");
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir(&root)?;
    let workspace = root.join("workspace.json");
    std::fs::write(&workspace, b"{}")?;
    std::fs::write(
        workspace.with_extension("reader.json"),
        serde_json::to_vec(&serde_json::json!({
            "worker":"C:/phoenix-bin/breeze-native-20260906/build/phoenix-breeze-worker.exe",
            "model":"D:/phoenix-tts/breeze-native-20260906/breeze-tts-2-q8_0.gguf",
            "dll_directory":"C:/phoenix-bin/breeze-native-20260906/build/bin",
            "storage":root.join("reader")
        }))?,
    )?;
    let text="# First\n\nThe room was quiet. Outside the window, rain fell softly on the garden. She placed the old book on the table and listened to the sounds of the sleeping house. Beyond the gate, a narrow path curved between the trees and disappeared into the mist. Somewhere in the distance, a bell began to ring, slow and steady in the morning air.\n\n# Second\n\nAt last, the sun appeared above the trees. He opened the door and stepped outside, carrying a small wooden box beneath his arm. The road was empty, but fresh footprints led toward the bridge at the edge of the village. They would have to reach the station before noon if they wanted to catch the last train. For a moment they stood in silence, watching the river move beneath the weathered stones.";
    let lease = Arc::new(DocumentLease {
        entry_id: EntryId(1),
        revision: DocumentRevision(1),
        content_hash: ContentHash::of(text.as_bytes()),
        content: Arc::from(text),
    });
    let b = worker::start(workspace.clone(), lease.clone(), false);
    wait(&b, |s| s.segments > 0);
    b.send(Command::Play);
    wait(&b, |s| s.segment == 2 && s.seconds >= 1);
    b.send(Command::Pause);
    let paused = wait(&b, |s| s.message.starts_with("Paused"));
    assert!(!paused.requested);
    assert!(paused.buffered_seconds > 0);
    assert_eq!(paused.rebufferings, 0);
    assert!(!paused.source_ranges.is_empty());
    assert!(!paused.voice_name.is_empty());
    assert_eq!(paused.chapters, 2);
    b.send(Command::Bookmark);
    thread::sleep(Duration::from_millis(250));
    assert_eq!(b.status.lock().unwrap().seconds, paused.seconds);
    b.shutdown();
    let b = worker::start(workspace, lease, false);
    let restored = wait(&b, |s| s.segments > 0);
    assert_eq!(restored.segment, paused.segment);
    assert_eq!(restored.seconds, paused.seconds);
    b.send(Command::Next);
    wait(&b, |s| s.chapter == 1);
    b.send(Command::ReturnBookmark);
    wait(&b, |s| {
        s.segment == paused.segment && s.seconds == paused.seconds
    });
    b.send(Command::Previous);
    wait(&b, |s| s.segment == 0);
    b.send(Command::Play);
    let completed = wait(&b, |s| s.message.starts_with("Completed"));
    assert_eq!(completed.rebufferings, 0);
    assert_eq!(completed.device_starvations, 0);
    assert!(completed.generated_during_playback > 0);
    b.send(Command::Stop);
    b.shutdown();
    std::fs::write(
        root.join("passed.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "pause":true,"bookmark":true,"restart_resume":true,"chapters":true,"shutdown":true,
            "rebufferings":completed.rebufferings,"device_starvations":completed.device_starvations,
            "generated_during_playback":completed.generated_during_playback
        }))?,
    )?;
    println!("SHELL_READER_CONTROLLER_SMOKE_PASSED");
    Ok(())
}
