# P1P3 label-blind acquisition receipt

- Date: 2026-09-24
- State: blind packets ready; human review not started
- Branch: `codex/phoenix-native-p1p3-consensus-observability-20260924`

## Result

The frozen label-blind intake produced four candidate relations and 120 physical context pairs. All 13 corpus hashes verified. The selected graphs contain 48 occurrence nodes across 48 distinct documents; no selected document overlaps the 464 prior reviewed documents, and no document crosses candidate or fit/holdout boundaries. Every candidate has two six-context graphs spanning at least three corpora, with the frozen structural-diversity requirements met.

| Candidate relation | Fit graph corpora | Holdout graph corpora | Fit overlap IQR | Holdout overlap IQR |
| --- | ---: | ---: | ---: | ---: |
| pluck ↔ pull | 6 | 5 | 0.1379 | 0.1026 |
| bout ↔ tear | 5 | 5 | 0.1026 | 0.1111 |
| cruel ↔ vicious | 4 | 5 | 0.1000 | 0.1000 |
| deterioration ↔ worsening | 5 | 5 | 0.1000 | 0.1034 |

The private overlap strata contain 40 low-, 48 middle-, and 32 high-overlap pairs. These are sampling counts only; no reviewer sees them. Each of the three reviewers receives the same physical pairs in a separately salted order, with distinct opaque packet IDs and independent left/right orientation. Each review package contains only `packets.json`, `judgments-template.json`, and `rubric.md`.

Candidate selection excluded 51 previously used lemmas. The acquisition read prior P1N3, P1N4, P1O1, and P1O2 public packet files only to collect candidate lemmas, and their private ledgers only to collect document hashes. No prior judgment file, qrels, query labels, feature vector, model prediction, authority update, or retrieval result was opened.

## Verification and seal

The release acquisition binary passed `cargo check`, release build, Clippy with warnings denied except the inherited P1O1 scanner's `too_many_arguments` lint, and 6/6 release tests. The label-blind validator confirmed packet/template ID integrity, reviewer-set physical-pair parity, unique reviewer IDs, 48 distinct documents, zero prior-document overlap, zero fit/holdout overlap, and no feature-bearing fields in the private ledger. It then created three separate reviewer ZIPs and recorded their hashes.

Pre-review root SHA-256:

```text
ed9eb8c94805b78fdfc90c0c5d8723d933043d238a0da316da6f99714eb0cafc
```

The sealed external artifact directory is `D:\phoenix-evals\lt9-la2-p1p3-20260924\acquisition-sealed`. The authoritative machine receipts are `acquisition-receipt.json`, `pre-review-root.json`, `pre-review-qc.json`, and `reviewer-distribution-receipt.json` in that directory.

No labels have been acquired, compared, or validated. No feature representations have been materialized for modeling, no probe has been fit, and no memory or retrieval behavior has changed. The next gate is three independent human reviews. The experiment author is not one of the three reviewers; Luna output cannot substitute for any missing human pass. After label validation, the frozen unanimity target and sufficiency gates run before the private overlap metadata is joined.
