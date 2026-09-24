# LT9-LA2 P1O2: Natural Pairwise Compatibility Results

Date: 2026-09-23
Status: **one candidate cleared the frozen label-count gate; discovery-only holdout diagnostic completed**

## Review and sufficiency

The 30 P1O2 packet labels were supplied by the experiment author in chat and are sealed as `AUTHOR_REVIEW`, not independent review. Validation matched all 30 packet IDs exactly and allowed only the frozen `SAME`, `DIFFERENT`, and `UNKNOWN` labels. Counts were 9 `SAME`, 21 `DIFFERENT`, and 0 `UNKNOWN`.

Only `save ↔ spare` had packets. Its fit graph had 5 `SAME`, 10 `DIFFERENT`, 0 `UNKNOWN`; its document-disjoint holdout graph had 4 `SAME`, 11 `DIFFERENT`, 0 `UNKNOWN`. Both satisfy the preregistered per-graph gate of at least eight determinate labels and at least three labels in each of `SAME` and `DIFFERENT`. The other four fixed candidates remain underpowered; pooled labels did not rescue them.

## Frozen probe results

Both candidate-specific depth-3 CART views were fit on the 15 fit-graph edges and evaluated once on the 15 holdout-graph edges. The `P_full_local` and `P_no_exact_overlap` holdout results were identical:

| Holdout measure | `P_full_local` | `P_no_exact_overlap` |
| --- | ---: | ---: |
| Accuracy | 10/15 (66.7%) | 10/15 (66.7%) |
| Observed-class balanced accuracy | 53.4% | 53.4% |
| `SAME` recall | 1/4 (25.0%) | 1/4 (25.0%) |
| `DIFFERENT` recall | 9/11 (81.8%) | 9/11 (81.8%) |
| False-SAME, `P(predicted SAME \| human DIFFERENT)` | 2/11 (18.2%) | 2/11 (18.2%) |
| Low-overlap `SAME` recall | 0/1 | 0/1 |
| High-overlap `DIFFERENT` recall | 2/4 (50.0%) | 2/4 (50.0%) |

The full-local tree used token-Jaccard in a branch where both leaves predict `DIFFERENT`; the no-exact-overlap tree used a structural delta there. The resulting holdout confusion counts were the same. Exact lexical overlap therefore showed no incremental holdout benefit in this one candidate. Fit accuracy was 14/15 for both views, so the fit-to-holdout drop is also visible.

No `UNKNOWN` label occurred in either graph. `UNKNOWN` recall and its confusion directions are therefore unmeasured, not zero-risk; the trained trees made no `UNKNOWN` predictions.

## Pairwise triangles

The sealed receipt contains every fully labeled triangle from the two six-context graphs: 20 fit and 20 holdout. Triangle edge order is `(node0,node1)`, `(node0,node2)`, `(node1,node2)`. There were 16 mixed-label triangles in fit and 13 in holdout (29/40 overall). This is descriptive evidence that these pairwise judgments do not behave like a clean equivalence relation in this sample. No transitive closure, clustering, or union-find was applied.

Fit patterns: `DDD=3`, `DDS=6`, `DSD=5`, `SDD=4`, `SDS=1`, `SSS=1`. Holdout patterns: `DDD=6`, `DDS=9`, `DSD=1`, `SDD=3`, `SSS=1`. The full node and packet-level triangle records are retained in the sealed analysis receipt.

## Interpretation and boundary

This single, author-reviewed, conditionally selected relation does **not** demonstrate useful held-out compatibility observability: balanced accuracy is 53.4%, `SAME` recall is low, and exact-overlap features do not improve the holdout result. The high-overlap `DIFFERENT` cell is only four pairs, with two false-SAME errors. The evidence is too small and conditional to generalize to other relations or reviewers, but it does not support promoting the current compatibility observer.

P1O2 remains a discovery assay. No lexical authority was updated, no serving or retrieval evaluation ran, and LA2-B remains blocked. The four candidates without both graphs stay `UNDERPOWERED`.

## Seals

- Pre-review root SHA-256: `d5d07c0cb8a8b41f39b51af82157627ab2dc4e3973fcd9a250a5870f795f1d54`.
- Packet SHA-256: `5aede9b8e8db614217d456923c9529f3b7c222fc32ae1e05bf8a5712c844fa88`.
- Author-reviewed judgment SHA-256: `754c8af76155e6a409a0f713033e681f1addb81a65a4c7fee8e2d51918c921bc`.
- Validation receipt SHA-256: `b3ea713098ed9ba1885eeaf1dd1e44c348e35bf8a25816e8147ef089fc6fae3c`.
- Sufficiency receipt SHA-256: `227e8aadc1d8d0934ddc1e2875c0df77e9bb03da1c9cdebc3b043a90569b4e10`.
- Analysis receipt SHA-256: `fc33e2c3856696f73c30a965d077086952cf95204df308bfc2f1b97f6b7a16fd`; deterministic rerun produced the same hash.
- Analyzer source/module SHA-256: main `2a2fbf601838415d63fc4239e6f7adf65a89841159a085b86abe9c635bd0d854`; CART/metrics core `b483ba5fe3275bccb1a542489041c5728d74e60ddd149dba336bac29eb71909d`.
- Analyzer manifest/lock SHA-256: `0eea5b113bf92df1ffd242a6464ca2bf56fda2c095357b651903a1602c1bc490` / `0603918fa1eefe6fd18ac0b970ac2f0bf9cda43a035cfed2c3f01ab777f86ac7`.
- Analyzer release binary SHA-256: `2c33f0248c97a7c10445dea071dfc6f6344f166a44f10ecc903557d69a7cb680`.
- Verification: 4/4 tests and strict Clippy passed; analysis receipt records `authority_updated=false`, `retrieval_run=false`.
- Full packet-level triangle list and model trees: `D:\phoenix-evals\lt9-la2-p1o2-20260923\analysis-final\analysis-receipt.json`.
