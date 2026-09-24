# Reader voices and model lifecycle, 2026-09-24

Scope: Phoenix Native product branch and its copied review workspace. The QPS
science work and other agents' branches were not changed.

## Voice enrollment

Gilly, Alyssa, and Jack were prepared from the user-supplied WAV recordings as
24 kHz mono reference excerpts, then encoded with the locally pinned Breeze
model. `enroll_reader_voices` validates the bundle and encoded reference hashes,
atomically installs missing assets, and saves hash-bound voice profiles. It does
not alter the book's selected narrator; Woods remains selected.

The transcript text was produced and checked locally with speech recognition.
The user chose clean complete excerpts because parts of the recordings were
clipped. The exact words and punctuation have not been independently verified
against a source script, so reference fidelity remains a listening question.

| Profile | Reference length | Treatment |
|---|---:|---|
| Gilly | 10.12 s | Reference voice |
| Alyssa | 14.21 s | Reference voice, neutral delivery |
| Alyssa · expressive narrator | 14.21 s | Same Alyssa reference; enthusiastic, expressive delivery instruction |
| Jack | 16.52 s | Reference voice |

The Alyssa A/B samples use the same encoded reference, text, seed 42, and
`cfg-scale=1`. Only the delivery instruction differs. This is a listening
experiment, not an acoustic qualification or Reader freeze verdict.
Review files: `D:\phoenix-reader-closeout-review-20260923\voices-final-20260924\alyssa-neutral-sample.wav`
and `D:\phoenix-reader-closeout-review-20260923\voices-final-20260924\alyssa-expressive-sample.wav`.

## GPU ownership

The Reader generator owns at most one Breeze worker and one generation job. A
paused or completed session keeps the worker warm for a short return, then
stops and reaps it after 20 seconds without a completed request. A subsequent
cache miss starts it from the same hash-pinned bundle. Cache hits and queued
PCM remain available without loading the model. An isolated voice preview stops
its Breeze worker as soon as sample PCM is cached, before device playback.

Supertonic uses a child process per synthesis request, which exits after each
job; there is no resident Supertonic model process between passages. Stopping a
Reader session drops the provider and reaps its owned Breeze worker.

The idle clock starts after the current generation finishes. A long in-flight
generation can therefore extend the release time. The policy avoids aborting
useful prefetch on a brief pause.

## Live check in the copied review workspace

The release executable was built on D: and copied to the versioned C: test
location. Its external SHA-256 is
`E93C2A302D3F9DE96025B9A0F64014C88EDE283E8954AA75AF2572FD37EF42E0`.
The same launch supplied the saved workspace, scene publications, GLiNER2.5
producer, NER model, and NLI model. The startup log reported a resident graph
scene with 6,941 nodes and 9,736 edges.

| State | Breeze worker | GPU memory used |
|---|---|---:|
| Phoenix closed, before launch | Absent | 4,239 MiB |
| Woods reading at passage 62 | PID 7296 | 8,292 MiB |
| Paused at passage 67, after idle release | Absent | 4,357 MiB |
| Resumed to passage 68 | New PID 21592 | 8,435 MiB |
| Directed Alyssa sample playing from cached PCM | Absent | 4,363 MiB |

The paused book resumed with Woods and advanced to passage 72. The voice panel
showed 13 Breeze voices, including Gilly, Alyssa, and the separate
`Alyssa · expressive narrator` entry with a `directed reference` label. Its
preview completed while Woods remained the selected book narrator. The live
sample was not assessed for subjective vocal quality in this check.
