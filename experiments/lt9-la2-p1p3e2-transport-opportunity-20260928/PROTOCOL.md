# P1P3E2 Transport Opportunity Bank

**Frozen:** 2026-09-28
**Status:** label-blind opportunity mining; no compatibility fitting or authority update
**Branch:** `codex/phoenix-native-p1p3e2-opportunity-bank-20260928`

## Question

Does an available qrels-bearing retrieval corpus contain cases where a query uses one candidate lexical form, a judged-relevant document uses its counterpart without the query form, and the frozen BM25F/QPS baseline misses or materially underranks that document?

This is an engineering opportunity screen. A positive qrel establishes document relevance, not that the word substitution is semantically authorized. The four relation pairs below are frozen candidate inventory only; they are not promoted lexical authority. Both directions are screened as separate candidate directions. No mined row may transport until relation support and local compatibility are separately earned. `deterioration -> worsening` remains blocked from transport pending negative-context support.

## Frozen relation inventory

Use the four candidate pairs already present in the P1P3E0 engineering set:

```text
pluck <-> pull
bout <-> tear
cruel <-> vicious
deterioration <-> worsening
```

The screening direction is `query_form -> relevant_document_counterpart`; both orientations are included and reported separately. Candidate forms are matched as exact QPS-style tokens (Unicode alphanumeric or `_`, case-folded), with no stemming, morphology, or fuzzy matching.

## Frozen corpus cohort

Use every locally extracted qrels-bearing corpus below as one prospective cohort. Do not stop after finding a favorable dataset. FiQA and SciFact are excluded because E1 already inspected their qrels for counterpart opportunities. This screen uses qrels only to find candidate relevant-document opportunities; qrels absence is never treated as a negative compatibility label.

| Dataset root | Qrels splits included |
| --- | --- |
| `D:\phoenix-evals\beir\screen-candidates\arguana` | `test` |
| `D:\phoenix-evals\beir\screen-candidates\hotpotqa` | `train`, `dev`, `test` |
| `D:\phoenix-evals\beir\screen-candidates\nfcorpus` | `train`, `dev`, `test` |
| `D:\phoenix-evals\beir\screen-candidates\nq` | `test` |
| `D:\phoenix-evals\beir\screen-candidates\quora` | `dev`, `test` |
| `D:\phoenix-evals\beir\screen-candidates\scidocs` | `test` |
| `D:\phoenix-evals\beir\screen-candidates\trec-covid` | `test` |
| `D:\phoenix-evals\beir\screen-candidates\webis-touche2020` | `test` |

The complete corpus, query, and included qrels file hashes are recorded in the receipt. Missing inputs fail the run rather than silently shrinking the cohort.

## Candidate opportunity definition

A row is a *qrels counterpart candidate* only when all are true:

1. The query contains the directed source form as an exact token.
2. The qrels row marks a document relevant (`score > 0`).
3. The document contains the directed target form as an exact token.
4. The document contains no exact-token occurrence of the source form in any indexed field.

This identifies a judged-relevant counterpart document, not yet a retrieval opportunity. Qrels with no row or no positive grade do not label a context `DIFFERENT`.

## Baseline and opportunity boundary

Build the frozen BM25F/QPS baseline on each corpus that yields at least one qrels counterpart candidate, using the existing BEIR baseline field configuration: title weight `2.5`, title `b=0.35`; body weight `1.0`, body `b=0.75`; proximity/order/phrase/segment residual weights all zero. Search each candidate query through the standard top-100 serving path.

Report separately:

* **Top-100 miss:** relevant counterpart document is absent from baseline top 100.
* **Materially underranked:** baseline rank is 11–100.
* **Already top-10:** baseline rank is 1–10; not an E2 recovery opportunity.

The first two categories form the transport-opportunity bank. The boundary is fixed before ranking. Retrieval relevance gains are not inferred until an authorized transport lane is actually run.

## Fresh split and leakage boundary

Before reading qrels outcomes, the runner assigns query IDs and document IDs independently using SHA-256 of `P1P3E2-20260928|Q|<dataset>|<id>` and `P1P3E2-20260928|D|<dataset>|<id>`. A key is `FIT` when the first digest byte modulo 10 is below 8 and `HOLDOUT` otherwise. A candidate pair is retained only when its query and document receive the same split. Thus any query or document has one deterministic split across every relation and qrels split; no document or query crosses fit/holdout. This split is label-blind and remains frozen for any later fitting. The opportunity receipt reports discarded cross-partition rows.

No feature extraction, reviewer-label join, classifier fitting, threshold selection, authority mutation, or serving change occurs during this screen. E1's six ALLOW cases are diagnostic-only and are not added to this bank.

## Next gate

Proceed to context-bank acquisition only if the baseline-ranked bank contains at least one top-100 miss or rank-11–100 candidate. Preserve query/document text windows for those rows, then add relation-specific SAME, DIFFERENT, and UNKNOWN context support, including wrong-sense hard negatives. Freeze a document-disjoint split before fitting. If the bank is empty, record an opportunity-supply failure and stop rather than training on examples that cannot matter to retrieval.

Even a successful P1P3E2 bank remains engineering evidence; it does not qualify a learned lexical authority or change production QPS.
