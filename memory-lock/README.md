# Phoenix memory benchmark lock

This directory freezes external benchmark identity without importing an
external memory runtime. The authoritative lock is
`longmemeval-cleaned-v1.json`.

The first official cleaned-small execution receipt is
`cleaned-small-baseline-2026-07-29.json`. It records two byte-identical
500-case retrieval runs and the negative gold-as-workload proof.

The QPS V2.01 production evaluation is frozen in
`qps-v201-cleaned-small-release-lock-2026-07-30.json`. It records the exact
binary, typed artifacts, precomputed-impact BM25 comparator, quality results,
query-length latency buckets, deterministic rerun, gold-firewall proof, and
the fail-closed shadow-only disposition.

The subsequent standalone product qualification is frozen in
`qps-standalone-qualification-lock-2026-07-30.json`. It qualifies one QPS
index with no BM25 runtime arm across ordinary, phrase, fuzzy, no-result,
document, and conversation queries.

Bounded worker scaling is frozen in
`qps-concurrency-qualification-lock-2026-07-30.json`. It covers the mixed
source suite and the pinned 500-case LongMemEval workload with sharded bounded
queues, worker-local scratch, deterministic rankings, and memory-plateau gates.

The optional learned ordering layer is qualified by `qps-learned-qualify`.
It mines a frozen hard-negative ledger from the same candidate pool, trains a
deterministic non-negative linear model over QPS's existing evidence, and
measures both paired query overhead and the absolute cost of scoring a complete
160-candidate rerank pool. It is one ranking layer, not a second search arm.

The harness has three artifact domains:

- `PHXLMW01`: retrieval workload; questions and history only.
- `PHXLMG01`: evaluator-only answers and answer-session IDs.
- `PHXLMR01`: deterministic retrieval output.

The baseline command can open only `PHXLMW01`. A gold artifact fails at the
magic and typed-decoding boundaries before retrieval runs.

From the `phoenix-native` workspace root:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-memory-cut0'

cargo run --release -p phoenix-memory-lock -- verify `
  --manifest memory-lock\longmemeval-cleaned-v1.json

cargo run --release -p phoenix-memory-lock -- prepare `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --variant small `
  --source C:\benchmarks\longmemeval_s_cleaned.json `
  --workload C:\benchmarks\longmemeval-small.plmw `
  --gold C:\benchmarks\longmemeval-small.plmg

cargo run --release -p phoenix-memory-lock -- baseline `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --workload C:\benchmarks\longmemeval-small.plmw `
  --output C:\benchmarks\longmemeval-small.plmr `
  --top-k 10

cargo run --release -p phoenix-memory-lock -- qps-shadow `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --workload C:\benchmarks\longmemeval-small.plmw `
  --output C:\benchmarks\longmemeval-qps-v201.plmr `
  --top-k 10 `
  --repetitions 32 `
  --profile full

cargo run --release -p phoenix-memory-lock -- qps-qualify `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --suite memory-lock\qps-mixed-qualification-v1.json `
  --repetitions 1024

cargo run --release -p phoenix-memory-lock -- qps-learned-qualify `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --suite memory-lock\qps-mixed-qualification-v1.json `
  --output D:\phoenix-target-qps-learned\qps-learned-ranker-v1.json `
  --repetitions 256

cargo run --release -p phoenix-memory-lock -- qps-concurrent-workload `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --workload C:\benchmarks\longmemeval-small.plmw `
  --workers 1,2,4,8,16 `
  --operations-per-worker 4096 `
  --queue-batches-per-worker 1 `
  --batch-size 1

cargo run --release -p phoenix-memory-lock -- evaluate `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --gold C:\benchmarks\longmemeval-small.plmg `
  --retrieval C:\benchmarks\longmemeval-small.plmr
```

`prepare` verifies the official source byte length and SHA-256 before parsing.
It will not prepare an unpinned or modified corpus. Cut 0 intentionally runs a
local deterministic retrieval baseline; the frozen external reader and judge
profiles are identities for later parity work, not claims of a completed model
run.
