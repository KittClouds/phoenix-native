# Phoenix memory runtime V1

`phoenix-memory-runtime` is the unregistered runtime cut connecting verified V3
semantic candidates to deterministic memory truth and bounded recursive graph
reasoning.

```text
verified PhoenixGraphGenerationV3
  -> exact candidate/evidence catalog
  -> deterministic policy proposal
  -> append-only policy decision receipt
  -> immutable current-memory projection
  -> packed CSR working set
  -> bounded recursive traversal
```

## Policy decision ledger

Every command binds:

- the exact V3 generation hash;
- the packed candidate row and ordered endpoint bindings;
- the ordered evidence bindings and exact evidence rows;
- the deterministic policy identity and constitutional policy proposal;
- the bounded semantic-observation hash and source-authority class;
- an optional replacement target for close-and-replace or supersession;
- explicit decision and effective times.

Receipts are create-new, file-synchronized, mmap-verified artifacts. A global
receipt chain catches missing or reordered files. Per-candidate chains preserve
decision lineage and support append-only undo. Repeating the same command is
idempotent and reuses the existing receipt.

The ledger rejects stale generation bindings, malformed action/reason pairs,
missing replacement targets, non-constitutional flags, corrupt receipts, and
broken restart chains.

## Current-memory projection

The projection is derived; it is never an independent source of truth. It
materializes committed decisions into packed `CurrentMemoryRecordV1` rows and
publishes an immutable `.phxmemory` artifact.

The projection preserves both valid time and decision-system sequence. A later
supersession closes the current validity interval without rewriting what an
earlier system sequence knew. Historical source validity is retained beside the
closed interval. Undo removes the superseding decision from the effective stack
and deterministically restores the prior current memory.

No elapsed-time decay exists. Memory changes only through append-only decisions,
validity transitions, and explicit supersession.

## Recursive working set

The working set compacts active memory candidates and their stable entity
endpoints into immutable CSR arrays:

```text
nodes:        Box<[WorkingNodeRecordV1]>
edge_offsets: Box<[u32]>
edges:        Box<[WorkingEdgeRecordV1]>
```

Construction may allocate and use temporary hash maps. Warm traversal does not:
it reuses caller-owned marks, queue, and result vectors. Query limits are
constitutional:

- maximum depth: `8`;
- maximum visited nodes: `4,096`;
- maximum scanned edges: `65,536`.

Seeds are canonicalized to stable node order. Adjacency is sorted and deduped.
Traversal is therefore deterministic for the same seed set and bounds, even
when caller seed order differs. Bounds truncate with a receipt rather than
silently widening the query.

This is recursive graph traversal, not a REPL and not an autonomous agent loop.
The caller chooses seeds and limits; the runtime walks only the verified current
memory substrate.

## Registration boundary

This crate is not registered in `phoenix-app-core` and does not change the live
application. Production wiring waits for larger-corpus projection benchmarks,
crash/restart qualification, provenance-completeness receipts, and live recall
integration.
