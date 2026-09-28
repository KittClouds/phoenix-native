# P1P3E2 synthetic UNKNOWN bank build

**Date:** 2026-09-28
**Disposition:** built for engineering abstention development; no review packet created.

The bank contains **368 synthetic UNKNOWN examples** derived from **92 unique natural FIT context pairs**. The two source sets contributed 46 FIT rows from the earlier E2 context bank and 47 FIT rows from the A1 fit-only packet bank; one exact relation/context duplicate was collapsed. Each base contributes one example for each of four predeclared evidence-erasure variants, yielding 92 rows per variant.

| Variant | Rows | Construction |
| --- | ---: | --- |
| `QUERY_SIDE_ABSENT` | 92 | Remove query-side context; keep document-side context. |
| `DOCUMENT_SIDE_ABSENT` | 92 | Remove document-side context; keep query-side context. |
| `BOTH_SIDES_ABSENT` | 92 | Remove both context sides. |
| `BOTH_SIDES_REDACTED` | 92 | Replace both sides with an opaque redaction sentinel. |

The target means **insufficient observable local context to authorize or refuse transport**. It is synthetic-by-construction, not a human judgment and not evidence that the underlying lexical relation is ambiguous. Every generated row is marked `SYNTHETIC_BY_CONTEXT_ERASURE`. Derived variants share a `base_group_id` and must stay together in any later split.

The builder used only the visible context packet files and the FIT/HOLDOUT partition field needed to exclude non-FIT rows. It did not open human judgment files or use qrels grades, retrieval ranks, or opportunity outcomes. The A1 sealed opportunity holdouts were absent from its fit-only packet input; non-FIT rows present in the earlier context bank were filtered before transformation. Candidate source/target forms are checked for absence from the context text, and the example schema exposes only the context arrays under `model_input`; relation/source references are metadata.

This is an UNKNOWN-only supplement. No fitting, retrieval, lexical-authority update, or serving change was performed. The natural human-reviewed banks and their labels remain unchanged.

Artifact: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-20260928\synthetic-unknown-bank.jsonl`

Build receipt: `D:\phoenix-evals\lt9-la2-p1p3e2-unknown-bank-20260928\build-receipt.json`
Bank SHA-256: `1cb5622bb56f176a88214d92456aaf9865542cb600788664c517956249a47336`
