# QPS standalone lexical qualification

## Decision

QPS V2.01 qualifies as Phoenix's standalone lexical search engine. BM25 Turbo
remains a benchmark-only comparator and is not a runtime candidate arm,
fallback, or dependency.

The production shape is:

```text
query or explicit expansion groups
  -> QPS precomputed lexical impacts
  -> bounded candidate selection
  -> positional phrase/order/proximity/segment scoring
  -> stable ranked results
```

## Frozen mixed suite

`memory-lock/qps-mixed-qualification-v1.json` contains 24 independent sources:

- 12 workspace-style documents
- 12 conversation sessions

Its 32 queries are frozen as:

- 12 ordinary queries
- 8 exact phrase queries
- 8 fuzzy queries using explicit weighted expansion groups
- 4 legitimate no-result queries

Expected IDs are evaluator data. They are never inserted into the QPS index.
Fuzzy expansion is an explicit caller contract; QPS does not silently invent
spell corrections.

The copied release binary executed 1,024 repetitions per query, producing
32,768 measured searches:

| Gate | Result |
|---|---:|
| hit@10 | 1.000 |
| MRR | 1.000 |
| top-1 accuracy | 1.000 |
| no-result accuracy | 1.000 |
| median | 0.9 us |
| p95 | 1.3 us |
| p99 | 1.7 us |
| warm capacity growths | 0 |
| deterministic ranking failures | 0 |

Shape p99:

| Shape | p99 |
|---|---:|
| ordinary | 1.9 us |
| phrase | 1.5 us |
| fuzzy | 1.4 us |
| no result | 0.7 us |

Document-target p99 was 1.9 us; conversation-target p99 was 1.5 us.

## Larger conversation regression

The same copied binary reran the pinned 500-case cleaned-small LongMemEval
workload for 32 repetitions per case:

- hit@10: 0.980
- MRR: 0.891005
- median: 236.1 us
- p95: 524.9 us
- p99: 697.8 us
- maximum observed query groups: 57
- maximum candidates/reranked: 62/60
- warm capacity growths: 0

This passes the standalone absolute gates. It does not claim to beat the
benchmark-only BM25 Turbo comparator.

## Product gates

Standalone qualification requires:

- mixed hit@10 at least 0.98
- mixed MRR at least 0.90
- mixed top-1 accuracy at least 0.85
- no-result accuracy exactly 1.0
- mixed and cleaned-small p99 at most 1 ms
- cleaned-small hit@10 at least 0.98 and MRR at least 0.89
- zero warm capacity growth
- deterministic rankings
- every requested query shape
- both document and conversation sources

All gates passed.

## Honest limits

- The mixed suite is curated functional qualification, not a blind universal
  relevance claim.
- LongMemEval supplies the larger, harder regression evidence.
- Concurrent multi-worker scaling remains unmeasured; current qualification is
  for the established single-owner coordinator.
- Explicit expansion groups prove fuzzy/synonym ranking semantics. A spelling
  or synonym producer remains a separate query-planning responsibility.

## Reproduction

```powershell
$env:CARGO_TARGET_DIR='D:\phoenix-target-qps-standalone-qualification'

cargo run --release -p phoenix-memory-lock -- qps-qualify `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --suite memory-lock\qps-mixed-qualification-v1.json `
  --repetitions 1024
```

The exact binary, suite, receipts, metrics, and artifact hashes are frozen in
`memory-lock/qps-standalone-qualification-lock-2026-07-30.json`.
