# QPS learned ordering V1

QPS now has an optional deterministic relevance-learning layer over the
evidence it already computes. It does not add a second retrieval engine,
embeddings, a wider candidate pool, or a transformer reranker.

```text
one QPS candidate pool
  -> twelve bounded evidence features
  -> optional non-negative linear ranker
  -> the existing stable top-k selection
```

The twelve features cover the baseline score, lexical evidence, weighted and
complete coverage, proximity, order, phrase evidence, segment coherence,
exact-field evidence, candidate strength, length prior, and expansion quality.
The hot evaluation is twelve multiply-adds per candidate with no allocation,
hashing, I/O, locking, or dynamic dispatch.

## Relevance flywheel

`HardNegativeLedgerV1` stores pairwise judgments with hashed query and document
identities, both feature vectors, a reason, and a weight. Training is stable:
judgments are sorted by their identities, the optimizer is deterministic, and
all weights are projected non-negative so stronger evidence cannot reduce a
score.

The qualification split is fixed by stable query identity. Training queries
mine the strongest wrong candidate from the same exhaustive qualification
pool; holdout queries are never placed in the ledger. Production feedback can
later append explicit corrections without changing the serving contract.

## 2026-07-31 scoped release proof

The mixed document/conversation suite was run from
`D:\phoenix-target-qps-kammi-20260731` with 256 repetitions.

| Gate | Result |
|---|---:|
| Training judgments | 10 |
| Correctly ordered after training | 10 / 10 |
| Pairwise loss | 0.46307713 -> 0.022996433 |
| Holdout answerable queries | 9 |
| Baseline / learned MRR | 1.000 / 1.000 |
| Holdout regressions | 0 |
| Candidate-pool changes | 0 |
| Paired query overhead p99 | 200 ns |
| Absolute 160-candidate evaluation p99 | 5.9 us |

The 200 ns paired figure is specific to this small frozen suite and timer
resolution. The 5.9 us absolute 160-candidate measurement is the safer serving
cost to use for this cut. Neither number is a broad corpus or concurrent
service claim.

The artifact was 21,900 bytes with SHA-256
`3808aa5fcf75891b90975c0d5259a1c3ffde4c03c2e56cd78671ca3279855b0a`.
The model identity was
`369a0458bdfa5394e989e6da3a3f2e68d7fe647ad5d5521806e6ba9b1fa59e13`.

## Boundaries

- Current default QPS behavior is unchanged until a verified model is supplied.
- The ledger is evaluator/training input, never serving-time relevance truth.
- Filters, schema constraints, and exact identifier rules stay outside learning.
- Candidate mining is exhaustive only in the offline qualification command.
- A later tree model must preserve this feature and artifact boundary; it may
  not quietly add another retrieval arm.
