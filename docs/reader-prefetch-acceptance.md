# Reader prefetch acceptance, 2026-09-06

Scope: native Reader preview, bounded background generation, continuous completed
audio queue, and a disposable live UI workspace. This does not qualify novel
content quality, the graph/extraction system, or the final packaged release.

## Software checks

Focused formatting, Clippy and all 39 contract tests passed:
`D:/phoenix-tts/prefetch-contracts-final.log`.
The new playback tests establish continuous cross-segment submission without
device reset, presentation-clock highlighting, a hard eight-segment prefetch
bound, and clearing/rejecting stale prefetch after seek. Existing provider tests
still reject failed/interrupted streams for completed-cache publication.

## Real-device experiment

The extended controller smoke uses a fresh cache and two chapters. It checks
pause, bookmark, durable restart, chapter selection, full playback completion,
zero rebufferings, zero observed within-segment device queue exhaustions, and
at least one newly generated segment completing during playback.

Attempt 01 exceeded the harness's old 180-second allowance while verifying model
files, before any inference. Attempt 02 was stopped by the assistant during the
same verification phase after D: showed heavy build/read contention. Neither is
a playback result. The harness now allows 600 seconds; provider deadlines and
cancellation tests were not relaxed. Build and inference acceptance run serially.

## Live UI acceptance procedure

Use `D:/phoenix-tts/reader-ui-prefetch-20260906/workspace.json`, with its sibling
Reader configuration. Leave the existing Phoenix process/workspace alone.
Identify the fresh executable and PID, then use native Computer Use to:

1. Open a saved two-chapter note in Reader.
2. Start playback and inspect buffering, highlight and advancing position.
3. Pause, set a bookmark, navigate chapters, and return to the bookmark.
4. Close and reopen the same executable/workspace and inspect restored position.
5. Confirm completed-cache replay and the visible continuity counters.

Actual observations and executable identity belong below once performed.

Native Computer Use observed the fresh preview (PID 46456, SHA-256
`8A9ABF439BB818DD76DAC78E7A207CAE336C4DEE5AF83D59605D4D7B95D589BC`),
created/saved the two-chapter note through the editor, loaded revision 1,
started uncached playback, observed the green active segment and advancing
position, paused at chapter 2 / segment 8 / four seconds, set a bookmark,
navigated to chapter 1, and returned to that exact displayed bookmark position.
Visible counters remained zero rebufferings and zero device gaps, with one
segment generated during playback. The unsaved revision-zero document was
correctly rejected before saving. Original Phoenix PID 26104 was left untouched.

Restarted the same executable/workspace with producer and both model roots
(PID 23548; `restart-identity.json` alongside `build.json` in the UI evidence
directory). Atlas now offered Warm Models, although its badge still read
Unsupported before warm-up. No model warm-up or graph creation was qualified.
An earlier restart naming a nonexistent explicit publication root exited before
opening a window; the successful restart uses the workspace's normal publication
location. This startup edge also remains relevant to packaging.

The restarted UI restored chapter 2 / segment 8 / four seconds. Clicking Play
resumed the cached tail to Completed (six seconds), with zero generated segments,
zero rebufferings, zero device gaps, and no Breeze worker process. This establishes
live saved-position restore and cache replay for the short acceptance note.
It does not establish a 60-minute or two-hour soak, narrow-window/DPI acceptance,
word-level alignment, or acoustic equivalence across the full novel suite.

The serial real-device run passed:
`D:/phoenix-tts/reader-prefetch-smoke-03/passed.json`: zero rebufferings, zero
observed within-segment device queue exhaustions, five segments generated during
playback, plus pause/bookmark/chapter/restart/shutdown assertions.

## Release blocker: graph backend launch configuration

The Reader-only preview was launched with `--design-preview --workspace ...`
and Atlas displayed Unsupported. This is not a complete product candidate.
The still-running qualified app supplies these existing, verified-to-exist paths:

- Producer: `C:/phoenix-bin/atlas-runtime-qualified-cbf349be-20260813/phoenix-analysis-bridge.exe`
- NER: `D:/hf-models/gliner-bi-base-v2.0-onnx`
- NLI: `D:/phoenix-models/modernbert-base-nli-onnx`

The shell requires `--producer`, `--ner-model-root`, and `--nli-model-root`
together. A final launcher must carry the qualified runtime and model identities
without requiring manual flags; graph creation must pass in the same packaged
application as Reader. Path existence is not runtime compatibility or graph
qualification. These existing roots do not establish GLiNER 2.5 promotion.
