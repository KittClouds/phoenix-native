# LT9-LA2 P1P2: Three-review label reliability

**Date:** 2026-09-24
**Status:** sealed label-side proxy-review diagnostic
**Scope:** P1O2's 30 `save↔spare` context pairs only. No feature values were used, no observer was fit, no authority was updated, and no retrieval was run.

## Result

The three reviews agree unanimously on 19/30 edges (63.3%): 7 `SAME` and 12 `DIFFERENT`. The remaining 11/30 (36.7%) are 2:1 splits and are classified `DISPUTED`, which maps to consensus `UNKNOWN` for a **future protocol only**. This does not replace P1O2's original author-labeled analysis or turn the current proxy votes into training truth.

Rater order throughout is R0 = experiment author, R1 = Luna A, R2 = Luna B. Their label counts are:

| Rater | SAME | DIFFERENT | UNKNOWN |
| --- | ---: | ---: | ---: |
| R0 author | 9 | 21 | 0 |
| R1 Luna A | 14 | 16 | 0 |
| R2 Luna B | 16 | 14 | 0 |

No reviewer used the individual `UNKNOWN` label. The 11 consensus `UNKNOWN` outcomes arise from disagreement, not from a reviewer marking an item indeterminate.

| Pair | Exact | Exact rate | SAME↔DIFFERENT reversals |
| --- | ---: | ---: | ---: |
| R0 vs R1 | 25/30 | 83.3% | 5 |
| R0 vs R2 | 19/30 | 63.3% | 11 |
| R1 vs R2 | 24/30 | 80.0% | 6 |

The directional 2:1 vote patterns (order R0/R1/R2) are:

| Votes | Count | Consensus target |
| --- | ---: | --- |
| `D,D,S` | 4 | UNKNOWN / disputed |
| `D,S,S` | 5 | UNKNOWN / disputed |
| `S,S,D` | 2 | UNKNOWN / disputed |

The pairwise confusion cells, with rows = first rater and columns = second rater in `SAME, DIFFERENT, UNKNOWN` order, are: R0/R1 `[[9,0,0],[5,16,0],[0,0,0]]`; R0/R2 `[[7,2,0],[9,12,0],[0,0,0]]`; R1/R2 `[[12,2,0],[4,12,0],[0,0,0]]`.

Three-rater nominal Fleiss' κ is **0.5023**, descriptive for this small set. The mean per-edge vote entropy is **0.3367 bits**; unanimous edges have 0 bits, and disputed edges have **0.9183 bits**. The full ordered vote vector for every packet, per-edge entropy, and all pairwise confusion matrices are in the machine receipt.

## Descriptive strata

| Stratum | Edges | Unanimous SAME | Unanimous DIFFERENT | Disputed | Mean pairwise exact agreement |
| --- | ---: | ---: | ---: | ---: | ---: |
| Low lexical overlap | 10 | 1 | 5 | 4 | 73.3% |
| Middle lexical overlap | 12 | 4 | 3 | 5 | 72.2% |
| High lexical overlap | 8 | 2 | 4 | 2 | 83.3% |
| Fit graph | 15 | 4 | 3 | 8 | 64.4% |
| Holdout graph | 15 | 3 | 9 | 3 | 86.7% |

These are descriptions of the reviewed sample, not evidence that overlap or graph membership causes agreement differences.

## Triangles

Triangles were reconstructed only when all three edges had unanimous three-review labels. There are **12** such triangles: 11 have one `SAME` and two `DIFFERENT` edges (`DDS`), and one is `SSS`. There are **zero** triangles with two `SAME` and one `DIFFERENT` edge. The triangle patterns reuse context nodes and are dependent; they do not establish or refute a general transitivity property. The 11 `DDS` patterns are compatible with an equivalence partition and should not be mislabeled as non-transitive.

## Interpretation boundary

This is **not independent-human reliability evidence**: R0 is the experiment author, and R1/R2 are separate agent contexts from the same model family. It does show material proxy-review boundary instability, including six direct R1↔R2 polarity flips. The stricter future target rule is therefore:

```text
all SAME       -> SAME
all DIFFERENT  -> DIFFERENT
any disagreement -> UNKNOWN
```

That rule is a prospective target definition, not a majority-vote adjudication and not a claim that natural compatibility must be transitive. A fresh observability experiment should acquire independent human labels before fitting anything.

## Reproducibility

The sealed receipt is at `D:\phoenix-evals\lt9-la2-p1o2-20260923\p1p2-label-reliability-20260924\reliability-receipt.json` with SHA-256:

```text
ef934be2431756162d79cac17f5d2ff4ba89270e9395d639b7b34a60643677ad
```

A second run produced the identical receipt hash. Inputs include the frozen packets, all three review files, private sampling metadata (restricted to IDs/split/overlap/node IDs), rubric, P1O2 analysis, and upstream validation/sufficiency/root receipts. The receipt records all input and executable hashes.

The analyzer has five passing unit tests; `cargo clippy --all-targets -- -D warnings` and the release build also pass.
