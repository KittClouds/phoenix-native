# Reader contract implementation v1

This slice implements `phoenix.reader/v1` in three native workspace crates.
It does not change the shell, run Breeze, install WSL, or claim audible playback
or narrator qualification. Existing dirty Kammi/shell work is outside this slice.

## Ownership and interfaces

| Crate | Implemented boundary |
| --- | --- |
| phoenix-tts-contract | Content-bound borrowed requests; explicit provider capabilities; ordered events, optional alignment events, completion receipts; cancellation/quiescence state machine |
| phoenix-reader-session | DocumentLease binding; deterministic Markdown rendering; complete source/spoken mapping; validated chapter/segment plans; historical snapshots; checkpoints and bookmarks; mmap cache and atomic completion |
| phoenix-audio | 64 preallocated 2,048-frame PCM blocks; bounded lock-free handoff; independent atomic pause/epoch control; source/device-clock mapping |

Use `DocumentBinding::from_lease` with a durable workspace identity. Never derive
workspace identity from a display name. `SnapshotStore::retain` persists exact
committed UTF-8 bytes; `retain_plan` binds the validated plan to that snapshot.
`NarrationPlan::new` accepts explicit planner output and rejects malformed coverage,
reordered mappings, chapter crossing, stale content hashes, and invalid UTF-8.
`plan_markdown` now supplies the first deterministic planner. It uses pinned
`pulldown-cmark` and Unicode sentence boundaries, makes H1/H2 headings chapter
boundaries by default, keeps heading/body source ranges, preserves visible link
labels and emphasis text, and records omitted code blocks/images. It rejects raw
HTML, tables, footnotes, math, and empty documents with source offsets. Long
sentences split only at grapheme and word boundaries. The planner does not infer
dialogue, rewrite pronunciation, insert vocal events, or claim model token
capacity; those remain explicit future planner inputs.

SourceMap validates once, then projects by binary search plus intersecting runs.
Copy projections use exact offsets; substitutions paint the complete original
range. Inserts paint nothing. The UI must perform grapheme-aware projection and
check the editor's document binding before applying any paint. SIMD-dispatched
memchr scanning and BLAKE3 are used where applicable; immutable audio and snapshots
use read-only mmap views, and cold indexes use hashbrown. There are no whole-book
PCM allocations or per-sample objects.

## Provider and cancellation

Adapters must tokenize the complete formatted prompt before request admission.
Hashes identify actual model/runtime/configuration contents, not filesystem paths.
Events carry request, epoch, plan, segment, audio key, and exact sequence. Start
must precede audio. Blocks must be contiguous and fit the frame budget. Protocol
violations poison the validator. Normal completion is the only source of a
Completion receipt; transport EOF and token-limit stops cannot seal a cache entry.
Alignment hints follow their audio and must preserve text/sample ordering and one
alignment authority. Storage additionally checks UTF-8 and spoken-text coverage.

Cancellation invalidates the playback epoch before worker shutdown. The control
path is independent of the PCM queue. The worker state machine requires unique
request IDs and rejects late quiescence acknowledgements for previous requests.
After two seconds it requests termination. Replacement is barred until the caller
confirms the owned process exited. This crate does not execute process termination.
Audible <=250 ms and worker deadline compliance require a real backend measurement.

## Playback and durable position

The ring renders mono s16 at 24 kHz and 1x. Pause is applied at the next callback
boundary and retains unread audio. Cancellation drops old-epoch blocks. The callback
performs no allocation, refcount cloning, blocking calls, filesystem I/O, or logging.
Render reports are queued-audio accounting, not proof that sound was presented.
Only `PresentationClock::acknowledge` driven by actual device positions may advance
the listening cursor or emit a segment boundary. Underrun silence has no source
span and cannot advance the cursor.

A future resampler/time-stretcher supplies piecewise source/output frame mappings
and accounts for DSP latency. The clock supports these mappings; pitch-preserving
speed DSP and an OS audio device are not implemented here. The selectable speed
contract is 0.5x through 3x; that is not a real-time generation performance claim.

Durable positions bind segment, source frame, audio key, and actual artifact hash.
`set_position` accepts verified completed CachedAudio, and `resume_position`
revalidates that exact artifact. Until a live segment completes, keep its precise
position ephemeral and retain the preceding durable checkpoint. Regeneration after
eviction must not silently reuse an old offset: require explicit segment restart.

The session store rejects stale sequences and throttles non-forced checkpoints to
one per second. Force checkpoints on pause, seek, bookmark and close. Snapshot edits
do not mutate plans. To adopt a new document revision in this slice, create a new
session ID; old sessions/bookmarks remain pinned. Automatic bookmark migration and
voice-profile switching are deliberately unavailable until validated migration
rules exist. There is no unverified fuzzy migration.

## Cache publication and recovery

One OS-owned writer per store root is enforced by an exclusive handle that is
released on process death. Reserve worst-case PCM plus manifest space before
writing. Eviction uses cold LRU metadata and cannot evict active mmap leases.

Writes go to unique `.writing` directories. The writer validates events against
the actual streamed byte counts, incrementally hashes PCM, verifies completion and
alignment, flushes PCM, writes and flushes the commit record last, and publishes the
directory by a same-volume Windows rename with write-through. No reader opens the
writer's files. Dropped writers clean up; recovery removes recognized unpublished
UUID directories. Commit and PCM hashes are reverified on open. OS handles deny
concurrent writing/truncation/deletion of mapped committed files.

Completion means structural audio integrity, not WER, voice quality, or semantic
alignment accuracy. Provenance identifies the producer of alignment, not evidence
that an external model was scientifically qualified. Power-loss durability still
depends on Windows/filesystem/device guarantees; simulated crash tests are not
physical power-cut certification.

## Verification

Run `scripts/verify-reader-contracts.ps1 -Benchmarks`. It builds on
`D:\phoenix-target-reader-contract`, creates/verifies a C: junction under
`C:\phoenix-bin`, and executes exact Cargo-emitted test/benchmark artifacts through
that link. It runs focused fmt/clippy checks and writes an executable-SHA-256 proof
receipt under the D: target. No existing Phoenix process is stopped or relaunched.

Coverage includes restart with cached audio, stale checkpoints, edited document
binding, Unicode and substitutions, invalid plans/alignment, incomplete cache
publication, truncated generation, cache corruption, exclusive ownership, leased
eviction, late request acknowledgements, full-queue cancellation, pause/resume,
device clock accounting, and allocation-free sustained pool reuse.

## Completed-cache playback runtime

`ReaderRuntime<D: PlaybackDevice>` joins revision-pinned plans, exact-artifact
resume, mmap cache leases, source-frame positions, bookmarks and checkpoints.
It advances position from the device sample clock, never from submitted bytes.
It explicitly requests the next segment after the current audio drains. Missing
audio remains `NeedsAudio`; provider generation is still external to this slice.
Attach checks epoch, segment, synthesis identity and exact resume artifact.
Seek/cancel synchronously reset the endpoint before accepting replacement audio.
Device errors fail playback and attempt a flush. Dropping the runtime also flushes.

The Windows `WaveOutput` backend owns eight fixed 2048-frame PCM buffers (32 KiB)
and uses WinMM sample-position reporting. It has no audio callback or hot-path
allocation. Its sample clock is backend evidence, not acoustic measurement.
This first backend plays mono 24 kHz at 1x; other speeds are rejected. It drains
between segments, so gapless narration is not claimed. The existing PCM ring
remains available for future live provider streaming; cached playback borrows
the mmap directly and copies into the bounded device queue.

Call `tick` from a control worker, service `NeedsAudio`, and call `checkpoint`
periodically (internally limited to once per second) and force it after pause,
seek, bookmark and before close. A caller must save before dropping the runtime;
Drop flushes audio but does not do filesystem checkpoint work.

`scripts/verify-reader-contracts.ps1 -DeviceSmoke` additionally builds and runs
two silent-PCM examples through the verified C: link and records their hashes.
They exercise the actual default Windows endpoint, pause/resume, device-clock
advance, flush, two-chapter progression, bookmarks and exact checkpoint reopen.
The Criterion `cached_runtime_verified_replay_4_frames` benchmark includes cache
verification, mmap acquisition, seek and completion with a scripted device;
it is a control-path baseline, not a narrator throughput measurement.

Remaining product work: dialogue and pronunciation planner inputs, sidecar
supervision and transport, provider tokenization and truthful completion reasons,
time-stretch DSP, kernel/UI integration, automatic policy-driven checkpointing,
long-form inference, WER/listening tests, GPU coexistence, and packaged UI proof.
