# Phoenix Native clean-room backend plan

Date: 2026-07-29

Status: Cuts 0 through 4 complete; V2 remains outside live application authority

Completed artifacts:

- `CLEANROOM_V1_INVENTORY.md`
- `CLEANROOM_CONTRACT_V2.md`
- `LEGACY_RULE_DISPOSITION.md`
- `CLEANROOM_CUT0_RECEIPT.md`
- `CLEANROOM_CUT1_RECEIPT.md`
- `CLEANROOM_CUT2_RECEIPT.md`
- `CLEANROOM_CUT3_RECEIPT.md`
- `CLEANROOM_CUT3_SEMANTIC_LENS_RECEIPT.md`
- `CLEANROOM_CUT4_RECEIPT.md`
- `crates/phoenix-graph-generation-v2`
- `crates/phoenix-document-producer`
- `crates/phoenix-entity-producer`
- `crates/phoenix-semantic-lens`
- `crates/phoenix-semantic-review`
- `crates/phoenix-story-producer`

The V2 crate is deliberately unregistered from `phoenix-app-core` and the
application. The scene compiler depends only on its borrowed structural-page
view, which has no source-text or reconstruction API. V2 is not live
application authority yet.

## Decision

Phoenix Native is not restarting from an empty repository, and it is not
migrating the Angular backend file by file.

The correct strategy is:

> Retain the verified native substrate, rebuild the semantic producer system
> behind new packed Rust contracts, and use Angular code plus persisted data
> only as a read-only behavioral specification.

This is a ground-up backend rewrite inside the already-clean native
architecture.

## Why this boundary

The native application already has valuable clean-room infrastructure:

- GPUI shell and Velotype editor
- kernel-owned workspace and document leases
- durable workspace CRUD
- bounded commands and events
- cancellation and shutdown
- canonical Atlas registry
- durable review decisions
- resident mmap-backed scene archives
- atomic scene and product-index publication
- one resident graph generation
- five-manifold renderer and reusable GPU allocations
- fail-closed archive opening
- native graph controls and Atlas Control surfaces

Discarding those systems would repeat work without removing the actual design
problem.

The missing or unqualified part is the semantic producer boundary:

- typed relationships
- events and timeline
- episodes and memberships
- temporal claims
- causal claims
- memory and state
- identity, aliases, and coreference
- exact capability and provenance receipts

Those products should be rebuilt rather than ported because the Angular stack
mixed source authority, semantic claims, compiler facts, projections,
compatibility paths, and UI state.

## Migration answer

### We do not migrate

- Angular or TypeScript source files
- Angular services or dependency-injection structure
- Tauri RPC graph freight
- browser packets
- OPFS runtime schemas
- Overgraph row wrappers
- Graph Model V2 object graphs
- lane-root UI models
- compatibility builders
- cached graph snapshots
- ten-field JSON scene blobs
- TypeScript projection reconstruction
- automatic semantic-promotion rules
- global-registry label matching

### We translate from specification

These concepts are redefined as Rust-native contracts after their behavior is
written down:

- relation-family vocabulary
- entity-family vocabulary
- source-coordinate conventions
- exact dynamic chunk invariants
- mention and evidence identity rules
- event and episode evidence requirements
- temporal, causal, and memory-state candidate shapes
- review status vocabulary
- scope and projection policies
- expected topology-family counts for frozen fixtures

No translated type retains Angular storage or lifecycle semantics.

### We may import once

A later, explicit offline importer may bring across user-owned durable data:

- document text
- folders and note identity
- manual entity tags
- user palette choices
- aliases confirmed by the user
- durable accept/reject/defer decisions when their evidence binding still
  matches exactly

The importer must emit native receipts and must never import a canvas snapshot
as canonical graph truth.

### We retain natively

- `phoenix-app-core`
- `phoenix-workspace`
- `phoenix-analysis-contract`
- `phoenix-graph-generation` as the V1 reader and compatibility test fixture
- `phoenix-scene-contract`
- `phoenix-scene-archive`
- `phoenix-scene-product-index`
- `phoenix-scene-publisher`
- `phoenix-scene-compiler`
- `graph-render-wgpu`
- the GPUI shell, editor, Atlas registry, Atlas Control, and decision ledger

Retaining a crate does not freeze every implementation detail. It means its
authority boundary remains valid while internals evolve behind tests.

## Authority model

The backend publishes four different classes of data.

### 1. Source authority

These products may be authoritative when bound to the exact document:

- document record
- chapter, paragraph, sentence, chunk, and span records
- mention records
- evidence anchors
- canonical entity bindings
- user-authored structural facts

### 2. Semantic candidates

These are candidate-only until explicitly promoted:

- identity and alias suggestions
- typed relationships
- event interpretations
- episode assignments
- temporal relations
- causal relations
- memory and state
- generic NLI evidence
- contextual co-occurrence

### 3. Durable decisions

Accept, reject, defer, and undo actions are immutable receipts bound to:

- candidate ID
- evidence IDs
- document ID, revision, and hash
- registry revision
- producer generation
- model and binary identities

Only a valid accepted decision can publish a semantic candidate into accepted
topology.

### 4. Projections

These are derived, replaceable read models:

- canvas nodes and edges
- episode hierarchy views
- CAPS semantic shells
- prepared straight, curved, and bundled paths
- labels and collision priority
- manifold positions
- scope masks
- review masks
- relation masks
- guides and route overlays

Visibility never grants authority.

## Target workspace shape

The exact names can be adjusted during Cut 0, but ownership should resemble:

```text
phoenix-native/
  crates/
    phoenix-source-model/
      typed source coordinates and immutable source records

    phoenix-document-producer/
      document, chapter, paragraph, sentence, chunk and span production

    phoenix-entity-producer/
      mentions, evidence, canonical bindings and identity candidates

    phoenix-semantic-lens/
      namespaced semantic vocabularies, endpoint policy and review origins

    phoenix-story-producer/
      relationships, events, episodes, temporal, causal and memory candidates

    phoenix-producer-coordinator/
      bounded scheduling, cancellation, capability matrix and receipts

    phoenix-graph-generation/
      existing V1 reader and frozen tests

    phoenix-graph-generation-v2/
      packed mmap format for complete clean-room products

    phoenix-scene-compiler/
      generation-to-projection compiler; no semantic discovery

    phoenix-app-core/
      kernel orchestration and atomic publication
```

The renderer, GPUI shell, and workspace crates must not depend on producer
implementations. They consume verified contracts.

## Packed generation V2

`PhoenixGraphGenerationV1` is already versioned and deployed. It must not be
silently changed to mean more than its current 15 sections.

Introduce `PhoenixGraphGenerationV2` with independently verifiable pages:

1. Generation header and section directory.
2. String slab.
3. Documents.
4. Chapters and paragraphs.
5. Sentences.
6. Dynamic chunks.
7. Spans.
8. Canonical entities.
9. Mentions.
10. Evidence anchors.
11. Authoritative structural edges.
12. Typed relationship candidates.
13. Identity/alias/coreference candidates.
14. Events.
15. Episode candidates.
16. Episode memberships.
17. Temporal candidates.
18. Causal candidates.
19. Memory/state candidates.
20. Contextual co-occurrence evidence.
21. NLI adjudications.
22. Durable decisions.
23. Producer capabilities.
24. Model and binary identities.
25. Stage and queue receipts.
26. Promotion and publication receipts.

Representation rules:

- dense arrays and offsets
- stable typed `u64` IDs
- compact flags and enums
- shared UTF-8 slab
- no graph-sized `String`
- no hash maps in the persisted serving format
- mmap opening with one-time page verification
- BLAKE3 hashes for header, directory, and pages
- bounded record counts and byte sizes
- checked offsets and arithmetic
- corrupt, stale, mismatched, unsupported, and oversized errors
- no JSON
- no label-based identity

## Producer design

### Structural producer

Input:

- one immutable document lease
- exact UTF-8 bytes
- document identity, revision, and hash

Output:

- document, chapter, paragraph, sentence, chunk, and span pages
- exact source-coordinate map
- structural receipt

Rules:

- dynamic chunks are computed once
- downstream producers borrow the frozen chunk/span tables
- the scene compiler cannot reconstruct paragraphs or chunks
- unchanged source hashes permit durable verified reuse

### Entity producer

Input:

- immutable source pages
- model identity and configuration
- user-tag ledger

Output:

- mention page
- evidence page
- canonical binding page
- identity/alias/coreference candidates

Rules:

- paint spans are a separate projection
- overlapping valid evidence remains in graph authority
- canonical identity requires a stable ID or coordinator decision
- labels never merge entities

### Story producer

Input:

- source pages
- mentions and evidence
- canonical identities
- qualified model outputs

Output:

- typed relationship candidates
- events
- episode candidates and memberships
- temporal candidates
- causal candidates
- memory/state candidates

Rules:

- every row is evidence-bound
- weak co-occurrence is contextual evidence only
- generic `Related` is not a typed relationship
- episode membership is candidate-only
- no automatic semantic promotion
- producer capabilities distinguish unsupported, skipped, failed, cancelled,
  reused, and produced

### Producer coordinator

The coordinator owns:

- a bounded command queue
- a latest-wins document request
- one cancellation generation
- explicit stage dependencies
- durable-reuse lookup
- model runtime leases
- page assembly
- atomic generation publication

It does not own:

- workspace text
- a second entity registry
- renderer state
- UI state
- a mutable global graph

## Data flow

```text
kernel document lease
  -> structural producer
  -> frozen source pages
  -> entity producer
  -> story producers
  -> candidate and evidence pages
  -> durable decision projection
  -> PhoenixGraphGenerationV2
  -> atomic generation publication
  -> scene compiler
  -> .psa + .pspi
  -> resident renderer and Atlas Control
```

Ownership moves forward. Stages exchange immutable borrowed views or owned
packed buffers, not graph-sized clones.

## Implementation cuts

### Cut 0 — freeze clean-room contracts

Work:

- Adopt the static Angular expected-topology manifest as a test oracle, not a
  runtime dependency.
- Inventory every current V1 section and native consumer.
- Freeze the V2 typed IDs, authority classes, status enums, page directory, and
  errors.
- Mark every legacy-derived rule as retained, rejected, or redesigned.

Stop/go:

- No unresolved source-vs-semantic-vs-projection ambiguity.
- V1 remains readable and unchanged.
- V2 can represent every expected topology family.
- No implementation imports an Angular type.

Rollback:

- Documentation and unused contract crate only.

### Cut 1 — structural substrate

Work:

- Build document, chapter, paragraph, sentence, dynamic-chunk, and span pages.
- Reuse only the verified native chunking behavior.
- Publish exact source-coordinate and content-hash receipts.

Stop/go:

- Same source produces identical IDs and page hashes.
- Exact dynamic chunk records reach every downstream consumer.
- Zero paragraph or chunk reconstruction in the compiler.
- Fresh-process durable reuse reports `durable_verified`.

Rollback:

- V1 producer remains available in test scope.

### Cut 2 — entities and evidence

Work:

- Produce mentions, evidence anchors, canonical bindings, and identity
  candidates.
- Separate graph evidence from non-overlapping editor paint spans.
- Merge user tags through stable identities and coordinator decisions.

Stop/go:

- Every mention has exact source and chunk bindings.
- Overlapping valid mentions survive in authority.
- Paint cannot delete graph evidence.
- No label-based merge.

Rollback:

- Retain structural V2 pages; disable the entity producer registration.

### Cut 3 — story candidates

Work:

- Implement typed relationships, events, episode candidates and memberships,
  temporal, causal, and memory/state producers.
- Start with deterministic evidence rules.
- Add model-backed ranking only behind the same candidate contracts.

Stop/go:

- Every output is evidence-bound.
- Unsupported capabilities are explicit.
- Zero automatic promotion.
- Episodes are real candidate records; no synthetic `Episode 1`.

Rollback:

- Disable individual producer registrations without invalidating source pages.

### Cut 3A-D — semantic lens boundary

Work:

- Keep authority, evidence, candidate status, capabilities, and decisions in a
  small domain-neutral kernel.
- Bind semantic vocabulary to a versioned namespace and immutable vocabulary
  hash.
- Declare Story V1 as the first lens without changing V2 page layouts or
  candidate IDs.
- Prove the seam using one non-production research witness.
- Bind future review actions to candidate, lens, vocabulary, document,
  evidence, producer generation, and registry revision.

Stop/go:

- Story V1 candidate pages remain byte-identical for identical inputs.
- V2 remains unchanged.
- Unknown codes, wrong endpoint types, corrupt packs, and mismatched lens
  identities fail closed.
- Two lenses cannot collide on identical candidate material.
- The review boundary does not import or pattern-match story enums.

Rollback:

- Remove the additive lens crate and Story V1 declaration. No data migration
  or generation rollback is required.

### Cut 4 — durable review and publication

Work:

- Project exact matching decisions onto the new generation.
- Mark changed bindings superseded.
- Publish accepted semantic edges atomically as a new generation.
- Preserve immutable old generations for rollback and audit.

Stop/go:

- Decisions survive restart.
- Actions are idempotent.
- Stale candidates cannot be decided.
- Accepted topology always has a decision receipt.

Rollback:

- Repoint the authority manifest to the previous verified generation.

### Cut 5 — native scene compiler V2

Work:

- Compile accepted source truth and receipt-backed semantics.
- Compile candidates into review masks and overlays.
- Compile episode hierarchy, continuity, guides, paths, and manifold pages as
  projections.
- Remove production synthetic episode creation.

Stop/go:

- Renderer receives no second graph model.
- Visibility and filtering never mutate topology.
- Exact stable IDs reach Atlas, editor, and graph surfaces.
- One resident generation.

Rollback:

- V2 publication remains valid while the V1 scene compiler stays test-only.

### Cut 6 — Atlas Control integration

Work:

- Show producer capabilities, stage progress, evidence, candidates, decisions,
  superseded rows, and publication history.
- Connect evidence selection to exact editor spans.
- Connect candidates to verified graph nodes.

Stop/go:

- Every control dispatches a bounded kernel command.
- No producer work occurs on the GPUI thread.
- No hidden acceptance.
- Empty, running, failed, cancelled, unsupported, stale, and current states are
  distinct.

Rollback:

- Remove the V2 UI adapter without changing generations or decisions.

### Cut 7 — optional user-data importer

Work:

- Import only explicitly approved user-owned records.
- Rebind by exact source hash and stable IDs.
- Emit an import receipt for every accepted record.

Stop/go:

- No canvas snapshot or cached projection becomes authority.
- Unmatched records remain quarantined.
- Import is idempotent and reversible by generation rollback.

Rollback:

- Repoint to the pre-import generation.

### Cut 8 — cutover

Work:

- Route production rebuild through the V2 producer coordinator.
- Remove production registration of `phoenix-legacy-bridge`.
- Keep V1 readers only for frozen fixtures and recovery inspection.
- Compile out Angular/legacy adapters.

Stop/go:

- Zero fallback count.
- Zero JSON graph freight.
- No production fixture.
- No production legacy adapter.
- Cold restart reopens the same verified generation.

Rollback:

- Revert the release commit as a unit; never add a runtime compatibility switch.

### Cut 9 — release and performance lock

Work:

- Freeze exact document, binary, model, configuration, generation, and page
  identities.
- Run deterministic repeated production, restart, cancellation, corruption,
  queue-pressure, review, drawer, manifold, interaction, memory, and device
  recovery gates.

Hard gates:

- exact source, chunk, entity, mention, and evidence parity
- candidate-only semantics until receipt-backed promotion
- stable IDs when inputs are unchanged
- one resident generation
- zero fallback count
- zero JSON graph freight
- UI interaction p95 at or below 16.7 ms
- warm manifold-switch CPU p95 at or below 8 ms
- bounded queue high-water marks
- stable memory plateau
- live Phoenix Native application is final authority

## What happens to the legacy bridge

`apps/phoenix-legacy-bridge` is quarantined immediately:

- no production registration
- no kernel dependency
- no release dependency
- no authority to publish

It may remain temporarily as:

- a frozen diagnostic harness
- an old-artifact inspector
- a record-level comparison utility

It is deleted or moved out of the production workspace after V2 cutover.

## What happens to Graph Model V2 and the eleven lanes

The old eleven lanes are useful as a capability checklist, not as an
architecture to port.

Native maps them into explicit products:

| Old lane | Native destination |
|---|---|
| document spine | source document and structural-edge pages |
| chunk spine | dynamic chunk and membership pages |
| entity anchor | canonical entity and mention pages |
| relationship fact | typed relationship candidate page |
| co-occurrence weak | contextual evidence page |
| event identity | event candidate page |
| temporal fact | temporal candidate page |
| causal fact | causal candidate page |
| memory state | memory/state candidate page |
| entity linker | identity/linker candidate page |
| anchor evidence | evidence-anchor page |

There are no runtime lane-root objects unless a projection specifically needs
them. Capability receipts replace lane UI as system truth.

## Performance design

The production path should be:

```text
mutable dense construction
  -> compaction
  -> immutable packed generation
  -> mmap read-only traversal
```

Rules:

- dense `Vec<T>` construction
- `hashbrown` maps only during construction
- integer IDs instead of pointer graphs
- arenas for generation-scoped temporary records where measurement supports it
- `memchr` and SIMD-friendly byte scans in source analysis
- batch model inference
- bounded crossbeam queues
- immutable `Arc<[T]>` only when mmap ownership is not appropriate
- no graph-sized clones
- no per-record locks
- no formatting or tracing in hot loops
- allocation and copy counters in receipts
- DHAT and instruction-count gates before allocator or unsafe tuning

## Explicitly rejected

- migrating Angular files into native
- wrapping Angular services in Rust
- treating old OPFS rows as the new database
- loading Angular JSON into the renderer
- preserving Graph Model V2 as canonical authority
- copying legacy automatic-promotion behavior
- reconstructing chunks in the scene compiler
- one mutable graph shared across producers and UI
- runtime V1/V2 fallback
- label-based identity
- fixture success reported as production parity

## First implementation move

The first implementation cut is not a producer.

It is Cut 0:

1. Freeze `PhoenixGraphGenerationV2`.
2. Freeze authority and status semantics.
3. Map every current V1 reader and consumer.
4. Add open/validation tests for every new page.
5. Prove V1 remains unchanged.

Only after that contract is accepted should structural production begin.
