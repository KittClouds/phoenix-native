# UNKNOWN-bank mixed evaluation protocol

**Frozen:** 2026-09-28
**Status:** protocol only; no fitting or mixed natural/synthetic evaluation run.

## Shortcut-resistant test composition

The UNKNOWN bank is synthetic by context erasure. Empty arrays and focal-marker-only arrays are easy-to-recognize inputs, so performance on those rows alone is not evidence of a useful transport gate. The held-out evaluation must mix UNKNOWN variants with natural FIT (`SAME`) and REFUSE (`DIFFERENT`) cases. Report the synthetic and natural populations separately as well as the combined decision table. A result that abstains on synthetic erasures but also abstains on nearly all natural FIT cases does not pass.

Use natural FIT/REFUSE labels only from the authorized nonsealed review population. Do not open sealed holdout judgments to fill a class quota. If the eligible natural review population cannot supply base-group-held-out FIT and REFUSE cases, report the evaluation as underpowered and acquire more nonsealed natural cases first.

Keep the two fully erased UNKNOWN variants distinguishable in receipts, but do not let their count dominate the aggregate. Hold out entire `base_group_id` families before any fit or threshold selection; all synthetic variants and natural rows derived from one base stay together. Also report per relation so a single lexical pair cannot carry the result.

## Split and populations

Split by `base_group_id` before fitting. Every synthetic variant and natural row derived from a base pair stays in the same partition. The test partition must contain both:

- synthetic UNKNOWN rows from held-out base groups; and
- natural held-out FIT (`SAME`) and REFUSE (`DIFFERENT`) context pairs.

Do not count a synthetic-only score as evidence that the gate recognizes naturally insufficient evidence. Keep the natural and synthetic populations separately reported. Relation support and the local gate are learned from training groups only; a relation that misses the frozen support floor remains unsupported and must abstain. Do not move threshold selection onto the final held-out groups.

The one-sided boundary file is not train/dev/test data. First run the existing frozen decision rule over it with no fitting, and report its conditional ALLOW/REFUSE/ABSTAIN outputs by variant and base pair. The current E1 gate has no completeness guard: it returns a tree DIFFERENT prediction as REFUSE and permits ALLOW only from a pure-SAME leaf after relation support passes. Some one-sided inputs may contain enough evidence to justify REFUSE; do not label every such erasure UNKNOWN. No operational one-sided target is assigned by this protocol.

## Natural incomplete-context slice

Within the natural held-out set, count noncandidate ASCII alphanumeric tokens on each side after candidate masking. Stratify by the smaller side's count: `0–2`, `3–5`, and `6+`. These are label-blind sparse-context strata, not semantic UNKNOWN labels. Report SAME/FIT and DIFFERENT/REFUSE decisions separately within each stratum; do not infer UNKNOWN merely from short length.

For each natural sparse stratum, report base-pair-level ALLOW, REFUSE, and ABSTAIN outcomes for both FIT and REFUSE labels. A system must show that abstention on naturally incomplete FIT contexts is an explicit coverage cost, while erroneous transport on naturally incomplete REFUSE contexts remains a safety failure. Report one-sided-natural cases separately when only one endpoint has fewer than three noncandidate tokens.

## Base-pair metrics

Rows and variants are not independent units. Report row-level decisions for diagnosis and the following base-pair counts/rates as the primary table:

- natural FIT base pairs allowed, refused, and abstained;
- natural REFUSE base pairs falsely allowed, correctly refused, and abstained;
- synthetic UNKNOWN base pairs with any erroneous ALLOW, all-variant abstention, any REFUSE, or mixed decisions;
- the same synthetic outcomes separately for `BOTH_SIDES_ABSENT` and `BOTH_ENDPOINT_MARKERS_ONLY`;
- all natural outcomes by sparse-context stratum and relation.

Count a synthetic UNKNOWN base pair as an erroneous-transport failure if **any** derived variant is ALLOW. Also report per-variant rates so a model cannot hide a bad boundary behind repeated rows. Report missed FIT/ALLOW as lost opportunity, separately from erroneous transport.

For synthetic UNKNOWN base pairs, report both (a) any erroneous ALLOW and (b) whether every erased variant abstains. REFUSE on a both-sides-absent target is a non-abstaining unsupported decision, even though it is not a false transport; count it separately. For natural REFUSE, any ALLOW is false transport. For natural FIT, REFUSE/ABSTAIN is lost opportunity, not a safety error.

## Promotion boundary

The synthetic UNKNOWN bank is a training supplement, not a natural validation set. Promotion requires mixed held-out natural FIT/REFUSE behavior, a separately reported natural sparse-context slice, and base-pair-level safety. Sealed retrieval holdouts, qrels, and production serving stay closed for this evaluation. No retrieval or authority change follows from this protocol alone.
