# LT9-LA2-P1K4: frozen tie resolver qualification

P1K4 evaluates the frozen `unique_endpoint_agreement` policy on HotpotQA, the
next unopened corpus after NQ in the preregistered suitability order. The P1K2
HotpotQA receipt is consumed unchanged; no corpus labels, learner state,
authority updates, ranking, or serving behavior are touched.

The router is hierarchical and categorical:

```text
plurality assignment
  -> only exact tie rows enter the resolver
  -> unique nomination family + witness two-way tie containing it: route
  -> unique witness family + nomination two-way tie containing it: route
  -> every other tie: abstain
```

No numeric thresholds, weight fitting, or fallback to the old fixed family
priority are permitted. NQ remains discovery-only.

Qualification uses the frozen P1K2 sufficiency floor (at least 20 valid
actionable rows, at least 5 invalid actionable rows, and invalid rows in at
least 2 shards), then applies episode-primary safety gates:

- zero resolved invalid actionable episodes;
- valid-loss episode fraction at most 10%;
- active actionable episodes acquiring a new tie-abstention at most 10%;
- invalid-shard coverage remains at least 2.

Rows are retained for audit, but an episode is the primary unit because both
directions can come from one nomination/witness document pair. A passing
receipt qualifies context routing only. It does not promote lexical authority
and does not open LA2-B by itself; the synthetic credit-routing gate must also
remain sealed and the later integration run must be authorized separately.
