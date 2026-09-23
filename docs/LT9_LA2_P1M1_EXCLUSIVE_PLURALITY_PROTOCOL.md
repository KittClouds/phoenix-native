# LT9-LA2 P1M1: exclusive-plurality routing anatomy

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m1-exclusive-plurality-anatomy-20260923`
Status: frozen before P1M1 corpus download, screening, or label review

## Question and scope

P1M1 asks whether invalid natural authority is concentrated in **contested-unique** routes, where distinct-marker plurality is unique but one or both endpoints also contain markers from another family. The causal probe routes only when both endpoints have exactly one active family and both families agree. Every mixed-family endpoint abstains.

The control is the P1L4 distinct-marker plurality with hard tie abstention: ties abstain, and a pair routes only when both endpoints have the same unique plurality winner. The candidate is `exclusive-only`: both endpoints must each contain markers from exactly one family, and those exclusive families must be the same. No thresholds, weights, marker changes, candidate exceptions, learned routing, retrieval, serving, or credit-router changes are permitted.

The P1L4 screen stopped after FEVER and MS MARCO had been assigned. CQADupStack, Climate-FEVER, and NQ were not opened by that screen. P1M1 does not reopen FEVER, MS MARCO, DBPedia, or any earlier discovery corpus.

## Frozen corpus order and role assignment

Screen only corpus text, in this order:

1. CQADupStack (`cqadupstack`): first eligible corpus is P1M1 discovery.
2. Climate-FEVER (`climate-fever`): next eligible corpus is P1M1Q qualification.
3. Natural Questions (`nq`): fallback only if either earlier corpus fails the frozen structural floor.

The first two eligible corpora are assigned discovery and qualification by order before any validity outcome is inspected. Stop screening once both roles are assigned. If fewer than two qualify, stop without opening validity labels. Only each archive's `corpus.jsonl` member may be extracted or read. Do not open qrels, queries, or BEIR relevance judgments.

Official BEIR archive identities frozen for the screen:

| Corpus | Archive URL | Expected MD5 |
| --- | --- | --- |
| CQADupStack | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/cqadupstack.zip | `4e41456d7df8ee7760a7f866133bda78` |
| Climate-FEVER | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/climate-fever.zip | `8b66f0a9126c521bae2bde127b4dc99d` |
| NQ | https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/nq.zip | `d4d3d2e48787a744b6f6e691ff534307` |

Dataset catalogue: https://github.com/beir-cellar/beir/wiki/Datasets-available.

## Label-blind structural screen

The router and classifications use distinct marker identities from the frozen P1L4 candidate bank. For an episode to count as `UNIQUE_PLURALITY`, both endpoint marker-count vectors must have one unique nonzero maximum and the same winning family. A tie, empty endpoint, or disagreement is outside this P1M1 state and abstains under the control.

Among control-routed `UNIQUE_PLURALITY` episodes:

- `exclusive_unique`: each endpoint has exactly one active marker family, and the families equal the route;
- `contested_unique`: at least one endpoint has active markers from another family in addition to the route winner.

These definitions are structural only. The screen may not inspect candidate expected context, witness polarity, actionable status, validity, or qrels.

A corpus is screen-eligible only if all conditions hold:

- at least 40 total control-routed `UNIQUE_PLURALITY` episodes;
- at least 20 `contested_unique` episodes, with at least 10 each for at least two candidate relations;
- at least 20 `exclusive_unique` episodes, with at least 5 each for at least two candidate relations;
- total unique-plurality and contested-unique episodes each occur in at least three of eight equal document-index shards.

Report event/episode totals and per-relation, per-class, and per-shard structural counts. The floor is frozen; do not lower it or screen on outcome labels.

## P1M1 discovery and P1M1Q qualification

After both corpus roles and input hashes are frozen, replay the control and exclusive-only candidate on discovery. Post-screen outcome auditing uses only the candidate bank's frozen expected context class; this is not BEIR relevance and loads no queries or qrels.

For every control-routed `UNIQUE_PLURALITY` episode, preserve endpoint family counts/masks, marker identities and their total/before/between/after occurrences, repeated surplus, winners and runner-up counts, active-family counts, endpoint agreement, support/negative cues, field agreement, candidate relation, episode delay, witness polarity, control/candidate routes and outcomes, and the mapped authority compartment. Preserve both directional relation rows and episode-level counts.

Before drawing a discovery conclusion, require at least 20 actionable valid and 5 actionable invalid control-routed contested-unique episodes, with invalid episodes represented in at least two shards. If this outcome floor fails, discovery is underpowered; do not select a policy.

Select exclusive-only for qualification only if the outcome floor passes, candidate replay has zero invalid episodes and zero invalid authority compartments, valid-actionable loss is at most 10%, new actionable abstention is at most 10% of baseline actionable routed episodes, and deterministic/credit-integrity checks pass. Report exclusive and contested classes separately, including actionable and authority outcomes. The diagnostic is not itself an authority promotion.

If exclusive-only is safe but exceeds the usefulness budgets, P1M2 spatial anatomy may be opened as a separate experiment. If any invalid episode or invalid authority compartment survives exclusive-only on discovery, stop router-rule refinement and audit the marker ontology/representation. If discovery selects exclusive-only, apply the exact frozen rule to the second eligible corpus. Qualification requires the same screen and outcome sufficiency plus zero invalid episodes and compartments, both 10% budgets, zero polarity errors and pending-capacity violations, bounded pending state, traceable owned-witness updates, and deterministic replay. Failure is closed and does not permit tuning on the qualification corpus.

Retrieval and serving remain dark. LA2-B remains blocked. P1L4Q and DBPedia residual episode rows remain closed.
