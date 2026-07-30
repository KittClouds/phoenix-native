# Phoenix graph generation V1 inventory

Date: 2026-07-29

Purpose: freeze the current V1 contract and enumerate every native consumer
before V2 exists.

## Frozen identity

- Crate: `phoenix-graph-generation`
- Contract: `phoenix.graph-generation/v1`
- Magic: `PHXGG001`
- Version: `1`
- Extension: `.phxgg`
- Maximum generation bytes: 1 GiB
- Section count: 15

V1 source hashes before Cut 0:

| File | SHA-256 |
|---|---|
| `src/build.rs` | `7DC4360BE5716AA54989734C4B2A998AA4E22F57DA7016AD7C1986D581DFEAE3` |
| `src/error.rs` | `D0203BF9B4FFF6A64F17B6227883C9BC9D05B66D2AF0C4A193C2BA4B542D8BC7` |
| `src/format.rs` | `B0BA05876599FDF2D53353471D50DEA6495BAE861747372A697F4C8F1982DD7F` |
| `src/ids.rs` | `73E0E62406F9B533B34503FC8C7FCB70D49C0B4C6DC31671DAE7F0767129E6B2` |
| `src/lib.rs` | `E9557356FEB66D288A6FDACFA299FA035C4CF11B4EB163D24F261018DAF101E7` |
| `src/open.rs` | `F66F32911CACF07ECF28FC29954D33CFB20C8CA7853D0551CA6DF4A6E052F625` |
| `src/tests.rs` | `ADC309CFC3A2E0A6BCC192C229B579A8EFC8E2A7B116BCA67D83A77E2957DDFF` |

Cut 0 must not change any of these files or hashes.

## Sections

| Tag | Section | Record | Authority represented |
|---:|---|---|---|
| 1 | Document | `DocumentRecord` | source |
| 2 | Chunks | `ChunkRecord` | source |
| 3 | Sentences | `SentenceRecord` | source |
| 4 | Spans | `SpanRecord` | source |
| 5 | Strings | raw UTF-8 bytes | shared storage |
| 6 | Entities | `EntityRecord` | canonical binding |
| 7 | Mentions | `MentionRecord` | source evidence |
| 8 | Evidence | `EvidenceRecord` | source evidence |
| 9 | Accepted edges | `AcceptedEdgeRecord` | mixed structural/promoted |
| 10 | Candidate edges | `CandidateEdgeRecord` | semantic candidate |
| 11 | Adjudications | `AdjudicationRecord` | model judgment |
| 12 | Decisions | `DecisionRecord` | durable review |
| 13 | Capabilities | `CapabilityRecord` | producer receipt |
| 14 | Identities | `IdentityRecord` | model/binary identity |
| 15 | Stage receipts | `StageReceiptRecord` | telemetry |

## Limitations that require V2

V1 cannot represent these products as distinct packed pages:

- chapters and paragraphs
- typed relationship candidates
- identity, alias, and coreference candidates
- events
- episodes
- episode memberships
- temporal candidates
- causal candidates
- memory/state candidates
- contextual co-occurrence evidence
- publication receipts

V1 also stores candidate semantics in one generic edge record. Extending its
existing tags or changing record layouts would silently reinterpret deployed
bytes, so V1 remains frozen.

## Production consumers

### `phoenix-app-core`

Cargo dependency:

- `crates/phoenix-app-core/Cargo.toml`

Consumers:

- `src/analysis.rs`
  - writes and opens V1 generations
  - retains verified durable analysis
  - publishes the active generation into kernel state
- `src/atlas_review.rs`
  - reads candidates, adjudications, decisions, identities, capabilities, and
    accepted edges
  - creates a new immutable V1 generation after a review decision
- `src/lib.rs`
  - stores `Arc<VerifiedGraphGeneration>` in resident kernel state
- `src/release_lock.rs`
  - validates counts, promotion state, identities, and generation authority
- `src/scene_rebuild.rs`
  - passes the resident V1 generation into scene compilation
- `src/scene_publication.rs`
  - carries the verified generation through atomic scene publication

V2 Cut 0 adds no dependency here.

### `phoenix-scene-compiler`

Cargo dependency:

- `crates/phoenix-scene-compiler/Cargo.toml`

Consumers:

- `src/compile.rs`
  - reads V1 documents, chunks, entities, mentions, evidence, accepted edges,
    candidates, adjudications, decisions, and strings
  - turns V1 authority into scene and product-index projections
- `src/tests.rs`
  - creates and opens V1 fixtures
  - proves reviewed promotion reaches scene compilation

V2 Cut 0 adds no dependency here.

## Test-only and indirect consumers

- `phoenix-graph-generation/src/tests.rs`
  - V1 writer/open/corruption/decision tests
- scene-compiler tests
  - end-to-end V1 fixture tests
- app-core tests
  - exercise V1 through kernel, review, publication, and release-lock APIs

No GPUI, renderer, workspace, archive, or product-index crate directly depends
on V1.

## V1 invariants retained

- immutable create-new publication
- mmap-backed read-only opening
- typed stable IDs
- fixed section tags and record sizes
- BLAKE3 page and generation hashes
- checked offsets and counts
- fail-closed reference validation
- document/revision/hash binding
- durable decision binding

## Cut 0 proof

V1 remains readable when:

1. None of the seven source hashes changes.
2. The existing `phoenix-graph-generation` test suite passes.
3. Scene compiler V1 tests pass.
4. The V2 crate has no production consumer dependency.
