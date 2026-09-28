# P1P3E2-A1 asymmetric acquisition screen

**Frozen:** 2026-09-28, before this extension's qrels rows were scanned

**Status:** discovery-only local cohort extension; no compatibility fitting or authority update

## Purpose

Use available local qrels-bearing corpora to extend relation support after the first P1P3E2 context review. The next data acquisition is deliberately asymmetric: seek counterexamples for `car→vehicle` and especially `vehicle→car`, seek compatible contexts for `engine→motor`, and add candidate-relation breadth. The current 47 reviewed packets remain frozen discovery evidence and are not re-used as new labels.

## Cohort and exposure

The cohort is exactly FiQA (`train`, `dev`, `test`) and SciFact (`train`, `test`), as listed in `cohort.json`. Both corpora were excluded from the original E2 screen because E1 had inspected qrels for its four seed relation pairs. Therefore this extension is **discovery-only** and cannot qualify a gate or serve as a fresh external test. The four E1 relation pair IDs (`pluck_pull`, `bout_tear`, `cruel_vicious`, `deterioration_worsening`) are excluded before screening. The remaining 12 candidate relation records come unchanged from the previously frozen E2 expanded inventory; no relation is selected from E2 compatibility labels or retrieval outcomes.

The screening cohort and candidate inventory are frozen before this run. All listed qrels splits are processed; the run does not stop on a favorable corpus. The exact query/document hash split and exact-token counterpart definition from P1P3E2 are reused without modification. A positive qrel remains relevance evidence only, never a compatibility label.

## Frozen outputs and next gate

This pass only identifies qrels-positive exact counterpart candidates and their FIT/HOLDOUT partition. Any candidate rows are ranked using the same frozen BM25F/QPS configuration as P1P3E2. The opportunity status is a top-100 miss or rank 11–100. Report relation, corpus, and partition counts, including non-opportunity rows. Do not fit a model from this discovery extension. Any context review must be a separate frozen packet acquisition that keeps HOLDOUT labels sealed until a gate is frozen.

No transport, lexical authority update, or production QPS change is in scope.
