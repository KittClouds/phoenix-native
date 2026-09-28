# P1P3E2 transport-opportunity discovery — 2026-09-28

**Status:** retrieval opportunities found; compatibility support remains unqualified. No model fit, authority update, or serving change.

## Result

The initial four-pair screen produced zero exact qrels counterpart candidates. A preregistered whole-block expansion to the four seed pairs plus the pre-existing 12-direction LT9 candidate bank produced 71 exact positive-qrels counterpart rows across the frozen eight-corpus cohort. The frozen label-blind query/document split retained 47 rows (46 FIT, 1 HOLDOUT) and discarded 24 cross-partition rows.

The unchanged BM25F/QPS rank pass classified the retained rows as:

| Baseline outcome | Rows |
| --- | ---: |
| Missed top 100 | 14 |
| Rank 11–100 | 3 |
| Already in top 10 | 26 |
| Query exceeds frozen QPS group bound | 4 |
| **Total** | **47** |

The 17 top-100 misses or rank-11–100 cases are retrieval-opportunity candidates. They are all FIT rows; none are HOLDOUT. Four very long ArguAna queries exceed QPS’s 128-group limit and are reported as unsupported instead of being truncated or rewritten.

| Dataset | Missed top 100 | Rank 11–100 | Top 10 | Unsupported |
| --- | ---: | ---: | ---: | ---: |
| HotpotQA | 11 | 3 | 26 | 0 |
| NQ | 2 | 0 | 0 | 0 |
| Quora | 1 | 0 | 0 | 0 |
| ArguAna | 0 | 0 | 0 | 4 |

The opportunity candidates are concentrated in three directed relations: `car→vehicle` (6), `vehicle→car` (8), and `engine→motor` (3). All 17 are in HotpotQA, NQ, or Quora; 14 are from HotpotQA. This is a useful discovery bank, but it is not broad enough for a reliable held-out model evaluation. The relation inventory remains candidate-only, and positive qrels establish document relevance rather than lexical compatibility.

## Context review set

Prepared 47 blinded natural context-pair packets from the full frozen corpus rows: all retained qrels counterpart rows, not only the 17 ranking gaps. This prevents a reviewer from identifying opportunities from the packet itself. Exact candidate forms are displayed as the relation under review and masked inside 12-token local context windows around their actual query/document occurrences. The extractor verified each source corpus SHA-256 against the QPS baseline receipt. Qrels, rank, partition, and dataset identifiers are in a separate private ledger. All judgments remain blank.

The 47 packets do not yet provide deliberate hard-negative or UNKNOWN coverage. No qrels absence was treated as a negative label. The context bank is therefore ready for review and targeted acquisition, but not for fitting a compatibility gate.

The quick-review files are `D:\phoenix-evals\lt9-la2-p1p3e2-context-bank-final-20260928\review-packets.json`, `judgments-template.json`, and `rubric.md`. Keep `private-ledger.json` closed during review.

## Reproducibility and disposition

The frozen BM25F/QPS ranker was built against an isolated copy of `phoenix-lexical-qps`, using title weight 2.5 / `b=0.35`, body weight 1.0 / `b=0.75`, zero positional/proximity residuals, and top-100 retrieval. Its release test passed 1/1. A corrected receipt-only rerun produced a byte-identical `ranked-opportunity-candidates.jsonl` (SHA-256 `b97e20afeb2832252a8e8a6055be298894d575bd06f45c9b7def8d6ef7d8bf0b`).

The candidate split leaves zero held-out ranking opportunities and only three opportunity-bearing directions. Do not fit or tune a model on these 17 cases. Next, review the 47 blinded contexts, acquire explicitly labeled hard negatives and UNKNOWN contexts without converting unjudged qrels into negatives, and expand the candidate/dataset bank before freezing any gate or running transport lanes.

Artifacts are listed in `P1P3E2_ARTIFACT_MANIFEST.json`. The executable baseline build and frozen configuration are documented in `BASELINE_BUILD.md`; the review packet contract is in `CONTEXT_REVIEW_PROTOCOL.md`.
