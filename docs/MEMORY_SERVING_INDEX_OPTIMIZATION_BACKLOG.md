# Memory serving-index optimization backlog

Status: **pinned and deliberately deferred until Memory Cuts 4–6 are complete**.

This note preserves the next serving/index pass without allowing it to expand
the current cut. These indexes are derived acceleration structures. They never
own source truth, semantic truth, review decisions, or generation authority.

## Planned indexes

### Entity and evidence adjacency

- Publish compact CSR offsets and neighbor rows for entity, mention, evidence,
  candidate, and accepted-edge traversal.
- Use dense typed IDs in hot rows and keep provenance in cold pages.
- Build once from a verified immutable generation and mmap the frozen result.
- Keep traversal allocation-free with caller-owned scratch.

### Temporal and bitemporal access

- Publish a `recorded_at`-ordered index for append/history scans.
- Publish valid-time start/end indexes for event and fact windows.
- Preserve both valid time and system-generation time so queries can answer:
  - what was valid at a historical instant;
  - what Phoenix believed at a historical generation;
  - what changed within a requested time window.
- Never collapse corrections or supersession chains into destructive updates.

### Filter masks

- Use Roaring bitmaps for large sparse/clustered memberships.
- Use dense bit masks where the ID space is compact enough to win.
- Cover namespace, source kind, review state, vocabulary/lens, and capability.
- Intersect masks before expensive scoring or graph traversal.

## Saved lexical follow-up

The current positional lexical engine already reuses one immutable index and
caller-owned scratch. The next tuning pass should evaluate:

- a compact coherence representation for queries with at most 64 exact groups,
  retaining the current wide fallback;
- hybrid merge/radix selection for dense touched-position sets;
- more compact mmap-ready position pages and a single-field specialization;
- paired ordinary, phrase, fuzzy, no-result, document, and conversation gates;
- multi-worker scaling with byte-identical rankings and bounded queues.

## Non-negotiable contracts

- Every index is cryptographically bound to one verified generation.
- Rebuilding an index cannot publish or mutate graph authority.
- Missing, stale, corrupt, mismatched, and oversized indexes fail closed.
- No label-based identity joins.
- No JSON freight.
- No hidden fallback to an unindexed full scan in production.
- Performance claims require warm/cold latency, p50/p95/p99, allocations,
  bytes, queue high-water marks, and a stable-memory soak.
