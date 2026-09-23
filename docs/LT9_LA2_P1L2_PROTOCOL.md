# LT9-LA2-P1L2: tie-resolved natural-tail anatomy

## Question

Determine whether the 31 invalid `TIE_RESOLVED` episodes found by P1L1 have a
repeatable structural signature that is absent or uncommon among valid
tie-resolved episodes from the same candidate stream. The existing
`unique_endpoint_agreement` resolver is held fixed.

This is a discovery-only analysis. It does not change routing, learning,
authority, ranking, retrieval, or serving.

## Frozen inputs

- HotpotQA `corpus.jsonl` with SHA-256
  `3e776d2343352f83341878202b8c49cc1ebe6e2ad4c2a77a21c116cafa229334`.
- P1L1 receipt and manifest under `D:\phoenix-evals\lt9-p1l1-hotpotqa`.
- P1L1 source SHA-256
  `feda3bfa5e18ac120b158c746348f226949b5426cc64addf9f29af4c7535b6d3`.
- Episode formation, marker sets, expected phenotype map, and route logic are
  replayed exactly from P1L1.

The executable fails closed if the corpus hash, P1L1 source hash, or P1L1
receipt hash does not match the frozen manifest, or if the reconstructed
invalid tie-resolved key set differs from P1L1.

## Control selection

Controls are valid `TIE_RESOLVED` episodes from the same candidate stream.
Invalid episodes are processed in nomination-document order. For each one,
select the unused valid episode with the same candidate and the nearest
nomination document index; break equal-distance ties by episode key. A control
is used at most once. This deterministic matching reduces broad corpus-region
differences without matching on the context features under investigation.

The receipt also preserves the full valid tie-resolved control pool so the
matching result and its coverage can be audited. Polarity, marker counts,
priority inversion, mixed-marker status, side masks, endpoint margins, and
opportunity delay are measured outcomes, not matching criteria.

## Recorded evidence

For each invalid episode and selected control, retain:

- candidate, route, route path, expected phenotype, and witness polarity;
- nomination and witness marker family counts, masks, before/after side masks,
  and plurality margins;
- unique versus tied endpoint structure and agreement shape;
- mixed-marker and fixed-priority inversion flags;
- marker-mask and side-mask Hamming distances;
- same-field status and candidate-opportunity delay.

No document text is copied into the receipt. Summaries are descriptive counts;
there is no learned classifier, threshold selection, or post-hoc routing rule.

## Decision boundary

P1L2 can identify a candidate structural condition for a later, separately
qualified experiment. It cannot qualify a resolver change. Feature overlap
between invalid episodes and their matched valid controls is an explicit
possible result. Any repair requires fresh unopened qualification evidence.
