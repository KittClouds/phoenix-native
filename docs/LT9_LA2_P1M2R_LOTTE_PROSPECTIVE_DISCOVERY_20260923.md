# LT9-LA2 P1M2R: LoTTE cohort expansion

Date: 2026-09-23

Branch: `codex/phoenix-native-p1m2-fresh-cohort-20260923`

Status: protocol frozen before LoTTE event screening; no LoTTE query or outcome data may be opened in this phase.

## Purpose

P1M2's seven-corpus discovery cohort did not meet its preregistered structural gate. This successor adds a new public text source while preserving the old floor and keeping the old Webis-Touche2020 qualification reserve untouched. P1M2 remains closed as an underpowered preflight; this protocol does not revise its result.

The question remains diagnostic: does removing source/target self-votes expose marker-identity, cross-endpoint, or spatial structure associated with the risky `UNIQUE_PLURALITY` route? No routing or learning rule is changed.

## Frozen cohort

The discovery set D is fixed before its new screen and is processed in this exact order:

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

The seven original corpora retain their frozen P1M2 discovery identities and are included in full. LoTTE's overlapping `pooled` collections are excluded. For each named LoTTE topic, only `dev/collection.tsv` followed by `test/collection.tsv` is read. Duplicate passage IDs are retained only at their first occurrence, preserving source order. The fixed archive source is <https://downloads.cs.stanford.edu/nlp/data/colbert/colbertv2/lotte.tar.gz>; its expected HTTP content length observed at protocol freeze is 3,576,167,599 bytes and its ETag is `61f9a72f-d527fcaf`. The actual archive SHA-256, extracted member hashes, normalized corpus hashes, byte lengths, and row counts must be recorded before event screening.

Do not extract or parse LoTTE question files, answer files, relevance judgments, or metadata. The archive digest is an opaque integrity check; archive contents outside the ten named collection files are not inspected. Normalization writes only `{docid,title,text}` JSONL records, with empty title and the collection text unchanged.

Webis-Touche2020 remains the reserved qualification corpus. Its validity outcomes stay unopened and it is not used to rescue discovery preflight. NQ is excluded because a prior receipt already records evaluated routing pairs. All other outcome-exposed LT9 corpora remain excluded. No discovery corpus can be dropped after counts are observed.

## Frozen mechanics and gate

Use the P1M2 tokenizer, candidate list, nearest-pair/event construction, distinct-marker hard-abstention route, shard rule, and marker/self-vote diagnostics unchanged. Credit replay resets at each corpus boundary. Report directional episodes and physical document-pair episodes separately. The label-blind screen may read only the twelve frozen corpus text inputs and their integrity metadata.

The unchanged structural gate must pass across the full twelve-corpus D before any expected-context outcome is opened:

- at least 240 baseline `UNIQUE_PLURALITY` contested directional episodes;
- at least 3 discovery corpora with at least 15 contested episodes each;
- at least 4 candidate relations with contested episodes; and
- at least 8 nonempty `(corpus, document shard)` cells.

If this gate fails, do not open validity outcomes, alter the floor, or inspect Webis qualification labels. Record a failed-closed result and require a new dated protocol for any further cohort.

## Outcome boundary

Only after the full cohort and integrity manifest are sealed and the structural gate passes may a separately frozen discovery replay open the candidate expected-context labels. That later replay is anatomy-only. It must retain the prospective outcome sufficiency gate from P1M2, report independent physical episodes and directional rows, and never select a router threshold. No BEIR queries/qrels, retrieval, ranking, serving, lexical authority promotion, or credit-router changes are permitted.

## Reproducibility and controls

The source archive URL, observed remote metadata, source SHA-256, exact member paths and hashes, normalized JSONL hashes, normalizer binary hash, screen binary hash, receipt hash, and deterministic rerun identity are part of the input receipt. The same corpus order, normalizer, and screen executable must reproduce identical corpus and screen receipts. P1M2's failed screen receipt remains immutable.
