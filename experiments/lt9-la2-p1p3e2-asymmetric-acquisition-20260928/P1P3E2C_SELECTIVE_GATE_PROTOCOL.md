# P1P3E2-C Selective Gate Development and Evaluation Protocol

**Protocol date:** 2026-09-28
**Status:** frozen protocol; no new candidates acquired, no feature join, no model fit, and no retrieval run.
**Scope:** engineering pilot for selective lexical transport. This is not external qualification, a natural-language learning claim, an authority promotion, or serving approval.

## Decision and scope

The only eligible directed relations are the three that met the frozen P1P3E2-B TRAIN support contract:

```text
engine -> motor
insurance -> coverage
stock -> bond
```

Gate 0 is a fixed relation-support lookup. These three relations may reach the local evidence gates. The following six remain `UNSUPPORTED_ABSTAIN` and are excluded from fitting, threshold selection, and model evaluation:

```text
bank -> lender
bank -> water
car -> vehicle
credit -> loan
loan -> debt
vehicle -> car
```

The 12 unopened `stock→bond` reserve rows remain sealed. P1P3E2-B produced 312 new natural labels (208 SAME, 86 DIFFERENT, 18 UNKNOWN). The natural UNKNOWN monitor remains 18/20 across five relations; no acquisition is authorized to fill the two-label gap.

P1P3E2-B support counts are training support only. Training support for the eligible relations is:

| Relation | LEGACY-TRAIN SAME/DIFFERENT | TRAIN-NEW SAME/DIFFERENT | Combined TRAIN SAME/DIFFERENT |
|---|---:|---:|---:|
| `engine→motor` | 0 / 5 | 16 / 7 | 16 / 12 |
| `insurance→coverage` | 3 / 0 | 16 / 10 | 19 / 10 |
| `stock→bond` | 0 / 6 | 9 / 4 | 9 / 10 |

All three meet the frozen support contract: at least 8 combined natural base groups per class and at least 4 TRAIN-NEW base groups per class. The current pilot partitions are small:

| Current partition, supported relations only | SAME | DIFFERENT | Future use |
|---|---:|---:|---|
| DEV-NEW | 8 | 7 | Exploratory DEV only |
| TEST-NEW | 6 | 9 | Historical pilot diagnostics only |

Those labels were already opened for support accounting. DEV-NEW may be combined with DEV-EXT for exploratory threshold selection. TEST-NEW is excluded from model selection and final evaluation; it cannot be described as fresh qualification evidence.

## One bounded evaluation extension

Before fitting, acquire one new, label-blind extension from the exact P1P3E2-B hash-locked source snapshots enumerated in the machine lock. It uses only the three supported relations, new query/document identities, and no new corpus. This bounded extension exists to improve development calibration and provide one fresh test; it is not an open-ended support hunt.

```text
DEV-EXT:  16 packets per eligible relation; 48 total
TEST-EXT: 20 packets per eligible relation; 60 total
```

Use the frozen B extraction contract: Unicode JSONL `_id`/`text` records; earliest case-insensitive whole-token source occurrence in query and target occurrence in title-plus-text; 18-token-radius windows (maximum 37 tokens); frozen masking and lowercase ASCII-alphanumeric tokenizer/stop list; and the namespaced E2C deterministic SHA-256 keys in the lock. For each relation and lane, rank eligible query/document pairs by the frozen lane key (semantic-near: descending content Jaccard then shared count; sense-contrast: ascending Jaccard then shared count; sparse/boundary: smaller-side token count, then side-count imbalance), and break ties by the E2C seeded SHA-256 pair key and canonical dataset/query/document IDs. Select the fixed DEV and TEST lane quotas in that order, with DEV assigned before TEST. Enforce one global dataset-scoped query/document identity per partition across all three relations; reuse within a partition is allowed. Collapse exact relation-plus-masked-context duplicates before allocation. Lane names describe label-blind acquisition heuristics only; they are not labels or predictions.

Apply a strict per-relation/per-partition corpus cap of half the packet quota (8 DEV and 10 TEST from any one corpus). If source availability, identity disjointness, duplicate collapse, or this cap prevents a quota from filling, report the partition underfilled and stop; do not relax constraints or add sources. Exclude all candidate identities present in E2B primary or reserve queues—including the unopened `stock→bond` reserve—as well as all prior legacy, E1/E2 bank, and retrieval-canary identities. The machine lock binds the source snapshot list and inherited exclusion projection hashes.

The reviewed class floors are at least 8 SAME and 8 DIFFERENT per relation in DEV-EXT, and at least 10 SAME and 10 DIFFERENT per relation in TEST-EXT. These are post-review sufficiency checks, not acquisition labels. If all floors pass, threshold development has 32 SAME / 31 DIFFERENT base groups (including current DEV-NEW), and TEST-EXT has 30 / 30; this is still pilot-sized evidence. Since each fixed packet quota equals the sum of its two class floors, any UNKNOWN or discordant item necessarily makes that relation/partition underpowered; this strictness is intentional and prevents outcome-conditioned backfill. Review every frozen packet once. Do not replace rows, open a reserve, or continue sampling after seeing labels. If any DEV-EXT floor is missed, stop before feature joining or fitting. If a TEST-EXT floor is missed, development may be reported but final test scoring is underpowered; do not replace rows or make a final pilot pass/fail claim. Any later acquisition requires a new protocol.

Use query/document identities disjoint from all P1P3E2-B primary and reserve rows, legacy discovery pairs, E1/E2 opportunity banks, and retrieval canaries. Keep identities disjoint between DEV-EXT and TEST-EXT; collapse exact duplicate masked context pairs before the split. No qrels, ranks, retrieval outcomes, corpus IDs, candidate relation support counts, or model outputs may influence candidate selection or review.

Review uses the frozen masked-context rubric. A single designated reviewer pass is acceptable for this engineering pilot; record reviewer/source provenance and do not claim independent-human consensus. The candidate relation is visible to the reviewer, while source/target forms remain masked in the contexts. Labels are `SAME`, `DIFFERENT`, or `UNKNOWN`; short context is not automatically UNKNOWN.

## Frozen partitions and data use

| Data | Permitted use |
|---|---|
| LEGACY-TRAIN plus P1P3E2-B TRAIN-NEW and opened TRAIN reserves, eligible relations only | Fit Gate 1 and Gate 2; relation support is fixed by the B receipt. Unsupported relations are absent from both stages. |
| Eligible-relation synthetic UNKNOWN variants derived from TRAIN bases | Fit Gate 1 only; never contribute to relation support or Gate 2. Unsupported-relation variants are excluded entirely. |
| Current DEV-NEW natural labels and synthetic UNKNOWN derivatives of its base groups | Exploratory threshold selection only; report its contribution separately from DEV-EXT. |
| Current TEST-NEW | Historical pilot diagnostics only; not model selection or final evaluation. |
| DEV-EXT natural labels and synthetic UNKNOWN derivatives of DEV-EXT base groups | Combine with DEV-NEW for exploratory threshold selection, then freeze the operating point. |
| TEST-EXT natural labels and its synthetic UNKNOWN derivatives | One final evaluation after model and threshold receipts are sealed. |
| Unopened `stock→bond` reserves and retrieval canaries | Remain sealed and unused. |

All split membership is by `base_group_id`; every synthetic derivative inherits its natural base's partition. No base group, query ID, document ID, or exact context identity may cross partitions. The extension's TEST labels are kept separate from fitting inputs; record/hash predictions before joining those labels for the final report.

Synthetic UNKNOWN is limited to the frozen `BOTH_SIDES_ABSENT` and `BOTH_ENDPOINT_MARKERS_ONLY` transformations. Derive the two variants from every eligible-relation natural base group in its fixed partition: TRAIN derivatives train Gate 1 only, DEV-NEW/DEV-EXT derivatives support development abstention checks, and TEST-EXT derivatives are scored only after prediction receipts are sealed. The prior 184 legacy variants remain LEGACY-TRAIN only, filtered to the three eligible relations before either training or evaluation; unsupported-relation variants are excluded entirely. One-sided erasures remain unlabeled audit cases and are not used as training targets or scored as truth.

## Frozen feature contract

The cheap lexical models receive only masked query/document context features; their feature vectors contain no relation ID. Gate 0 alone uses the directed relation key for the fixed support lookup. The frozen LFM readout is a separately declared candidate-conditioned arm and receives the literal directed relation line plus both masked contexts, as specified below; its relation-redacted probe measures dependence on that conditioning. Neither family may use source/target token occurrences or presence in the context, corpus/document/query ID, split, lane, qrels, rank, retrieval outcome, acquisition support count, or authority status as an input.

Use the P1P3E2-B tokenizer and fixed stop list (bound by the parent builder hash) over the visible noncandidate context. Freeze these feature groups before feature materialization:

1. Content lexical overlap: lowercase content-token sets; shared-token count, Jaccard, and weighted Jaccard. Empty-union ratios are zero.
2. Rarity: `idf(t)=ln((N+1)/(df(t)+1))+1`, where `N` is the number of TRAIN base groups and `df(t)` counts TRAIN base groups containing `t`. Weighted Jaccard is the IDF sum over the intersection divided by the IDF sum over the union; a zero denominator yields zero.
3. Ordered evidence: exact shared contiguous content bigram/trigram counts and Jaccards; exact-match flags for the nearest content token on corresponding left and right sides of `[SOURCE]` / `[TARGET]`.
4. Sidedness and distance: content-token overlap for left↔left, right↔right, left↔right, and right↔left; bins for nearest shared-token distance from the placeholder: `1`, `2–3`, `4–7`, `8+`, `no shared token`.
5. Cue evidence: presence per side and agreement of the fixed cue set `{no, not, never, without, neither, nor, cannot, can't, lack, lacks, lacking}`; no learned or hand-added relation exceptions.
6. Structural evidence: visible noncandidate token counts, placeholder position fraction, placeholder-at-boundary flags, and left/right side lengths. Field labels are receipt metadata only because query/document field roles are fixed in this bank.

Two predeclared ALLOW guards apply to every model:

* Function-word overlap alone cannot authorize transport.
* Structural-only evidence cannot authorize transport. An ALLOW requires at least one exact shared non-stopword content token or content n-gram between the two masked contexts.

The guards may force ABSTAIN; they do not alter model scores, labels, or the support table. Do not add features, relation exceptions, weights, thresholds, or fallback rules after inspecting DEV-EXT outcomes.

## Gate architecture and model set

```text
Gate 0: fixed supported-relation lookup
    unsupported -> ABSTAIN without invoking a local model
    supported -> continue

Gate 1: evidence sufficiency
    insufficient -> ABSTAIN
    sufficient -> Gate 2

Gate 2: local SAME score
    score >= T_allow and both ALLOW guards pass -> ALLOW_TRANSPORT
    score < 0.50 -> REFUSE_TRANSPORT
    otherwise -> ABSTAIN
```

REFUSE and ABSTAIN both leave original retrieval unchanged; receipts preserve the distinction. No model rewrites the query or adds a ranking bonus. If ALLOW is issued, the authorized counterpart may participate at the frozen lexical weight; BM25F continues to rank candidates.

Compare only this frozen set on identical base-group partitions and declared input contracts:

* E1-style depth-3 CART refit on E2C TRAIN, as the historical simple-model control;
* L2-regularized logistic cascade, primary candidate;
* one shallow boosted-tree cascade, nonlinear comparator;
* three-class regularized logistic model, architecture control only, trained on natural TRAIN labels plus permitted TRAIN synthetic UNKNOWN variants. Its `P(SAME)` output receives the shared allow-threshold check; predicted UNKNOWN maps to ABSTAIN and predicted DIFFERENT maps to REFUSE. It cannot bypass Gate 0 or either ALLOW guard.

No broad hyperparameter search. Use fixed scikit-learn estimators and parameters: depth-3 `DecisionTreeClassifier`; `LogisticRegression(C=1.0, penalty="l2", solver="lbfgs", max_iter=1000, class_weight=None)`; and one `HistGradientBoostingClassifier(max_iter=50, max_leaf_nodes=3, learning_rate=0.05, l2_regularization=1.0, early_stopping=False)`. Set every estimator random seed to `20260928` where applicable; other parameters remain library defaults. The frozen typed readouts use the same L2 logistic settings. Record and hash the exact Python/scikit-learn and feature-code versions before fitting. The relation ID is not a feature for the cheap lexical models. Use one shared `T_allow` across all three supported relations; no per-relation threshold search. Gate 1 uses its fixed 0.50 decision threshold and receives no DEV-fitted threshold. The three-class logistic control cannot authorize unless the shared `T_allow` and both ALLOW guards pass.

After the cheap models and their DEV operating points are frozen, compare `LFM2.5-230M-Base` and `LFM2.5-1.2B-Base` as frozen representations with tiny typed readouts, before TEST-EXT is scored. Do not fine-tune either backbone or generate text. Serialize plain UTF-8 input exactly as `RELATION: source -> target\nQUERY: masked query context\nDOCUMENT: masked document context`; empty context is an empty field, with no sentinel text. Use the final-layer hidden state at the final non-padding prompt token as the fixed representation; do not add a chat template, truncate, or pool generation tokens. Fit the same L2 logistic readouts (`sufficient?`, `compatible?`) on the same TRAIN base groups, and apply the same DEV threshold constraints. Record exact model revision/hash, tokenizer hash, serialization, extraction layer, and inference cost before TEST-EXT. If either exact model artifact is unavailable or cannot be pinned, record it unavailable; do not substitute another model. A secondary relation-redacted probe omits the relation line and remains diagnostic only. Freeze the full candidate roster, model/readout hashes, thresholds, and selection rule before scoring TEST-EXT.

Gate 1 is trained as sufficient versus insufficient. Natural TRAIN SAME/DIFFERENT rows are sufficient; natural TRAIN UNKNOWN and permitted synthetic TRAIN UNKNOWN rows are insufficient. Gate 2 is trained only on natural TRAIN SAME versus DIFFERENT. Synthetic UNKNOWN never trains the compatibility boundary. Unsupported relations are absent from both stages.

## Development selection rule

Threshold selection is exploratory engineering calibration, not a claim of zero risk. Select one global `T_allow` on combined current DEV-NEW + DEV-EXT by maximizing natural SAME base-group ALLOW coverage subject to all of the following observed development constraints. Search only `[0.50, 1.00]` over the sorted unique score cut points induced by development predictions; this is the one predeclared threshold-selection operation, not a broad hyperparameter search.

* zero ALLOW among natural DIFFERENT base groups, both pooled and separately in each supported relation;
* zero ALLOW among natural UNKNOWN and synthetic UNKNOWN base groups;
* both predeclared ALLOW guards pass for every ALLOW.

If multiple thresholds tie on SAME coverage, choose the highest (most conservative) threshold. If no threshold satisfies the constraints, the pilot has no eligible ALLOW operating point; do not relax the constraint. Report counts and exact binomial confidence intervals so zero observed errors are not presented as zero underlying risk.

Select the model using only DEV-NEW + DEV-EXT: among candidates with a feasible threshold, maximize natural SAME coverage under the same safety constraints; ties prefer the L2 logistic cascade, then the E1-style CART, then the shallow boosted tree, then 230M, then 1.2B. The three-class model remains a control and cannot win deployment selection. Current TEST-NEW and TEST-EXT labels do not select the model or threshold.

## Diagnostics and final pilot evaluation

After model/threshold selection is sealed, run mandatory non-gating leave-one-relation-out diagnostics on combined DEV-NEW + DEV-EXT. For each held relation, train on the other two supported relations' TRAIN rows, set the threshold using only those two relations' DEV-NEW + DEV-EXT rows, then report the held relation's DEV-NEW + DEV-EXT decisions. Bypass Gate 0 only for this diagnostic and label it clearly; this does not grant support or authorize transport for an unsupported production relation. Report each directed relation separately. Do not change the selected model or thresholds from these diagnostics.

After the cheap gate is frozen, TEST-EXT is scored once. Primary units are base groups. Report per relation and pooled:

* SAME: ALLOW / REFUSE / ABSTAIN and ALLOW coverage;
* DIFFERENT: false ALLOW / correct REFUSE / ABSTAIN;
* natural UNKNOWN: ALLOW / REFUSE / ABSTAIN;
* synthetic UNKNOWN: any ALLOW, all-variant ABSTAIN, any REFUSE, and mixed variants;
* sparse natural contexts by minimum-side token bins 0–2, 3–5, and 6+;
* support-gate abstentions for all six unsupported relations;
* exact confidence intervals, coverage, and deterministic replay identity.

Frozen **engineering-pilot** pass conditions for TEST-EXT are: zero observed ALLOW on natural DIFFERENT (pooled and per relation); zero ALLOW on any available natural or synthetic UNKNOWN; at least 25% natural SAME ALLOW coverage pooled; and at least 90% of synthetic-UNKNOWN base groups route all variants to ABSTAIN. Any miss is reported as a pilot failure or underpowered result; do not tune on TEST-EXT. Report exact binomial intervals for false ALLOW: even 0/10 DIFFERENT errors in one relation leaves a wide upper confidence bound, so this pilot cannot establish a low operational risk. These criteria do not qualify serving or establish generalization beyond the three supported relations and this source cohort.

Retrieval, authority updates, lexical promotion, and serving remain closed. Retrieval evaluation requires a later frozen bank and a separate authorization after the observer result is reviewed.
