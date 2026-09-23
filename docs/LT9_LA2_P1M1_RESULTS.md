# LT9-LA2 P1M1: exclusive-plurality anatomy results

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m1-exclusive-plurality-anatomy-20260923`

## Decision

**P1M1 discovery is outcome-underpowered. No routing policy was selected, and P1M1Q was not run.** The first screen-eligible corpus, Climate-FEVER, supplied 286 actionable contested-unique episodes, all valid under the frozen control and zero invalid. The frozen discovery gate requires at least five actionable invalid contested episodes in at least two shards before judging exclusive-only as a repair. Exclusive-only would abstain on all 286 of those valid contested episodes, sacrificing 9.33% of baseline actionable routes. This is within the 10% usefulness budget, but safety benefit was not demonstrated because the target failure did not occur.

The result does **not** qualify exclusive-only. It also does not establish that unique plurality is safe generally; it says this structurally eligible discovery corpus did not exercise the invalid contested state needed to decide.

## Label-blind corpus screen

The preassigned order was CQADupStack, Climate-FEVER, then NQ. The final screen receipt records `validity_labels_opened=false` and `qrels_or_queries_opened=false`.

| Corpus | Documents | Episodes | Unique plurality | Exclusive | Contested | Eligible | Frozen role |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| CQADupStack | 457,199 | 211 | 167 | 157 | 10 | No | None |
| Climate-FEVER | 5,416,593 | 5,911 | 5,176 | 4,630 | 546 | Yes | Discovery |
| NQ | 2,681,468 | 5,317 | 4,886 | 4,424 | 462 | Yes | Qualification |

CQADupStack missed the structural floor: its ten contested episodes were below the required 20, and no two candidate relations each had ten contested episodes. Climate-FEVER and NQ each cleared the frozen structural floor across all eight shards and had the required relation coverage. NQ was assigned the qualification role by order only; its validity outcomes remain unopened.

CQADupStack's archive contains twelve forum `corpus.jsonl` members. They were concatenated in ascending archive-path order as preregistered in `LT9_LA2_P1M1_MULTIMEMBER_INTAKE.md`; no query or qrels members were extracted or read.

## Climate-FEVER discovery outcome

| Class | Episodes | Actionable | Control valid | Control invalid | Candidate valid | Candidate invalid | Candidate abstained |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Contested-unique | 546 | 286 | 286 | 0 | 0 | 0 | 286 |
| Exclusive-unique | 4,630 | 2,779 | 2,779 | 0 | 2,779 | 0 | 0 |

Both control and candidate had zero invalid episodes and zero invalid authority compartments. The candidate retained all 2,779 actionable exclusive episodes, abstained on the 286 actionable contested episodes, lost 229 owned-witness updates, and remained inside both frozen 10% loss/abstention budgets (9.3312%). Deterministic replay, owned-witness traceability, polarity integrity, and pending-capacity checks passed; each arm had zero polarity errors and zero capacity violations, with pending peak 7/7.

Because the control produced **zero** actionable invalid contested episodes, the minimum outcome floor failed. The candidate's zero-invalid result is not evidence of a repair when the control also had zero. No rule selection, qualification replay, retrieval evaluation, serving change, or LA2-B integration followed.

## Frozen input and artifact identities

- CQADupStack archive MD5: `4e41456d7df8ee7760a7f866133bda78`; archive SHA-256: `6072f7d345496387d24194c8af35c5fb6c2f0e5f9130c5b4a78b98bbfac88558`.
- CQADupStack assembled corpus SHA-256: `5f879180578f90a5595c62ddc2a2058b51b64754a66086de0b1f5a3f975ccc0a`.
- Climate-FEVER archive MD5: `8b66f0a9126c521bae2bde127b4dc99d`; corpus SHA-256: `628d610ce1175f69f9d6759b482aa000f87e88b9d81178d0c1799d5c004c91d1`.
- NQ archive MD5: `d4d3d2e48787a744b6f6e691ff534307`; existing corpus matched the archive's sole corpus member, SHA-256: `f7e3588dd416772c9830baf67e827d49678f8d5a6ccf6e101b9ce990cc7b208c`.
- Final label-blind screen SHA-256: `53b1a690637c5d6893754403978b381a1182b1b3db330c8beb338c957ba79785`.
- Climate-FEVER discovery receipt: `D:\phoenix-evals\lt9-p1m1-20260923\climate-fever-discovery.json`.
- Full screen receipt: `D:\phoenix-evals\lt9-p1m1-20260923\screen-final.json`.
- Release/test binary SHA-256: `5B41BF109D04965AD1ADAF968B091493DD2DDC49B1C962562A40EF0BD0483839` (D: release and C: test copy matched).

Validation: `cargo fmt --check`; `cargo test --release --locked` passed 5/5; release build succeeded; the label-blind synthetic screen smoke test passed. P1M1 Rust sources remain below 800 lines each.

## Next gate

Do not use NQ's preassigned qualification role as a tuning or discovery set. P1M1 has no selected candidate to qualify. A further discovery attempt needs a separately frozen protocol and a fresh corpus whose outcomes can supply the contested-invalid minimum; only after a new discovery selects a rule may an untouched qualification corpus be opened.
