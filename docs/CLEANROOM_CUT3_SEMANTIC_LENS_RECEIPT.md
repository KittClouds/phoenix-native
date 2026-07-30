# Clean-room Cut 3 semantic-lens receipt

Date: 2026-07-29

Scope: the approved pre-Cut-4 core-plus-lens boundary. This slice does not
register a new producer with the kernel, mutate `PhoenixGraphGenerationV2`,
publish a live scene, alter the workspace store, or restart the application.

## Result

Phoenix now has a small domain-neutral semantic contract and an explicit
Story V1 lens.

The shared contract is implemented in `phoenix-semantic-lens`. It owns:

- immutable lens identity
- versioned, namespaced semantic vocabularies
- compact semantic-code records
- endpoint-kind masks
- candidate-key domain separation
- lens-neutral review origins and bindings
- mmap-backed lens packs
- one-time page and vocabulary verification
- explicit corruption, mismatch, duplicate, unknown, oversized, and
  unsupported-version errors

The story producer remains responsible for story meaning. The semantic-lens
crate has no dependency on `phoenix-story-producer`.

## Frozen generality boundary

The domain-neutral classes are intentionally small:

1. identity
2. relation
3. occurrence
4. grouping
5. temporal constraint
6. influence
7. attributed state
8. contextual evidence

The core does not contain `Character`, `Scene`, `Remembers`, `Citation`,
`Experiment`, or other domain vocabulary.

Story V1 declares 29 stable semantic codes under:

```text
phoenix.story/v1
```

Its candidate hash namespace remains:

```text
phoenix.story-candidate/v1
```

This was already the Cut 3 hash domain. Formalizing it as a lens therefore
does not reset existing candidate IDs.

## Packed lens artifact

`PhoenixSemanticLensPackV1` is:

- fixed-layout and bytemuck-safe
- bounded to 4,096 codes
- bounded to 1 MiB of UTF-8 vocabulary strings
- bounded to 16 MiB total
- bound to one non-zero V2 generation hash
- protected by independent header, payload, vocabulary, and lens-identity
  hashes
- opened through `memmap2`
- verified before typed records or strings are exposed

The pack contains no JSON, hash map, graph copy, or domain object graph.

## Story V1 compatibility

`phoenix-story-producer` now validates the frozen Story V1 vocabulary before
publishing. Its existing packed V2 pages and 28-page V2 directory are
unchanged.

The compatibility test rebuilds the legacy Cut 3 relationship candidate key
through the generic `CandidateKeyBuilder` and requires exact byte equality.

The existing deterministic-generation test also remains green, proving that
identical Story V1 rules still produce byte-identical candidate pages.

## Non-story witness

A test-only `phoenix.research/v0` vocabulary proves the seam with:

- `relation.supports`
- `occurrence.experiment`
- `grouping.study`
- `state.result`

It creates and reopens a compact mmap pack, produces candidate identity in a
separate namespace, and uses the same lens-neutral review binding.

It is not registered with the kernel and does not add a production feature.

## Review boundary

`CandidateOrigin` binds:

- candidate ID
- lens ID
- vocabulary hash
- semantic code
- core semantic class

`LensNeutralReviewBinding` additionally binds:

- typed source and target endpoints
- document hash
- evidence hash
- producer generation
- registry revision

Validation against the opened lens pack proves that the code, class, and
endpoint kinds match the exact vocabulary. Unknown codes, wrong endpoint
types, zero hashes, and missing generation/revision authority fail closed.

Cut 4 can therefore persist decisions against this binding without matching
story enums or UI labels.

## Verification

Scoped target:

```text
D:\phoenix-target-semantic-lens-v1
```

Focused proof:

- 5 semantic-lens integration tests passed
- 5 Story V1 integration tests passed
- Story candidate identity compatibility passed
- Story generation-to-lens-pack binding passed
- non-story namespace isolation passed
- corrupt-pack rejection passed
- duplicate-vocabulary rejection passed
- lens-neutral review and endpoint validation passed
- Clippy passed for all targets with warnings denied
- rustfmt passed

The broader clean-room suite passed 44 tests across:

- packed V2 generation
- structural production and fresh-process reuse
- entity/evidence production
- semantic lenses
- Story V1 production
- the V2 scene compiler

## Stop/go

| Gate | Result |
|---|---|
| V2 format and page directory unchanged | Pass |
| Story V1 candidate identity preserved | Pass |
| Lens vocabulary is versioned and hash-bound | Pass |
| Two lenses cannot collide on identical candidate material | Pass |
| Unknown lens code fails closed | Pass |
| Wrong endpoint kind fails closed | Pass |
| Research witness imports no Story implementation | Pass |
| Review boundary is domain-neutral | Pass |
| Candidate-only authority remains unchanged | Pass |
| Live kernel or application registration | Not performed |

## Rollback

Remove `phoenix-semantic-lens` from the workspace, remove `lens.rs` and its
validation call from `phoenix-story-producer`, and remove these additive tests.

No V1 or V2 artifact migration, workspace migration, graph rebuild, or scene
rollback is required.
