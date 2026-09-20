# Native Breeze qualification

The selected candidate is HoppouAI/Breeze-TTS-2.cpp on Windows/Vulkan.
No WSL or BIOS changes are required by this route. It is a separate runtime;
these weights cannot load into the existing llama.cpp server.

## Reproducible experiment root

`D:\phoenix-tts\breeze-native-20260906`, tested via junction
`C:\phoenix-bin\breeze-native-20260906`.

- Breeze source: `a5436642d4c64304b398ceeda9b8fce4577bfdb1`
- ggml submodule: `36da57138425487184aa1da2eee2cde155909c6f`
- Model repository revision: `81b22bad9f05b99970e30c5ee5e4bbc52fedf2f8`
- Q8_0 model size: 3,568,844,480 bytes.
- Model SHA-256: `a02bcc4b69b0601032727f8040c4942149b1b73aa0f69022fe5aaa6a8f0ef879`
- Shaderc source: `27d8e0b55b53b97c1f25a90014b403993a114392`, dependencies
  pinned by that checkout's DEPS file and downloaded with its git-sync-deps tool.
- Vulkan-Headers, SPIRV-Headers and Vulkan-Loader source tag: `vulkan-sdk-1.4.357.0`.
- MSVC from the installed Visual Studio 18 environment; exact compiler recorded
  in `build/CMakeCache.txt` and `build.log`.

Automatic approval review rejected the combined Vulkan SDK download/installer
command with "blocked by policy". No installer was run. Build prerequisites were
instead compiled from upstream source into this experiment root. The system's
existing Vulkan loader is used; the import library is generated from the pinned
loader export definitions. No global PATH, registry or driver changes are needed.

`build.ps1` and `vs-env.cmd` reproduce configuration and compilation from the
already pinned source trees. `run-qualification.ps1` verifies the C: junction,
runs only the experiment child process, enforces a ten-minute deadline, samples
total GPU use and child working set, and writes executable hash/exit receipts.

## Qualification-only patches

The portable build needs ggml-vulkan linked to the imported SPIRV-Headers CMake
target so its separate include directory propagates correctly.

The upstream generator's boolean return does not distinguish normal EOS from
frame-limit exhaustion. Its WebSocket `done` event is therefore insufficient for
Phoenix cache completion. A local patch records EOS, limit and callback
cancellation in `GenTimings` and makes `GenSession::speak` reject non-EOS endings.
This does not by itself repair the upstream HTTP/WebSocket protocols: their callers
still require explicit failure propagation, bounded transport and request identity.
Do not point Phoenix's completed-cache writer at those unqualified endpoints.

`apps/qualify.cpp` loads once, uses one session with a generated reference anchor,
and synthesizes three original prose/dialogue fixtures twice. It records first
PCM latency, generation wall time, duration and RTF. It writes WAVs for listening
and performs real-model forced-limit and callback-cancellation checks.

These six short synthetic passages are an engineering smoke test, not a novel
gold set, blinded quality test, p95 latency qualification, or long-form soak.
The stopwatch measures first PCM delivery, not first audible speech. GPU telemetry
is sampled total device usage and cannot attribute memory to Breeze under WDDM.

## Observed result

The `compatibility` run passed on the RTX 3080 with process-local settings:

```
GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM=1
GGML_VK_DISABLE_COOPMAT2=1
```

The selected GPU path was Vulkan0 / KHR_coopmat. No CPU fallback was used.
The baseline failed allocating device memory. Disabling host-visible VRAM allowed
loading, but NV_coopmat2 then failed NVIDIA pipeline compilation. Both failed runs
are retained separately and are not qualification successes. Their telemetry
included intermittent NVIDIA driver query failures. The compatibility run had
149/149 valid GPU telemetry samples and exited 0.

| Measurement | Observed |
| --- | --- |
| Successful fixtures | 6/6 normal EOS; total 49.44 seconds of audio |
| Second-pass first PCM, three requests | 271.3–273.3 ms |
| Second-pass RTF | 0.635–0.650 (about 1.54–1.57x realtime) |
| First two first-PCM latencies | 5763.8 ms and 2560.8 ms |
| Model load in successful run | 118.9 seconds; loading delay needs investigation |
| Sampled peak total device memory | 8539 MiB (includes other applications) |
| Initial total device memory | 4408 MiB |
| Sampled peak child working set | 732,016,640 bytes |
| WAV integrity | Six valid mono 24 kHz PCM16 files, nonzero signal, no clipped samples |
| Forced limit and callback cancellation | Both rejected as normal completion |

The second-pass numbers are individual measurements, not p50/p95 estimates.
Integrity/amplitude checks do not establish transcript accuracy or voice identity.
Sample onset above -50 dBFS was 12.5–22.5 ms into the files; this is a simple
amplitude threshold, not a validated perceptual TTFA measurement.

Evidence under the experiment root:
- `pins.json`, `binary-hashes.json`, `qualification.patch`, `ggml-portable-headers.patch`
- `compatibility/run-receipt.json`, `compatibility/telemetry.csv`
- `compatibility/outputs/measurements.csv`, six WAV files
- `compatibility/audio-integrity.json`

Reproduction: set the two process-local flags, then call
`run-qualification.ps1 -RunId <new-name>`. Existing run directories are never
overwritten. The original baseline logs are in the experiment root; the second
failure is under `device-local`. Model/runtime files remain on D:.

## Promotion remains gated

The subsequent user-selected Shortrun Chapter 1 run is **rejected for content
fidelity**: the user confirmed extra speech after paragraph 77, whose source is
only "Drop your weapons!". All 102 paragraphs completed mechanically, producing
17:18.72 of audio, but normal EOS did not guarantee faithful narration. Warm p95
RTF was 0.8065, also above the 0.8 target. See
[chapter qualification](shortrun-chapter-quality.md) and the retained regression
fixture `crates/phoenix-tts-native/fixtures/short-dialogue-hallucination.json`.

The owned worker and Rust cache integration are now implemented and exercised;
see [native provider/cache contract](native-provider-cache-v1.md). The upstream
HTTP/WebSocket server remains outside completed-cache authority.

Before Reader product promotion: perform listening/transcript checks on
representative local novel passages and qualify stable voice identity. Then wire
the shell playback/UI with truthful alignment granularity and automatic
checkpoints, followed by the long-form and packaged-app gates.
