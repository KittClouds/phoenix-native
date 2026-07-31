# Memory Cut 0: MenteDB behavior disposition

Status: implemented contract and harness slice, 2026-07-29.

MenteDB is a behavioral reference, not a library, service, storage engine, or
authority dependency. The studied source is pinned to commit
`c7cf78679317c69cd163cfa66068868dac2a75ad` (workspace version `0.35.0`).
LongMemEval source, cleaned datasets, prompts, judge, and model identifiers are
frozen in `memory-lock/longmemeval-cleaned-v1.json`.

## Governing architecture

Phoenix keeps the existing clean-room authority:

```text
turn/document input
  -> bounded producers
  -> immutable PhoenixGraphGenerationV2
  -> mmap-backed retrieval/graph indexes
  -> bounded context assembly
  -> disposable caches and derived analytics
```

Mutable database state never owns memory truth. Semantic products remain
candidate-only until an evidence-bound decision receipt promotes them into a
new immutable generation.

## Adopt, redesign, reject

| MenteDB subsystem or behavior | Decision | Phoenix clean-room destination |
|---|---|---|
| Read-half recall before write-half ingest | Adopt | Explicit `RecallTurn` and `IngestTurn` coordinator commands with sequenced receipts |
| Episodic turn capture | Adopt | Immutable source/turn pages with document, session, time, and content hashes |
| Entity-centric memory | Adopt | Canonical entity IDs, mention/evidence pages, and verified entity-to-node indexes |
| Hybrid dense plus lexical retrieval | Adopt | Generation-bound ANN plus BM25 indexes with deterministic fusion |
| Temporal and graph-aware retrieval | Adopt | Typed time indexes, packed adjacency, and mask-based traversal |
| Reciprocal-rank fusion | Adopt | Deterministic derived ranking stage with a frozen scoring profile |
| Contextual retrieval and chunk context | Adopt | Dynamic chunks are authoritative records; contextual fields are derived pages |
| Bi-temporal validity | Adopt | Valid-time and system-generation intervals on facts and decisions |
| Provenance and evidence | Adopt | Exact document/chunk/span bindings on every candidate and accepted semantic edge |
| Delta-aware serving | Adopt | Generation deltas and stable typed IDs; never in-place authority mutation |
| Bounded context assembly | Adopt | Token-budgeted, provenance-preserving context packet with deterministic ordering |
| Memory spaces and isolation | Adopt | Workspace/document namespaces enforced before retrieval, not filtered afterward |
| Offline consolidation | Adopt | A generation compiler that emits proposals and compacted pages |
| HNSW vector search | Redesign | Existing Phoenix HNSW machinery plus DiskANN-compatible packed/mmap pages; no vector database |
| Storage engine, WAL, page manager, snapshots | Reject | Immutable artifact publication and atomic authority manifest already provide rollback |
| Mutable `MemoryNode` records | Redesign | Hot/cold structure-of-arrays, typed IDs, offsets, bitsets, and string slabs |
| Knowledge graph manager | Redesign | Generation-bound CSR/CSC and typed edge pages; graph is a projection of authority |
| Automatic extraction writes | Redesign | Producers emit evidence-bound candidates; no direct semantic promotion |
| Entity resolution and alias linking | Redesign | Stable identity decisions and durable receipts; never UI-label equality |
| Contradiction detection | Redesign | Candidate conflict records with supporting and opposing evidence |
| Belief propagation | Redesign | Derived confidence/invalidation proposals; never self-authorizing truth |
| Salience, decay, reinforcement | Redesign | Query-time scores or a new derived generation; authority records do not silently decay |
| Cognitive memory tiers | Redesign | Explicit source, semantic, procedural, and derived page families |
| Working-memory cache | Redesign | Bounded disposable cache keyed by generation, query profile, and source hash |
| Speculative cache | Redesign | Optional latest-wins cache with hit/miss/eviction receipts and no authority role |
| Consolidation, archival, forgetting | Redesign | New compacted/redacted generations with explicit retention and erasure policy |
| GDPR deletion | Redesign | Redaction generation plus verified destruction policy for superseded artifacts |
| Action rules and procedural memory | Redesign | Typed, capability-checked commands; no arbitrary runtime code execution |
| MQL query language | Defer/redesign | Typed Rust query API first; a language is justified only by measured product demand |
| Binary/quantized embeddings | Redesign | Optional packed sidecar selected by recall-quality and memory benchmarks |
| Embedding providers | Redesign | Provider adapters outside authority with exact model/config identities |
| REST, gRPC, MCP, server runtime | Reject for core | Optional future boundary; never required for local ingest, recall, or rendering |
| Replication subsystem | Defer/redesign | Immutable generation transfer and manifest reconciliation, not mutable row replication |
| Background maintenance daemon | Redesign | Bounded cancellable jobs publishing explicit receipts |
| Pain signals and proactive recall | Redesign | User-facing candidate signals and ranked suggestions, never hidden writes |
| Phantom detection | Redesign | Unsupported-evidence diagnostics; cannot create memory authority |
| Trajectory and ghost memories | Reject as truth | May exist only as explicitly synthetic, disposable analytics |
| Sentiment analysis | Defer | Typed candidate producer if a concrete product surface needs it |
| Correction tagging | Adopt | Explicit supersession links and decision receipts |
| Project-scope weighting | Adopt | Scope mask applied before scoring with a frozen profile |
| Filter-after-retrieval tenant isolation | Reject | Namespace must constrain index access before candidate generation |
| MenteDB crates and runtime | Reject | No Cargo, process, service, file-format, or migration dependency |

## LongMemEval freeze

The behavioral benchmark is the official cleaned dataset repository at revision
`98d7416c24c778c2fee6e6f3006e7a073259d48f`, paired with LongMemEval commit
`9e0b455f4ef0e2ab8f2e582289761153549043fc`.

The lock records:

- exact LFS SHA-256 and byte length for small, medium, and oracle datasets;
- exact SHA-256 for generation, retrieval, and evaluation source files;
- the direct-reader and task-aware judge prompt symbols and containing source
  hashes;
- exact external model IDs where the upstream code resolves them;
- an explicit `unavailable-provider-managed` marker where weights cannot be
  independently hashed.

The last item is an honest reproducibility boundary. An API model identifier is
not a weight hash. Cut 0's executable baseline therefore uses
`phoenix-bm25-session-v1`, a deterministic local retrieval model with no
external weights, reader, or judge.

## Gold firewall

The official JSON mixes workload and evaluation truth. Phoenix prepares it once
into incompatible typed artifacts:

| Artifact | Contains | Explicitly excludes |
|---|---|---|
| `PHXLMW01` workload | question, question type/date, session IDs/dates, roles, content | `answer`, `answer_session_ids`, `has_answer` |
| `PHXLMG01` evaluator gold | question ID/type, answer, answer-session IDs | history bodies and retrieval index material |
| `PHXLMR01` retrieval | question ID, ranked session IDs, exact score bits | answer text and gold session IDs |

The baseline entry point accepts `WorkloadArtifact`, and its reader rejects the
gold magic before postcard decoding. Evaluation is a separate operation after
retrieval output has been frozen. Gold answers cannot influence ingestion,
index construction, query scoring, ranking, or context assembly.

## Cut 0 stop/go

GO requires:

- manifest verification passes;
- the local source file matches the frozen size and SHA-256 before preparation;
- workload/gold artifacts have different magic, types, and payload digests;
- deterministic repeated baselines produce byte-identical retrieval artifacts;
- a gold artifact is rejected by the baseline workload reader;
- the package and workspace contain no MenteDB dependency.

This cut does not claim LongMemEval answer accuracy, model parity, or a complete
Phoenix memory engine. Those begin only after the recall/ingest contracts and
retrieval indexes are implemented against the frozen workload.
