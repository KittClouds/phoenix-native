# LT9-LA2 P1N3 human-label result

Date: 2026-09-23
Status: **single frozen holdout scored; discovery-only and underpowered for the planned hard-negative test**

## Label intake

The human pass contained all 96 sealed packet IDs, unchanged lexical-pair/context fields, and complete allowed labels. Counts were 73 `SAME`, 23 `DIFFERENT`, and 0 `UNKNOWN`. The reviewed packet copy and validation receipt remain outside Git at `D:\phoenix-evals\lt9-la2-p1n3-20260923\human-review-pass1.json` and `human-review-pass1-validation-20260923.json`.

The reviewer was the experiment author, who knew the study hypotheses before labeling. Only the blind packets and rubric were used during the review; no private ledger or sampling strata were consulted. This is human-labeled evidence with limited reviewer independence, and no inter-reviewer reliability estimate.

## Frozen fit/holdout result

The preassigned split was 67 fit / 29 holdout. A deterministic per-candidate depth-3 CART recipe, frozen before joining labels to private features, was run once. No holdout tuning followed. Full metrics and input/code hashes are in the private report at `D:\phoenix-evals\lt9-la2-p1n3-20260923\analysis\p1n3-fit-holdout-report.json`.

| View | Holdout accuracy | Balanced accuracy | False-SAME on human DIFFERENT | Hard-positive SAME recall | Hard-negative DIFFERENT recall |
|---|---:|---:|---:|---:|---:|
| `P_full_local` | 27/29 (93.1%) | 0.879 | 1/5 (20%) | 15/15 (100%) | not estimable: 0 cases |
| `P_no_exact_overlap` | 24/29 (82.8%) | 0.579 | 4/5 (80%) | 15/15 (100%) | not estimable: 0 cases |

All five holdout `DIFFERENT` judgments were for `bank → water`; the other 24 holdout cases were `SAME` (1 bank, 10 car, 13 insurance). There were no `UNKNOWN` labels. Thus the holdout contains low-overlap hard positives but no high-overlap hard negatives, and offers no evidence about UNKNOWN calibration. `car → vehicle` had only SAME labels in fit and holdout; `insurance → coverage` had only one DIFFERENT fit example and no DIFFERENT holdout examples. Their apparently perfect holdout accuracy is not evidence of discrimination.

## Interpretation boundary

The full-local view beats its exact-overlap ablation on this small holdout, particularly on the five DIFFERENT bank cases, but the false-SAME denominators are tiny and the planned hard-negative cell is empty. The sample therefore does not qualify natural pairwise compatibility or a context router. It gives a limited indication that exact local overlap may help separate some incompatible bank contexts; a fresh, preregistered sample needs high-overlap DIFFERENT cases and both labels represented per candidate before claiming generalization.

Pairwise judgments and predictions remain non-transitive observations. The acquisition deliberately used every context instance only once, so this packet set contains no complete shared-context triangles with which to measure non-transitivity. No semantic components, lexical authority, retrieval, or serving changes were produced.
