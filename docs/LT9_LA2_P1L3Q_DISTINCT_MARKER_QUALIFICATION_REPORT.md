# LT9-LA2-P1L3Q Distinct-Marker Router Qualification — Failed Closed

Date: 2026-09-23
Branch: codex/phoenix-native-p1l3q-distinct-marker-gate-20260923

## Decision

Distinct-marker voting is **not qualified**. It substantially reduced valid-episode loss and new tie abstention, and the frozen DBPedia-Entity corpus met the preregistered risk-state coverage floor. However, five invalid episodes remain and the full replay creates one invalid authority compartment. The zero-invalid episode and zero-invalid-compartment gates therefore fail. Do not promote this router, reopen LA2-B, or enable retrieval or serving.

## Frozen scope

P1L3Q applied the P1L3 discovery counterfactual unchanged: each family receives one vote per distinct active marker identity; the existing unique-endpoint-agreement tie resolver and all other stream, credit, expiry, and pending-capacity behavior remain frozen. The corpus was selected by a label-blind structural screen. No BEIR qrels, queries, retrieval judgments, or serving metrics were loaded. Expected phenotype was used only for post-hoc routing validity. HotpotQA remains discovery-only; DBPedia-Entity is the external qualification set and must not be reused to tune a repair.

## Qualification evidence

| Measure | Result | Gate |
| --- | ---: | --- |
| Corpus documents | 4,635,922 | frozen input |
| Events / episodes | 2,689 / 2,678 | complete stream |
| Label-blind risk signature | 9 episodes / 2 relations | floor passed (>=8 / >=2) |
| Baseline actionable valid / invalid | 1,462 / 11 | baseline sufficiency passed |
| Baseline invalid shards | 6 of 8 | >=2 required; passed |
| Distinct-router invalid episodes | 5 (2 actionable, 3 non-actionable) | **0 required; failed** |
| Invalid episodes repaired | 0 | diagnostic |
| Invalid baseline episodes abstained | 11 | diagnostic |
| Valid episodes lost | 3; 0.2052% | <=10%; passed |
| New tie abstentions | 12; 0.8147% | <=10%; passed |
| Invalid authority compartments | bank_to_water@finance (1) | **0 required; failed** |
| Polarity errors / pending-capacity violations | 0 / 0 | passed |
| Pending peak / capacity | 7 / 7 | passed |
| Owned witnesses / positive updates / negative updates | 1,314 / 1,302 / 12 | traceable; passed |
| Deterministic replay | identical | passed |

All five remaining invalid episodes route bank_to_water to finance where the frozen expected context is geography. The two actionable cases are enough to produce the persistent invalid bank_to_water@finance compartment; the other three are non-actionable episode outcomes. Distinct voting therefore narrows but does not eliminate the failure mode.

The 9-episode/two-relation preflight qualification demonstrated that the risky state was exercised; it did not establish that the router was correct. Episode safety and compartment safety both remain mandatory.

## Reproducibility and inputs

The full-stream replay matched the frozen LA2-B source and corpus identities. The second qualification replay was byte-identical to the first; receipt SHA-256: CBA857F9678B60842B9D2EC7E853F5ECB801FE337DEF12C39AD66A3349459C94.

- Corpus SHA-256: abd0a993f69b0abc2a4a5367695bceb081f0c0efd22d65b9986d4ac6ff8baf95
- P1L3 source SHA-256: 1b22cada58c79bb565301012ee8c099e20009a1f191d8db66ee43fa01ce75e46
- P1L3 preflight receipt SHA-256: ec267453a7870411803301d0823e2e71003b4087475b43cc3155c5bc6549b41b
- Executable SHA-256: 2894eaa9ca2a8dae0747c90a84847aa279ebf93db40d99f4dddb9747b90d620e

Focused release tests passed 2/2; target-drive release build and cargo check passed. No learning, retrieval, or serving state was changed.

## Disposition

Keep P1L3Q as an external failed-closed qualification result. Do not search thresholds or alter marker definitions on DBPedia-Entity. A future policy requires a fresh, label-blind risk-signature screen and a new unopened qualification corpus. The surviving bank_to_water@finance compartment is a context-routing defect; credit routing remains sealed and is not implicated by this result.
