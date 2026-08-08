# Memory runtime cut V1 receipt

Date: 2026-08-08

Status: implemented, registered in `phoenix-app-core`, scoped qualification
passed, and live restart verified.

## Delivered pipeline

```text
verified PhoenixGraphGenerationV3
  -> exact candidate and evidence catalog
  -> deterministic GLiClass plus ModernBERT-NLI policy proposal
  -> append-only policy decision receipt
  -> immutable current-memory projection
  -> packed active-memory CSR graph
  -> bounded recursive traversal
```

## Append-only policy decisions

`phoenix-memory-runtime` adds create-new `.phxpdecision` receipts with:

- contiguous global sequence lineage;
- per-candidate decision lineage;
- exact V3 generation, candidate, endpoint, and evidence bindings;
- deterministic policy identity;
- bounded semantic-observation identity;
- source-authority class;
- policy action and reason;
- commit, reject, defer, or undo disposition;
- decision time and effective valid time;
- optional replacement target;
- full artifact hash.

Identical logical commands are idempotent. Decision time is receipt metadata and
is excluded from the logical command hash. It remains included in the receipt
identity. Missing, reordered, duplicated, stale, malformed, or corrupt receipts
fail closed during reopen or projection.

## Materialized current memory

Committed decisions materialize into fixed-width `CurrentMemoryRecordV1` rows.
The immutable `.phxmemory` artifact is hash-verified and mmap-readable.

The projection retains:

- candidate and decision receipt identities;
- source and candidate-row bindings;
- original and currently closed valid-time boundaries;
- ledger-system sequence boundaries;
- active, superseded, disputed, historical, candidate-only, or evidence-only
  state;
- replacement lineage.

Supersession closes the earlier memory at the decision's effective time. A query
against an earlier ledger sequence still sees the earlier original validity.
Append-only undo removes the superseding decision from the effective decision
stack and restores the prior projection deterministically.

## Recursive working set

Active projected candidates and stable entity endpoints compile into packed CSR
arrays. Candidate-to-candidate expansion occurs through shared stable entity
nodes. The working set is immutable after construction.

Warm traversal reuses caller-owned marks, queue, and result storage. It performs
no hashing, locking, I/O, model invocation, or allocation. Bounds are fixed at:

- depth `<= 8`;
- visited nodes `<= 4,096`;
- scanned edges `<= 65,536`;
- construction upper bound `8,000,000` nodes and `64,000,000` edges.

Seeds and adjacency are canonicalized, so traversal is stable across equivalent
seed orderings. Hitting a query bound returns an explicit truncated receipt.

## Qualification

The scoped suite proves:

- exact stale-binding rejection before any write;
- idempotent repeated decisions;
- restart-safe ledger reopen;
- corruption rejection;
- missing global receipt detection;
- validity closure and bitemporal historical lookup;
- deterministic undo;
- immutable projection publish and mmap reopen;
- shared-entity recursive expansion;
- deterministic seed ordering;
- explicit traversal truncation;
- 10,000 warm traversal performance smoke with zero capacity growth.

The complete memory slice currently passes 38 tests across contract,
coordinator, semantics, and runtime, scoped Clippy with warnings denied, and an
optimized release build on the isolated target.

## Application registration receipt

- `ResidentMemory` owns one registered policy runtime rooted beside the V3
  authority store.
- Every verified V3 generation materializes and verifies its immutable
  `.phxmemory` projection before becoming the resident publication.
- The Atlas Control runtime lane reports registration, policy sequence, active
  current-memory records, and recursive working-set node/edge counts.
- Live restart used
  `C:\phoenix-bin\memory-runtime-registered-20260808\phoenix-shell.exe` and
  restored workspace document 7 plus scene generation 24.
- The live empty decision ledger produced the expected 176-byte header-only
  projection for ledger sequence 0. Existing candidate status was not silently
  promoted into policy memory.

## Explicit limits

- Real GLiClass and ModernBERT-NLI inference outputs are not yet connected to
  decision commands.
- Policy decisions and recursive queries do not yet have mutation controls in
  the application UI; Atlas Control is read-only for this runtime cut.
- The recursive working set currently contains active semantic candidates and
  stable entity endpoints. Event, episode, causal, and temporal-envelope nodes
  remain the next graph-widening cut.
- Larger-corpus latency and peak-memory qualification remain required before
  production registration.
