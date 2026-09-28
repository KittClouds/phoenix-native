# P1P3E2 synthetic UNKNOWN bank and one-sided audit

**Date:** 2026-09-28
**Disposition:** UNKNOWN supplement built; one-sided examples quarantined for decision-rule audit; no review packet created.

The revised bank contains **184 synthetic UNKNOWN examples** derived from **92 unique natural FIT context pairs**. The two source sets contributed 46 FIT rows from the earlier E2 context bank and 47 FIT rows from the A1 fit-only packet bank; one exact relation/context duplicate was collapsed. Each base contributes two definite no-local-evidence variants. The previous 368-row v1 artifact is preserved and hash-bound, but superseded for fitting because its word-like `[CONTEXT_REDACTED]` marker could become a lexical feature, and it assigned UNKNOWN to one-sided cases before checking the gate boundary.

| Variant | Rows | Construction |
| --- | ---: | --- |
| `BOTH_SIDES_ABSENT` | 92 | Remove both context sides. |
| `BOTH_ENDPOINT_MARKERS_ONLY` | 92 | Keep only `[SOURCE]` / `[TARGET]` focal placeholders, which the current feature extractor excludes from lexical evidence. |

The target means **insufficient observable local context to authorize or refuse transport**. It is synthetic-by-construction, not a human judgment and not evidence that the underlying lexical relation is ambiguous. Every generated row is marked `SYNTHETIC_BY_CONTEXT_ERASURE`. The 184 one-sided variants are in a separate audit file with no target label; all four rows for a base share `base_group_id` and must stay together in any later split.

The builder used only the visible context packet files and the FIT/HOLDOUT partition field needed to exclude non-FIT rows. It did not open human judgment files or use qrels grades, retrieval ranks, or opportunity outcomes. The A1 sealed opportunity holdouts were absent from its fit-only packet input; non-FIT rows present in the earlier context bank were filtered before transformation. Candidate source/target forms are checked for absence from natural context text, and the example schema exposes only the context arrays under `model_input`; relation/source references are metadata.

The frozen E1 `P_full_local` feature path and tree were replayed without fitting or labels. This is a **conditional tree-path diagnostic only**: relation support was not applied, and the current E1 wrapper would abstain for every E2 relation because those relation IDs have no E1 support. Under that conditional tree path, each both-absent variant routed to `REFUSE` for all 92 bases. The tree therefore does not itself express “no evidence means abstain”; an evaluation must not mistake this synthetic score for useful UNKNOWN recognition.

The one-sided cases were left unlabeled and audited separately. Conditional tree outputs were:

| Variant | ALLOW | REFUSE | ABSTAIN | Mixed across context pairs |
| --- | ---: | ---: | ---: | ---: |
| `QUERY_SIDE_ABSENT` | 3 | 4 | 74 | 11 |
| `DOCUMENT_SIDE_ABSENT` | 15 | 58 | 16 | 3 |

At base-pair level, the two one-sided variants produced 13 abstain/abstain pairs, 45 abstain/refuse, 13 abstain/allow, 3 abstain/mixed, 1 allow/abstain, 2 allow/refuse, 2 mixed/abstain, 2 mixed/allow, 7 mixed/refuse, and 4 refuse/refuse. Thus one surviving side can lead to a non-abstaining decision in this frozen tree, including some `ALLOW`s. This is a reason to inspect the decision boundary, not to assign UNKNOWN to all one-sided erasures. No one-sided label or policy change was made.

`MIXED_ABSTAIN` is an audit summary: the multiple query/document context combinations produced different tree actions. It does not claim the frozen application wrapper itself uses this aggregation policy.

The complete base-pair cross-tab is:

| `QUERY_SIDE_ABSENT` | `DOCUMENT_SIDE_ABSENT` | Base pairs |
| --- | --- | ---: |
| ABSTAIN | ABSTAIN | 13 |
| ABSTAIN | ALLOW | 13 |
| ABSTAIN | MIXED_ABSTAIN | 3 |
| ABSTAIN | REFUSE | 45 |
| ALLOW | ABSTAIN | 1 |
| ALLOW | REFUSE | 2 |
| MIXED_ABSTAIN | ABSTAIN | 2 |
| MIXED_ABSTAIN | ALLOW | 2 |
| MIXED_ABSTAIN | REFUSE | 7 |
| REFUSE | REFUSE | 4 |

The current evaluation plan explicitly mixes base-group-held-out synthetic UNKNOWN variants with held-out natural FIT and REFUSE cases. It reports synthetic and natural populations separately, makes the base pair the primary unit, and separately reports performance on naturally sparse contexts. A gate cannot pass by recognizing empty arrays alone: it must also retain natural FIT transport and avoid ALLOW on natural REFUSE, including within the sparse-context strata. The evaluation protocol is specified in `UNKNOWN_EVALUATION_PROTOCOL.md`; no model was fitted and no mixed evaluation was run in this update.

This is an UNKNOWN-only supplement. No fitting, retrieval, lexical-authority update, or serving change was performed. The natural human-reviewed banks and their labels remain unchanged.

Current UNKNOWN bank: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-v2-20260928\synthetic-unknown-bank.jsonl` (SHA-256 `1f7f7eb3e941975b66c1a46a5bd3978c12a64e7eda6325f9bbedaa6115e1cd4d`)

One-sided audit inputs: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-v2-20260928\one-sided-boundary-audit.jsonl` (SHA-256 `0e4d65449617c30b19fcd013157bef1f966db1bd11f24dede78ca20324442941`)

Conditional frozen-tree audit: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-v2-20260928\one-sided-gate-audit-v3.json` (SHA-256 `05de0c798a72ab1c8a5eb7ab329bd41530c389ffd87940cd76e66d98a9e4d901`).

Build receipt: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-v2-20260928\build-receipt.json`
