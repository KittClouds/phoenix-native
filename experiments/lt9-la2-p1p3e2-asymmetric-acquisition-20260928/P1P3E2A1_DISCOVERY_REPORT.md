# P1P3E2-A1 asymmetric acquisition screen — 2026-09-28

**Disposition:** discovery-only opportunity extension. No compatibility model was fit, no holdout context was reviewed, and no lexical authority or serving behavior changed.

## Frozen scope

This extension uses local FiQA (`train`, `dev`, `test`) and SciFact (`train`, `test`) with the 12 pre-existing candidate relation records that were not among the four qrels-exposed E1 seed pairs. The cohort and candidate subset were recorded before running the screen. Because FiQA and SciFact were previously used elsewhere, this is a discovery extension, not an external qualification corpus.

## Label-blind qrels and ranking results

The exact counterpart screen found 76 raw FiQA rows and zero SciFact rows. The frozen query/document split retained 49 FiQA rows (47 FIT, 2 HOLDOUT) and dropped 27 cross-partition rows. BM25F/QPS ranked all 49 retained rows:

| Baseline result | FIT | HOLDOUT | Total |
|---|---:|---:|---:|
| Missed top 100 | 38 | 2 | 40 |
| Rank 11–100 | 5 | 0 | 5 |
| Already top 10 | 4 | 0 | 4 |
| **Total** | **47** | **2** | **49** |

The 45 top-100 misses or rank-11–100 rows span seven directed relations. The two HOLDOUT opportunities are both `credit→loan`; their contexts were excluded from the review packet set and remain sealed for later evaluation after a gate is frozen.

| Directed relation | FIT opportunities | HOLDOUT opportunities | FIT top-10 controls |
|---|---:|---:|---:|
| `bank→lender` | 6 | 0 | 0 |
| `car→vehicle` | 9 | 0 | 1 |
| `credit→loan` | 4 | 2 | 0 |
| `insurance→coverage` | 2 | 0 | 1 |
| `loan→debt` | 13 | 0 | 0 |
| `stock→bond` | 5 | 0 | 1 |
| `vehicle→car` | 4 | 0 | 1 |

This is a useful opportunity expansion: new FiQA rows add nine `car→vehicle` and four `vehicle→car` FIT gaps, plus candidate directions beyond the earlier three-relation bank. `engine→motor` produced no FiQA or SciFact counterpart rows, so this cohort did not acquire the requested SAME-context support for that relation.

## Fit-only context review bank

Prepared 47 blinded packets from every retained FIT counterpart row, including the four top-10 retrieval controls. The packet set therefore does not expose which rows are retrieval gaps. Both HOLDOUT rows are absent. The visible packet fields contain the directed relation and masked local query/document contexts; qrels, rank, dataset, split, and document identifiers remain in the private ledger.

The review batch is opportunity-focused; it does not itself guarantee hard-negative or UNKNOWN labels. The next acquisition should continue targeting those support gaps, especially counterexamples for `car→vehicle` and `vehicle→car`, plus compatible `engine→motor` contexts. Labels from this batch must be joined only after the reviewer file is sealed. Do not use the sealed HOLDOUT rows to choose features, thresholds, or a model.

Review files are in `D:\phoenix-evals\lt9-la2-p1p3e2-asym-fit-context-review-20260928\`: `review-packets.json`, `judgments-template.json`, and `rubric.md`. Keep `private-ledger.json` and `acquisition-receipt.json` closed during review.

## Reproducibility and limits

The frozen QPS baseline output reran byte-identically (ranked JSONL SHA-256 `316d6964339042db08460d3c00d4ee791b8e08573672525d4c0443f0ad9d7fef`). Packet QA verified 47 unique packet IDs in the same order as the blank template and private ledger, FIT-only ledger rows, no qrels/rank/dataset fields in visible packets, and masking of exact candidate forms from displayed contexts.

These are qrels-positive counterpart candidates, not proof of lexical compatibility. The FiQA/SciFact cohort is discovery-only; the two-row HOLDOUT is small and confined to one relation and one previously used corpus. This result does not qualify a compatibility gate or establish safe transport.
