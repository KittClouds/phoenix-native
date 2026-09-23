# Native Breeze provider and cache boundary

Implemented in `crates/phoenix-tts-native`. Windows native Vulkan; no WSL and no
upstream HTTP/WebSocket server. This is a library/control-worker slice, not shell
Reader UI activation or a long-form quality qualification.

## Trust and ownership

`Bundle::open` enrolls actual executable, model and application DLL bytes with
BLAKE3 and read-only Windows file leases. Files are mmap-hashed; writes/deletion
are denied while the provider holds them. DLLs from both executable and explicit
DLL directories participate in runtime identity. Working directory and PATH are
controlled; only the system directory and declared DLL directory are on PATH.
Windows/driver dependencies remain platform prerequisites, not portable bundle
artifacts. Bundle enrollment assumes trusted local directories and audited worker
code; it does not make arbitrary executables trustworthy.

Rust owns one hidden child and one authenticated loopback connection. The child
echoes a per-launch 128-bit nonce before readiness. Model load, request I/O and
generation have deadlines. Cancellation closes the socket, kills only that owned
child, and confirms exit before replacement. Unconfirmed exit blocks new work.
There is no automatic replay after partial audio; callers explicitly submit the
next request with a fresh cancellation token. Drop also attempts cleanup.

## Protocol PBN1

Every header is exactly 32 bytes: magic `PBN1`, little-endian kind u32, request
u64, sequence u64, value u64. No JSON, base64 or whole-chapter audio RPC.

| Event | Meaning |
| --- | --- |
| Ready (0) | Request/sequence zero; value is sample rate 24000 |
| Request (10) | Monotonic request ID, seed in sequence, maximum PCM frames in value; followed by two u32 byte lengths, text and instruction UTF-8 |
| Started (1) | Sequence zero; value is actual prompt tokens plus output-token budget and 8 slots of context slack |
| Audio (2) | Next sequence; value is 1..2048 mono PCM frames, followed by exactly twice that many signed-16 LE bytes |
| EOS (3) | Next sequence; exact nonzero total PCM frames; only normal model EOS |
| Barrier (11) | Rust echoes EOS request, sequence and frame count |
| Quiet (4) | Worker acknowledges barrier at EOS sequence + 1, same total |
| Limit (5), Failed (6) | Never authorize cache publication |

Text is bounded to 16384 bytes, direction to 4096, full context to 2048 tokens,
and output to 1440000 PCM frames (60 seconds). Worker uses a 16 KiB send buffer;
Rust reads one stack-backed 4096-byte PCM block at a time. Blocking worker sends
apply backpressure; no unbounded event/audio queue exists. The model/vocoder have
their own bounded-per-request allocations, separate from the transport buffer.

## Completion and cache publication

The upstream completion patch is checked in as `native/completion.patch`, against
Breeze source `a5436642d4c64304b398ceeda9b8fce4577bfdb1`. It distinguishes model
EOS, token limit and callback cancellation, and rejects undersized decoder output
before exposing its PCM pointer. The worker additionally requires successful
generation, finite samples, and PCM total equal to codec frame count times 1920.

Rust converts admitted packets to existing bound provider envelopes. It requires
exact request ID, contiguous sequences/frames, normal EOS and matching Quiet.
Only then does `CacheWriter::finish` validate completion, sync audio and commit,
and publish the directory. EOF, limits, malformed/late events, sink failures and
cancellation before the final publication decision drop the partial writer.
An abandoned `.writing` directory is never a hit and is cleaned on cache reopen.

The final cancellation check before `finish` is the publication decision point.
Cancellation after that point does not revoke already completed content. Disk
sync and a synchronous sink are not forcibly interruptible: sinks must return
promptly, and this API must run off the UI thread. The 250 ms cancellation target
is a measured gate, not a guarantee during arbitrary filesystem stalls.

`generate` returns the audio **lookup key**, distinct from the realized PCM hash
returned internally by `CacheWriter::finish`. Cache hits are fully validated by
`AudioCache::get` and returned without regeneration. `generate_streamed` emits
borrowed `PcmChunk { binding, first_frame, pcm }` only on cache misses. Playback
must fence live chunks by binding/epoch, flush on failure and use the normal
cached playback path for hits. This slice does not wire the shell audio device.

## Honest identity and alignment

The key covers model/tokenizer/codec, executable/DLL runtime, direction, seed,
output limit and fixed generation/PCM policies. Fresh GenSession per request
avoids hidden voice-anchor history. Reference cloning and stable character voice
profiles are not yet exposed. The provider reports segment alignment only;
sentence/word timings must not be invented from uniform duration estimates.

## Reproduction and evidence

Build the pinned Breeze tree on D: with `native/completion.patch` applied, the
qualified Vulkan toolchain, and include `native/CMakeLists.txt` after declaring
`breeze_core`. The experiment build script is
`D:\phoenix-tts\breeze-native-20260906\build.ps1`; the worker target is
`phoenix-breeze-worker`. The supervisor sets the two qualified process-local
compatibility flags, host-visible VIDMEM disabled and NV coopmat2 disabled.

`scripts/verify-reader-contracts.ps1` runs focused fmt, clippy and tests, building
on D: and executing via the C: junction. Provider tests use a scripted child for
success, cache hit, resident reuse, admission rejection, crash, stale request,
oversized/partial audio, EOF, token limit, wrong count, audio instead of Quiet,
startup/prefill/audio cancellation, timeout, sink failure, stale epoch and file
lease protection. Existing Reader/cache/playback tests run in the same gate.

The real `cache_smoke` example accepts worker, model, DLL directory and fresh
cache root. It tests normal GPU generation, bound streaming, validated cache hit,
forced token limit, cancellation after PCM, confirmed-exit recovery and durable
cache reopen. It produces no device playback or UI interaction. Evidence lives
under `D:\phoenix-tts\breeze-native-20260906`, including `worker-build.log`,
`provider-smoke-02.log` and `provider-cache-proof-02`.

Quality, voice continuity, WER, cold-start optimization, long-form throughput,
host-crash/driver-hang soak, and packaged Reader UX remain separate gates.
