# LT9-LA2-P1K1: Mixed-Marker Assignment Diagnostic

## Status and scope

P1K1 is a descriptive replay of the already inspected P1J3 discovery set. It
does not qualify a routing policy, train or update lexical authority, change
the join/compatibility rule, or touch ranking and serving. The 79 broad rows
and 14 original P1J rows remain immutable. The six directional invalid
actionable rows are also reported as three underlying `(shard, nomination
document, witness document)` episodes.

The hypothesis under audit is that fixed family priority can override stronger
local marker evidence, causing a mixed-marker endpoint to receive the wrong
phenotype and thereby participate in a false same-phenotype join.

## Frozen population and integrity checks

Reconstruct the P1J3 eight contiguous FiQA JSONL shards using its exact marker
families, window, candidate list, first candidate/phenotype nomination,
earliest later same-phenotype witness (falling back to the earliest later
candidate event), support/contradiction/abstention rule, and exclusion of the
14 P1J identities. Before interpreting P1K1, verify the corpus digest, P1J and
P1J3 receipt hashes against the P1J3 feature-audit manifest, the two P1J3 pair
roster digests against each other and the manifest, and exact same-phenotype
row parity. Required counts are 79 broad rows: 67 same-phenotype and 12
cross-phenotype, with 9 shard rows excluded by P1J identity.

No row may be dropped or relabeled based on the counterfactual. P1J3's 79-row
population and 14-row discovery set are not qualification evidence for P1K2.

## Endpoint assignment record

At both endpoints of every frozen pair, report all three counts in the order
`finance, geography, transport`, the current fixed-priority assignment, all
raw count-argmax families, tie status, maximum and runner-up count, their
margin, whether a unique argmax disagrees with fixed priority, and whether the
assignment is unchanged under the counterfactual. An inversion is defined
only when there is a unique raw argmax and fixed priority chooses a different
family. Ties are not inversions; disagreement is undefined for tied maxima.

## Single descriptive counterfactual

Recompute endpoint routing as `argmax_f n_f`. If multiple families tie for the
maximum—including an all-zero vector—the endpoint abstains. Keep nominations,
witnesses, ownership, witness labels, and the frozen pair roster unchanged.
This counterfactual changes assignment only; it does not regenerate joins or
apply authority updates.

For every pair and physical episode, report endpoint route transitions and
the resulting descriptive state: valid, invalid, cross-phenotype, or abstain
because an endpoint tied. Explicitly count whether each invalid actionable
episode ceases to be finance-local, whether both endpoints receive non-finance
plurality routes, whether both-endpoint plurality repairs the expected family,
and whether the counterfactual creates a cross-route or abstention. Count
valid episodes whose endpoint assignment changes and those with at least one
valid direction converted to tie-abstention.

## Prevalence

At directional-join level, report `P(invalid | priority inversion)` and
`P(invalid | no priority inversion)` among actionable rows, and again among
all conclusive valid/invalid rows. Cross-phenotype and abstention outcomes are
not silently called valid or invalid.

At physical-episode level, use any endpoint inversion across its directional
rows as the episode exposure. Report both actionable-only and all-conclusive
rates. If an episode contains both valid and invalid directions, report it as
mixed and outside the binary rate denominator; never treat its directional
rows as independent episodes. Preserve directional counts alongside episode
counts.

## Interpretation and next gate

P1K1 can support or weaken the assignment hypothesis, but cannot establish a
purity invariant or authorize a guard. A supportive pattern may motivate a
separate P1K2 assignment experiment. P1K2 must freeze its policy before using
genuinely unopened episodes; all eight P1J3 shards have already been inspected.
No learner or serving change follows from P1K1 alone, and LA2-B remains
blocked pending context-routing qualification.
