# Memory Cut 4 — bounded mixed-source retrieval and context

Status: **implemented and scoped verification passed**

## Authority path

```text
verified mutable ingestion state
  -> prepare one positional lexical index
  -> publish immutable PhoenixGraphGenerationV3
  -> bind the index to the published generation hash
  -> RecallTurn searches exact chunks and committed turns
  -> bounded context packet with source IDs, content IDs, spans and proposals
```

There is no runtime database, JSON freight, label join, answer ingestion, or
fallback full scan.

## What Cut 4 now guarantees

- Workspace documents and conversations share one recall corpus.
- Document rows are the producer's exact dynamic chunks; they are not rebuilt
  from paragraphs.
- Conversation rows are committed turns; pending responses never enter the
  index.
- Index construction completes before publication. An oversized or invalid
  index rejects the mutation and leaves the previous generation current.
- Corpus rows use deterministic source, chunk, conversation and turn IDs that
  match `PhoenixGraphGenerationV3`.
- Context bytes and item counts are hard bounded.
- Proposed semantic rows are included only with exact source evidence and
  remain visibly `Proposed`.
- Every response carries query, corpus and resident-generation hashes plus
  scorer work/timing telemetry.
- The positional engine reuses caller-owned scratch on warm requests.

## Capability truth

The production authority in this cut is the native positional lexical path:

```text
phoenix.lexical.positional/v1
```

ANN is not falsely reported as active. A derived
`PhoenixEmbeddingPagesV1` sidecar and an EmbeddingGemma CLI proof now bind
vectors to exact V3 dynamic-chunk and committed-turn rows. The sidecar is
generation-, source-set-, model-, asset-, and configuration-hash bound and is
verified once when opened through mmap.

That proof does not register vector recall as production authority. The
positional lexical path remains the only required Cut 4 retrieval path until
the ANN query path, budgets, and coordinator registration receive their own
explicit cut. Recall never fabricates vectors or silently substitutes a scorer.

Graph adjacency, temporal range indexes and namespace/review/lens masks are
also deliberately deferred and pinned in
`MEMORY_SERVING_INDEX_OPTIMIZATION_BACKLOG.md`.

## Verification

The scoped release suite proves:

- exact document-chunk and conversation-turn retrieval in one request;
- irrelevant rows are not returned;
- pending-answer exclusion;
- candidate-only semantic context with evidence;
- generation/query/corpus receipt binding;
- index-bound rejection leaves the prior generation unchanged;
- gold-answer and gold-session ingress rejection;
- deterministic generation reuse;
- bounded queue cancellation;
- optional shadow comparison cannot alter authority.

Production app registration, ANN vector retrieval, and the later serving
indexes are not claimed by this cut. The verified embedding sidecar format,
model runner compatibility, deterministic CLI cohort, and mmap reopen proof
are implemented separately in `MEMORY_EMBEDDING_PAGES_GEMMA_PROOF.md`.
