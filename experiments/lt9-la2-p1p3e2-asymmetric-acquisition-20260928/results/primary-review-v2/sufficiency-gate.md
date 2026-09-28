# P1P3E2-B Primary Review Sufficiency Gate

**Disposition:** labels and frozen sampling metadata were joined for sufficiency accounting only. No context feature join, model fit, threshold selection, retrieval run, or serving change occurred.

- Primary rows: 216 across 9 directed relations.
- Submitted labels: SAME 141, DIFFERENT 59, UNKNOWN 16.
- TRAIN support contract: 1/9 relations supported; 8/9 remain unsupported.
- Natural UNKNOWN monitor: NOT MET.

## Per-relation split and support counts

| Directed relation | TRAIN-NEW S/D/U | DEV-NEW S/D/U | TEST-NEW S/D/U | Combined TRAIN S/D | Support |
|---|---:|---:|---:|---:|---|
| `bank->lender` | 14/0/0 | 5/0/0 | 2/1/2 | 20/0 | UNSUPPORTED_ABSTAIN |
| `bank->water` | 0/14/0 | 0/5/0 | 0/5/0 | 0/16 | UNSUPPORTED_ABSTAIN |
| `car->vehicle` | 8/1/5 | 5/0/0 | 4/0/1 | 33/2 | UNSUPPORTED_ABSTAIN |
| `credit->loan` | 12/0/2 | 3/0/2 | 4/1/0 | 14/3 | UNSUPPORTED_ABSTAIN |
| `engine->motor` | 8/3/3 | 2/3/0 | 2/3/0 | 8/8 | UNSUPPORTED_ABSTAIN |
| `insurance->coverage` | 9/5/0 | 3/2/0 | 2/3/0 | 12/5 | UNSUPPORTED_ABSTAIN |
| `loan->debt` | 14/0/0 | 4/1/0 | 4/1/0 | 28/0 | UNSUPPORTED_ABSTAIN |
| `stock->bond` | 9/4/1 | 3/2/0 | 2/3/0 | 9/10 | SUPPORTED |
| `vehicle->car` | 13/1/0 | 4/1/0 | 5/0/0 | 38/1 | UNSUPPORTED_ABSTAIN |

## Coverage diagnostics

- DIFFERENT labels in the high-overlap `SEMANTIC_NEAR` lane: 14 across 4 relations.
- UNKNOWN labels in `SPARSE_OR_BOUNDARY`: 15 across 5 relations.
- Explicit natural UNKNOWN: 16 across 5 relations; monitor requires at least 20 across 4 relations.
- DEV-NEW and TEST-NEW class presence is shown per relation in the table; these labels do not contribute to support state.

## Disposition

`OPEN_FROZEN_RESERVES_FOR_UNSUPPORTED_RELATIONS`. Only relations missing the frozen training support contract may draw from the preordered reserve queue. The reserve labels remain unopened. Reviewer provenance is recorded in the external judgment seal receipt; this report does not claim independent review or consensus.
