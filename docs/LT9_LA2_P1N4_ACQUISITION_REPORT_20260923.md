# LT9-LA2 P1N4 acquisition receipt

Date: 2026-09-23
Status: **blind packets ready; no judgments or model evaluation**

The deterministic sampler produced 106 packets over 212 unique documents, drawing from 10 of the 12 frozen text corpora. It excluded all 192 P1N3 source documents and all 192 candidate-specific masked-context templates present in the P1N3 private ledger. The fixed target was 120 packets; no shortfall was backfilled or relaxed.

| Candidate relation | Ambiguity / low evidence | High overlap / structural divergence | High overlap control | Low overlap | Ordinary middle | Total |
|---|---:|---:|---:|---:|---:|---:|
| `bank → water` | 4 | 8 | 7 | 7 | 7 | 33 |
| `car → vehicle` | 8 | 8 | 8 | 8 | 8 | 40 |
| `insurance → coverage` | 6 | 7 | 6 | 6 | 8 | 33 |

The only fully filled candidate quota is `car → vehicle`. All three candidates meet the hard-overlap/structural-divergence target except `insurance → coverage`, which is short by one. These are acquisition counts only; they do not indicate human labels or compatibility outcomes.

The sealed review packet hash is `bfe2141fadbf2ab4e3d9db0225dad290e7897cee7760e6516cd9b2a524eb74dd`. The label-only return template is ID-aligned with all 106 packets and starts completely blank. A deterministic replay reproduced the packets, rubric, judgment template, private ledger, and acquisition receipt byte-for-byte.

The post-review per-candidate analysis gate remains frozen: at least 12 `SAME`, 12 `DIFFERENT`, and four human-`DIFFERENT` packets in the high-overlap/structural-divergence stratum. A candidate that misses any floor remains underpowered. No labels have been opened, no compatibility model has been fit, and no memory, retrieval, or serving behavior has changed.
