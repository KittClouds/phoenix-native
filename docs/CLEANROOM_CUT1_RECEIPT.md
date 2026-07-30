# Clean-room Cut 1 structural-substrate receipt

Date: 2026-07-29

Verdict: pass for the isolated V2 structural boundary

Cut 2 amendment, 2026-07-29: the V2 directory gained required empty
candidate-evidence-binding and canonical-entity-binding pages before production
registration. Structural record bytes, stable IDs, source-coordinate hashes,
and the compiler's borrowed structural view are unchanged. Whole-generation
hashes necessarily changed because the frozen directory is part of the
generation hash.

## Scope

Cut 1 adds:

- `phoenix-document-producer`
- the V2 packed generation writer
- document, chapter, paragraph, sentence, dynamic-chunk, span, and structural
  edge pages
- exact content, cohort, source-coordinate, page, and generation hashes
- atomic publication and mmap-backed durable reuse
- a borrowed compiler structural view

No application, kernel command, database, workspace document, model artifact,
scene archive, or live authority registration changed. Phoenix Native was not
launched.

## Authority path

```text
verified native chunker
  -> PhoenixStructuralSubstrateV1
  -> phoenix-document-producer
  -> packed PhoenixGraphGenerationV2 pages
  -> one-time mmap verification
  -> VerifiedStructuralSource borrowed by the compiler
```

The producer does not port or invoke Angular behavior. It consumes the existing
verified native structural artifact. Chunk start/end, sentence range,
paragraph range, chapter index, token count, and content hash are copied
exactly once into the packed chunk page.

`VerifiedStructuralSource` exposes borrowed typed pages only. It deliberately
has no source-text field and no paragraph/chunk construction API, so the V2
compiler path cannot silently split the document again. The older V1 compiler
path remains unchanged until its later explicit cutover.

## Determinism and persistence

- Stable IDs are domain-separated BLAKE3 identities over exact source
  authority and coordinates.
- Completed vectors are streamed as borrowed byte slices into aligned pages.
- Every required V2 page exists; unsupported semantic pages are empty and
  described by capability receipts.
- Publication first writes an incomplete header, syncs all pages, then seals
  the complete header.
- The final path is content/cohort bound and published with an atomic rename.
- Existing artifacts are mmap-opened and fully verified before reporting
  `durable_verified`.
- Persisted page receipts contain no elapsed time or wall-clock value, so
  identical authorities produce identical bytes.

## Verification

Scoped target:

```text
D:\phoenix-target-cleanroom-structural-v2
```

Focused tests prove:

- identical sources produce identical generation hashes, page hashes, IDs,
  and structural bytes
- exact dynamic chunk rows reach the compiler-side borrowed view
- stale source text cannot reuse or publish an artifact
- a separate fresh process reopens the same generation and reports
  `durable_verified`
- corrupt pages and wrong authority fail closed
- V1 graph-generation tests remain green

Commands:

```text
cargo fmt --all -- --check
cargo test -p phoenix-analysis-contract -p phoenix-graph-generation \
  -p phoenix-graph-generation-v2 -p phoenix-document-producer \
  -p phoenix-scene-compiler --no-fail-fast
cargo clippy -p phoenix-graph-generation-v2 -p phoenix-document-producer \
  -p phoenix-scene-compiler --all-targets -- -D warnings
```

Results:

- analysis contract: 4 passed
- document producer: 5 passed, including a child-process durable-reuse probe
- V1 graph generation: 8 passed
- V2 graph generation: 9 unit plus 2 static-oracle tests passed
- scene compiler: 13 passed
- focused total: 41 passed, 0 failed
- rustfmt: passed
- warnings-denied Clippy: passed
- all seven frozen V1 source hashes: unchanged

## Stop/go

| Gate | Result |
|---|---|
| Same source gives identical IDs and page hashes | Pass |
| Exact dynamic chunks reach downstream compiler input | Pass |
| V2 compiler performs zero paragraph/chunk reconstruction | Pass: structural view has only borrowed verified pages |
| Fresh-process durable reuse | Pass: child-process probe reports `durable_verified` |
| V1 rollback boundary | Pass: V1 code remains unchanged and available in test scope |

## Deliberately not claimed

- V2 is not registered with the kernel or live application.
- The existing V1 live compiler has not been removed.
- Entity, evidence, story, review, or projection pages are not implemented by
  this producer.
- No live-app, model, exact-cohort, or performance claim is made.
