# P1P3E2 expanded relation-inventory discovery pass

**Frozen:** 2026-09-28, after the initial four-pair screen
**Status:** discovery-only extension; not an external qualification set

The initial frozen P1P3E0 four-pair inventory produced zero exact qrels counterpart-document candidates across the eight-corpus cohort. It therefore could not supply retrieval opportunities or justify a QPS baseline build. Preserve that run as a completed zero-supply result.

To avoid mistaking a narrow seed inventory for a corpus-wide absence of transport opportunities, this second pass adds the **entire pre-existing 12-direction LT9 synthetic candidate bank** as a block. No individual pair is selected from its prior outcome, and no result from the first pass changes a feature, threshold, model, or corpus. The original four pairs remain included. The expanded set is frozen in `candidate-relations-expanded.json` before its qrels outcomes are read.

All entries remain `CANDIDATE_ONLY`; the inventory is not a qualified authority artifact. Both directions are included only where the pre-existing bank explicitly lists them or the P1P3 pair inventory declares them symmetric for screening. No inferred reverse direction is added. A resulting qrels counterpart is still only an opportunity candidate, not proof of substitution validity.

Use the same eight-corpus cohort, exact-token matching, ID-derived query/document split, positive-qrels criterion, and baseline-rank boundary from `PROTOCOL.md`. Do not stop at the first corpus with an opportunity. If any counterpart candidates survive, build the unchanged BM25F/QPS baseline for every corpus with candidates; retain top-100 misses and ranks 11–100 as the opportunity bank. No model fitting, context-label join, authority mutation, or serving change is part of this pass.

Because this expansion is outcome-aware at the inventory-size level, it is a discovery extension. Any later model comparison requires the frozen document-disjoint split and a separately untouched retrieval qualification set.
