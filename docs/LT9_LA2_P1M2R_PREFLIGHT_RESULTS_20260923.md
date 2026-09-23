# LT9-LA2 P1M2R label-blind preflight result

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m2-fresh-cohort-20260923`

Protocol: [`LT9_LA2_P1M2R_LOTTE_PROSPECTIVE_DISCOVERY_20260923.md`](LT9_LA2_P1M2R_LOTTE_PROSPECTIVE_DISCOVERY_20260923.md)

## Disposition

The frozen twelve-corpus cohort **passes the structural gate**. This permits a separate, preregistered outcome-anatomy replay; it does not qualify a router or open Webis-Touche2020. The screen and deterministic rerun have identical SHA-256 receipts. Expected-context labels and BEIR queries/qrels remain unopened.

The structural receipt reports 3,806 routed unique-plurality episodes, 254 contested directional episodes, three discovery corpora with at least 15 contested episodes, eight candidate relations with contested episodes, and 28 nonempty corpus-shard cells. Frozen minimums were 240, three, four, and eight respectively.

## Label-blind cohort counts

| Corpus | Documents | Routed unique-plurality episodes | Contested episodes |
| --- | ---: | ---: | ---: |
| CQADupStack | 457,199 | 167 | 10 |
| ArguAna | 8,674 | 7 | 0 |
| NFCorpus | 3,633 | 0 | 0 |
| Quora | 522,931 | 87 | 14 |
| SciDocs | 25,657 | 51 | 8 |
| TREC-COVID | 171,332 | 71 | 0 |
| SciFact | 5,183 | 2 | 0 |
| LoTTE-writing | 277,072 | 498 | 40 |
| LoTTE-recreation | 263,025 | 143 | 4 |
| LoTTE-science | 1,694,164 | 473 | 26 |
| LoTTE-technology | 1,276,222 | 242 | 14 |
| LoTTE-lifestyle | 268,893 | 2,065 | 138 |
| **Total** | — | **3,806** | **254** |

The three corpora meeting the per-corpus contested floor are LoTTE-writing (40), LoTTE-science (26), and LoTTE-lifestyle (138). The prospective P1M2 physical-episode outcome sufficiency gate remains a separate requirement and has not been evaluated.

## Integrity and firewall

The LoTTE source archive SHA-256 is `37c0f39af23a6e3464f63395a4d04a22b91fe59c1aa64ea1773a8aff113c7ab5` at 3,576,167,599 bytes. Only the ten named topic `collection.tsv` members were extracted. Normalization records the source-member and normalized-corpus hashes; the artifact manifest records normalizer/screen binary hashes and the identical deterministic rerun hash.

`validity_labels_opened=false`, `qrels_or_queries_opened=false`, and `reserved_qualification_labels_opened=false`. The normalizer also records `queries_or_qas_opened=false`. Webis-Touche2020 remains qualification-only. No router, credit, learner, retrieval, ranking, or serving behavior changed.

## Next boundary

Freeze the exact screen, normalization, and source identities with a separate P1M2R outcome-anatomy protocol before inspecting expected-context labels. If the physical-episode outcome gate fails, report outcome-underpowered and do not choose a router. Any qualification remains a later, separate step.
