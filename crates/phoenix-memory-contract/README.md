# Phoenix mixed-source graph contract

`phoenix-memory-contract` defines `PhoenixGraphGenerationV3`, the immutable
mixed-source authority for Phoenix documents and conversations.

This Cut 1 crate is deliberately isolated:

- production remains registered to `PhoenixGraphGenerationV2`;
- V2 readers and files are unchanged;
- no Angular, legacy bridge, database, JSON, renderer, or GPUI dependency is
  present;
- the crate publishes only a new `.phxgg3` file supplied by its caller.

## Source invariants

A generation belongs to exactly one namespace. Notes and conversations may
coexist inside that namespace, but a reader can require an exact namespace hash
and source-set hash when opening the mmap.

Documents retain:

- stable source and document identities;
- revision, path, exact UTF-8 body, and content hash;
- exact dynamic chunk records and source-coordinate ranges.

Conversations retain:

- stable source, conversation, and turn identities;
- role, ordinal, event time, reply target, actor, and model identity index;
- exact UTF-8 turn bodies and content hashes.

Semantic candidates may carry evidence-bound `TemporalEnvelopeRecordV1`
records. The envelope preserves source, assertion, occurrence, observation,
valid, and system clocks independently; unknown time is explicit and never
conflated with an unbounded interval.

Source text has a dedicated page. Semantic labels use a separate string slab,
so adding labels cannot change the source-set identity.

## Identity and ordering

Stable IDs use domain-separated BLAKE3 over namespace and stable external
identity. They never depend on insertion order. `MixedSourceBuilder` sorts
source inputs before building the string and source-text slabs.

Range-addressed pages are not reordered by the writer. Their producers must
emit grouped canonical order so offsets remain valid. The verified mmap opener
checks those ranges and rejects invalid source, chunk, turn, reply, evidence,
time, namespace, hash, and supersession bindings.

## Authority classes

The V3 page directory retains V2's authority separation:

- source-authoritative source, structure, entity, mention, evidence, and
  accepted structural pages;
- semantic-candidate relationship, identity, event, episode, temporal, causal,
  memory-state, and NLI pages;
- contextual-evidence-only co-occurrence pages;
- decision-bound decisions, validity intervals, and supersessions;
- runtime-only capability, model, stage, and publication receipts.

Model identities also declare their semantic duty. Steerable semantic
observers and dedicated NLI observers are distinct lanes. Verified NLI
adjudications must reference a model explicitly registered for dedicated NLI.

Every page is required, typed, aligned, schema-hashed, payload-hashed, and
bounded. Empty capability pages express absence; later producers must use
explicit capability records to distinguish unsupported work from zero output.

## Deliberately deferred

This slice does not:

- register V3 in `phoenix-app-core`;
- convert V2 or legacy artifacts;
- ingest live notes or model turns;
- publish a current-authority manifest;
- compile a V3 scene;
- implement hybrid recall.

Those are later cuts. Until registration changes in a dedicated cutover, V3 is
an additive contract and proof surface only.
