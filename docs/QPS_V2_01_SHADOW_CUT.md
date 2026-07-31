# QPS V2.01 production shadow cut

## Boundary

`phoenix-lexical-qps` is the cleanroom-native promotion of the measured V2.01
positional BM25F scorer. It does not depend on the Angular/Overgraph workspace.

The serving path ID is:

```text
qps/v2.01/shadow
```

The current memory-coordinator retrieval remains authoritative. QPS runs only
after the authority packet has been assembled and returns comparison telemetry
in `ContextPacket::qps_shadow`. The receipt always identifies whether the path
was disabled, ready, empty, rejected, failed to build, or rejected an oversized
corpus.

No QPS hit is copied into `ContextPacket::items`. There is no runtime fallback
and no feature switch which makes shadow results authoritative.

## Index lifecycle

- A conversation index is rebuilt only after its V3 generation publishes.
- Publication failure cannot expose an index for uncommitted turns.
- One immutable compact index and one reusable scratch arena are retained per
  active conversation.
- The V2.01 rerank pool is hard-bounded at 160.
- Oversized corpora report `OversizedCorpus`; authority recall keeps operating.
- Warm scratch capacity growth is recorded per query.

The current cut indexes committed conversation turns. Document retrieval does
not yet have a serving command and is not silently approximated.

## Telemetry

Every ready receipt records:

- path ID and non-authority proof
- query and corpus hashes, never raw query text
- query-shape class
- corpus, authority-result and shadow-result counts
- exact top-set overlap
- posting rows and position values visited
- sparse, dense-SIMD or exhaustive selection
- scratch growth
- accumulation, selection, coherence, ordering and total nanoseconds

These timers are parent/child stages. Only `total` is a wall-clock query time;
the four stage values are children.

## Frozen workload runner

The existing LongMemEval lock now accepts:

```powershell
$env:CARGO_TARGET_DIR='D:\phoenix-target-qps-shadow'
cargo run --release -p phoenix-memory-lock -- qps-shadow `
  --manifest .\memory-lock\longmemeval-cleaned-v1.json `
  --workload C:\benchmarks\longmemeval-small.plmw `
  --output C:\benchmarks\longmemeval-qps-v201.plmr `
  --top-k 10 `
  --repetitions 32 `
  --profile full
```

The command opens only the cleaned workload. It cannot open the separately
typed gold artifact. Quality is evaluated afterward with the existing
`evaluate` command.

The runner reports paired measurements against a same-corpus BM25 Turbo
equivalent. At construction time it freezes sorted term columns, column
offsets, and contiguous precomputed `(document, BM25 impact)` rows. Serving
performs sparse column accumulation into caller-owned scratch with no BM25
scoring math. The receipt identifies this comparator as
`precomputed-impact-csc-equivalent/v1`.

It alternates execution order, records median/p95/p99/max, records the p99 of
per-query paired deltas, and reports stage percentiles, query shape counts, and
independent latency distributions for 1, 2-4, 5-8, 9-16, 17-32, 33-64, and
65-128 query groups.

`--profile` supports `full`, `lexical-coverage`, `no-proximity`,
`no-order-phrase`, `no-segment`, and `no-coverage`. Each profile writes a
separately named retrieval artifact which can be evaluated against gold after
retrieval. Disabled positional signals perform no hidden positional work.
Corpus size buckets (`0-32`, `33-128`, `129-512`, `513+` sessions) report
independent latency percentiles.

The coordinator is intentionally single-owner. The receipt marks concurrency
evaluation false instead of fabricating a multi-threaded service result.

## Gates

The provisional shadow gates remain:

- representative MRR delta is positive after separate gold evaluation
- all frozen adversarial regressions stay fixed
- oracle recall@10 remains 1.0
- rerank pool remains 160
- aggregate warm p99 overhead is at most 100 microseconds
- paired-difference p99 is at most 150 microseconds
- warm scratch capacity growth is zero
- QPS has zero authority over returned context

## Cleaned-small release decision (2026-07-30)

The official pinned 500-case cleaned-small source was split into separately
typed workload and gold artifacts. Retrieval never opened gold. The full
V2.01 scorer ran 32 alternating-order repetitions per case against the
precomputed-impact comparator:

| Metric | BM25 Turbo | QPS V2.01 full | Paired overhead |
|---|---:|---:|---:|
| median | 2.3 us | 207.2 us | 204.7 us |
| p95 | 4.8 us | 420.3 us | 416.5 us |
| p99 | 6.1 us | 528.7 us | 523.1 us |

The rerank pool stayed at 160, the observed candidate maximum was 62, the
observed reranked maximum was 60, and 16,000 measured searches produced zero
scratch-capacity growth. The full scorer's p99 is 61.6% below the original
1,378.3 us implementation, with byte-identical retrieval output.

The remaining tail is query-length driven:

| Query groups | QPS p99 | Paired p99 |
|---|---:|---:|
| 2-4 | 129.4 us | 127.6 us |
| 5-8 | 256.4 us | 253.6 us |
| 9-16 | 348.0 us | 345.6 us |
| 17-32 | 542.5 us | 538.1 us |
| 33-64 | 592.5 us | 586.6 us |

The lexical-only ablation proves that the frozen impact/index substrate is not
the problem: 9.3 us p99 versus 3.9 us BM25 Turbo p99, with 5.8 us paired p99
and zero position payloads opened. Positional coherence owns the remaining
cost.

Quality does not pass on this cohort:

| Engine | hit@10 | MRR |
|---|---:|---:|
| BM25 | 0.982 | 0.907371 |
| QPS V2.01 full | 0.980 | 0.891005 |
| QPS without coverage multiplier | 0.984 | 0.905446 |

The no-coverage result is diagnostic, not a gold-tuned production profile.
It improves hit@10 but still does not beat BM25 MRR.

**Disposition:** QPS V2.01 remains a non-authoritative specialized shadow
reranker. It must not replace recall authority. A future promotion attempt
needs a training/development cohort separate from this frozen release cohort,
must show positive held-out MRR, and must retain both latency gates. The frozen
result is a successful release lock and an honest production no-go.

## Rollback

Remove:

- `phoenix-lexical-qps`
- `phoenix-memory-coordinator::shadow`
- `ContextPacket::qps_shadow`
- the `phoenix-memory-lock qps-shadow` command

No V3 artifact migration or authority rollback is required.
