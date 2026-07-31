# Memory Cut 3 — domain-general semantics and lenses

Status: implemented in shadow-only V3 scope on 2026-07-29.

## Authority boundary

Source, content-unit, chunk, entity, mention, and evidence pages remain
authoritative. Semantic candidates now carry:

- a registered vocabulary-pack ID;
- a relation-kind string;
- a compact endpoint range;
- an exact evidence range;
- a producer identity hash;
- valid-time bounds;
- candidate-only status.

`VocabularyPacks` and `CandidateEndpointBindings` are new packed V3 pages.
They are hash-verified once when the mmap generation opens. Candidate endpoints
are no longer limited to one subject and one object.

## Packs

- `phoenix.core.memory/1` owns domain-general identity, attribute, preference,
  relationship, event, time, cause, state, belief, commitment, goal,
  procedure, correction, conflict, supersession, and repeated-evidence terms.
- `phoenix.lens.narrative/1` owns scenes, episodes, conflicts, and character
  state.
- `phoenix.lens.conversation/1` owns sessions, speakers, preferences,
  commitments, and corrections.
- `phoenix.lens.document/1` owns sections, claims, citations, definitions, and
  revisions.

Removing a lens removes only candidate rows for its pack. No source, entity,
mention, evidence, or stable core-candidate identity depends on a lens.

The V2 `phoenix-story-producer` is not imported into the V3 authority path.
Useful deterministic rules may be ported behind the Narrative pack, but the
old producer cannot publish V3 candidates or act as a universal ontology.

## Recall and consolidation

Recall can return proposed candidates, but every returned candidate includes
its producer identity, pack, endpoints, `Proposed` status, and exact source
excerpt for each evidence binding. Recall does not ingest the pending answer
and does not publish.

Offline consolidation proposes duplicate identity, repeated fact, conflict,
correction/supersession, and scoped-summary candidates. Duplicate identity
requires an explicit stable identity key; display labels are never an identity
join. Consolidation has no decision or publication capability.

## Shortrun shadow proof

Source:

`C:\code land\clean-rust\docs\shortrun.md`

- bytes: `161490`
- BLAKE3:
  `0da62ebf93c97df40f71fba1c37dd1788e8cf73ad5a5e688e57f1ab3a54e6c3a`
- core candidates: `3`
- Narrative-lens candidates: `3`
- exact evidence ranges: `6`
- accepted candidates: `0`

The shadow probe passed with `SHORTRUN_SHADOW_OK`. This source is useful,
substantial Shortrun narrative data, but it is not byte-identical to the frozen
Angular comparison cohort (`159402` bytes, SHA-256
`9beafe8bd23317e118d218d99a21a21d53ed61371d6fbf60d76b0f0eedc9b8c7`).
No parity claim is made from this run.
