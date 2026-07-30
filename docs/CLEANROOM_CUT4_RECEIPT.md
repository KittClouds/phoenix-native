# Clean-room Cut 4 receipt: durable review and publication

Date: 2026-07-29

Status: implemented and scoped verification passed; not registered with the
live Phoenix kernel

## Authority decision

Accepted semantics remain in their typed semantic-candidate pages with status
`Accepted`. They are not appended to `StructuralEdges`, because that page is
source-authoritative. A candidate can be accepted only when the same immutable
generation contains its evidence-bound `DecisionRecord`.

Human-readable reasons remain in the immutable external decision receipt. The
V2 `DecisionRecord` carries the stable receipt-derived ID, action, status,
candidate ID, evidence hash, document revision, registry revision, and producer
generation without extending the frozen V2 page schema or rewriting the shared
source string slab.

## Added contract

`phoenix-semantic-review` provides:

- a dense `ReviewCatalog` indexed by typed `CandidateId`
- exact row and evidence hashes on every lens-neutral review binding
- append-only, mmap-verified `DecisionReceiptHeaderV1` artifacts
- idempotent command hashes that deliberately exclude wall-clock time
- per-candidate receipt chains for accept, reject, defer, and undo
- immutable reviewed V2 generation publication
- immutable sequenced authority records
- rollback by appending a new authority record pointing to the prior verified
  generation

The Story V1 adapter derives its catalog from verified V2 pages. It hashes the
exact packed candidate row plus every candidate-evidence binding and referenced
evidence row. It never compares labels.

## Exact projection rules

- A decision may be created only against the active generation hash and exact
  candidate/evidence hashes supplied by the current catalog.
- Repeating the same action and reason returns the existing receipt without a
  write, including after reopening the ledger.
- A later generation reuses a decision when document authority, candidate row,
  evidence, lens vocabulary, registry revision, and producer generation still
  match exactly.
- A changed or missing binding projects the old decision as `Superseded`; the
  current candidate remains `Proposed`.
- Source-authoritative page hashes remain byte-identical.
- Publication creates a new V2 file with a new generation hash and appends one
  publication receipt.

## Atomicity and rollback

Generation files and decision receipts use create-new files, `sync_all`, and an
atomic rename from a same-directory pending file. Authority records are never
overwritten. The current authority is the highest fully verified, hash-chained
record. A crash before the authority append can leave only an unreferenced
generation, never a partially active one.

Rollback appends a new authority record whose active generation is the previous
verified generation. Both old and new generation artifacts remain immutable
and readable for audit.

## Proof

The scoped Story V1 integration test proved:

- one exact relationship acceptance
- a second identical command returned the same receipt
- the ledger reopened from a fresh owner with the decision and reason intact
- the reviewed generation carried an accepted candidate and matching decision
  receipt
- `StructuralEdges` retained its exact source page hash
- reranking the same stable candidate changed its row hash, superseded the old
  decision, and left the current row proposed
- an explicitly stale command wrote no receipt
- authority advanced from source to reviewed generation
- rollback appended authority sequence 3 and restored the source generation
- every generation file remained present

## Explicit limits

- Cut 4 is not registered in `phoenix-app-core`.
- The live application was not launched or restarted.
- The scene compiler does not consume accepted V2 semantics until Cut 5.
- The review ledger is append-only; compaction and historical query indexes are
  later serving concerns, not correctness prerequisites.
