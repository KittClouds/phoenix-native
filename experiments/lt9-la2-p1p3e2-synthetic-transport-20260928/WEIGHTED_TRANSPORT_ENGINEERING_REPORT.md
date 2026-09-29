# P1P3E2 weighted lexical transport engineering run

**Date:** 2026-09-28

**Disposition:** QPS transport path executes; neither fitted gate provides useful safe transport on the reviewed E2 application slice. No lexical authority, serving policy, or LA2-B memory changed.

## What ran

The 151,736-row SWORDS/CoInCo seed bank fed one global, two-stage weighted logistic gate. Gate 1 estimates whether visible local evidence is sufficient; erased contexts and either endpoint with no content abstain. Gate 2 estimates SAME versus DIFFERENT from masked, candidate-excluding local features. Explicit SWORDS rejections retain higher weight; CoInCo not-elicited negatives retain their weak `label_strength` (0.25). Relation ID, qrels, corpus ID, and retrieval rank are absent from the classifier features. An exact shared non-stopword content anchor is required for the cheap gate to ALLOW.

The first Gate 1 cutoff (`0.9872`) abstained on all 47 reviewed E2 packets. That engineering failure remains in `results/weighted-gate-v1-receipt.json`. The corrected run uses a visible-content check plus a `0.5` sufficiency cutoff; DEV selected `0.6947` as the ALLOW cutoff. The corrected model and receipt are checked in under `results/weighted-gate-v2*`.

For comparison, `LiquidAI/LFM2.5-230M-Base` revision `9d2be5519834990d30996f878b6771cccbd24f2c` was held frozen. Final-layer, last nonpadding-token vectors from a deterministic 5,000/1,440/1,440-row subset of the bank trained two tiny logistic readouts. Candidate terms were masked inside contexts; the directed relation was explicit in the input header. No generation or backbone tuning occurred. Its exact weights, thresholds, and provenance are in `results/lfm230-*`.

## Internal seed-bank behavior

| Gate, internal TEST | SAME ALLOW | DIFFERENT ALLOW | UNKNOWN ALLOW | Explicit SWORDS-rejection ALLOW |
|---|---:|---:|---:|---:|
| Weighted lexical, all 17,292 TEST rows | 22 / 806 | 44 / 7,808 | 2 / 8,678 | 0 / 437 |
| Frozen 230M readout, 1,440 sampled TEST rows | 0 / 400 | 0 / 600 | 0 / 440 | 0 / 200 |

The cheap gate correctly abstained on all 8,614 synthetic both-erased TEST rows, but its 22 SAME ALLOWs were outweighed by 44 weak-negative DIFFERENT ALLOWs. These weak negatives are *not* explicit human rejections; they are still a warning that the seed proxy does not yield an attractive operating point. The 230M readout found no DEV threshold satisfying its false-ALLOW budget while recovering SAME rows, so its safe operating point was all-abstain. This is a readout and seed-target result, not a claim that the pretrained backbone lacks useful language knowledge.

## Full-corpus QPS application check

The frozen E2 set had 17 qrels-positive retrieval-gap probes: 14 reviewed SAME and three DIFFERENT. QPS rebuilt the complete HotpotQA, NQ, and Quora indexes with the same corpus hashes as the prior baseline. Original query groups remained present; an authorized counterpart expansion entered at a fixed `0.5` quality. A proposed document was eligible for gate inspection only when it actually contained the target term. Baseline scores stayed intact in merged lanes.

Only **four previously missed target documents** entered the expanded top-100 candidate pool: three SAME and one DIFFERENT. Three other targets were already in baseline top 100 at ranks 13, 28, and 35; the remaining ten stayed outside expanded top 100. The lane merger preserves scores of baseline hits.
Their unconditional expanded ranks were 98, 67, and 20 for SAME, and 87 for the incompatible `engine→motor` target. None reached top 10.

| Lane | Additional reviewed SAME targets recovered | Reviewed DIFFERENT targets falsely admitted | New qrels-positive docs | New qrels-unjudged docs |
|---|---:|---:|---:|---:|
| L0 baseline | 0 | 0 | 0 | 0 |
| L1 unconditional | 3 | 1 | 4 | 187 |
| L2 weighted lexical gate | 0 | 0 | 0 | 4 |
| L3 reviewed-target oracle | 3 | 0 | 3 | 0 |
| L4 frozen 230M readout | 0 | 0 | 0 | 0 |

The oracle is deliberately limited to the one reviewed target per probe; it does not label or authorize other retrieved documents. Qrels-absent admissions are **unjudged**, not proven irrelevant. The weighted gate admitted four such documents but recovered no reviewed relevant target. The frozen 230M gate admitted nothing. Oracle efficiency for both learned gates is `0/3` recoverable SAME targets.

On the four reachable target pairs, cheap compatibility scores were `0.502`, `0.580`, and `0.425` for SAME, versus `0.633` for DIFFERENT. The 230M scores were `0.966`, `0.966`, and `0.686` for SAME, versus `0.9997` for DIFFERENT. The incompatible `engine→motor` case ranks *above* the compatible examples for both readouts. Threshold relaxation alone cannot fix that ordering.

Median per-probe QPS search time was about 573 ms baseline and 558 ms expanded on the corrected full-corpus replay. The Python lexical gate measured about 129 µs at median per inspected candidate. Batched 230M extraction took 2.08 seconds for 191 QPS candidates on an RTX 3080, excluding model load. These are diagnostic measurements, not optimized serving latency.

## Engineering conclusion

The execution path is concrete and replayable: a masked local decision controls participation of a frozen QPS expansion, and each lane has a target-level receipt. The current seed-derived SAME/DIFFERENT proxy is **not aligned well enough** with the application compatibility decision to enable useful safe transport. The next engineering change should target that decision representation or training objective; another threshold sweep, UNKNOWN bank, or packet quota would not address the observed score ordering. Baseline QPS remains the operational behavior.

The E2 review is discovery-exposed and small; the bank's context-disjoint internal TEST is an internal proxy test. This run establishes an engineering failure mode and an executable harness, not external qualification. Retrieval serving, lexical authority, and memory integration remain unchanged.

## Reproduction and artifacts

`train_weighted_gate.py` streams the external gzipped bank and writes the lexical model and internal receipt. `qps_transport_harness` builds and searches the full corpora, using the frozen isolated QPS source referenced in its Cargo manifest. `score_qps_lanes.py` joins reviewed target labels and qrels only after searches, applies the frozen model decisions, and emits lane receipts. `lfm230_readout.py` performs the bounded frozen-backbone comparison. Exact input/model hashes, selected pair IDs, corpus hashes, and lane results live in the checked-in `results/` JSON files. Corpus text, raw seed banks, and pretrained weights stay in the external eval/model directories.

Validation: six Python unit tests and two Rust release unit tests passed. QPS corpus hashes matched the prior baseline receipt. A second full-corpus replay after a UTF-8 masking fix had **zero non-timing output mismatches** across all 17 probes (`results/qps-maskfix-parity.json`). The current Git branch contains no production QPS or authority changes.
