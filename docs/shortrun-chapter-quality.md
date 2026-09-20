# Shortrun Chapter 1 quality qualification

User-selected corpus: one chapter from `docs/shortrun.md`; Chapter 1, Quicksave.
This qualification keeps the current stateless supervised provider unchanged.
It does not promote a model, activate Reader UI, or assert subscription parity.

## Frozen run

- Output: `D:\phoenix-tts\breeze-native-20260906\shortrun-chapter1-quality-01`.
- Harness: `crates/phoenix-tts-native/examples/chapter_quality.rs`.
- Build target: `D:\phoenix-target-reader-contract`; executable launched through
  `C:\phoenix-bin\reader-contracts-proof-20260904`.
- Current native worker, model revision and artifact SHA-256 values are recorded
  in `run-authority.json`. `plan.json` contains full-source BLAKE3, exact chapter
  extent, chapter revision digest, copy mapping and paragraph ranges.
- Chapter selection ends immediately before the line beginning `Chapter 2:`.
  Source bytes are copied unchanged. Indented prose is treated as plain text,
  avoiding Markdown code-block omission. There are 102 nonempty paragraphs.
- This is a qualification-only paragraph plan, not the production Markdown
  sentence planner. Paragraph ordinals are not claims of sentence alignment.
- Voice instruction: `A calm, clear English narrator.` Seed 42, CFG 1,
  output cap 60 seconds per request; no reference clip or hidden voice anchor.
- Every segment must obtain supervised EOS, exact frame total and quiescence
  before its cache entry is accepted. Only validated cache PCM enters the WAV.
  No normalization, inserted silence, crossfades or speed changes are applied.
- A failed render retains diagnostics and a `chapter.partial.wav`; only an
  all-segment success publishes `chapter.wav` and `completed.json`.

## Measurements and gates

All segments contribute to receipts; none are discarded for bad performance.
The first request is reported separately as startup. Warm percentiles use all
remaining non-cache-hit requests and the nearest-rank rule.

| Area | Gate or disposition |
| --- | --- |
| Render coverage | All planned paragraph ranges contiguous; all normal completion |
| First PCM | Report p50/p95; these are not perceptual TTFA |
| Generation speed | Warm p95 RTF <=0.8 |
| Transcript | Desired adjudicated TTS WER <=0.5%; ASR alone cannot award this |
| Missing/repeated sentences | Must be zero after transcript/listening adjudication |
| Voice identity | Human judgment across paragraph boundaries and opening/middle/end |
| Audio signal | Report silent segments, clipping, RMS, onset/trailing threshold diagnostics |
| Memory | Sample worker/private bytes and total GPU; no two-hour slope claim |
| Playback underruns | Unmeasured: this is offline generation, not a device soak |
| Preference | Unmeasured: no randomized current-engine or ElevenReader comparison |

GPU telemetry begins after the first three segments and includes other apps.
ASR dependency/model preparation overlaps part of rendering, so this is a shared
desktop workload, not an isolated hardware benchmark. ASR inference runs after
the Breeze render finishes and uses CPU only.

## Transcript diagnostics

`scripts/chapter-quality-review.py` uses a separately installed, pinned
faster-whisper 1.2.1 environment on D:. Model repository revision and file hashes
are saved in `asr-model-pins.json`; packages are saved in `asr-packages.txt`.
The CPU int8 path follows the [upstream faster-whisper documentation](https://github.com/SYSTRAN/faster-whisper).
Local novel text and audio are not uploaded.

Each paragraph is recognized independently using small.en, beam size 5,
temperature 0, English, no reference prompt, hotwords, VAD or previous-transcript
conditioning. Comparison uses NFKC, case-folding, Unicode word tokens and exact
word Levenshtein distance. Numbers are not expanded and proper-name spellings
are not corrected. `asr-diagnostic.jsonl` preserves reference and hypothesis.
Disagreements identify listening review locations; they conflate ASR and TTS
errors and must not be presented as certified synthesis WER.

## Listening

The full chapter and opening/middle/ending clips are provided alongside a review
sheet. Rate clarity, naturalness, names, identity continuity and boundary pauses
from 1 to 5; annotate wrong, missing or repeated words with timestamps.
Completed transport plus plausible signal levels cannot establish these ratings.
Human listening remains pending until the user actually supplies judgments.

The generated `quality-summary.json` and `review.md` are the measured run result;
this document specifies methodology, not a quality-pass declaration.

## Outcome: content fidelity rejected

All 102 paragraphs completed, with 1038.72 seconds of audio and no silent
segments or clipped samples. Warm first PCM p50/p95 was 286/323 ms. Warm p95 RTF
was 0.8065 (target <=0.8; eight warm requests exceeded 0.8).

User listening found the opening very clear and no issues in middle/ending
spot checks. A targeted check at paragraph 77 (13:24.56) then confirmed extra
speech after the source ends at "Drop your weapons!". This is a content-fidelity
failure despite mechanically valid normal completion. Keep this candidate out
of Reader promotion until the failure is resolved and requalified.

ASR found 56 word edits over 2744 normalized reference tokens (2.04% diagnostic
disagreement). Many are spelling, homophones or numeric formatting; this is not
adjudicated TTS WER. Only the targeted added-speech failure is human-confirmed.
No claim of exhaustive transcript accuracy or whole-chapter listening is made.

The retained short-dialogue fixture freezes text, seed, instruction, output cap,
source/model revisions, source extent and evidence location. Next qualification
should replay this fixture and other short dialogue with the same worker, then
compare controlled voice/reference and segment-context changes. Do not trim the
observed extra speech out of this run or relabel its cache entry as qualified.
