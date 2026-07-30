# Phoenix graph generation V2 frozen contract

Date: 2026-07-29

Status: frozen structural writer/reader contract; not registered in the production application

## Identity

- Crate: `phoenix-graph-generation-v2`
- Contract: `phoenix.graph-generation/v2`
- Magic: `PHXGG002`
- Version: `2`
- Extension: `.phxgg2`
- Header: 256 bytes
- Page descriptor: 128 bytes
- Required pages: 28
- Page alignment: 64 bytes
- Maximum generation: 1 GiB
- Maximum records per page: 16,000,000
- Schema-directory hash:
  `3d0486e6e33145fcebbca97186edd2c55d6d5ae76f0e2213106933414e54a66a`

The schema-directory hash covers every page tag, record size, alignment, and
field-order/type signature. Any layout change must be a new version.

## Page directory

Every page is required even when its record count is zero. This makes
unsupported, skipped, and empty distinguishable through the capability page
instead of changing the physical directory.

| Tag | Page | Authority | Bytes / align |
|---:|---|---|---:|
| 1 | Strings | source authoritative | 1 / 1 |
| 2 | Documents | source authoritative | 80 / 8 |
| 3 | Chapters | source authoritative | 64 / 8 |
| 4 | Paragraphs | source authoritative | 56 / 8 |
| 5 | Sentences | source authoritative | 64 / 8 |
| 6 | Chunks | source authoritative | 64 / 8 |
| 7 | Spans | source authoritative | 80 / 8 |
| 8 | Entities | source authoritative | 56 / 8 |
| 9 | Mentions | source authoritative | 56 / 8 |
| 10 | Evidence | source authoritative | 48 / 8 |
| 11 | Structural edges | source authoritative | 40 / 8 |
| 12 | Typed relationship candidates | semantic candidate | 80 / 8 |
| 13 | Identity candidates | semantic candidate | 72 / 8 |
| 14 | Events | semantic candidate | 56 / 8 |
| 15 | Episodes | semantic candidate | 56 / 8 |
| 16 | Episode memberships | semantic candidate | 40 / 8 |
| 17 | Temporal candidates | semantic candidate | 72 / 8 |
| 18 | Causal candidates | semantic candidate | 72 / 8 |
| 19 | Memory/state candidates | semantic candidate | 104 / 8 |
| 20 | Contextual evidence | contextual evidence only | 56 / 8 |
| 21 | NLI adjudications | semantic candidate | 56 / 4 |
| 22 | Decisions | decision receipt | 120 / 8 |
| 23 | Capabilities | runtime receipt | 48 / 8 |
| 24 | Model identities | runtime receipt | 120 / 8 |
| 25 | Stage receipts | runtime receipt | 80 / 8 |
| 26 | Publication receipts | runtime receipt | 112 / 8 |
| 27 | Candidate evidence bindings | semantic candidate | 48 / 8 |
| 28 | Canonical entity bindings | source authoritative | 32 / 8 |

Candidate records address a contiguous range in page 27, not in the general
evidence page. This permits multiple overlapping candidates to share exact
evidence records without requiring those evidence rows to be duplicated or
laid out adjacently.

Page 28 records the source stable identity, resulting canonical identity,
source provenance, and coordinator decision ID. Equal labels never create a
binding.

No V2 generation page has projection authority. Manifold positions, scene
geometry, visibility masks, labels, guides, and prepared paths remain in the
separately versioned scene archive and product index.

## Typed identities

Stable `u64` newtypes:

- `GenerationId`
- `DocumentId`
- `ChapterId`
- `ParagraphId`
- `SentenceId`
- `ChunkId`
- `SpanId`
- `EntityId`
- `MentionId`
- `EvidenceId`
- `StructuralEdgeId`
- `EventId`
- `EpisodeId`
- `TemporalId`
- `CausalId`
- `MemoryStateId`
- `DecisionId`
- `ReceiptId`

Semantic candidate identity is a 32-byte content-bound `CandidateId`. IDs are
never inferred from display labels.

## Frozen authority classes

| Tag | Authority |
|---:|---|
| 1 | source authoritative |
| 2 | semantic candidate |
| 3 | contextual evidence only |
| 4 | decision receipt |
| 5 | projection only |
| 6 | runtime receipt |

`ProjectionOnly` is reserved for cross-contract vocabulary. It is not legal
for a V2 page.

## Frozen status enums

- Candidate: proposed, accepted, rejected, deferred, superseded
- Decision: accept, reject, defer, undo
- Capability: produced, durable verified, unsupported, skipped, failed,
  cancelled
- Cache: computed, durable verified
- Publication: prepared, published, replaced, rejected
- Episode member: chunk, event
- Semantic family: identity, alias, coreference, relationship, event, episode,
  temporal, causal, memory/state, contextual co-occurrence, generic related
- Evidence role: source, target, premise, hypothesis, subject, state, cause,
  effect, membership

Numeric discriminants are regression-tested and may not be reordered.

Capabilities identify the exact producer product: document structure,
canonical entities, identity, relationships, events, episodes, temporal,
causal, memory/state, contextual evidence, or NLI adjudication.

## Authority semantics

Source authoritative:

- document hierarchy and exact dynamic chunks
- spans, mentions, and evidence coordinates
- canonical entity bindings
- structural edges that are source facts

Candidate only:

- identity, alias, and coreference
- typed relationships
- events, episodes, and memberships
- temporal and causal claims
- memory/state claims
- NLI adjudications

Contextual evidence only:

- weak co-occurrence and proximity

Accepted semantic topology requires an evidence-bound decision receipt.
Visibility in a scene never promotes authority.

## Static expected-topology oracle

`tests/angular_expected_topology_oracle.rs` transcribes the legacy expected
families into a static test-only table:

- document spine
- chunk spine
- entity anchors
- anchor evidence
- identity/linker
- typed relationships
- events
- episodes and memberships
- temporal
- causal
- memory/state
- contextual co-occurrence

The oracle imports no Angular or TypeScript type and has no runtime authority.
It proves that every expected family has an unambiguous V2 page and authority
class.

## Fail-closed errors

Opening rejects:

- truncated, oversized, incomplete, or wrong-version generations
- invalid header, total length, directory, tags, order, or alignment
- unknown, duplicate, missing, overlapping, or out-of-bounds pages
- wrong authority, record size, record alignment, schema, count, or length
- page or generation hash mismatch
- invalid record layout, string reference, endpoint, evidence, or decision
  binding
- wrong cohort or stale document/registry revision
- pages recognized but unsupported by a consumer

Opening is mmap-backed. The directory uses a fixed stack array and page
validation performs no graph-sized allocation.

## Version boundary

V1 stays readable and unchanged. V2 is not a compatibility reinterpretation of
V1 and does not extend V1 tags. Cut 1 added a structural writer and a borrowed
compiler-side reader. Cut 2 added the isolated entity/evidence producer and
amended the pre-registration schema with pages 27 and 28 after proving that
candidate evidence cannot safely be represented as a range into the shared
evidence page and that canonical results alone do not preserve merge
provenance. Cut 3 added the isolated story producer over the already-frozen
relationship, event, episode, membership, temporal, causal, memory/state, and
candidate-evidence pages. Every story output remains proposed and
evidence-bound; optional model ranking can only score an existing deterministic
candidate key. No kernel or application registration exists. V2 cannot become
live authority until the later producer-coordinator and cutover gates.
