# Clean-room Cut 3 receipt

Date: 2026-07-29

Scope: story candidates only. No kernel registration, live scene publication,
accepted-topology mutation, application launch, or model execution occurred.

## Result

Cut 3 adds the independent `phoenix-story-producer` crate. It consumes one
verified entity/evidence V2 generation and publishes another immutable V2
generation containing only proposed semantic candidates.

The source-authoritative pages are borrowed from the verified input while
building and copied unchanged into the new packed artifact:

- document structure
- exact dynamic chunks
- entities
- mentions
- evidence
- canonical identity bindings
- structural edges

Their page hashes are verified unchanged after publication.

## Candidate products

The crate implements independently registered producers for:

| Product | V2 page | Authority |
|---|---|---|
| Typed relationships | 12 | semantic candidate |
| Events | 14 | semantic candidate |
| Episodes | 15 | semantic candidate |
| Episode memberships | 16 | semantic candidate |
| Temporal claims | 17 | semantic candidate |
| Causal claims | 18 | semantic candidate |
| Memory/state claims | 19 | semantic candidate |

Each product is either:

- `Deterministic { producer_id, rules }`; or
- `Unsupported { producer_id }`.

Supported with zero output is recorded as `Produced / 0`. An unavailable
producer is recorded as `Unsupported / 0`. The two states cannot be confused.

Individual registrations can therefore be disabled without changing or
invalidating source pages.

## Evidence and identity contracts

Every candidate row has a non-empty contiguous range in the shared candidate
evidence-binding page.

Additional validation includes:

- relationship source and target evidence must bind their exact entity IDs
- event and episode labels must be non-empty exact UTF-8 source spans
- label evidence must overlap that exact source span
- episode candidates must contain at least one real chunk or event membership
- every membership is independently evidence-bound
- temporal and causal endpoints must resolve to verified entities, chunks,
  events, or episodes
- memory/state evidence must include the exact subject entity
- evidence lists are unique, ordered, and refer to existing authority rows
- all candidate keys are content-, producer-, semantic-, and evidence-bound
- candidate keys cannot collide with earlier identity evidence bindings

Events and episodes have stable typed IDs. Their reconstructible 32-byte
candidate keys address evidence and optional ranking. No display label is used
as identity.

An episode label must be present in the source itself. The producer cannot
invent a generic `Episode 1` label unless that exact text is actually in the
document and evidence overlaps it.

## Model ranking boundary

Optional model scores target an existing deterministic `CandidateId`.

A model may:

- replace the confidence of that exact candidate
- append its model identity and a ranking stage receipt
- set the model-ranked flag

A model may not:

- create a candidate
- change its type or endpoints
- add evidence
- change its authority
- promote it

Unknown, duplicate, non-finite, or out-of-range scores fail closed.

## Zero promotion

All produced story records have `CandidateStatus::Proposed`.

The producer:

- appends no decision receipt
- appends no accepted structural edge
- mutates no earlier accepted topology
- performs no automatic promotion

Promotion remains a later receipt-backed operation.

## Test proof

Scoped target:

```text
D:\phoenix-target-cleanroom-story-v2
```

Four focused integration tests prove:

1. Every relationship, event, episode, membership, temporal, causal, and
   memory/state output is proposed and evidence-bound.
2. Supported-empty and unsupported capabilities remain visibly distinct.
3. Synthetic source labels and model scores for absent candidates fail closed.
4. Identical authority and rules produce byte-identical packed generations.

The complete fixture produces:

```text
relationships=1
events=2
episodes=1
episode_memberships=2
temporal=1
causal=1
memory_state=1
story_evidence_bindings=20
decisions=0
accepted_topology_delta=0
```

Verification:

- focused Cut 3 tests passed
- `rustfmt --check` passed
- scoped Clippy passed with warnings denied
- all Cut 3 Rust files remain below 800 lines

## Stop/go

| Gate | Result |
|---|---|
| Every output is evidence-bound | Pass |
| Unsupported capabilities are explicit | Pass |
| Zero automatic promotion | Pass |
| Episodes are real candidate records | Pass |
| No synthetic `Episode 1` | Pass |
| Model ranking cannot create candidates | Pass |
| Source page hashes remain unchanged | Pass |
| Live Shortrun story semantics | Not claimed |
| Kernel or application registration | Not performed |

## Rollback

Remove `phoenix-story-producer` from the workspace and delete its crate.

No V1 format, existing V2 source page, workspace store, live generation,
database, or migration must be changed. Previously valid structural and entity
generations remain readable.
