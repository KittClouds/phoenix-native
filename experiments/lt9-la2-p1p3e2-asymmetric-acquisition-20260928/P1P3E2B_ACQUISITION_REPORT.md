# P1P3E2-B Acquisition Result

**Date:** 2026-09-28  
**Protocol lock:** `P1P3E2B_ACQUISITION_LOCK.json`  
**Lock SHA-256:** `33dfd6045388ff05948cdf739546b70b2c3c5034260fe2dea38fcef0f9588a7b`  
**Protocol commit:** `c1c9239` (`Freeze P1P3E2-B natural acquisition protocol`)

## Result

The frozen label-blind acquisition filled the complete bank:

| Population | Planned | Acquired |
|---|---:|---:|
| Primary natural pairs | 216 | 216 |
| Reserve natural pairs | 108 | 108 |
| Primary per directed relation | 24 | 24 for all 9 |
| Reserve per directed relation | 12 | 12 for all 9 |

Every relation met the pre-frozen `14 TRAIN-NEW / 5 DEV-NEW / 5 TEST-NEW` split. Each lane met its frozen split quota. No relation/lane/split group was underfilled. The maximum primary corpus contribution was 12 per relation; the maximum reserve contribution was 6 per relation.

| Directed relation | Primary source corpus counts |
|---|---|
| `bank→lender` | FiQA 12, HotpotQA 5, Quora 7 |
| `bank→water` | Quora 10, FiQA 6, HotpotQA 8 |
| `car→vehicle` | Quora 12, FiQA 5, HotpotQA 7 |
| `credit→loan` | FiQA 12, Quora 11, HotpotQA 1 |
| `engine→motor` | Quora 12, HotpotQA 11, NQ 1 |
| `insurance→coverage` | Quora 7, FiQA 11, HotpotQA 6 |
| `loan→debt` | Quora 9, FiQA 12, HotpotQA 3 |
| `stock→bond` | Quora 9, FiQA 12, HotpotQA 3 |
| `vehicle→car` | HotpotQA 12, SciDocs 6, Quora 4, ArguAna 1, FiQA 1 |

The nine frozen sources were FiQA, SciFact, ArguAna, HotpotQA, NFCorpus, NQ, Quora, SciDocs, and TREC-COVID. Webis-Touche2020 remained sealed. Acquisition read query/document text and IDs only. It did not read qrels, ranking outcomes, E1 predictions, or labels. Relation support counts were used only as frozen search priorities.

## Integrity checks

The acquired artifacts passed the label-blind verifier:

* 216 primary reviewer packets have exactly four visible fields: packet ID, directed relation, masked query context, and masked document context.
* All 324 private primary/reserve records remain unlabeled.
* All candidate terms are masked from their respective context pairs.
* No duplicate base groups remain.
* No query/document identity from the frozen prior/canary exclusion projection appears in the new bank.
* Cross-split query/document identity overlap is zero for every split pair.
* Primary and reserve corpus caps hold.

Three synthetic-fixture unit tests passed; Python compilation passed. The full corpus acquisition itself was a single frozen run. No model fitting, E1 inspection, authority update, retrieval run, or serving change occurred.

## Frozen artifacts

The private candidate ledger, randomized review packets, and blank judgment template are stored outside the repository at:

`D:\phoenix-evals\lt9-la2-p1p3e2b-acquisition-20260928\bank-v1\`

| Artifact | SHA-256 |
|---|---|
| `private-candidate-ledger.jsonl` | `d481a5845f028ae6141559112ad1eb70e88f1efe2160fc7b5e50cdf011c85f84` |
| `review-packets.json` | `95a6686b2b33d7203674d683d5617fcbb5bce13e635a8b3ee5f3b434016b7d94` |
| `judgments-template.json` | `eab790bf78895a0e4cb6916e7dcb22b597df575b6327b4e0de626ec1e56f30de` |
| `acquisition-receipt.json` | `7d969672591c4d007955a04269b0b3e55ef005a48c7ad5f0255c4c1db2b2f922` |
| `verification-receipt.json` | `5ac22e206735f3bf40d0d84893b9d0079aeb6539b93c03a798ce00df2c6b9f45` |

The acquisition and verification receipts are also checked into this experiment folder as `P1P3E2B_ACQUISITION_RECEIPT.json` and `P1P3E2B_VERIFICATION_RECEIPT.json`.

Review packets intentionally omit source corpus, IDs, partition, lane, support counts, qrels/rank, and retrieval-opportunity status. The blank template contains only `packet_id` and `judgment: null`. Labels have not been opened or joined. Continue by reviewing the randomized packet file and returning judgments against the template; preserve reviewer provenance and keep TRAIN/DEV/TEST identities private.
