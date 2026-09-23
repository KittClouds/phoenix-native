# LT9-LA2 P1M2R outcome-anatomy protocol

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m2-fresh-cohort-20260923`

Status: frozen before expected-context outcomes are opened.

## Question and scope

Run the frozen P1M2 anatomy replay over the complete twelve-corpus P1M2R
discovery cohort and determine whether its unchanged physical-episode
outcome-sufficiency gate is met. This is discovery-only. It may describe
where valid and invalid outcomes occur and report the already specified
self-vote, endpoint-identity, competing-family, spatial, and authority
receipts. It may not select, tune, or recommend a routing rule.

No natural learner, credit rule, context router, retrieval, ranking, serving,
lexical authority, or query-time transport behavior changes in this protocol.
Webis-Touche2020 remains the qualification-only reserve, and its validity
outcomes remain unopened.

## Frozen input identity

The exact cohort, order, source archive, normalized corpora, screen, and
preflight outcome are those sealed by commit `b3df4d0` and recorded in:

- `docs/LT9_LA2_P1M2R_LOTTE_PROSPECTIVE_DISCOVERY_20260923.md`
- `docs/LT9_LA2_P1M2R_PREFLIGHT_RESULTS_20260923.md`
- `experiments/lt9-la2-p1m2r/artifact-manifest-20260923.json`
- `experiments/lt9-la2-p1m2r/lotte-normalization-receipt-20260923.json`
- `experiments/lt9-la2-p1m2r/screen-label-blind-20260923.json`
- `experiments/lt9-la2-p1m2r/screen-label-blind-rerun-20260923.json`

The artifact-manifest SHA-256 is
`092d7e5e730a5b07780f233bf6755b8d60ef21b4921d087d51b990b02e145360`.
The label-blind screen and deterministic rerun both have SHA-256
`bc32d310b757e8919b45de9c058ebce59892de15da2c5f11527ee800d26b18a0`.
The normalized-input receipt SHA-256 is
`b8f87432ea9aa12edc545f7f4a07b3b9389628f76eeceae690aed1da559ad80b`.

The discovery order is fixed and the entire cohort must be replayed:

1. CQADupStack
2. ArguAna
3. NFCorpus
4. Quora
5. SciDocs
6. TREC-COVID
7. SciFact
8. LoTTE-writing
9. LoTTE-recreation
10. LoTTE-science
11. LoTTE-technology
12. LoTTE-lifestyle

Do not stop early when any corpus appears favorable or unfavorable. Do not
drop or replace a corpus after outcomes are observed.

The frozen source-mechanism inputs at this boundary have these SHA-256 values:

- `lt9_la2p1m1_core.rs`:
  `dc4220b075ac2df92c32bc37d853fc1f5deb2ed0ce8837cbe48d5a31f9f0d215`
- `lt9_la2p1m2r_screen.rs`:
  `356a9f4418c106355d06910d93ba67e8524156390345a4ec6d97fd96c4f4db77`
- `lt9_la2p1m2_sufficiency.rs`:
  `e63b81dc7a90436d8eeb5639f9a420098780a85ec4005865a8e4f9ecd7921635`
- `lt9_la2p1m2.rs`:
  `4f024b7e2c57a198e57453be2b91d5ff7275f00661800358c3d0b3192fd5d60a`

The P1M2R screen passed its frozen label-blind structural floor: 254
contested directional episodes, three corpora with at least 15, eight
candidate relations, and 28 corpus-shard cells. The exact screen and input
hashes above are mandatory replay inputs.

## Outcome replay contract

Use the P1M2 tokenizer, candidate definitions and expected-context mapping,
nearest-pair/event construction, distinct-marker plurality with hard
tie-abstention, episode construction, credit trace, actionability rule,
shard rule, and anatomy calculations unchanged. Reset credit state at each
corpus boundary. The new replay executable must validate the P1M2R screen
schema, exact twelve-corpus order, roles, structural-gate values, and hashes
before reading expected-context outcomes. Freeze its source and executable
hashes in a prepared replay manifest before the first outcome replay.

After that prepared manifest is sealed, one complete replay may inspect the
candidate expected-context labels frozen in the candidate table. These are
diagnostic class labels only; they do not change nomination, routing, witness
ownership, or credit updates. No BEIR query, qrel, relevance judgment, or
LoTTE query/QAS data may be read.

Report every routed directional row, then report physical episodes using
the frozen key `(corpus, nomination document, witness document)`. Keep
directional rows visible but do not count them as independent episodes. For
the outcome gate, include only actionable `CONTESTED_UNIQUE` rows, where
actionable means a frozen support or contradiction witness exists. A physical
episode with both valid and invalid directional rows is mixed and excluded
from unanimous valid/invalid counts. Unresolved, unspecified, and abstaining
rows do not create authority and do not enter the valid/invalid sufficiency
counts.

The baseline outcome is `VALID` when the frozen route matches the candidate's
expected family, `INVALID` when a routed actionable episode does not match,
`ABSTAIN` when the route abstains, and `UNSPECIFIED` where no expected family
was declared. The context-only/self-vote route and all other predeclared
anatomy remain descriptive counterfactuals; none may replace the baseline in
the gate.

## Frozen physical-episode outcome gate

Retain the original P1M2 thresholds and definitions without modification.
P1M2R is outcome-sufficient only if the complete twelve-corpus cohort has:

- at least 12 unanimous actionable contested-invalid physical episodes;
- invalid episodes in at least 2 discovery corpora, spanning at least 3
  candidate relations and at least 4 `(corpus, shard)` cells;
- at least 40 unanimous actionable contested-valid physical episodes; and
- valid episodes in at least 2 discovery corpora, spanning at least 3
  candidate relations.

These floors authorize interpretation of the frozen anatomy only. They are
not targets for router selection. If any floor fails, report
`OUTCOME_UNDERPOWERED`; do not lower a threshold, reinterpret abstention,
choose a router, inspect the reserve, or create a replacement cohort under
this protocol. Any further discovery requires a new dated protocol and new
sealed input identities.

## Required receipt and firewall

The final replay receipt must bind this protocol hash, the prepared replay
manifest, source/screen/input hashes, executable hash, all twelve corpus
receipts in the frozen order, directional and physical episode totals,
valid/invalid/unresolved/abstain/unspecified counts, mixed physical episodes,
the unchanged sufficiency-gate details, and all predeclared anatomy fields.
It must explicitly record:

```text
validity_labels_opened=true
discovery_cohort_complete=true
qrels_or_queries_opened=false
reserved_qualification_labels_opened=false
router_selected=false
retrieval_or_ranking_run=false
```

The result is an anatomy-only discovery receipt. It cannot qualify a router,
authorize LA2-B, or promote lexical authority. Webis-Touche2020 stays sealed.
