# Phoenix local LLM candidate qualification — 2026-09-16

## Decision

MiniCPM5-2B Q8_0 is qualified for a supervised live trial in Phoenix Kammi.
The Shortrun packaged-app settings now select this exact model and the official
CUDA-enabled llama.cpp runtime described below.

K2-Horizon 0.9B remains an experimental runtime candidate. It is not approved
for the Phoenix live-provider setting because upstream llama.cpp does not yet
support its architecture, this machine has no CUDA compiler for the required
fork, and its current CPU latency and reasoning behavior do not satisfy the
interactive Kammi contract.

This qualification changes model/runtime artifacts and the saved Shortrun Kammi
settings. It does not change Phoenix source code.

## Qualification boundary

- Host GPU: NVIDIA GeForce RTX 3080, 12 GiB VRAM.
- Host CPU: AMD Ryzen 7 5800X3D, 8 cores / 16 threads.
- Phoenix package:
  `D:\phoenix-target-atlas-runtime-shell-20260813\release\phoenix-shell.exe`
- Phoenix package SHA-256:
  `ccac4fc13458fa8f98efcc39cf288f2b640741b195b0b52006f845d58645dcdf`
- Workspace:
  `D:\phoenix-gliner25-live-20260914\shortrun\workspace.json`
- Settings:
  `D:\phoenix-gliner25-live-20260914\shortrun\workspace-v1.json.kammi-v1.json`
- Pre-change settings backup:
  `D:\phoenix-qualifications\local-llm-candidates-20260916\workspace-v1.json.kammi-v1.before-local-llm.json`
- Qualification used the Phoenix request shape: OpenAI-compatible chat
  completions, no explicit local reasoning override, and a 4,096-token output
  ceiling.

## MiniCPM5-2B Q8_0

### Frozen identity

- Repository: `openbmb/MiniCPM5-2B-GGUF`
- Repository revision:
  `2079a22f3beaa4e306449978533478fe0522f4b3`
- License: Apache-2.0
- Model:
  `D:\phoenix-models\candidates\minicpm5-2b\2079a22f3beaa4e306449978533478fe0522f4b3\MiniCPM5-2B-Q8_0.gguf`
- Model SHA-256:
  `c5415f8989bf88a8288f1b55a3cc371af53c07b0faa220a63bd7a990cfaba078`
- Runtime:
  `D:\phoenix-runtimes\llama.cpp\b10982\runtime\llama-server.exe`
- Runtime revision:
  `llama.cpp b10982 / fc82583e65ad753710fbd69a9244d9a35dca667a`
- Runtime SHA-256:
  `ffaee576ad271ede87b92a7d8c3863dc8f331bbac666ee00099c250e8809e743`
- Runtime configuration: CUDA, 8,192 context, 99 GPU layers, 8 CPU threads.

### Measured performance

| Workload | Result |
|---|---:|
| Prompt processing, 512 tokens | 11,297.81 ± 1,397.30 tok/s |
| Generation, 128 tokens | 201.36 ± 1.66 tok/s |
| Exact `PHOENIX_READY` probe | 457 ms, correct |
| Structured extraction probe | 2,385 ms, correct JSON |
| Rust implementation probe | 912 ms, valid code |
| 3,784-token needle retrieval | 370 ms, correct |

The Rust probe added Markdown fences despite an explicit no-fence instruction.
That is a minor adherence defect and should remain in downstream acceptance
coverage. It does not block the live trial.

### Promotion state

`qualified_for_live_trial`

The saved Shortrun Kammi settings select this model and runtime at
`http://127.0.0.1:8080/v1`. Phoenix owns server startup, health checking,
cancellation, and shutdown through its existing supervised llama.cpp manager.

## K2-Horizon 0.9B

### Frozen identity

- Repository: `IFM/K2-Horizon-0.9B-GGUF`
- Repository revision:
  `c9e2c6d99a9c682cdc2c4f439c480c07a6ded35c`
- License: Apache-2.0
- Source BF16 model SHA-256:
  `371010db1807bb07b62e738422ee0de26c1e15a347f31108ed2c6e219095a8b8`
- Local Q8_0 model:
  `D:\phoenix-models\candidates\k2-horizon-0.9b\c9e2c6d99a9c682cdc2c4f439c480c07a6ded35c\K2-Horizon-1B-Q8_0-phoenix.gguf`
- Local Q8_0 SHA-256:
  `741fb9ad263956c003883cb7caddeb14b4dfb9b4b67b5414ad12967e965dd547`
- Runtime:
  `D:\phoenix-target-k2-llama-cpu-35999d1\bin\Release\llama-server.exe`
- Runtime revision:
  `MBZUAI-IFM/llama.cpp 35999d101cf2233fc54f09c3c8d599da7303ce02`
- Native-Windows Unicode patch SHA-256:
  `4e95655371cbda911ba8864c5317a60009b9a89517bdc2a704f5f5f596a6e4ca`
- Runtime SHA-256:
  `8b616c48130fccad3a5384367b2db0a3d49fec3a86d0a44ccac8ef8e49967e66`
- Runtime configuration: CPU AVX2/FMA, 8,192 context, 8 threads.

### Measured performance

| Model | Prompt 512 | Generate 128 |
|---|---:|---:|
| BF16 | 147.66 ± 0.72 tok/s | 11.26 ± 0.54 tok/s |
| Q8_0 | 155.50 ± 2.15 tok/s | 20.84 ± 1.03 tok/s |

App-shaped Q8_0 probe latency ranged from 4.4 seconds for the exact-answer
probe to 38.1 seconds for long-context retrieval. Structured extraction took
31.4 seconds and omitted one requested object. High reasoning recovered the
missing detail but remained slow. Low reasoning emitted a model-control token
inside the response, and the BF16 reasoning probe exhausted a 512-token cap
without returning content.

### Promotion state

`experimental_runtime_only`

Reconsider K2 only after all of these gates pass:

1. Its architecture is supported in an upstream llama.cpp release, or a
   revision-bound CUDA runtime is built and qualified locally.
2. The Phoenix local-provider request can express the model's reasoning control
   without leaking control tokens.
3. Structured extraction is complete and schema-valid under the actual Phoenix
   4,096-token output ceiling.
4. Interactive latency is competitive with the qualified MiniCPM lane.

## Receipts

| Artifact | SHA-256 |
|---|---|
| MiniCPM probe receipt | `03ff9001974d5ea3747a8760129e0d85a500927fcc8f063f8bba37dc4e35762f` |
| K2 Q8 probe receipt | `5c2224b72ec88831e90bd84a524e7c26049ca8fe5ef0ebd2b21da08723fc281b` |
| K2 BF16 probe receipt | `a26e260372535f3a3285c5d89af593e0ec15f1d92620208aba62c7c410815fc6` |
| MiniCPM candidate manifest | `4f9517e9779281bc474335302e2263249d70f4cb6e400a0403639e0472b4d1e7` |
| K2 candidate manifest | `ae75da413a5b6e9a8640c9e4fae906388348573d8548e661f587ddf1094f97be` |
| Pre-change settings backup | `5608357e8097cd58f7044ad82c50a1087a1861cade885b97d0c4af5b77e74637` |
| Active MiniCPM settings | `5d21bef3b59ab93d06a76c4597b1dae3a2b687358ea682e442f8f6a2e6e19a34` |

Focused Phoenix validation:

```text
cargo test -p phoenix-shell llama_cpp -- --nocapture
3 passed; 0 failed
```

The tests cover loopback-only endpoint enforcement, missing-file rejection
before process launch, and the local request contract that uses the llama.cpp
model alias without OpenRouter reasoning fields.

## Remaining live acceptance

The packaged Phoenix process was restarted successfully with the Shortrun
workspace and the saved MiniCPM configuration. Its graph authority restored
generation 13 with 6,941 nodes, 9,736 edges, 86 entities, and 1,371 of 1,371
highlights mapped.

Native Windows UI automation is unavailable in the current Codex tool surface,
so a visual Kammi send was not claimed. The remaining manual acceptance is to
open Kammi and send `Return exactly PHOENIX_READY with no punctuation.` The
visible answer must be exactly `PHOENIX_READY`; Phoenix should supervise the
server for the request and leave no orphan `llama-server.exe` after app exit.
