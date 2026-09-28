# P1P3E2 synthetic UNKNOWN bank

**Frozen:** 2026-09-28

**Purpose:** add explicit abstention examples to the engineering transport bank.
**Status:** synthetic-by-construction; not human review, not natural semantic UNKNOWN ground truth.

## Label meaning

`UNKNOWN` means the local compatibility gate has insufficient observable context to justify either transport or rejection. It does **not** claim that the underlying lexical relation is semantically ambiguous. The natural source contexts may have previously been reviewed; their prior labels are not read or copied by this builder.

## Inputs and firewall

The builder consumes only the visible `review-packets.json` files and the `partition` field from their corresponding `private-ledger.json` files. It selects FIT rows, excludes HOLDOUT rows before context transformation, and reads no judgment files, qrels values, retrieval ranks, or opportunity outcomes. The context packet `judgment` field is ignored. Only context arrays are copied into `model_input`; relation identifiers and source references remain metadata for grouping and provenance and must not be model features.

Inputs are the original P1P3E2 context bank and the later A1 fit-only context review. Exact duplicate relation/context payloads are deduplicated before variants are produced. Candidate source/target forms must already be masked from every context string; the builder fails closed if either candidate token appears. No packets or judgment templates are created.

## Frozen variants

For each unique natural FIT context pair, produce four synthetic examples:

1. `QUERY_SIDE_ABSENT`: query context is empty; document context remains.
2. `DOCUMENT_SIDE_ABSENT`: document context is empty; query context remains.
3. `BOTH_SIDES_ABSENT`: both contexts are empty.
4. `BOTH_SIDES_REDACTED`: each side is represented by one opaque `[CONTEXT_REDACTED]` sentinel.

Every variant has `target=UNKNOWN`, `target_semantics=INSUFFICIENT_OBSERVABLE_LOCAL_CONTEXT`, and `label_origin=SYNTHETIC_BY_CONTEXT_ERASURE`. These cases train/check abstention under information loss; they do not estimate how frequently natural text is genuinely ambiguous.

## Split and use constraints

All variants derived from one `base_group_id` must remain in the same future train/dev/test partition. Do not split variants independently. This artifact is an UNKNOWN-only supplement; it cannot be used alone to fit a compatibility gate. Do not infer relation support, promote lexical authority, run retrieval, or change serving from this bank.

The two sealed `credit→loan` HOLDOUT rows remain excluded. Output is an external evaluation artifact; Git records the builder, protocol, tests, and hashes only.
