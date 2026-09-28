# P1P3E2 natural relation support map — 2026-09-28

**Disposition:** discovery-only support accounting. No model fitting, threshold selection, retrieval transport, lexical authority update, or serving change.

The matrix joins two already-reviewed natural context sets to their private ledgers only after label files were sealed or import-validated. It counts FIT partition rows only. The earlier bank's one non-FIT row is excluded before label aggregation; A1's two sealed `credit→loan` retrieval holdouts were never in the packet set. Exact duplicate context payloads are counted once in the pooled support totals.

## Review-source provenance

| Source | FIT rows | SAME | DIFFERENT | Explicit natural UNKNOWN | Retrieval opportunities (S/D/U) | Top-10 controls | Provenance |
|---|---:|---:|---:|---:|---:|---:|---|
| Prior context bank | 46 | 38 | 8 | 0 | 14 / 3 / 0 | 25 | One user-supplied pass; identity and reviewer independence unverified |
| A1 FiQA bank | 47 | 38 | 9 | 0 | 35 / 8 / 0 | Codex model-origin single pass; not independent-human review |
| **Combined event rows** | **93** | **76** | **17** | **0** | **49 / 11 / 0** | **29** | Sources remain distinguishable; one exact duplicated context pair |
| **Unique natural context pairs** | **92** | **75** | **17** | **0** | — | — | Duplicate labels agreed; no duplicate conflict |

The combined totals are descriptive, not a consensus set: the two sources have different, limited reviewer provenance. The Codex pass had access to the prior conversation's hypotheses and relation-level results, although it did not see A1 item labels or the private A1 ledger before sealing. “UNKNOWN” means an explicit natural UNKNOWN label; the 184 synthetic UNKNOWN variants remain a separate training-supplement population and are not included here.

## Per-relation natural support

Opportunity and top-10 counts below are event rows before exact-context deduplication. Unique-pair label counts are deduplicated across review sources.

| Directed relation | Unique SAME | Unique DIFFERENT | Natural UNKNOWN | Opportunity rows (S/D/U) | Top-10 controls | FIT support state |
|---|---:|---:|---:|---:|---:|---|
| `bank→lender` | 6 | 0 | 0 | 6 / 0 / 0 | 0 | Insufficient REFUSE evidence |
| `bank→water` | 0 | 2 | 0 | 0 / 0 / 0 | 1 | Insufficient FIT evidence |
| `car→vehicle` | 25 | 1 | 0 | 15 / 0 / 0 | 11 | Both classes present; REFUSE side sparse |
| `credit→loan` | 2 | 3 | 0 | 1 / 3 / 0 | 0 | Both classes present; both sides sparse |
| `engine→motor` | 0 | 5 | 0 | 0 / 3 / 0 | 2 | Insufficient FIT evidence |
| `insurance→coverage` | 3 | 0 | 0 | 2 / 0 / 0 | 1 | Insufficient REFUSE evidence |
| `loan→debt` | 14 | 0 | 0 | 13 / 0 / 0 | 0 | Insufficient REFUSE evidence |
| `stock→bond` | 0 | 6 | 0 | 0 / 5 / 0 | 1 | Insufficient FIT evidence |
| `vehicle→car` | 25 | 0 | 0 | 12 / 0 / 0 | 13 | Insufficient REFUSE evidence |

No directed relation currently has substantial natural support on both sides. Only `car→vehicle` and `credit→loan` have any observed SAME and DIFFERENT examples together; their unique-pair counts are 25/1 and 2/3 respectively. Every other relation is one-sided. **No relation meets the proposed 8-SAME / 8-DIFFERENT reference floor.** That floor remains an unfrozen engineering suggestion, not an adopted policy.

The opportunity intersection contains 60 FIT qrels-positive miss/underranked event rows: 49 SAME and 11 DIFFERENT. The `engine→motor` and `stock→bond` opportunities are all DIFFERENT in these reviews, illustrating why relevance alone cannot authorize transport. There are no reviewed natural UNKNOWN opportunity rows.

## Sparse-context accounting

Using the frozen evaluation convention, count noncandidate ASCII alphanumeric tokens across each side's visible masked context windows, remove `[SOURCE]` and `[TARGET]`, and bin by the smaller side. The 92 unique natural context pairs distribute as:

| Smaller-side token bin | Unique context pairs |
|---|---:|
| 0–2 | 0 |
| 3–5 | 5 |
| 6+ | 87 |

These are evidence-length strata, not semantic UNKNOWN labels. The natural review material still does not contain an adequate sparse-context UNKNOWN validation set.

## Consequence for the engineering program

The next need is support acquisition, not classifier fitting. Acquire natural counterexamples for `car→vehicle`, `vehicle→car`, `bank→lender`, `insurance→coverage`, and `loan→debt`; acquire compatible contexts for `engine→motor`, `stock→bond`, and `bank→water`; broaden both classes for `credit→loan`. Build natural insufficient-evidence examples separately from the synthetic UNKNOWN bank. Keep `deterioration→worsening` and all other unsupported directions from transporting until their relation-support contract says otherwise.

The count table can guide relation-first acquisition, but it cannot set a threshold or establish a decision boundary. Keep the two `credit→loan` retrieval holdouts sealed. The complete row-level join and hashes are in the external support-map artifact; the in-repository report intentionally contains aggregate counts only.

## Reproducibility

- Sealed A1 model-origin review receipt v2 (supersedes v1 provenance description; label bytes/counts unchanged): `D:\phoenix-evals\lt9-la2-p1p3e2-asym-fit-context-review-20260928\proxy-label-seal-receipt-v2.json`
- Support-map report: `D:\phoenix-evals\lt9-la2-p1p3e2-support-map-final-20260928\relation-support-matrix.md` (SHA-256 `3A9BCFE875A96F2CAC7223A56F208959AA83CBBAF1944140276A5A7F3BFC41E7`)
- Support-map JSON: `D:\phoenix-evals\lt9-la2-p1p3e2-support-map-final-20260928\relation-support-matrix.json` (SHA-256 `4EA80EF9C2E1B17AB4AD5D0364A93DA0AACE3C6806D32849DBEA95D606F36C18`)
- Rebuild script: `build_relation_support_matrix.py`; smoke/unit checks: `python -m unittest test_relation_support_matrix.py`
