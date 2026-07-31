# EmbeddingGemma mmap page proof

Status: **implemented; two deterministic model-backed CLI runs passed**

## Boundary

`PhoenixGraphGenerationV3` remains source and semantic authority. Embeddings
are a derived `PhoenixEmbeddingPagesV1` sidecar so a model, quantization, or
ANN index can be replaced without mutating source truth.

```text
verified PhoenixGraphGenerationV3
  -> exact dynamic chunks + committed turns
  -> EmbeddingGemma document prefixes
  -> one batched native ONNX invocation
  -> canonical row table + contiguous normalized f32 slab
  -> hash-bound .phxe1 publication
  -> one-time verified mmap open
```

The sidecar header binds:

- V3 generation hash
- V3 source-set hash
- model identity hash
- exact model-asset hash
- embedding-configuration hash
- row and vector page hashes
- whole-artifact hash

Opening rejects incomplete, corrupt, oversized, noncanonical, or
authority-mismatched artifacts. Serving views borrow directly from the
read-only mmap.

## Frozen CLI cohort

The deterministic mixed-source fixture contains:

- four exact document dynamic chunks;
- one committed human conversation turn;
- one committed assistant conversation turn.

The query is `Which world is called the Red Planet?`. The hard semantic gate
requires Mars evidence to rank first.

Runner:

```text
apps/phoenix-memory-embed-proof
```

Default model:

```text
onnx-community/embeddinggemma-300m-ONNX
onnx/model_q4.onnx
```

The runner uses the model's direct two-dimensional `sentence_embedding`
output. The existing hidden-state pooling path remains intact for models such
as Jina and MDBR.

## Verified identities

```text
generation
d43fff4fd6bcc3be85e7f9e44e49d9160627cc9ecffb4777373fe7f3df66cf0a

source set
73b761244ea65eca9a54e04be7557620d42f3dd291d2f13ad80c6c17b1adc243

embedding artifact
64f7beeaf8185339bff33628cbd507fee929218a19c8953f7d8349c8b51bdb22

model identity
6a48ef479603a64e44ed8016ccb988c4e2cb1b156202ea19bddee140324c24c1

model assets
246b0cb0955048a322f48ba077db988a3e91cc6418aead618c102d5e0b3c0cfb

configuration
5eb2d54660dffa41574487c59469cf35ae79e562987a04bdb5e9676f910298de
```

Both runs reproduced all six hashes and the exact ranking scores.

## Measured proof

CPU execution provider, six rows, 768 dimensions:

| Measurement | Run 1 | Run 2 |
|---|---:|---:|
| Fixture publication | 54.932 ms | 2.813 ms |
| Model load | 2,485.892 ms | 2,232.520 ms |
| Batched embedding | 199.417 ms | 181.260 ms |
| Sidecar write | 23.367 ms | 12.692 ms |
| Verified mmap reopen | 95 us | 96 us |
| Artifact bytes | 19,392 | 19,392 |

Top results were identical:

1. committed conversation answer, score `0.6184162`;
2. exact Mars document chunk, score `0.61385185`;
3. committed conversation question, score `0.5661258`.

These timings qualify the CLI proof only. They are not a production
throughput, GPU, ANN, or live-app claim.

## Commands

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-memory-embed-gemma'
cargo test -p phoenix-memory-embeddings -p phoenix-memory-embed-proof --release
cargo clippy -p phoenix-memory-embeddings -p phoenix-memory-embed-proof --release --all-targets -- -D warnings
cargo build -p phoenix-memory-embed-proof --release

$env:ORT_DYLIB_PATH = 'C:\code land\clean-rust\node_modules\@huggingface\transformers\node_modules\onnxruntime-node\bin\napi-v6\win32\x64\onnxruntime.dll'
D:\phoenix-target-memory-embed-gemma\release\phoenix-memory-embed-proof.exe --replace
```

Artifacts:

```text
D:\phoenix-memory-embedding-proof\embeddinggemma-cohort.phxgg3
D:\phoenix-memory-embedding-proof\embeddinggemma-pages-v1.phxe1
D:\phoenix-memory-embedding-proof\embeddinggemma-proof-receipt.json
```

## Explicitly still open

- No production coordinator command publishes embedding sidecars.
- No ANN query path consumes this sidecar.
- No GPU execution-provider result is claimed.
- No embedding page is source or graph authority.
