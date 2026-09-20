# LT9-LA2-P1J3 protocol

P1J3 is a diagnostic context-routing qualification. It does not change the
natural learner, authority state, ranking, or serving path.

## Frozen inputs and replay

- Re-read the FiQA corpus in JSONL line order and pin its SHA-256.
- Verify that recomputation of the global P1J first-nomination / earliest
  same-phenotype witness pairs exactly matches the 14 identities in the sealed
  P1J receipt, including its class and witness-kind labels. P1J records both
  actionable and abstaining invalid joins under its single `INVALID` label;
  P1J3 splits those by witness kind only for impact reporting. Abort on any
  identity or semantic mismatch.
- Evaluate those same 14 joins as the discovery receipt.
- For generalization, partition all FiQA lines into eight equal contiguous
  shards using integer boundaries `start=floor(i*N/8)` and
  `end=floor((i+1)*N/8)`. Inside each shard, replay the same first
  candidate/phenotype nomination rule and witness ownership: earliest later
  same-candidate/same-phenotype event, falling back to the earliest later event
  for that candidate. Exclude any broad pair identical to one of the frozen 14.
- Apply only the preregistered `side_mask_hamming <= 2`, existing P1J
  runner-up route agreement, and their conjunction. No other features, new
  thresholds, corpus selection, or retrieval-quality values are permitted.

Route agreement means nomination and witness `runner_up()` marker-family
labels are equal. Side distance is the sum of Hamming distances over the
existing nomination/witness `before_masks` and `after_masks` arrays. Both
guards run only after a witness arrives: rejection leaves the nomination
present, turns an actionable witness into an abstention, and prevents its
downstream authority update.

## Frozen context-routing gate

The broader replay must have at least 20 valid and 5 invalid actionable
same-phenotype joins, with invalid joins represented in at least two shards.
For a guard to qualify:

- it must reject at least one invalid actionable join;
- candidate-level false authorization must decrease;
- valid actionable loss must be at most 10%;
- new abstentions among actionable same-phenotype witnesses must be at most
  10%.

If the broad sample floor or any guard condition fails, context routing is not
qualified and LA2-B remains blocked. Passing this diagnostic qualifies only a
routing mechanism for a separate integration experiment; it does not promote
lexical authority or serving behavior.

If both standalone guards qualify and make identical broad-slice decisions,
select route agreement as the smaller rule: equality of the existing route
labels. If both qualify but differ, select by lowest valid actionable loss,
then lowest remaining false-authorized candidate count, then fewest new
abstentions. Use the conjunction only when neither standalone guard qualifies.

## Required receipt

Report valid/invalid joins, actionable witnesses retained/rejected, nomination
counts and any suppression, existing/new abstentions, family distribution
before/after, authority updates prevented, candidate-level false
authorization, broad shard coverage, pair-decision equivalence, input hashes,
and deterministic rerun hashes. Prefer the simpler guard only when decision
sets and safety outcomes remain equivalent on the broad slice.

## Post-gate feature audit

If no guard qualifies, the optional fifth CLI argument writes a pair-level
diagnostic roster for the same broad slice, excluding the frozen discovery
identities. It exposes only already-present P1J context evidence: family
counts/masks, side masks, runner-up labels, support/negative cues, window
length, same-field agreement, fingerprint equality, and document delay.
This roster is descriptive only. It does not alter guard decisions, search new
thresholds, train a compatibility gate, or authorize lexical updates.
