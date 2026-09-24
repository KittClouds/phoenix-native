# LT9-LA2 P1O2: Acquisition Receipt Summary

Date: 2026-09-23
Status: **one candidate packet set ready; four candidates underpowered at label-blind intake**

## Intake outcome

The final label-blind replay verified all 13 corpus hashes and excluded 452 previously reviewed document-content hashes from P1N3, P1N4, and P1O1. The P1O2 cohort remained the five P1O1 `MIXED` candidates fixed in advance. It produced 30 packets for `save ↔ spare`: 15 fit-graph edges and 15 holdout-graph edges, based on 12 distinct natural contexts from 12 distinct documents across six fit and four holdout corpora. The two graphs meet the frozen context-diversity and token-overlap floors. Their token-Jaccard IQRs are 0.1026 and 0.1000.

The other four candidates remain `UNDERPOWERED` and were not replaced:

- `allotment ↔ apportioning`: a fit graph existed, but no disjoint holdout graph met the frozen requirements.
- `leaning ↔ tilt`: a fit graph existed, but no disjoint holdout graph met the frozen requirements.
- `publication ↔ publishing`: a fit graph existed, but no disjoint holdout graph met the frozen requirements.
- `occlusive ↔ stop`: no graph met the frozen requirements.

The initial pre-review implementation run generated zero packets because it selected only the first hash-ranked contexts before testing graph diversity. It was retained as an empty implementation-shortfall attempt. It exposed no packets and no labels. The final run uses the deterministic hash-ranked triplet selector, preserves the exact candidate cohort and evidence floors, and is bound by a separate final pre-review root. This correction changes no outcome-based choice.

## Seals and limits

- Final packet count: 30; exact fit/holdout split: 15/15.
- Unique occurrence contexts and source documents in the released packet set: 12/12.
- Label-blind candidate outcome: one ready, four underpowered.
- All corpus hashes verified; no query/qrels, previous labels, model fit, authority update, or retrieval run.
- Canonical review files are in the external sealed output `acquisition-sealed/blind-review/`; the older `acquisition-final/` copy is superseded and must not be used for labeling.
- Final pre-review root SHA-256: `d5d07c0cb8a8b41f39b51af82157627ab2dc4e3973fcd9a250a5870f795f1d54`.
- Final packet SHA-256: `5aede9b8e8db614217d456923c9529f3b7c222fc32ae1e05bf8a5712c844fa88`.
- Final judgment-template SHA-256: `7edf1e14265d119556f6e7b0661b6d8729ced0b039c12b902ca2766152eb57fa`.
- Final acquisition-receipt SHA-256: `3865c5cb518be6fe20dd43130fc0a5246ac67c86d17b28ab9b9a75946320008d`.
- Final protocol SHA-256: `ef1090287af5f04472ad3e085d0ad3890d001354f1bb828a6d5fb97692fb61d9`.
- Final generator binary SHA-256: `09c62abcf6fc1fb6f3ee8dbc0ea94ec839e4d93b9a0a388f6b79b641aa7d5a48`.
- Bound P1O2 source SHA-256 values: main `478c1ff03fcce2847e8660f4cffcc84a2a592b0b871603e5c533adca43cb8b7a`; feature module `7d858c30590a5626072286a5f104f99cbe503e6655641e0fad51b3a41886e1a5`.

At the acquisition boundary, this 30-row set was not yet reviewed and could test only `save ↔ spare`; it could not support a claim across all five conditional candidates. The labels were subsequently validated and passed the frozen per-candidate fit/holdout sufficiency gate for `save ↔ spare` only. The discovery-only results are recorded in `LT9_LA2_P1O2_RESULTS_20260923.md`. No authority or retrieval work was run.
