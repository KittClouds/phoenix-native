# LT9-LA2-P1K2: Phenotype Assignment Qualification

## Scope

P1K2 is the preregistered follow-up to the descriptive P1K1 inversion audit.
It uses the unopened SciFact corpus and does not reuse the inspected FiQA P1J3
shards, P1J3 exclusions, P1J3 joins, or P1J3 outcomes. It changes no natural
learner, lexical authority, ranking, or serving state.

The only two assignment policies are:

1. the existing fixed priority (`geography` first, then `finance`, then
   `transport`, with fallback), and
2. the P1K1 counterfactual: route to the unique maximum marker-count family;
   a tie, including all-zero counts, abstains.

No threshold search, feature addition, compatibility guard, or dominance rule
is allowed in this experiment.

## Population

Replay the exact frozen marker families, context window, candidate list,
nearest occurrence, pair-distance, support/contradiction/abstention, and
first-nomination/earliest-same-phenotype witness contract used by P1J3. Form
eight deterministic contiguous document shards and report both directional
candidate rows and physical document-pair episodes. The corpus digest and
document count are sealed in the receipt. P1K2 does not claim independence
from other BEIR quality work; it is unopened specifically for this phenotype
assignment protocol.

## Assignment comparison

For every endpoint record the complete finance/geography/transport count
vector, fixed-priority route, unique plurality route or tie, margin, and
priority inversion. Keep pair identity, witness ownership, and witness kind
fixed while evaluating the counterfactual. A plurality tie is an abstention;
it is not a priority inversion and cannot silently count as valid or invalid.

## Predeclared sufficiency and routing gate

The corpus must contain at least 20 current valid actionable rows, 5 current
invalid actionable rows, and invalid actionable rows in at least 2 shards.
The assignment policy may qualify only if all of the following hold:

* counterfactual invalid actionable rows are fewer than current invalid
  actionable rows;
* current valid actionable loss is at most 10%; and
* new tie-abstentions among current actionable rows are at most 10%.

Report current and counterfactual rows, valid loss, invalid rejection,
abstention, shard coverage, and physical-episode counts. The episode report
must retain directional rows and must not treat reciprocal candidate rows as
independent physical episodes.

If any sufficiency or safety condition fails, the assignment gate remains
unqualified. A pass authorizes context-routing integration investigation only;
it does not unblock LA2-B until pending-credit routing is also qualified and a
separate integration run is frozen.
