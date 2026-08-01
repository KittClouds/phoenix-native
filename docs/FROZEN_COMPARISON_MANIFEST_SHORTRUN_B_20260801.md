# Frozen comparison manifest: Shortrun B

This is the native comparison lock for the exact cohort exercised on 2026-08-01.
The binary authority is the `.pnrm` file; this document is its human-readable
index and does not replace verification through `--verify-release-manifest`.

## Cohort identity

| Field | Frozen value |
|---|---|
| Source | Phoenix workspace document `7` (`Shortrun B`) |
| Revision | `3` |
| Content identity | `722aa55741d9fadb` prefix; the complete digest is stored in the `.pnrm` payload |
| Native source bytes | `159,411` |
| UI measurement | `152,061` chars / `26,202` words |
| Registry revision | `11` |
| Analysis generation | `10` |
| Scene generation | `8` |
| Registry entities | `68` (`1` user-tagged + `67` NER) |

The older Angular receipt is not the same frozen cohort: it reported
`154,105` document characters and `129` chunks. Those values remain a
comparison target, not a native parity claim.

## Native artifact identity

| Artifact | Frozen identity |
|---|---|
| Release manifest | `D:\\phoenix-topology-alignment-20260801\\shortrun-b-topology-alignment-20260801.pnrm` |
| Release manifest SHA-256 | `08CC9B4ECC463412461468042A77406A0BC79750A51C4A1AABFB7089C8405212` |
| Scene archive cohort hash | `1e5f4436c0c717c1f802b854aa04aabebb6c3de18d2028cbc77b57fdd30dcd0e` |
| Scene product-index hash | `4635cab0ba26fb681d388e2a58ce166b8f6e7440fe9fad9bb5da22d2a3df76a4` |
| Runtime binary | `C:\\phoenix-bin\\native-topology-alignment-4c76ad549c1a\\phoenix-shell-topology-alignment-4c76ad549c1a.exe` |
| Runtime binary SHA-256 | `4C76AD549C1ADA3E92F1D1B423DE5B847333458E297F44643E5CCEDFF479931E` |
| NER producer | `C:\\phoenix-bin\\native-cut6-interaction-20260728\\phoenix-analysis-bridge.exe` |
| NER model | `D:\\hf-models\\gliner-bi-base-v2.0-onnx` |
| NLI model | `D:\\phoenix-models\\modernbert-base-nli-onnx` |

## Structural and semantic inventory

The exact native producer run recorded:

| Product | Count |
|---|---:|
| Dynamic chunks | 395 |
| Mentions | 1,097 |
| Evidence rows | 1,097 |
| Typed relationship candidates | 511 |
| Events | 227 |
| Episodes | 40 |
| Episode memberships | 84 |
| Temporal candidates | 75 |
| Causal candidates | 7 |
| Memory/state candidates | 41 |
| Contextual evidence | 990 |
| NLI adjudications | 0 |
| Durable decisions | 0 |

All semantic products are candidate/evidence products. No candidate was
automatically promoted.

## Scene projection inventory

| Projection | Nodes | Edges |
|---|---:|---:|
| Total resident scene | 3,452 | 6,188 |
| Entities | 68 | 0 |
| Structure | 1,533 | 2,713 |
| Facts | 861 | 1,495 |
| Discourse | 990 | 1,980 |

The scene was opened through the native publication authority. All five
manifolds rendered, note switching worked, registry highlighting restored, and
cold restart reopened generation `8` with the same `3,452 / 6,188` topology.

## Verification commands

```powershell
$exe = 'C:\phoenix-bin\native-topology-alignment-4c76ad549c1a\phoenix-shell-topology-alignment-4c76ad549c1a.exe'
$manifest = 'D:\phoenix-topology-alignment-20260801\shortrun-b-topology-alignment-20260801.pnrm'
& $exe --release-manifest-only --verify-release-manifest $manifest `
  --workspace 'C:\Users\shuga\AppData\Local\Phoenix\NativeShell\workspace-v1.json' `
  --scene-publication-root 'C:\Users\shuga\AppData\Local\Phoenix\NativeShell\scene-publications-v1' `
  --producer 'C:\phoenix-bin\native-cut6-interaction-20260728\phoenix-analysis-bridge.exe' `
  --ner-model-root 'D:\hf-models\gliner-bi-base-v2.0-onnx' `
  --nli-model-root 'D:\phoenix-models\modernbert-base-nli-onnx' `
  --require-full-scene
```

Expected result: `PHOENIX_RELEASE_MANIFEST_VERIFIED`, document `7`, revision
`3`, scene generation `8`, with no cohort drift.
