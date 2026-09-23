# LT9-LA2 P1M2: prospective multi-corpus routing anatomy

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m2-prospective-multicorpus-discovery-20260923`

Status: frozen before P1M2 label-blind preflight; no candidate expected-context outcomes opened by this protocol.

## Question

P1M2 tests whether candidate-token marker identities are leaking into natural
phenotype assignment, and collects cross-endpoint marker-identity and spatial
anatomy across a prospective multi-corpus discovery cohort. It is diagnostic
only. It does not change the router, credit mechanism, learner, lexical
authority, ranking, retrieval, or serving.

Primary diagnostic:

```text
baseline marker evidence = all marker occurrences in the frozen context window
context-only diagnostic = same evidence after subtracting the exact source and
                          target token positions from marker counts
```

The context-only route is a descriptive counterfactual. It is not applied to
memory, authority, ranking, or serving. No marker, relation, threshold, or
weight may be added or tuned from this experiment.

## Prior-use boundary and cohort

The P1M2 discovery cohort is fixed in this order:

1. CQADupStack
2. Arguana
3. NFCorpus
4. Quora
5. SciDocs
6. TREC-COVID
7. SciFact

All seven members are replayed as one discovery cohort. No member is dropped
or stopped early because another corpus produces an attractive result. The
following corpus is reserved for a later, separate qualification only:

8. Webis-Touche2020

Its P1M2 expected-context outcomes remain unopened. Its corpus text may be
used for the same label-blind preflight as the discovery set.

Natural Questions is excluded from P1M2 discovery and cannot serve as fresh
qualification evidence: the existing `D:\phoenix-evals\lt9-la2p1k2-nq\receipt.json`
records 131 evaluated routing pairs on the NQ corpus hash. This does not alter
the P1M1 historical record; it prevents P1M2 from describing NQ as outcome
unopened.

Other excluded corpora have already had expected-context routing outcomes
opened in prior LT9 work: FiQA, HotpotQA, DBPedia, Climate-FEVER, FEVER, and
MS MARCO. They are not eligible for this prospective cohort.

## Label-blind input freeze

Before outcome replay, run the preflight on exactly the eight listed corpora.
The preflight may read only each frozen source archive and `corpus.jsonl`.
It records archive SHA-256, corpus SHA-256, sizes, document counts, event
counts, routed episode structure, candidate relations, shard distribution,
and self-vote route transitions. It must not inspect queries, qrels, BEIR
relevance judgments, or candidate `expected_phi` values. The resulting input
manifest is immutable before the replay opens expected-context outcomes.

The frozen candidate order is `lt9_la2p1m1_core::CANDIDATES`; the event
tokenizer, nearest source/target occurrence, context window, pair formation,
distinct-marker hard-abstention router, witness polarity, and credit replay
are unchanged from P1M1/LA2-B. Eight equal document-index shards are used.
Directional candidate episodes and de-duplicated physical document-pair
episodes are reported separately. The seven corpus streams have no shared
chronological order, so the unchanged in-memory credit replay resets at each
corpus boundary; authority never transfers between corpora. Cohort statistics
aggregate the seven separate receipts.

Before expected-context outcomes may be opened, the label-blind preflight
must show, across the seven discovery corpora:

- at least 240 baseline `UNIQUE_PLURALITY` contested directional episodes;
- at least 3 corpora with 15 or more contested episodes each;
- at least 4 distinct candidate relations with contested episodes; and
- at least 8 nonempty `(corpus, document shard)` cells.

These structural floors are fixed before the screen. If they fail, do not
open expected-context outcomes. Add fresh corpora only under a new dated
protocol and freeze the extended cohort before outcome review.

P1M2 reports endpoint family masks, distinct identities, raw occurrences,
repeated surplus, and before/between/after placement. It also reports the
winner identity overlap/Jaccard between endpoints, persistent competing
families, and whether the same competing marker identities persist.

## Prospective sufficiency gate

The outcome gate is frozen before any P1M2 expected-context result is read.
Use physical `(corpus, nomination document, witness document)` episodes as
the independent unit. Directional rows remain visible but do not multiply
the sufficiency count. A physical episode with both valid and invalid
directional rows is mixed and excluded from the unanimous valid/invalid
gate counts.

P1M2 anatomy is interpretable only if the complete seven-corpus discovery
cohort contains all of the following:

- at least 12 unanimous actionable contested-invalid physical episodes;
- those invalid episodes occur in at least 2 discovery corpora, span at
  least 3 candidate relations, and cover at least 4 `(corpus, shard)` cells;
- at least 40 unanimous actionable contested-valid physical episodes;
- those valid episodes occur in at least 2 discovery corpora and span at
  least 3 candidate relations.

These are sufficiency gates, not fitting objectives. If any floor fails,
report outcome-underpowered and do not select a routing policy. Any larger
or replacement cohort requires a new dated protocol and a new frozen input
identity set.

## Self-vote diagnostic

For each routed episode, compute the baseline family masks from the frozen
distinct marker identities. Then compute context-only masks by removing one
occurrence for each exact candidate endpoint token that is itself a family
marker. A marker identity remains active in context-only evidence if at least
one occurrence of that identity remains elsewhere in the context window.
This removes only the two selected source/target positions; other occurrences
of the same token remain valid contextual observations.

Report baseline-to-context-only endpoint and pair transitions: unchanged,
changed route, route-to-abstain, abstain-to-route, or unchanged abstention.
Compare transitions for valid, invalid, unresolved, and abstaining baseline
episodes after the cohort input manifest is sealed. Do not turn any observed
separation into a router change within P1M2.

## Decision boundary

P1M2 can describe whether self-votes, endpoint marker identity overlap,
persistent competitor evidence, or spatial placement co-vary with routing
validity. It cannot choose a guard, a threshold, or a new router. A follow-up
must use a fresh dated protocol and evidence that was not used to choose its
rule. Webis-Touche2020 remains reserved until that follow-up is frozen.

Retrieval, serving, and LA2-B remain blocked. Credit-routing rules remain
sealed and unchanged.
