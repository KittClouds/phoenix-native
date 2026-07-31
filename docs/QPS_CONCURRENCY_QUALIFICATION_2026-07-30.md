# QPS bounded concurrency qualification

## Decision

QPS qualifies for bounded multi-worker use without adding BM25 Turbo or any
other runtime search arm.

The serving architecture tested here is:

```text
immutable QPS index pages
  -> one bounded queue per worker
  -> fixed-size request batches
  -> worker-local SearchScratch and hit buffers
  -> deterministic results
```

There is no lock around the index, shared mutable scorer state, per-query
ranking allocation, or shared receiver contention.

## Refinements made during qualification

The realistic workload exposed a real cross-index bug. `SearchScratch::prepare`
shrunk document arrays before `begin_query` drained document IDs touched by a
larger preceding index. Reusing scratch across heterogeneous indexes could
therefore index beyond the shortened array.

Scratch storage is now grow-only. A regression test exercises:

```text
large index -> small index -> large index
```

The determinism checks also stopped allocating a temporary `Vec<u64>` for
every result comparison. They now compare hit IDs directly against the frozen
ranking slice.

Queue-depth tuning at 16 workers showed that depth 1 is the better product
default:

| Per-worker depth | Throughput | End-to-end p99 |
|---:|---:|---:|
| 1 | 27.5K QPS | 1.91 ms |
| 2 | 27.3K QPS | 2.64 ms |
| 4 | 27.9K QPS | 3.94 ms |
| 8 | 30.0K QPS | 6.23 ms |

Depth 1 retains roughly 92% of maximum measured throughput while reducing p99
by roughly 69%.

## Mixed document and conversation suite

The 24-source suite exercises ordinary, phrase, fuzzy, and no-result queries
over documents and conversations.

| Workers | Throughput | Speedup | Engine p99 | End-to-end p99 |
|---:|---:|---:|---:|---:|
| 1 | 0.811M QPS | 1.00x | 1.7 us | 133.8 us |
| 2 | 1.337M QPS | 1.65x | 2.2 us | 81.1 us |
| 4 | 2.671M QPS | 3.29x | 2.2 us | 80.1 us |
| 8 | 4.371M QPS | 5.39x | 2.2 us | 80.6 us |
| 16 | 6.561M QPS | 8.09x | 2.3 us | 76.6 us |

Every sweep reported zero scratch growth, zero ranking drift, and zero steady
private-memory growth.

## Pinned 500-case LongMemEval workload

This stress run keeps all 500 independent case indexes resident. It covers
23,867 indexed sessions and 40,156,285 positions.

| Workers | Throughput | Speedup | Engine p99 | End-to-end p99 |
|---:|---:|---:|---:|---:|
| 1 | 3.57K QPS | 1.00x | 638.3 us | 1.84 ms |
| 2 | 6.43K QPS | 1.80x | 588.7 us | 1.60 ms |
| 4 | 10.15K QPS | 2.85x | 639.0 us | 1.69 ms |
| 8 | 17.89K QPS | 5.01x | 662.0 us | 1.66 ms |
| 16 | 27.17K QPS | 7.62x | 773.5 us | 1.95 ms |

Again, ranking drift, scratch growth, and steady private-memory growth were all
zero.

The stress process held about 1.13 GB privately because it deliberately kept
500 independent indexes resident at once. That is not the memory budget of one
Phoenix production index; it is cache-pressure evidence.

## Reproduction

```powershell
$env:CARGO_TARGET_DIR='D:\phoenix-target-qps-concurrency'

cargo test -p phoenix-memory-lock -p phoenix-lexical-qps --release
cargo clippy -p phoenix-memory-lock -p phoenix-lexical-qps `
  --all-targets --release -- -D warnings

C:\phoenix-bin\qps-concurrency-qualified\phoenix-memory-lock.exe `
  qps-concurrent-workload `
  --manifest memory-lock\longmemeval-cleaned-v1.json `
  --workload C:\benchmarks\phoenix-qps-v201-release-lock-20260730\longmemeval-small.plmw `
  --workers 1,2,4,8,16 `
  --operations-per-worker 4096 `
  --queue-batches-per-worker 1 `
  --batch-size 1
```

Exact source, binary, workload, receipt, and machine identities are frozen in
`memory-lock/qps-concurrency-qualification-lock-2026-07-30.json`.
