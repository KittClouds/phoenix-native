# Clean-room Cut 5 receipt — native scene compiler V2

Date: 2026-07-29

## Boundary

Cut 5 adds a compiler from a verified, mmap-backed
`PhoenixGraphGenerationV2` and its exact `ReviewCatalog` to the existing atomic
scene publication pair:

```text
verified .pgg2 pages
  + exact review bindings and decision rows
  -> transient packed publication buffers
  -> one immutable .psa + .pspi pair
  -> one mmap-backed ResidentScene
```

The renderer still opens the archive pages directly. It does not receive or
retain a `GraphSnapshot`, browser packet, JSON representation, or second
authoritative graph model.

The kernel registration is intentionally deferred to Cut 6. The currently
running Phoenix process was not restarted or modified during this cut.

## Authority rules

- Document, chapter, paragraph, sentence, dynamic-chunk, entity, evidence, and
  structural-edge IDs are preserved exactly from the V2 generation.
- Dynamic chunk rows are borrowed through `VerifiedStructuralSource`; the scene
  compiler has no source-text input and cannot reconstruct chunks.
- Display-only evidence attachments and semantic overlay edges use
  domain-separated deterministic projection IDs.
- Every typed candidate must have an exact catalog location and review binding.
- `Accepted`, `Rejected`, and `Deferred` rows additionally require an exact
  decision row bound to the evidence hash, document revision, registry
  revision, producer generation, action, and status.
- Proposed rows compile only into proposed masks and overlays.
- Contextual co-occurrence remains proposed contextual evidence.
- Superseded rows remain addressable but are hidden by the default review mask.
- No label matching participates in identity or endpoint resolution.

## Projection rules

- Source structural topology and receipt-backed accepted semantic edges are
  immutable scene topology.
- Candidate semantics use product-index review masks. Visibility changes query
  those masks and cannot rewrite node, edge, topology, or position pages.
- All five manifold position pages are emitted over the same node identities.
- CAPS consumes real episode and membership records when present.
- The publisher derives bounded guide pages and straight, curved, and bundled
  prepared path pages for each manifold.
- Entity-to-node mappings and evidence source references use exact stable IDs.

## Synthetic episode removal

The legacy compiler no longer creates `Episode 1`. When no verified episode
authority exists, its chunks attach directly to the document. The V2 compiler
creates episode nodes only from `EpisodeRecord` rows and attaches accepted
memberships only when their decision authority is exact.

## Proof

Focused tests prove:

- An empty V2 episode page publishes no episode node or `Episode 1` label.
- Source structural edge IDs survive compilation.
- Entity mappings and evidence source coordinates survive publication.
- All five manifold pages open with guide and prepared-path pages.
- One published generation reopens as one resident scene.
- An accepted candidate without a decision receipt fails closed.
- The same exact candidate in proposed state compiles as one proposed overlay
  and zero accepted semantic edges.

Verification commands use `D:\phoenix-target-cleanroom-scene-v2`.

## Rollback

The V2 compiler and publication are additive. Existing V1 archives remain
readable. The current kernel keeps its V1 registration until the V2 generation
owner is connected in the cutover slice; that path no longer manufactures a
synthetic episode. After cutover it can be reduced to fixture/test scope. No
data migration or in-place archive mutation is required.
