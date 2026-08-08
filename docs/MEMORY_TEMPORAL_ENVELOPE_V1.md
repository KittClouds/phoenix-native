# Memory temporal envelope V1

Status: implemented in the unregistered `PhoenixGraphGenerationV3` development
contract and the dual-face memory coordinator. This does not change production
authority or register V3 in the application.

## Purpose

Phoenix memory must preserve when a source was written, when a statement was
made, when an event happened, when Phoenix observed it, and when a belief was
valid without collapsing those meanings into one timestamp.

The first temporal-memory cut adds a compact immutable envelope around an
evidence-bound semantic subject:

```text
immutable document or turn
  -> evidence span
  -> semantic candidate
  -> temporal envelope
  -> verified mmap pages
  -> bounded recall context
```

The envelope is candidate-only evidence. It cannot promote a candidate into
accepted graph truth. Existing decision, validity, and supersession pages keep
that authority boundary.

## Clock meanings

| Clock | Meaning |
|---|---|
| `source_time` | Timestamp supplied by the source or source container. |
| `asserted_at` | Time at which the speaker or author made the assertion. |
| `occurred_from/to` | Normalized event instant or interval described by the source. |
| `observed_at` | Time Phoenix ingested or observed the evidence. Required. |
| `valid_time_from/to` | Interval in the represented world for which the statement may hold. |
| `system_generation_from/to` | Immutable Phoenix generations in which the envelope is present. |

`TIME_UNKNOWN` is distinct from an open-ended interval. A missing occurrence
time therefore cannot masquerade as ?always.? Presence flags must agree with
their stored clocks, and an occurrence interval is either complete or absent.

The record also preserves:

- original temporal wording;
- normalized precision (`instant`, `day`, `relative`, `ordinal`, and others);
- optional timezone offset;
- bounded confidence;
- explicit uncertainty and normalization flags;
- one or more ordered subject/evidence bindings.

## Packed representation

Two required, mmap-readable V3 pages were added:

- `TemporalEnvelopes` stores fixed-width `TemporalEnvelopeRecordV1` rows.
- `TemporalEnvelopeBindings` stores fixed-width
  `TemporalEnvelopeBindingRecordV1` rows.

Bindings use dense ranges (`binding_start`, `binding_count`) rather than heap
pointers. IDs are deterministic and ordering is canonicalized before
publication. The verified opener rejects duplicate envelopes, broken ranges,
orphan rows, nonexistent subjects, missing evidence, invalid clocks, invalid
timezone offsets, non-finite confidence, and malformed original-text flags.

The contract can bind envelopes to semantic candidates, events, episodes, or
turns. The current coordinator slice intentionally accepts candidate bindings
only; event and episode producers can be wired after their production path has
equivalent provenance tests.

## Recall behavior

Context candidates retain their own valid-time interval and carry only the
temporal envelopes bound to that candidate. A timeless candidate remains
timeless; an envelope attached to a sibling candidate cannot bleed across the
candidate boundary. Original temporal wording participates in the bounded
context byte budget.

## Model-role boundary

Phoenix records semantic model duty in the immutable model identity:

| Role | Owner | Allowed output |
|---|---|---|
| `SteerableSemanticObserver` | GLiClass Instruct | Broad event, attribute, relation, state, and temporal candidate observation. |
| `DedicatedNliObserver` | ModernBERT-NLI | Entailment, contradiction, and neutral adjudication. |
| `Other` | Non-semantic models | No implicit semantic authority. |

The roles are not aliases or fallbacks:

- a dedicated NLI model cannot author broad semantic candidates;
- an NLI adjudication must reference a dedicated NLI model;
- the three NLI probabilities must be finite, bounded, and normalized;
- a GLiClass identity is never silently substituted for ModernBERT-NLI;
- model observations remain candidate evidence until deterministic policy and
  explicit decision receipts promote them.

## No artificial decay

This layer does not delete or weaken memory based on elapsed wall time. Recency
is a queryable signal derived from explicit clocks, not an authority mutation.
Corrections and changing beliefs use validity intervals and supersession
lineage. Original source text and prior generations remain auditable.

## Deterministic semantic adjudication

`phoenix-memory-semantics` now exposes `DeterministicAdjudicatorV1`. It consumes
two bounded learned observations:

- a GLiClass Instruct memory-event classification plus semantic cue flags;
- a ModernBERT-NLI entailment, contradiction, or neutral relationship.

It also consumes deterministic scope, temporal-relation, source-authority, and
evidence-count inputs. The fixed Rust table produces only a policy proposal.
It never writes a decision or changes graph truth.

The first policy table distinguishes:

- authoritative explicit correction -> close and supersede;
- authoritative later state -> close and replace;
- different scope -> retain both;
- corroborating entailment -> add evidence;
- compatible elaboration -> elaborate;
- historical statement -> preserve historically;
- hypothetical, conditional, planned, or abandoned state -> candidate only;
- unresolved contradiction -> open dispute;
- ambiguous or low-confidence observation -> defer.

Every outcome preserves history and requires an explicit decision receipt.
Invalid model lanes and non-finite or unbounded scores fail closed.

## Next cuts

This slice supplies temporal substrate, not the entire second-brain runtime.
The next bounded cuts are:

1. produce temporal envelopes from real document and conversation analysis;
2. bind event and episode records to the same clock model;
3. integrate the implemented append-only policy ledger and immutable
   `.phxmemory` current-memory projection with the coordinator;
4. connect the implemented bounded CSR recursive working set to recall;
5. qualify larger-corpus latency, allocation growth, provenance completeness, and restart
   determinism before any production V3 registration.
