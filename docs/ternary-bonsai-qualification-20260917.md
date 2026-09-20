# Ternary Bonsai 2 27B qualification — 2026-09-17

## Result

Ternary Bonsai 2 27B is a serious high-capability local candidate for Phoenix,
but it remains a sandbox candidate. The model loaded successfully on the RTX
3080 through the publisher's Prism llama.cpp CUDA fork and produced clean
structured extraction when the server ran with reasoning disabled.

The active Phoenix MiniCPM configuration was not replaced. No Phoenix source
code was changed for this experiment.

## Frozen artifacts

- Hugging Face repository:
  `prism-ml/Ternary-Bonsai-2-27B-gguf`
- Model revision:
  `6ed5e12bf84b7a63069882c91dd9e9218647d17b`
- License: Apache-2.0
- Selected pack:
  `D:\phoenix-models\candidates\ternary-bonsai-2-27b\6ed5e12bf84b7a63069882c91dd9e9218647d17b\Ternary-Bonsai-2-27B-PTQ1_0.gguf`
- Selected pack size: `5,946,648,928` bytes
- Selected pack SHA-256:
  `53107f530aa52eb00912263ab1ee29bd199261c87cd7b4ad4ca1318c1fe33ee`
- Prism runtime archive:
  `D:\phoenix-runtimes\prism-llama.cpp\prism-b10685-7dffb15\win-cuda-12.4\llama-prism-b10685-7dffb15-bin-win-cuda-12.4-x64.zip`
- Prism release: `prism-b10685-7dffb15`
- Runtime archive SHA-256:
  `7aa73f2c52081a7280fa4b02660ba902b964e461b0cfc0b652935e4a59b01c6e`
- Runtime executable:
  `D:\phoenix-runtimes\prism-llama.cpp\prism-b10685-7dffb15\win-cuda-12.4\runtime\llama-server.exe`
- Runtime executable SHA-256:
  `e0ea4fd53e6f0c741cbd28093b427f333ada0eb03b83073e1f99c483793ea976`
- Runtime version: `0.2.0-dev`, build `10685`, commit `7dffb158d`

### Runtime uncensoring adapter

- Adapter repository: `Continuum-AI-Corp/OrcaBonsai-27B-Uncensored`
- Adapter:
  `D:\phoenix-models\candidates\ternary-bonsai-2-27b\6ed5e12bf84b7a63069882c91dd9e9218647d17b\bonsai-abliterate-lora.gguf`
- Adapter size: `9,682,464` bytes
- Adapter SHA-256:
  `f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`
- Adapter license: Apache-2.0
- Server launch flag: `--lora <adapter>`
- Runtime control: `POST /lora-adapters` with `[{"id":0,"scale":0|1}]`

The adapter is a runtime refusal-direction intervention over the same 129
residual writers used by the published project. It does not replace or
re-quantize the 27B weights. The adapter is specifically exported for the
Bonsai PTQ1_0 GGUF and loaded successfully by the Prism Windows CUDA server.

The alternate `PQ2_0` pack is 7.21 GB. PTQ1_0 was selected because it is the
smaller pack and leaves the most room for context and KV state on a 12 GiB
card. The model repository warns that stock llama.cpp cannot load these files;
the Prism fork is mandatory.

## Runtime qualification

Host hardware was an RTX 3080 with 12 GiB VRAM and a Ryzen 7 5800X3D. The
server was launched on loopback port 8090 with:

```text
--ctx-size 8192 --n-gpu-layers 99 --flash-attn on --jinja
```

The model reached `/health` successfully. Resident GPU memory was approximately
11.1–11.4 GiB, leaving less than 1 GiB free. This is viable for a single
interactive slot, but it is not a safe multi-slot or large-context profile.

## Probe results

### Reasoning enabled

The default reasoning profile can answer exact prompts, but reasoning consumes
the output budget. With a 512-token completion cap, structured extraction
reached the cap and returned truncated JSON. A 256-token reasoning budget
produced valid JSON, but increased latency to approximately 8.8–11.1 seconds
for the short extraction.

### Reasoning disabled

The same server, restarted with `--reasoning off`, was tested with Phoenix-like
OpenAI chat requests and no provider-specific reasoning fields.

| Probe | Result |
|---|---:|
| Exact `PHOENIX_READY` | 570 ms; exact content |
| Structured extraction, 94 input tokens | 3,291 ms; valid minified JSON, exact `predicate` schema |
| 5,035-token needle retrieval | 9,384 ms; correct `COBALT-7319` |
| Rust code generation | 1,849 ms; valid code, no fences |
| Decode timing observed in server response | 36–50 tok/s |

The extraction prompt explicitly fixed the Phoenix schema. Without that schema
example the model sometimes used `relation` instead of `predicate`; the
provider's existing parser contract should remain strict and fail closed.

With the adapter applied at scale `1`, the same probes remained usable:

| Probe | Result with adapter scale 1 |
|---|---:|
| Exact `PHOENIX_READY` | 2,444 ms on first cold request; exact content |
| Structured extraction | 3,594 ms; valid minified JSON |
| Rust code generation | 775 ms; valid code, no fences |
| 5,035-token needle retrieval | 8,581 ms; correct `COBALT-7319` |

The adapter endpoint was then set to scale `0` and `1` for a deterministic,
temperature-zero extraction comparison. Both scales returned valid JSON with
the same four relations. This is the expected result for a refusal-direction
adapter on a normal Phoenix extraction request. The adapter changes refusal
behavior, not ordinary extraction semantics; that behavior is not being
treated as a general quality improvement.

## Phoenix integration boundary

The current Phoenix `LlamaServerManager` starts a server with model, alias,
host, port, context size, GPU layers, and thread count. It does not yet expose
runtime flags for Prism's required `--flash-attn` or the model's necessary
reasoning policy. It also intentionally omits local reasoning fields from the
OpenAI request.

Therefore this candidate is not promoted into the saved Phoenix settings. The
next implementation slice, if promoted, is a revision-bound local runtime
profile containing:

1. The Prism executable hash and model hash.
2. `--flash-attn on` and a single-slot memory profile.
3. An explicit reasoning policy (`off` for extraction, or a bounded budget for
   conversational reasoning).
4. A startup receipt proving the selected runtime, model, flags, VRAM headroom,
   and schema-valid probe before the provider becomes selectable.

The uncensored adapter adds two policy decisions before product exposure:

1. Phoenix must make adapter scale visible and default it to `0` for ordinary
   installs, with explicit user choice for research use.
2. Any adapter-enabled session must retain the base model hash, adapter hash,
   scale, and runtime hash in its request/session receipt.

MiniCPM remains the active local Phoenix trial until that profile exists and
passes the same live packaged-app checks.

## Sources

- Model card: https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf
- Required runtime fork: https://github.com/PrismML-Eng/llama.cpp
- Tested setup and runtime guidance: https://github.com/PrismML-Eng/Bonsai-demo
- Uncensored adapter repository: https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored
