# P1P3E1 selective lexical transport — 2026-09-28

## Result

The QPS transport harness runs end to end, but the frozen selective gate did **not** earn useful transport on this slice. The frozen CART allowed incompatible contexts in the retrieval probe, while the more conservative E1 rule abstained on every candidate it considered. Keep both as diagnostics; neither is ready for authority or serving.

This is functional/proxy engineering evidence over natural excerpt pairs. It is not independent-human validation, formal P1P3 qualification, BEIR topical retrieval quality, lexical-authority promotion, or a serving change. The formal P1P3 protocol and its underpowered status are unchanged.

## Harness and lanes

The harness uses the real `phoenix-lexical-qps` query-group API. It indexes 60 right-side excerpts and searches with 60 left-side excerpts from the frozen P1P3 holdout. Only 36 queries have the counterpart focal word on the document side and are eligible for transport; among those, the proxy labels are 9 SAME and 27 DIFFERENT, with no UNKNOWN. The single holdout UNKNOWN is outside the transport-eligible orientation, so this run does not validate UNKNOWN handling.

L3's transported weight was fixed at `0.5` as a diagnostic setting; no alpha sweep or threshold selection was run.

| Lane | Target docs before → after | New SAME targets | New DIFFERENT targets | Labeled retrieval decisions: SAME allow / DIFFERENT allow | Readout |
|---|---:|---:|---:|---:|---|
| L0 original | 8 → 8 | 0 | 0 | 0 / 0 | Baseline |
| L1 unconditional | 8 → 14 | 2 | 4 | 21 / 26 | More recall, substantial contamination |
| L2 frozen CART E0 | 8 → 8 | 0 | 0 | 0 / 4 | Four observed false authorizations; three more allows were unlabeled |
| L3 selective E1 | 8 → 8 | 0 | 0 | 0 / 0 | Safe by abstention, but inactive |
| L4 label oracle | 8 → 10 | 2 | 0 | 21 / 0 | Upper bound on these labeled pairs; 29 other candidates remain unlabeled |

Among retrieval candidates with proxy labels, L1 allowed all 21 SAME and all 26 DIFFERENT cases. L2 allowed no SAME cases and four DIFFERENT cases, so its observed allow precision was 0/4. L3 allowed none. Its frozen policy required at least four fit SAME and four fit DIFFERENT examples per relation, then a pure-SAME CART leaf. Only `cruel→vicious` met relation support; its reached leaves were mixed, so it abstained. The other relations lacked sufficient fit positives or negatives.

The oracle recovered two additional SAME targets, so the maximum observed opportunity on this slice is two target contexts. E1 recovered 0/2. These are context-pair retrieval counts, not relevant-document recall or ranking metrics. A separate qrels preflight found no counterpart-positive opportunity in FiQA dev (500 judged queries) or SciFact (1,109 queries); FiQA test had one `pluck/pull` query and no judged-relevant counterpart document. In the FiQA corpus, exact-token document coverage is also skewed: `pluck` 0, `pull` 524, `bout` 5, `tear` 66, `cruel` 15, `vicious` 19, `deterioration` 11, `worsening` 10. No document contains both terms of any of these four pairs. We therefore make no BEIR quality claim and stop before model expansion: this frozen application slice offers no relevant-document opportunity to optimize.

## Allow autopsy

The receipt includes all six `P_full_local` holdout ALLOW cases, their tree paths, leaf counts, and candidate-masked pair features. Four were labeled SAME and two DIFFERENT.

- `pluck→pull` false allow: shared function words (`of`, `in`, `to`, `and`) and token Jaccard 0.111 crossed the 0.1 split; the leaf contained 2 SAME and 1 DIFFERENT.
- `cruel→vicious` false allow: no shared tokens or n-grams and zero token overlap. The path used only structural deltas (`after=12`, `before=3`) and reached a leaf with 8 SAME, 0 DIFFERENT. This is a structural overgeneralization, distinct from the overlap-driven error.

The three `deterioration→worsening` true ALLOWs do not establish transport safety: fit has 15 SAME and zero DIFFERENT. E1 correctly marks the relation `INSUFFICIENT_NEGATIVES` and denies it support.

## Engineering disposition

The application path is now executable and decision receipts are inspectable. The current gate fails the usefulness requirement: it does not recover a target missed by baseline. The frozen CART also fails the safety requirement on labeled retrieval candidates. Do not tune thresholds on these exposed examples. The next engineering dataset needs more candidate-specific natural contexts, especially genuine negative contexts for `deterioration→worsening` and transport-eligible UNKNOWN cases. It should be collected before comparing logistic or tree alternatives.

The retrieval harness keeps original results in every lane. It does not update lexical authority or production serving. Exact local overlap remains a useful feature family, but the two false-ALLOW paths show it is not a sufficient authorization rule. No model was promoted.

## Validation and reproducibility

- Isolated target-drive build, using `D:\phoenix-target-overgraph` because the root workspace cannot resolve the missing `crates/phoenix-tts-native/Cargo.toml` member.
- Harness tests: 8 passed, 0 failed.
- QPS library tests: 28 passed, 0 failed in the isolated QPS crate validation; no QPS ranking score change was made.
- Release rerun receipt SHA-256 matched exactly: `94c7f5401a3f98077d482c4b4546fca5efde964b1d3e9ab83b7eee5f3cfabaf0`.
- No authority updates or serving changes occurred.

The machine-readable result is [transport-receipt.json](transport-receipt.json). The runnable harness is in `apps/phoenix-memory-lock/src/bin/lt9_la2p1p3e1_transport.rs`; its type, query-helper, and test modules are adjacent files with the same `lt9_la2p1p3e1_transport_` prefix.
