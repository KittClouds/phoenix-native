# LT9-LA2 P1M2 label-blind preflight result

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m2-prospective-multicorpus-discovery-20260923`

Protocol: [`LT9_LA2_P1M2_PROSPECTIVE_MULTICORPUS_DISCOVERY.md`](LT9_LA2_P1M2_PROSPECTIVE_MULTICORPUS_DISCOVERY.md)

Receipt: [`screen-label-blind-20260923.json`](../experiments/lt9-la2-p1m2/screen-label-blind-20260923.json)

## Disposition

The frozen discovery cohort is **structurally underpowered** for P1M2. Its gate did not pass, so P1M2 stops before any expected-context validity outcomes are opened. No router, marker, credit, learner, retrieval, ranking, or serving behavior changed.

The receipt confirms `validity_labels_opened=false` and `qrels_or_queries_opened=false`. The discovery cohort contained 385 unique-plurality episodes, 32 contested episodes, five candidate relations represented among contested episodes, and 11 nonempty corpus-shard cells. The frozen requirements were 240 contested episodes, three corpora with at least 15 contested episodes each, four relations, and eight corpus-shard cells. The contested-count and per-corpus coverage requirements failed.

## Label-blind corpus counts

| Corpus | Documents | Unique-plurality episodes | Contested episodes |
| --- | ---: | ---: | ---: |
| CQADupStack | 457,199 | 167 | 10 |
| ArguAna | 8,674 | 7 | 0 |
| NFCorpus | 3,633 | 0 | 0 |
| Quora | 522,931 | 87 | 14 |
| SciDocs | 25,657 | 51 | 8 |
| TREC-COVID | 171,332 | 71 | 0 |
| SciFact | 5,183 | 2 | 0 |
| **Discovery total** | — | **385** | **32** |
| Webis-Touche2020 (qualification reserve) | 382,545 | 1,036 | 68 |

Webis-Touche2020 remains role-frozen as the P1M2 qualification reserve; its outcomes were not opened. It is not used to rescue the failed discovery preflight.

## Next boundary

Do not lower the preflight floor or inspect outcomes from this cohort. Any added discovery corpora require a new dated protocol, a frozen corpus list and hashes, and a prospective role assignment before outcome review. Keep the failure diagnostic-only: it says the chosen P1M2 cohort lacks the required risky-state coverage, not that the self-vote hypothesis is false.
