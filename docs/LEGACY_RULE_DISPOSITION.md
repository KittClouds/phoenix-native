# Legacy graph rule disposition

Date: 2026-07-29

Authority:

- Angular graph-rebuild source is a read-only behavioral specification.
- The persisted Angular scoped-document generation is a static test oracle.
- Neither is a runtime dependency.

## Retained

| Rule | Native interpretation |
|---|---|
| Exact document identity, revision, and content hash bind every product | Required header authority |
| Dynamic chunks are stable source records | Produced once and carried forward unchanged |
| Mentions retain exact source intervals | Packed mention and evidence pages |
| Canonical entities use stable IDs | No label-based identity |
| Evidence is independently addressable | Typed `EvidenceId` and contiguous bindings |
| Semantic products identify their producer and model | Capability and identity pages |
| Weak co-occurrence is not accepted truth | Contextual-evidence-only page |
| Corrupt or mismatched products fail closed | Typed V2 open errors |
| Persisted products are immutable generations | Create-new plus atomic manifest publication |
| Canvas state is derived | Scene archive and product index remain projections |

## Rejected

| Legacy rule or behavior | Reason |
|---|---|
| Import Angular/TypeScript implementations | Reintroduces old lifecycle and authority coupling |
| Global registry label matching | Can merge unrelated identities |
| Generic `Related` as a typed relationship | Loses semantic meaning |
| Automatic acceptance of heuristic relationships | No durable evidence-bound decision |
| Automatic acceptance of temporal facts | Model output is not user-approved truth |
| Automatic acceptance of memory/state facts | State interpretation is semantic and reviewable |
| Co-occurrence promoted by repetition alone | Frequency is contextual evidence, not truth |
| Graph Model V2 lane roots as domain nodes | Compiler scaffolding is not story topology |
| Canvas edge arrays as semantic authority | Projection deduplication changes visible counts |
| OPFS snapshots as the new native database | Old storage schema is not the clean-room contract |
| JSON graph freight | Repeats copies, allocations, parsing, and split authority |
| Runtime legacy fallback | Makes clean-room failure silent |
| Scene compiler reconstructing chunks or paragraphs | Creates a second structural authority |
| Synthetic `Episode 1` | Renderer/compiler cannot manufacture story facts |

## Redesigned

| Legacy concept | Clean-room design |
|---|---|
| Eleven compiler lanes | Producer capability matrix plus typed pages |
| Typed relationships | Evidence-bound candidates; receipt-backed promotion |
| Entity linker | Identity/alias/coreference candidate page |
| Events | Candidate records with source evidence |
| Episodes | Candidate episode records plus typed memberships |
| Episode hierarchy edges | Projection derived from verified candidates/decisions |
| Temporal facts | Candidate page with source/target and evidence ranges |
| Causal facts | Candidate page with cause/effect and evidence ranges |
| Memory state | Candidate page with subject, key, value, and evidence |
| Review status | Explicit proposed/accepted/rejected/deferred/superseded states |
| Promotion | Immutable decision receipt creates a new generation |
| Source/read-model counters | Separate resource, authority, and projection counts |
| Durable reuse | Page hashes plus exact document/model/config binding |
| Angular topology comparison | Static frozen oracle, never executable dependency |

## Frozen policy

Source records may be authoritative.

Semantic interpretations are candidate-only by default.

Contextual co-occurrence is evidence-only.

Durable accepted topology requires a valid decision receipt.

Projection visibility never grants authority.
