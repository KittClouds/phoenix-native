# LT9-LA2 P1L4: hard tie-abstention results

Date: 2026-09-23
Branch: `codex/phoenix-native-p1l4-hard-tie-abstention-20260923`
Disposition: **discovery passed; qualification failed closed**

## Question and frozen rule

P1L4 tested whether distinct-marker plurality should abstain whenever either endpoint has an exact plurality tie, instead of allowing the frozen `unique_endpoint_agreement` tie resolver to route it. Corpus order, screen floors, candidate relations, routing rules, memory rules, and decision limits were frozen before screening. DBPedia remained sealed and was not reopened.

The label-blind screen read only corpus text. It did not open BEIR qrels or query files, and it assigned the first eligible corpus to discovery and the second to qualification. Replay compared routed phenotypes against the fixed candidate-context metadata only after corpus roles had been sealed. Retrieval and serving were not run.

## Screen and corpus identity

The screen floor required at least 40 tie-resolved episodes, at least 10 each for two candidate relations, and coverage across at least three document shards.

| Frozen role | Corpus | Documents | Episodes | Tie-resolved episodes | Relations | Shards |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Discovery | FEVER | 5,416,568 | 5,911 | 422 | 4 | 8 |
| Qualification | MS MARCO | 8,841,823 | 42,000 | 1,299 | 6 | 8 |

Both passed the structural floor. The roles were assigned by the frozen corpus order, not by validity outcomes. Only each corpus archive's `corpus.jsonl` member was extracted. The archive checksums match the official BEIR archive index; the BEIR catalog identifies the datasets and their formats ([catalog](https://github.com/beir-cellar/beir/wiki/Datasets-available), [archive index](https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/)).

| Corpus | Archive MD5 | Extracted corpus SHA-256 |
| --- | --- | --- |
| FEVER | `5a818580227bfb4b35bb6fa46d9b6c03` | `f7ddf098ead635e46242c435547215f48ed2fe3eeeab6ffdc2d0833d43174858` |
| MS MARCO | `444067daf65d982533ea17ebd59501e4` | `bb51d9f60a8c7e3fd2749040a217f316d5c0a9e2a72bc0b939234d628da67795` |

## Frozen replay results

| Measure | FEVER discovery | MS MARCO qualification |
| --- | ---: | ---: |
| Baseline tie-resolved episodes | 422 | 1,299 |
| Baseline invalid episodes | 52 | 82 |
| Hard-abstain invalid episodes | **0** | **21** |
| Invalid actionable episodes prevented | 25 | 31 |
| Valid actionable episodes, baseline → candidate | 3,245 → 3,065 | 25,701 → 24,998 |
| Valid actionable episodes sacrificed | 180 (5.55%) | 703 (2.74%) |
| New abstentions | 422 (6.27% receipt rate) | 1,299 (2.85% receipt rate) |
| Authority updates lost | 184 | 522 |
| Total authority updates, baseline → candidate | 2,730 → 2,546 | 19,710 → 19,188 |
| Invalid authority compartments, candidate | **0** | **5** |
| Candidate routed phenotype counts (finance, geography, transport, other) | 950, 149, 4,077, 0 | 20,216, 66, 18,847, 0 |
| Credit-integrity / deterministic replay | pass / pass | pass / pass |

The discovery gates passed: hard abstention removed all 52 invalid FEVER episodes and both contaminated baseline authority compartments, with valid-actionable loss and abstention within the frozen 10% limits. The frozen rule was therefore carried to MS MARCO unchanged.

Qualification failed. Hard abstention removed 61 of 82 invalid MS MARCO episodes, but 21 remained, including 10 actionable invalid episodes, and five invalid authority compartments persisted:

```text
bank_to_shore@finance
bank_to_water@finance
car_to_vehicle@finance
insurance_to_coverage@transport
vehicle_to_car@finance
```

The candidate meets the valid-loss and abstention budgets on MS MARCO, but it fails the required zero-invalid-episode and zero-invalid-compartment gates. The remaining invalid compartments show that eliminating tie rescue is not sufficient to make the current router safe across the qualification corpus.

## Decision

**Hard tie-abstention is not qualified for context routing.** P1L4 closes with a failed external qualification, not a promoted router. Do not tune on MS MARCO or reuse it as a discovery surface. LA2-B and retrieval remain blocked; E1Y remains sealed. No P1L5 branch was opened because the discovery result met the usefulness budget; the failure occurred at the separate external safety gate.

## Reproducibility and artifacts

The final release binary passed `cargo fmt --check`, `cargo test --release` (5/5), and `cargo build --release`. The binary in the D: build directory and the C: test junction have the same SHA-256. The discovery and qualification receipts both report deterministic replay and credit-integrity success. Earlier independent duplicate runs were byte-identical; the final-build qualification failure was rerun after a test-only source-file split.

Protocol: [`LT9_LA2_P1L4_HARD_TIE_ABSTENTION_PROTOCOL.md`](LT9_LA2_P1L4_HARD_TIE_ABSTENTION_PROTOCOL.md)
Runner: [`lt9_la2p1l4.rs`](../apps/phoenix-memory-lock/src/bin/lt9_la2p1l4.rs)
Core: [`lt9_la2p1l4_core.rs`](../apps/phoenix-memory-lock/src/bin/lt9_la2p1l4_core.rs)
Tests: [`lt9_la2p1l4_core_tests.rs`](../apps/phoenix-memory-lock/src/bin/lt9_la2p1l4_core_tests.rs)

External sealed artifacts:

- Screen: `D:\phoenix-evals\lt9-p1l4-final\screen.json` — SHA-256 `7b92ea1bdaf66089062097d1bd943f942eb309ecb88748c511970552a9603dff`
- FEVER discovery: `D:\phoenix-evals\lt9-p1l4-final\fever-discovery.json` — SHA-256 `66ac31e76719ea3c03de867563e11533575242ce399b35c0e74f0023279224d6`
- MS MARCO qualification: `D:\phoenix-evals\lt9-p1l4-final\msmarco-qualification.json` — SHA-256 `726a80e0228c62f0f4dcab24c4f7f722901ebf4fd38beb83789f3d9b1ce94211`
- Extraction receipt: `D:\phoenix-evals\beir\p1l4-fresh\extraction-receipt.json` — SHA-256 `4dfaee7ad33668a82ff91d7f7b7df04bdacb7fe7c9419bfb8863e30f553923d4`

Final executable SHA-256: `22f5ef607890c26aa13debbaa5bc54d34b0c080eb944bf5cdf687420cde1ee42`.
