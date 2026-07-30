# Clean-room Cut 2 entities-and-evidence receipt

Date: 2026-07-29

Verdict: pass for the isolated V2 entity boundary

## Scope

Cut 2 adds:

- `phoenix-entity-producer`
- packed canonical entity, canonical-binding, mention, evidence,
  identity-candidate, and candidate-evidence-binding pages
- exact document, chunk, sentence, span, model, registry, and generation
  binding checks
- an explicit coordinator identity-decision input
- a disposable non-overlapping editor-paint projection
- entity/evidence capability, model, stage, and publication receipts

No kernel command, application surface, database, workspace document, scene
archive, model artifact, or live authority registration changed. Phoenix
Native was not launched.

## Authority path

```text
verified structural V2 generation
  + exact source text
  + verified PhoenixNerArtifactV1
  + stable-ID user tags
  + explicit coordinator merge decisions
  + evidence-bound identity candidates
  -> phoenix-entity-producer
  -> packed PhoenixGraphGenerationV2
```

Every mention retains its exact source range, dynamic chunk ID, sentence
ordinal, entity ID, and evidence ID. Overlapping mentions remain independent
rows in the authoritative mention and evidence pages.

The editor paint projection is derived after authority is frozen. User-tagged
paint has precedence, then NER paint is reduced deterministically by source
range, longest match, accepted-evidence state, confidence, and evidence ID.
Omitting an overlapping paint span never edits or filters graph evidence.

## Identity contract

- Stable IDs are the only implicit identity key.
- Equal labels and equal source surfaces do not merge entities.
- A user identity may merge into an existing direct canonical stable identity
  only through an explicit coordinator decision.
- Decisions have non-zero unique IDs and may not rewrite one NER canonical
  identity into another.
- Identity candidates remain `Proposed` and reference exact evidence through
  the dedicated packed candidate-evidence-binding page.
- No candidate is promoted by this producer.

## V2 schema amendment

The Cut 2 audit disproved the earlier assumption that every candidate's
evidence could be represented as a contiguous range into the shared evidence
page. Valid candidates overlap and share evidence, so adjacency is not
guaranteed.

V2 therefore gained required page 27, `CandidateEvidenceBindings`, and required
page 28, `CanonicalEntityBindings`, before any kernel or application
registration. Candidate ranges address page 27. Source-to-canonical identity,
provenance, and coordinator decision IDs are carried by page 28.
The current schema-directory hash is:

```text
3d0486e6e33145fcebbca97186edd2c55d6d5ae76f0e2213106933414e54a66a
```

V1 remains unchanged and readable.

## Verification

Scoped target:

```text
D:\phoenix-target-cleanroom-entity-v2
```

Focused tests prove:

- all valid overlapping NER and user mentions survive in authority
- non-overlapping paint cannot delete graph evidence
- every mention resolves to one verified sentence and to the smallest exact
  containing dynamic chunk, with ordinal tie-breaking when chunks overlap
- equal labels remain separate without a decision
- an explicit decision merges source provenance and mention counts
- identity candidates bind exactly two evidence records through page 27
- stale source and invalid canonical rewrites fail closed
- identical authority produces identical generation, page, evidence, and
  paint hashes
- structural page hashes are preserved through entity publication

Commands:

```text
cargo fmt --all -- --check
cargo test -p phoenix-analysis-contract -p phoenix-graph-generation \
  -p phoenix-graph-generation-v2 -p phoenix-document-producer \
  -p phoenix-entity-producer -p phoenix-scene-compiler --no-fail-fast
cargo clippy -p phoenix-graph-generation-v2 -p phoenix-document-producer \
  -p phoenix-entity-producer -p phoenix-scene-compiler \
  --all-targets -- -D warnings
```

Recorded result:

- 46 focused tests passed
- `rustfmt --check` passed
- scoped Clippy passed with warnings denied
- `git diff --check` passed
- all new entity-producer source modules remain below 800 lines

## Stop/go

| Gate | Result |
|---|---|
| Every mention has exact source and chunk bindings | Pass |
| Overlapping valid mentions survive in authority | Pass |
| Paint cannot delete graph evidence | Pass |
| No label-based merge | Pass |
| Rollback retains structural V2 pages | Pass: remove entity-producer membership/registration; structural producer and artifacts remain valid |

## Deliberately not claimed

- The entity producer is not registered with the kernel or live application.
- No real model run or exact live cohort was executed in this cut.
- No entity generation has been compiled into a scene.
- No semantic candidate has been accepted or promoted.
- No live-app or performance claim is made.
