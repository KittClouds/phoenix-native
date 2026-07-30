# Clean-room Shortrun V2 shadow receipt

Date: 2026-07-29

Verdict: Cut 2 entity/evidence boundary passes for the current immutable
Phoenix Native Shortrun authority. Exact Angular cohort parity is not claimed.

## Safety and scope

- Phoenix Native remained running and untouched.
- The workspace, registry, analysis authority, scene authority, database, and
  application binary were not written.
- Input artifacts were opened read-only and verified before use.
- Generated V2 artifacts were confined to:
  `D:\phoenix-target-cleanroom-entity-v2\shadow-shortrun-v2-20260729`.
- No kernel command, scene compilation, scene publication, or UI mutation ran.

## Input authority

```text
source_document_id=native:0000000000000007
document_id=7
document_revision=1
document_blake3=dae0a8b09fcc733284a4aad2bc4f66641f259ab4a40b5a974b760396f46fe422
source_bytes=159332
analysis_generation=2
source_registry_revision=1
target_registry_revision=2
producer_binary_hash=c31c5b49e03a370717cebd994a1bebcece633fa6c644afcc7e6c68b38cd73218
coordinator_queue_high_water=1/1
```

Models:

```text
chunker=phoenix-chunker/structural-v1
chunker_runtime=rust-native
chunker_artifact=c31c5b49e03a370717cebd994a1bebcece633fa6c644afcc7e6c68b38cd73218
chunker_config=49aeb804303249e0a3465c02d7379d0321939d6e27446ec58e05cb5dc0cbe735

dynamic_ner=phoenix-dynamic-ner+gliner-bi-base-v2.0
dynamic_ner_runtime=ort-2.0.0-rc.9
dynamic_ner_artifact=2feae499d143f197442dee200acbd1dbb3daf85f0011d94a34431d6195bebb24
dynamic_ner_config=16857b462b6561ae225fc0ef3beaaa7d14a51def7947a6e3b38720bf6b363656

nli=onnx-community/ModernBERT-base-nli-ONNX
nli_runtime=ort:auto-gpu
nli_artifact=68af8c14317fe7a02ebe1a4025af69becdd538e3a0da85921a44e9a863f8ad7f
nli_config=6f3386da5c57430be5a460e7808a67450ce0a08ef8066017fd0b98bcbca19475
```

## Shadow result

```text
structural_reuse=durable_verified
chunks=393
sentences=2361
entities=65
mentions=1101
graph_evidence=1101
editor_paint_spans=1100
canonical_bindings=65
identity_candidates=0
identity_already_canonical=57
generic_related_candidates=0
generation_hash=a3e8e3ac0aecad7f3ff03500b29740403195fc031e090e3de131cc693a345610
graph_evidence_hash=7cd9a76405577e41b52b1be0547419fb1bcf819f2f3d08422b00f31c8356df77
paint_projection_hash=cfc504bd5211f8385e5af0ddce1238d3d38a336096a125d1207d6b2044080461
```

Two independent publications produced byte-identical V2 files:

```text
sha256=074187b6b3303908a2dbc04dff12b124810e2f07e580355d8bb226637f85e516
```

Both files reopened and passed the fresh-process authority check.

## Findings

### Overlapping dynamic chunks are valid

The first run correctly failed because Cut 2 required exactly one containing
dynamic chunk. Real mention 47 at source range `2479..2485` is contained by
chunks 5 and 6.

The clean-room rule now matches the verified V1 rule:

1. retain the exact published chunk rows
2. choose the smallest containing dynamic chunk
3. use source ordinal as the deterministic tie-breaker
4. fail if no exact containing chunk exists

A focused regression test freezes this behavior.

### NLI identity rows are already canonical

All 57 non-`Related` NLI rows compare mentions already bound to the same stable
entity. They are confirmation evidence, not merge candidates. The shadow
adapter therefore publishes zero identity merge candidates rather than
creating self-merges or user review work.

The strict entity producer still rejects same-identity candidates. The adapter
performs the explicit semantic classification and reports the rows separately.

### Paint is not authority

All 1,101 mentions produce graph evidence. The non-overlapping editor
projection contains 1,100 spans. Suppressing one colliding paint span did not
remove its mention or evidence row.

## Frozen Angular comparison boundary

The frozen Angular release manifest identifies a 159,402-byte markdown source,
while this current native authority contains 159,332 bytes. The Angular replay
fixture also reports 129 chunks, while this native authority has 393.

These are not the same replay input, so this run cannot honestly claim
cross-application chunk, entity, or topology parity. Before an exact comparison,
the 70-byte source discrepancy must be resolved or recorded as an intentional
new cohort.

## Stop/go

| Gate | Result |
|---|---|
| Every mention has exact source and chunk binding | Pass: 1,101 / 1,101 |
| Overlapping valid mentions survive authority | Pass |
| Paint cannot delete graph evidence | Pass: 1,101 evidence / 1,100 paint |
| No label-based merge | Pass: 65 direct stable-ID bindings |
| Deterministic publication | Pass: byte-identical repeated outputs |
| Fresh-process reopen | Pass |
| Exact Angular cohort parity | Stop: source identities differ |

