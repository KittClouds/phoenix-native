# LT9-LA2-P1L4: Distinct-Marker Plurality with Hard Tie Abstention

Date: 2026-09-23
Branch: codex/phoenix-native-p1l4-hard-tie-abstention-20260923
Status: frozen before P1L4 corpus-text screening; no labels opened

## Question and scope

P1L4 tests one counterfactual only: keep distinct-marker family counts and replace the frozen unique_endpoint_agreement tie rescue with abstention whenever either endpoint has an exact plurality tie. This is discovery first, qualification second. No marker edits, lexical exceptions, thresholds, tie heuristics, learned weights, retrieval, serving, or credit-router changes are allowed.

The control is the unchanged distinct-marker plurality plus unique_endpoint_agreement router. The candidate routes only when both endpoints have one distinct-marker plurality winner and those winners agree. Every other case, including a tie at either endpoint, abstains.

## Frozen corpus selection and role assignment

Run the label-blind corpus-text screen in exactly this order:

1. FEVER (`fever`) — first eligible corpus is P1L4 discovery.
2. MS MARCO (`msmarco`) — next eligible corpus is P1L4Q qualification.
3. CQADupStack (`cqadupstack`).
4. Natural Questions (`nq`).
5. Climate-FEVER (`climate-fever`).

The first two corpora meeting the structural floor are assigned by position: first is discovery, second is qualification. Do not inspect validity outcomes until both roles and hashes are frozen. If fewer than two candidates qualify, stop before opening labels. DBPedia-Entity, HotpotQA, and all previously locally screened corpora are excluded from P1L4 corpus selection.

The order and public archive checksums are frozen from the official BEIR catalogue/archive. Only each archive's `corpus.jsonl` member may be extracted and read. Do not open `qrels`, queries, or any relevance judgments during screening. The screen may read corpus text only to replay the frozen candidate/marker event builder.

The official archive checksums and URLs frozen for this ordered screen are:

| Corpus | Official archive | Expected MD5 |
| --- | --- | --- |
| FEVER | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/fever.zip | 5a818580227bfb4b35bb6fa46d9b6c03 |
| MS MARCO | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/msmarco.zip | 444067daf65d982533ea17ebd59501e4 |
| CQADupStack | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/cqadupstack.zip | 4e41456d7df8ee7760a7f866133bda78 |
| NQ | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/nq.zip | d4d3d2e48787a744b6f6e691ff534307 |
| Climate-FEVER | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/climate-fever.zip | 8b66f0a9126c521bae2bde127b4dc99d |

Source: official BEIR datasets catalogue at https://github.com/beir-cellar/beir/wiki/Datasets-available.

## Label-blind structural coverage floor

A corpus is eligible only if its frozen distinct-marker event stream has:

- at least 40 episodes for which the frozen unique_endpoint_agreement returns a route through `TIE_RESOLVED`;
- at least 10 such episodes for each of at least 2 candidate relations;
- such episodes represented in at least 3 of the 8 equal document-index shards.

Report total tie-resolved episodes, candidate-relation counts, shard counts, all episodes, events, and corpus hash. No expected phenotype, witness polarity, validity outcome, or qrels may affect eligibility. The floor is fixed; do not lower it or substitute generic episode counts.

## P1L4 discovery

After the eligible corpus order and corpus hashes are frozen, evaluate the unchanged control against hard tie-abstention on the first corpus only. Use the same P1L3/LA2-B tokenizer, marker identities, context windows, nearest-pair construction, episode ordering, witness ownership, pending-credit, confidence, expiry, and capacity rules.

Post-hoc routing validity uses only the frozen candidate bank's expected context class. It is not BEIR relevance judgment and does not load qrels or queries. The control must provide at least 20 actionable valid episodes and 5 actionable invalid episodes overall across at least 2 shards, including at least 20 valid and 5 invalid actionable tie-resolved episodes, with tie-resolved invalids in at least 2 shards. Otherwise discovery is underpowered and stops without changing the policy.

Record invalid episodes prevented, valid episodes lost, authority updates lost, invalid authority compartments, positive/negative authority-update counts and summed authority magnitude, routed phenotype distribution, new abstentions, and all frozen credit-integrity receipts. Discovery may select the simpler candidate only if it prevents the invalid tie authority without breaching the existing 10% valid-loss or new-abstention budgets, and the full candidate replay has zero invalid episodes and zero invalid authority compartments. If not, hard abstention does not advance; P1L5 tie-information anatomy is the only next resolver branch.

## P1L4Q untouched qualification

Only if discovery selects hard tie-abstention, evaluate that exact frozen policy on the second eligible corpus. Do not use the qualification corpus to alter it. Apply the same corpus floor and label-based sufficiency and safety gates. Require zero invalid episodes and zero invalid authority compartments, valid-loss and new-abstention fractions each at most 10%, zero polarity errors, zero pending-capacity violations, bounded pending state, traceable owned-witness updates, and deterministic replay. Failure is closed and does not authorize another rule on this corpus.

If qualification passes, it only qualifies context routing. A fresh protocol and fresh stream are still required before LA2-B memory integration. Retrieval and serving remain dark.

## Reproducibility and stopping

Hash source, protocol, downloaded archive, extracted corpus, screen receipts, discovery/qualification receipts, and executable. Run deterministic replay twice. Preserve a separate row-level receipt while reporting episodes as the statistical unit. No qrels or ranking evaluation may be loaded in P1L4.
