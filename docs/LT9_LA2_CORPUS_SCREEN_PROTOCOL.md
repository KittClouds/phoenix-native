# LT9-LA2 Corpus Suitability Screen

## Scope

This is a label-blind preflight for selecting an unopened BEIR-style corpus
for P1K2. It reads only corpus records. It does not read queries, qrels,
review packets, human judgments, or validity labels, and it makes no learner,
authority, ranking, compatibility, or serving changes.

The screen replays the frozen LT9 candidate bank, nearest-occurrence rule,
context marker families, eight document shards, pair-distance limit, and
earliest same-phenotype witness contract. It reports event density and routing
features only; it does not classify joins as valid or invalid.

## Frozen viability floor

A corpus is screen-eligible only if all of these are met:

- at least 4 distinct frozen candidate relations;
- at least 20 same-phenotype physical episodes;
- at least 10 mixed-marker endpoints;
- at least 5 priority-inversion endpoints;
- at least 2 observed phenotype families.

These floors are fixed before suitability results are inspected. They are
intended to provide enough routing events to exercise P1K2, not to optimize a
qualification outcome.

## Reported fields

For each corpus the receipt records:

- corpus SHA-256 and document count;
- observed event and nomination counts;
- distinct candidate relation count;
- directional joins and same-phenotype joins;
- same-phenotype physical episodes;
- actionable witness count (support or contradiction only);
- mixed-marker and priority-inversion endpoint counts and rates;
- priority-inversion episode count;
- phenotype-family coverage;
- total physical episode count;
- frozen-floor result and explicit unmet reasons.

“Actionable” here means only that the frozen witness evidence is support or
contradiction. It is not a relevance or validity label.

## Selection rule

The first corpus in the predeclared screen order that meets the floor may be
used for the unchanged P1K2 assignment comparison. Corpus selection uses only
event density and routing coverage. FiQA and SciFact remain excluded from
assignment qualification because they were already used in prior discovery or
underpowered qualification work.

P1K2 remains the authority gate. A screen pass cannot qualify a routing policy
or unblock LA2-B.
