# Reader voice workflow and CPU provider

## Product behavior

- Voices are grouped into Breeze designs/references and Supertonic CPU styles.
- Preview sample plays a fixed short passage through an isolated sample cache.
  It pauses book playback and does not select a voice, move the book position,
  or highlight unrelated document text. Stop sample cancels generation/playback.
- Use voice & listen persists the book narrator, retires the previous controller,
  and begins listening. Existing character assignments remain independent.
- Create a voice is a dedicated form with visible name/description labels and
  visible validation errors. It creates a Breeze description profile. It does
  not promise a fixed speaker identity or enroll a reference recording.
- Cast current passage pauses playback and captures the current source range.
  Casting remains an explicit whole-passage assignment, not automatic dialogue
  attribution. Starting playback is offered when there is no passage to cast.
- Reader inputs use the measured sidebar width. Percentage/flex-only sizing in
  the GPUI input/scroll hierarchy collapsed after layout changes in live checks.

## Installed voice inventory

Breeze has Calm narrator and Storyteller QA as saved voice designs. The live
acceptance pass added Evening storyteller through the creation form.
User-created profiles extend that list. These labels are Phoenix profiles, not
an upstream named-speaker catalogue. The existing experimental reference asset
is not automatically enrolled into this book's voice library.

Supertonic provides the installed F1–F5 and M1–M5 style files. The UI keeps those
identifiers rather than inventing names or unverified accent descriptions.

## CPU provider contract

The native Reader can select Supertonic without loading Breeze for a session
whose narrator and character assignments all use CPU voices. Mixed assignments
enroll both required engines. No WSL or GPU flag is used by the CPU adapter.

The optional workspace `.reader.json` configuration is:

```json
{
  "storage": "D:/phoenix-tts/my-reader",
  "supertonic": {
    "runner": "C:/phoenix-tts/supertonic-rust/example_onnx.exe",
    "models": "C:/phoenix-tts/supertonic-3"
  }
}
```

Breeze worker/model/DLL paths remain necessary when any selected passage uses
Breeze. This integrates the already-installed external Rust CLI; it does not
vendor its inference source or create a redistributable installer.

- Retained read-only handles pin executable, adjacent DLLs, model/config/tokenizer
  files and all ten style files. Their hashes determine cache identity.
- English, five denoising steps and speed 1.05 are explicit identity inputs.
  The upstream CLI offers no seed control or direction/reference API.
- The runner has a 120-second deadline, cancellation polling and owned-process
  termination. Unconfirmed termination disables further generation on that owner.
- Each request writes to a new private UUID directory. Exit must succeed and
  produce exactly one structurally complete supported WAV. Partial, duplicate,
  missing, oversized and failed outputs cannot publish.
- Mono PCM16 at 44.1 kHz is converted to 24 kHz using a normalized 64-tap
  Blackman-windowed sinc filter with 80 precomputed rational phases. Mono PCM16
  at 24 kHz passes through. Other WAV formats fail explicitly.
- A mapped WAV feeds bounded 2,048-frame blocks to the existing stream validator
  and cache writer. Cancellation and sink errors abort publication, including
  cancellation during the final block. Cache completion follows validated process
  exit, validated frame totals, and the final cancellation check.
- The CLI is not a streaming inference API. Audio becomes available after that
  segment's process finishes. Continuous playback uses the existing prefetcher.
- Alignment remains segment-level. Transport completion does not prove transcript
  accuracy, sentence completeness, voice quality or lower-end-PC performance.

## Verification record

- Initial live build CE27093F reproduced collapsed inputs despite relative-width
  changes. It is not the final UI acceptance artifact.
- Real CLI probing identified upstream failure on Windows verbatim paths with
  mixed separators. Ordinary equivalent absolute paths fix that boundary.
- Contract tests cover malformed/duplicate/truncated output, process failure,
  cancellation during generation and the final block, sink failure, cache reopen,
  recovery, distinct style identities, DC preservation, block continuity and alias
  rejection. The verification script compiles on D: and runs via the C: junction.
- Real all-style smoke: `supertonic_smoke RUNNER MODEL_ROOT OUTPUT_ROOT`, producing
  per-style generation duration, audio duration, RTF and validated cache-hit timing.

## September 14 acceptance evidence

- Final preview executable: `C:/phoenix-bin/reader-shell-preview-20260906/debug/phoenix-shell.exe`
  via the D: target junction; PID 29820 at launch; SHA-256
  `6AC3540856263A1C72C4E129E0979812B8EA3973BD60A53A0A7D736FE01674EF`.
- Receipt: `D:/phoenix-tts/reader-voices-ui-20260906/voice-ux-final-20260914.json`.
- The measured-width form was visibly usable at normal and maximized window
  sizes in the preceding layout preview; empty submission showed a visible error.
- Final build displayed CPU styles, prepared and played the F1 sample, then
  reported sample completion while retaining Calm narrator as the book choice.
- User confirmed the F1 sample: “Yes, I heard it clearly.” This is direct audible
  confirmation, not long-form voice-quality qualification.
- The real all-style smoke passed. Generation plus validation/resampling took
  2.58–3.77 seconds for 5.31–6.68 seconds of audio (RTF 0.41–0.59). Validated cache
  hits took 0.59–0.81 ms. Debug build on this host; no lower-end-device claim.
  Receipt: `D:/phoenix-tts/supertonic-native-smoke-20260914-02/receipt.json`.
- The CPU-only enrollment test passed with no Breeze paths configured. The three
  adversarial CPU provider tests passed again through the C: junction, including
  the upstream path-join regression. Native-provider Clippy passed with warnings
  denied. Full Reader contract verification passed before these additional tests.

- Final build selected F1 with one click and played the saved book; chapter
  progression reached chapter 2 / passage 7. Pause changed to Resume and removed
  the active narration paint. The existing passage-specific Calm narrator casting
  remained in effect rather than being overwritten by the CPU narrator selection.
- Final form accepted the name Evening storyteller and a complete descriptive
  prompt, saved the profile, closed the form and displayed the selected voice card.

- Evening storyteller generated a sample, displayed Playing sample and then
  Sample finished. This verifies the workflow; no listener quality rating was
  collected for that new Breeze profile.
- Stopping the uncached Storyteller QA preview during Preparing showed Stopping
  sample, then Sample stopped. The sample cache retained its three prior complete
  entries (F1, Evening storyteller and Calm narrator); no fourth complete entry
  appeared. The selected book voice remained Evening storyteller.
- The final executable closed gracefully and restarted with the same SHA-256,
  PID 40436. Reopening Reader and Voices displayed Evening storyteller, its full
  description and Selected for this book. Playback did not start automatically.
  Restart receipt:
  `D:/phoenix-tts/reader-voices-ui-20260906/voice-ux-restart-20260914.json`.

This completes the focused voice-workflow acceptance pass. Lower-end-device
performance, full novel quality, word-level alignment and packaged-release
qualification remain separate gates.
