# P1P3E2-B Reserve TRAIN Support Update

**Disposition:** sealed reserve labels were joined only to frozen sampling metadata for TRAIN support accounting. No context feature join, fitting, threshold selection, retrieval, or serving occurred.

- Primary labels: 216 packets; SAME 141, DIFFERENT 59, UNKNOWN 16.
- Opened reserve labels: 96 packets; SAME 67, DIFFERENT 27, UNKNOWN 2.
- Combined new natural labels: SAME 208, DIFFERENT 86, UNKNOWN 18.
- Support contract: 3/9 relations supported; 6/9 remain unsupported.
- Natural UNKNOWN appears across 5 relations across primary plus opened reserves.
- Natural UNKNOWN monitor: NOT MET (18/20 judgments across 5/4 relations).

## Relation support

| Directed relation | TRAIN-NEW S/D/U | DEV-NEW S/D/U | TEST-NEW S/D/U | New TRAIN groups S/D | Combined TRAIN S/D | State |
|---|---:|---:|---:|---:|---:|---|
| `bank->lender` | 26/0/0 | 5/0/0 | 2/1/2 | 26/0 | 32/0 | UNSUPPORTED_ABSTAIN |
| `bank->water` | 1/25/0 | 0/5/0 | 0/5/0 | 1/25 | 1/27 | UNSUPPORTED_ABSTAIN |
| `car->vehicle` | 15/4/7 | 5/0/0 | 4/0/1 | 15/4 | 40/5 | UNSUPPORTED_ABSTAIN |
| `credit->loan` | 23/1/2 | 3/0/2 | 4/1/0 | 23/1 | 25/4 | UNSUPPORTED_ABSTAIN |
| `engine->motor` | 16/7/3 | 2/3/0 | 2/3/0 | 16/7 | 16/12 | SUPPORTED |
| `insurance->coverage` | 16/10/0 | 3/2/0 | 2/3/0 | 16/10 | 19/10 | SUPPORTED |
| `loan->debt` | 25/1/0 | 4/1/0 | 4/1/0 | 25/1 | 39/1 | UNSUPPORTED_ABSTAIN |
| `stock->bond` | 9/4/1 | 3/2/0 | 2/3/0 | 9/4 | 9/10 | SUPPORTED |
| `vehicle->car` | 23/3/0 | 4/1/0 | 5/0/0 | 23/3 | 48/3 | UNSUPPORTED_ABSTAIN |

## Boundary and disposition

`KEEP_UNSUPPORTED_RELATIONS_ABSTAIN; ANY_NEW_ACQUISITION_REQUIRES_A_NEW_FROZEN_PROTOCOL`. Only the eight pre-authorized relation queues were opened; `stock→bond` reserve rows remain sealed. The remaining six unsupported relations have no unopened rows in their frozen queues. All reserve labels contribute to TRAIN-NEW only. DEV/TEST labels and identities did not contribute to support and were not modified. The support result is engineering data sufficiency only, not model qualification.
