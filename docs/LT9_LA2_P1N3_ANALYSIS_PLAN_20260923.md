# LT9-LA2 P1N3 post-label analysis plan

Date: 2026-09-23
Status: **frozen before joining human labels to private features**

## Input seal

The only label input is the validated human pass at `D:\phoenix-evals\lt9-la2-p1n3-20260923\human-review-pass1.json`, SHA-256 `54362fa6471a238e7a00154a7e98c8c7148390cd5effe9c5ac8a9139faca9604`. It was validated against the blind packet hash in the pre-review root. The reviewer is the experiment author and had prior awareness of the study hypotheses; no provenance ledger or overlap strata were used during review. This is human relevance/compatibility judgment, but reviewer independence is limited and no second-reviewer reliability estimate exists.

## Frozen model

Fit one deterministic, per-candidate, three-class CART decision tree for each feature view. The fixed settings are Gini impurity, maximum depth 3, minimum 3 fit rows in each child, unweighted classes, and no pruning or tuning. Candidate identity selects its separately fit model but is never a feature. At each split, enumerate thresholds between adjacent distinct fit values; maximize weighted Gini reduction, breaking exact ties by lexicographic feature name and then lower threshold. A leaf predicts its fit-label majority; a majority tie predicts `UNKNOWN`. A candidate/view with fewer than two observed fit classes or no legal split remains a constant fit-majority diagnostic and is reported as such.

All categorical feature vocabularies are constructed from that candidate's fit rows only. Holdout-only feature identities map to zero. The fixed view is trained once on the 67 fit packets and scored once on the 29 holdout packets; no feature choice, threshold, depth, or model choice uses holdout judgments.

### `P_full_local`

Use all available exact shared non-candidate token identities, role-tagged shared token identities, and exact shared bigram/trigram identities as binary categorical features, plus the precomputed token/bigram/trigram Jaccard values and the numeric structural/cue fields (`role_count_abs_delta`, `token_count_abs_delta`, `distance_bin_abs_delta`, support-cue equality, contradiction-cue equality, and field-kind equality).

### `P_no_exact_overlap`

Remove all shared-identity categorical features and all three Jaccard values. Retain only structural/cue fields listed above. This removes direct lexical and n-gram overlap and their scalar summaries rather than allowing overlap to leak through an aggregate feature.

## Scoring and reporting

Report the fit/holdout label counts by candidate before metrics, and mark a candidate/view underpowered when fit lacks both SAME and DIFFERENT. UNKNOWN recall/calibration is not estimable if no human UNKNOWN labels occur. On holdout, report confusion counts, exact accuracy, balanced accuracy over observed classes, per-class recall, and the dangerous false-SAME rate `P(predicted SAME | human DIFFERENT)`. Also report hard-positive performance (low-overlap SAME) and hard-negative performance (high-overlap DIFFERENT), separately by candidate and macro-averaged where supported. The stratified 96-packet sample is an observability probe, not a population-prevalence estimate.

Do not infer transitivity or construct semantic components from pairwise predictions or labels. Triangle patterns, if present in these pairings, remain descriptive pairwise observations.

## Stopping boundary

The holdout is opened once for this frozen analysis. No classifier or feature changes follow its result within P1N3. A positive result remains discovery-only and requires a separately sealed natural-context qualification. No authority or serving integration is authorized by this analysis.
