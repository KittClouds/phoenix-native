# P1P3E2-B Natural Support Acquisition Lock

**Protocol date:** 2026-09-28  
**Branch:** `codex/phoenix-native-p1p3e2-asymmetric-acquisition-20260928`  
**Status:** frozen before candidate acquisition; no labels or model outputs used.

## Purpose and boundary

Acquire a prospective natural-context bank to fill relation-specific compatibility support. This is an acquisition and split protocol only. It does not fit a model, inspect E1 predictions, assign compatibility labels, use retrieval judgments to form negative examples, modify lexical authority, or change serving/retrieval behavior.

The existing 92 unique natural context pairs are `LEGACY-TRAIN` / discovery material only. They are never DEV or TEST. Existing E2 retrieval-opportunity contexts, reviewed packets, and the two sealed `credit→loan` retrieval canaries are excluded by query/document identity. The canaries remain outside every acquisition, fit, development, and test population.

## Frozen directed relation roster

The previous support counts below guide search only. They are not labels and may not be shown to reviewers.

| Directed relation | Legacy SAME | Legacy DIFFERENT | Primary acquisition need |
|---|---:|---:|---|
| `bank→lender` | 6 | 0 | DIFFERENT |
| `bank→water` | 0 | 2 | SAME |
| `car→vehicle` | 25 | 1 | DIFFERENT |
| `credit→loan` | 2 | 3 | both |
| `engine→motor` | 0 | 5 | SAME |
| `insurance→coverage` | 3 | 0 | DIFFERENT |
| `loan→debt` | 14 | 0 | DIFFERENT |
| `stock→bond` | 0 | 6 | SAME |
| `vehicle→car` | 25 | 0 | DIFFERENT |

No other relation may enter this acquisition.

## Frozen source cohort

Read only query text and corpus document text from these local BEIR source snapshots:

1. `fiqa`
2. `scifact`
3. `arguana`
4. `hotpotqa`
5. `nfcorpus`
6. `nq`
7. `quora`
8. `scidocs`
9. `trec-covid`

For each source, both `queries.jsonl` and `corpus.jsonl` are inputs. The exact canonical paths, byte lengths, and SHA-256 hashes are frozen in `P1P3E2B_ACQUISITION_LOCK.json` before content is mined. `webis-touche2020` is sealed and excluded. No qrels, rankings, retrieval outputs, document relevance, or search scores are acquisition inputs.

## Candidate construction

Create at most 24 primary query/document context pairs per directed relation (216 total) plus at most 12 reserves per relation (108 total). Candidates are natural query and document occurrences, not generated text. The query must contain the directed source form and the document must contain its target form. Mask those occurrences as `[SOURCE]` and `[TARGET]` in reviewer-visible contexts. Candidate strings are not context features.

Each relation has three acquisition lanes, eight primary candidates per lane:

* `SEMANTIC_NEAR`: deterministic high non-candidate lexical-context overlap.
* `SENSE_CONTRAST`: deterministic low non-candidate lexical-context overlap, subject to available natural contexts.
* `SPARSE_OR_BOUNDARY`: deterministic preference for short, asymmetric, fragment-like, heading-like, or boundary-near contexts.

Lane membership is an acquisition heuristic only. It is not a predicted label, expected class, or evidence of validity. Lane identity is private during review. Do not create a negative label from qrels absence or from a low-overlap lane.

The executable extraction contract is: Unicode JSONL records with `_id` and `text` fields; query context is the first 18-token-radius window around the earliest source-token occurrence (maximum 37 tokens), and document context is the first title-plus-text 18-token-radius window around the earliest target-token occurrence (maximum 37 tokens). Both relation terms are replaced case-insensitively with `[SOURCE]` and `[TARGET]`. Tokenization for lane ranking is lowercase ASCII alphanumeric tokens, excluding the fixed stop list encoded in the builder. Per dataset/relation, retain at most 512 query occurrences and 2,048 document occurrences by ascending seeded SHA-256 key; for each retained query, consider at most 96 documents selected by ascending seeded pair hash. Candidate-pair Jaccard and shared-content-token count are descriptive selection keys only. Semantic-near sorts by descending Jaccard then shared count; sense-contrast sorts ascending; sparse/boundary sorts by minimum side-token count, then descending side-count imbalance, then deterministic hash. Hash ties break by canonical dataset/query/document IDs.

The corpus cap is 12 primary and 6 reserve candidates per relation per corpus whenever source availability permits. If quotas cannot be met under the frozen source list and caps, report underfill; do not add a corpus, loosen the cap, or inspect outcomes to curate replacements. Exact duplicate masked context pairs collapse before splitting. Query IDs and document IDs are disjoint across TRAIN-NEW, DEV-NEW, and TEST-NEW. Every identity present in the legacy, prior E2 opportunity/review banks, or canary set is excluded. The exclusion projection may read only dataset, query ID, and document ID fields from prior ledgers; qrels split/grade, rank, retrieval status, and outcome fields are not read or used.

## Freeze splits before labels

Use the frozen seed and the canonical deterministic ordering described in the machine lock. Rank each relation/lane pool by its frozen lane key, use the seeded SHA-256 pair key as tie-break, then greedily fill the fixed split quotas in TRAIN-NEW, DEV-NEW, TEST-NEW order. Reject any candidate whose dataset-scoped query or document identity has already been assigned to another split; reuse within the same split is allowed. Apply corpus caps during this same frozen pass. This is all completed before review labels exist. Per relation, the fixed lane allocations are:

| Lane | TRAIN-NEW | DEV-NEW | TEST-NEW | Total |
|---|---:|---:|---:|---:|
| `SEMANTIC_NEAR` | 5 | 1 | 2 | 8 |
| `SENSE_CONTRAST` | 5 | 2 | 1 | 8 |
| `SPARSE_OR_BOUNDARY` | 4 | 2 | 2 | 8 |
| **Per relation** | **14** | **5** | **5** | **24** |

The 12 reserves are split evenly across lanes: four per lane per relation.

Reserves are frozen separately, are not part of the initial split, and may only be opened mechanically in the pre-recorded order to address a support shortfall. Any opened reserve enters TRAIN-NEW only and cannot change DEV/TEST. The reserve queue is consumed without viewing labels, predictions, qrels, or retrieval outcomes.

All 92 legacy unique pairs and their existing synthetic erasures remain `LEGACY-TRAIN`. The two sealed `credit→loan` retrieval canaries are `RETRIEVAL-CANARY`, outside every split.

## Review and privacy boundary

Reviewers receive only a randomized packet ID, the directed lexical relation, and the two masked natural contexts. They do not receive source corpus, query/document IDs, partition, lane, rank, qrels, retrieval-opportunity status, legacy counts, relation support, prior judgments, model output, or other reviewers' labels.

TRAIN-NEW may use one blind review pass. DEV-NEW and TEST-NEW use two independently performed blind passes when available. Concordant pairs map as `SAME/SAME→SAME`, `DIFFERENT/DIFFERENT→DIFFERENT`, and `UNKNOWN/UNKNOWN→UNKNOWN`. Any disagreement becomes `REVIEW_DISCORDANT`; it is not silently converted to UNKNOWN. It may be sent to a third blind adjudication or excluded from definite-class metrics while remaining in diagnostic receipts. Proxy/model-origin judgments are permitted for engineering continuity only when provenance is explicit; they are not represented as independent-human validation.

## Relation support contract, frozen before fitting

A relation is `SUPPORTED` only if TRAIN contains at least 8 unique natural SAME base groups and 8 unique natural DIFFERENT base groups, with at least 4 of each class from TRAIN-NEW. Legacy TRAIN may contribute to those totals. Synthetic UNKNOWN, DEV, and TEST contribute zero to relation support. Below the floor, the relation is `UNSUPPORTED` and the runtime action is ABSTAIN.

Corpus diversity and lane coverage are reported. No class or lane is relabeled to satisfy support. If the primary set yields fewer than 20 explicit natural UNKNOWN judgments across at least four relations, record that result; any additional natural sparse/UNKNOWN acquisition requires a separately frozen protocol.

## Synthetic UNKNOWN supplement

The existing 184 definite synthetic UNKNOWN variants from the 92 legacy bases remain `LEGACY-TRAIN` only. For a new natural base, synthetic variants inherit that base's partition. Generate only `BOTH_SIDES_ABSENT` and `BOTH_ENDPOINT_MARKERS_ONLY` as definite synthetic UNKNOWN variants. One-sided erasures remain audit-only and unlabeled. Synthetic UNKNOWN contributes no relation support and is reported separately from natural UNKNOWN.

## Prohibited actions until this acquisition is sealed

No feature join, fitting, threshold selection, E1 inspection, neural/frozen-model contact, qrels lookup, rank lookup, retrieval run, authority update, or serving change. After candidates and splits are frozen, review may proceed under the blind packet contract above.
