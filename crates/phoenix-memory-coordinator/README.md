# Phoenix dual-face ingestion coordinator

`phoenix-memory-coordinator` is the unregistered Cut 2–4 worker that gives
notes and conversations one bounded publication and recall boundary.

- `IngestDocumentRevision` validates an exact workspace lease, obtains the
  authoritative structural substrate from an injected producer, and preserves
  its dynamic chunks byte-for-byte.
- `RecallTurn` searches exact document chunks and committed conversation turns
  through one required positional lexical index, then returns a bounded,
  provenance-preserving context packet. It cannot mutate coordinator state or
  publish.
- `IngestTurn` commits one explicit turn. Human and assistant turns therefore
  cross the authority boundary separately and only after the host decides they
  are source truth.

All semantic products remain proposed candidates. Unsupported producers have
an explicit `Unsupported` capability row. Benchmark history may be ingested,
but LongMemEval gold answers and gold sessions are rejected at ingress.

## Recall authority

The lexical index is constructed from the same mutable candidate state before
an immutable generation is published. If the corpus is oversized or indexing
fails, the ingestion mutation and publication are rejected together. Runtime
recall never falls back to the old full-history term-overlap scan.

The index contains:

- every authoritative dynamic document chunk;
- every explicitly committed conversation turn;
- stable source and content IDs matching the generation formulas.

Returned proposed semantics retain exact evidence excerpts and proposed status.
They also retain candidate-scoped temporal envelopes with distinct source,
assertion, occurrence, observation, valid, and system clocks. The coordinator
does not collapse unknown time into an unbounded interval and does not promote
temporal observations into accepted truth.

Semantic model duties are explicit: a steerable semantic observer may produce
broad semantic candidates, while a dedicated NLI observer is reserved for NLI
adjudication. Neither lane silently substitutes for the other.
The pending model answer is never indexed because it has not crossed the commit
boundary.

`QpsShadowConfig` remains disabled by default and exists only for frozen
comparison receipts. It is not a second production retrieval arm.

The crate has no GPUI, renderer, database, JSON, or model-runtime dependency.
Production registration remains intentionally deferred.
