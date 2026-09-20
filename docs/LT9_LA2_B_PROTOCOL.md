# LT9-LA2-B: natural authority composition

LA2-B integrates only mechanisms that already qualified on their own:

```text
BM25F foundation
  + plurality dispatcher
  + unique-endpoint-agreement tie resolver
  + pending existence
  + decaying confidence
  + semantic expiry
```

The run is memory-first and uses the unopened HotpotQA corpus selected by the
predeclared corpus suitability screen. It does not change QPS serving, ranking
weights, lexical authority artifacts, or production state.

Frozen arms:

- `B0`: no learned lexical authority; replay-only baseline.
- `B1`: old fixed-priority context routing with qualified pending,
  confidence, and expiry credit routing.
- `B2`: qualified plurality plus unique-endpoint-agreement context routing with
  the same qualified credit router.
- `B3`: B2's learned authority quantities deterministically permuted across
  phenotype compartments after replay; this is a causal context-locality
  control, not a serving arm.

The natural replay pairs each candidate occurrence with the first later
compatible occurrence in the same candidate stream. The relevant-opportunity
clock is candidate-specific. Semantic expiry is explicit: deadline after 32
candidate opportunities, same-owner opposite-polarity supersession, and a
seven-entry pending capacity with oldest-entry eviction. Expired,
superseded, or evicted obligations cannot be resurrected by a late witness.

The first gate is memory integrity: deterministic replay, no resolved invalid
episodes, no stale-credit resurrection, no polarity ownership errors, pending
capacity respected, and every authority update traceable to an owned witness.
Late witnesses rejected after expiry are recorded separately from the failure
metric; rejection is expected safety behavior, while resurrection means an
expired obligation was incorrectly allowed to update authority.
Retrieval evaluation is authorized only after that receipt passes. B3 is
expected to preserve quantities while destroying phenotype ownership; parity
with B2 weakens the contextual-memory claim.
