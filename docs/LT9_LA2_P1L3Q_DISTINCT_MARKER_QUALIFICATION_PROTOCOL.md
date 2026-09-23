# LT9-LA2-P1L3Q Distinct-Marker Router Qualification

Date: 2026-09-23
Branch: `codex/phoenix-native-p1l3q-distinct-marker-gate-20260923`

## Frozen policy and purpose

P1L3Q tests the exact discovery counterfactual from P1L3 on the first corpus
that meets P1L3's label-blind risk-state coverage floor: DBPedia-Entity.

The only policy change is:

```text
family vote = number of distinct active marker identities in that family
```

The `unique_endpoint_agreement` tie resolver, candidate bank, tokenization,
nearest-pair selection, context window, episode formation, credit router,
decay, semantic expiry, and pending capacity remain frozen. No thresholds,
weights, marker edits, lexicon exceptions, retrieval, or serving changes are
permitted.

The policy is tested as a qualification candidate, not promoted. P1L3's
HotpotQA discovery results remain sealed and are not used for qualification.

## Frozen input and label boundary

- Corpus is the DBPedia-Entity `corpus.jsonl` whose SHA-256 is recorded in the
  P1L3 corpus-screen receipt and artifact manifest.
- The preflight receipt must confirm the label-blind floor: at least 8
  risk-signature episodes across at least 2 candidate relations.
- The preflight screen read corpus text only. It did not read qrels, queries,
  or validity outcomes.
- Qualification uses the already frozen candidate bank's expected phenotype
  as post-hoc contextual-routing validity. It does not load BEIR qrels or
  retrieval labels.
- Candidates whose expected phenotype is unspecified are excluded from the
  valid/invalid gate denominators but remain in route and memory replays.

## Gates

Retain all P1K4 routing sufficiency and episode-primary safety thresholds:

- current/raw occurrence routing has at least 20 actionable valid episodes;
- at least 5 actionable invalid episodes, spanning at least 2 of 8 equal-size
  document-index shards;
- zero invalid episodes after distinct-marker routing;
- valid-loss episode fraction at most 10%;
- active actionable episodes acquiring a new tie abstention at most 10%;
- at least 2 shards represented among raw invalid actionable episodes.

Add the LA2-B memory-level safety condition:

- the exact full-stream distinct-marker replay creates **zero invalid
  authority compartments**.

Also require the frozen credit integrity invariants to remain satisfied:

- zero polarity ownership errors;
- zero pending-capacity violations and peak pending at or below the frozen
  capacity;
- owned-witness update count equals positive plus negative authority updates;
- deterministic replay.

All conditions must pass. A failure is a qualification failure; it does not
authorize threshold changes, marker changes, repair work, or another policy.

## Unit of analysis

An adjacent candidate-event pair is one episode. Episodes may share a document
because this is the frozen LA2-B stream contract. Row totals remain descriptive;
episode counts are primary for the loss and new-abstention fractions. Shard
assignment is `floor(nomination_document * 8 / document_count)`, clamped to
shards 0 through 7.

## Stopping boundary

P1L3Q is a context-routing qualification only. It does not authorize natural
learning, authority promotion, LA2-B, retrieval, or serving. If all gates pass,
the next integration requires a separately frozen protocol. If any gate fails,
keep the policy nonqualified and do not revise it using this corpus.
