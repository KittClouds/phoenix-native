# Breeze E2E qualification — 2026-09-06

**Superseded runtime choice:** the user selected native Windows/Vulkan through
HoppouAI/Breeze-TTS-2.cpp. The WSL/firmware blockers below apply only to the
historical Python route. See `breeze-native-qualification.md` for current work.

Status: blocked before inference; no narrator, streaming adapter, or Reader UI
qualification is claimed. Existing cached playback tests are independent evidence.

## Pinned experiment

- Root: `D:\phoenix-tts\breeze-qualification-20260906`
- Source: `source`, detached at `43e2ea1595297c4059477e2e4a300653761c759b`
- Model candidate revision: `799624c0b4a1daa8db6d28bbd9850043c0270734`
  from `BreezeBlue/Breeze-TTS-2`; weights have NOT been downloaded.
- Model license: root `MODEL_LICENSE`, version 1.1, SHA-256
  `F158B88CD51473925E1CE8C39F4C7C9E419304AD71371896C4FF18FA1351DCA2`.
- Runtime requirements: Linux, Python >=3.10, torch/torchaudio 2.9.1,
  qwen-tts 0.1.1, transformers 4.57.3. Transitive dependencies still need a
  Linux-resolved frozen lock and installed-environment receipt.
- All fast flags disabled; do not use H100 timings as RTX 3080 predictions.
- Keep model, Linux filesystem, dependency caches and generated audio on D:.

## Observed blockers

RTX 3080: 12288 MiB total, initial 4960 MiB used, 7328 MiB free (~7.16 GiB).
This is below the published ~7.7 GiB eager footprint, before coexistence headroom.
Windows driver reports CUDA compatibility, not a tested Linux Torch environment.
WSL explicitly reports not installed. This process is not elevated.
Windows reports firmware virtualization false, hypervisor false, and CPU SLAT
and VM-monitor extensions true. Motherboard: ASUS ROG STRIX B550-F GAMING.

User action is needed to enable SVM Mode in BIOS/UEFI and restart. Once back in
Windows, install WSL from Administrator PowerShell with
`wsl --install --no-distribution`, then follow any restart requirement. Install an
isolated Linux distribution on D: after checking the installed WSL command options.
The agent has not changed firmware, installed WSL, restarted, or stopped workloads.

Re-run `scripts/check-breeze-readiness.ps1` after setup. Validate `nvidia-smi` and
Torch CUDA inside the named Linux distribution before downloading/loading weights.
Free sufficient GPU memory without terminating unrelated user-owned processes.

## Inference gate

1. Pin downloaded model-file hashes, tokenizer/config hashes, installed Python,
   Torch/CUDA versions and `pip freeze`. Verify the source checkout remains clean.
2. Use synthetic prose fixtures first, then the user's locally selected novel
   passages: exposition, dialogue, unusual names, punctuation, and consecutive
   paragraphs. Synthetic fixtures alone cannot qualify novel narration.
3. Load once in eager mode. Record cold load, first PCM latency, initial silence
   and TTFA separately, output duration, generation wall time, RTF, peak allocated
   and reserved VRAM, total GPU use, configuration, seed and text hashes per run.
4. Exclude warm-up from warm timing aggregates. Use enough repeated requests for
   meaningful p50/p95 and a sustained chapter run; never call a short sample a soak.
5. Preserve WAVs and transcripts for listening/WER. Voice consistency and blinded
   preference remain unevaluated until actual listening; same seed is not proof.

## Provider contract gate

At the pinned source, `breeze_infer/api.py` returns raw PCM and releases its request
lock in `finally`. It does not emit an application-level normal-finish receipt.
`models/fast_streaming.py:849` can stop on EOS, context limit, or token limit;
`is_final` also marks token-limit chunks. Neither HTTP EOF nor `is_final` proves EOS.

Instrument the owned sidecar at those termination branches to emit explicit
normal EOS, token-limit, context-limit, cancellation, and failure outcomes. Emit
Completed only after final codec drain and exact emitted-frame accounting.
Run forced-EOS/limit tests before accepting this hook as truthful. Do not infer
termination from transcript similarity or monkey-patch sampling globally.

Supervise a single owned worker with request/epoch/plan/audio-key binding, bounded
PCM frames and backpressure, independent cancellation signaling, deadlines, and
confirmed process exit before replacement. Use bounded logs and loopback-only
transport. Never stop unrelated Python/WSL processes. Preserve existing request
validation, prompt token admission, immutable cache completion rules, and late
event rejection. Do not retry audible partial speech automatically.

Test truncated transport, worker kill, wrong binding/sequence, context/token
limits, cancel during prefill/decode/drain, backpressure, and restart. Each failed
generation must produce no completed cache entry. Replay a valid artifact through
the native output backend and verify exact positions after reopening the session.

## UI gate after inference and provider qualification

Add a control worker to the native shell for a committed document lease, play/pause,
chapter navigation and automatic checkpoint policy. Source highlighting must use
the pinned source-to-spoken map and declared alignment granularity. Breeze raw
PCM provides no sentence timestamps: use truthful segment highlighting until
sentence alignment is independently produced and validated. Stop highlighting
against an edited revision unless explicitly viewing its retained snapshot.

Build on D:, test through the C: link, and launch an isolated qualification binary
with visible build identity and a disposable workspace. Preserve the running app
and unrelated dirty Kammi changes. Live UI, audible quality, gapless playback and
long-form soak need separate receipts before promotion.

## Sources

- https://github.com/breezeblue-ai/breeze-tts/tree/43e2ea1595297c4059477e2e4a300653761c759b
- https://huggingface.co/BreezeBlue/Breeze-TTS-2/tree/799624c0b4a1daa8db6d28bbd9850043c0270734
- https://learn.microsoft.com/en-us/windows/wsl/install
- https://learn.microsoft.com/en-us/windows/wsl/basic-commands
- https://www.asus.com/us/support/faq/1045141/
