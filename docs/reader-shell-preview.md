# Reader shell preview slice

The native shell now has a READER footer entry. It opens a dedicated center view
while preserving editor/graph state. The source is the editor's saved document
lease, not an uncommitted text buffer. The view explicitly labels Breeze as a
preview with incomplete content qualification.

## Implemented behavior

- Load saved Markdown using the existing H1/H2 chapter planner, or load plain
  text as one chapter using explicit whitespace-omission mappings.
- Play/resume and pause through the real WaveOutput device and ReaderRuntime.
- Stop cancels owned generation independently of the bounded command queue.
- Previous/next chapter, one replaceable bookmark and return-to-bookmark.
- Display the active segment against its frozen revision. The segment background
  highlights during playback; no word/sentence timing is inferred.
- Checkpoint presented position at the existing one-second throttle; force on
  pause, bookmark, generation transition, stop and orderly app shutdown.
- Reopening the same plan and synthesis identity restores the durable position.
  Different revisions or model/runtime settings produce a different session.
- Generate a cache miss with the supervised provider, then attach only verified
  completed audio. No implicit promotion of provider output to semantic quality.

The control thread owns the session store and audio device. A separate single
generation thread owns the provider and mutable cache. It accepts one job and
returns one verified mmap lease through bounded channels; no PCM crosses them.
The UI sends commands through a 16-slot channel and reads a compact snapshot at
10 Hz. Model hashing, inference, disk writes and device submissions do not run on
the UI thread. Application quit cancels and joins the owned controller. A final
position is saved after pausing/resetting the device.

Continuous prefetch holds at most eight future completed segments (each at most
60 seconds), with one generation job in flight. A measured generation/audio-time
ratio sets the desired buffer between 8 and 30 seconds. Playback initially waits
for that reserve, the segment bound, or the end of the document. Long segments
can overshoot the duration target, but cannot escape the segment/frame bounds.
The device queues up to eight 2048-frame blocks across segment boundaries without
resetting. Highlighting and saved position advance when the sample clock crosses
those boundaries. Seek clears queued audio, cancels obsolete generation and
rejects stale completion epochs. Pause remains responsive during generation.
The UI reports rebuffering and observed within-segment device queue exhaustion.

## Local preview configuration

For a workspace `workspace-v1.json`, use sibling `workspace-v1.reader.json`:

```json
{
  "worker": "C:/phoenix-bin/breeze-native-20260906/build/phoenix-breeze-worker.exe",
  "model": "D:/phoenix-tts/breeze-native-20260906/breeze-tts-2-q8_0.gguf",
  "dll_directory": "C:/phoenix-bin/breeze-native-20260906/build/bin",
  "storage": "D:/phoenix-tts/phoenix-reader-preview"
}
```

The default local workspace configuration was created if absent. Existing files
were preserved. Models, snapshots, cache and sessions stay on D:. A second app
opening the same store is rejected by the existing exclusive store leases.
Configuration failures appear in the Reader view.

## Limits and next checks

This is a completed-segment playback slice: no uncommitted streaming-to-device
integration, ±15-second control, speed DSP, bookmark list, library/import dialog,
voice studio or full-document scrolling/highlighting yet. Prefetch removes
generation waits when the reserve keeps up; it cannot guarantee continuity if
generation stays slower than playback or the device/control thread stalls.
Initial buffering and uncached seeking still take time. Plain-text mode is a single chapter; use Markdown
headings for chapter navigation. Return to editor keeps playback alive; Stop ends
the session controller. Reload the saved document to start a new controller.

The exact controller is exercised by
`phoenix-tts-native/examples/reader_controller_smoke.rs`: real generation/device,
pause, saved position, bookmark, restart, chapter selection, return bookmark and
shutdown. This does not establish live GPUI control or rendered-layout proof.
The final app freeze and packaged-release qualification remain separate gates.

## Verification receipt (2026-09-06)

The native shell development build completed on D: and is accessible through
`C:/phoenix-bin/reader-shell-preview-20260906/debug/phoenix-shell.exe`.
Its SHA-256 and source HEAD are recorded in
`D:/phoenix-tts/reader-shell-preview-build.json`; the checkout contains uncommitted
work, and this build directory is mutable. This is not a sealed release.

The exact controller smoke passed at
`D:/phoenix-tts/reader-controller-smoke-03/passed.json`, exercising real Breeze
generation and the default audio device, pause, bookmark, restart/resume, chapter
navigation and shutdown. It exposed and fixed an unchanged restored checkpoint
being incorrectly republished; a regression test now covers that condition.
The controller smoke does not click or inspect GPUI widgets.

The focused formatting, Clippy and contract test script passed on the final
rerun, including cancellation timing and interrupted-generation nonpublication.
Receipt: `D:/phoenix-target-reader-contract/reader-contract-proof.json`;
log: `D:/phoenix-tts/reader-shell-contract-tests-final.log`.
One earlier cancellation timing assertion failed while the shell was building;
the passing rerun does not establish latency under concurrent build load.
