# LT9-LA2-P1L2 discovery result

## Result

P1L2 found a sharp structural split in the frozen HotpotQA discovery tail.
All 31 invalid tie-resolved episodes route through an endpoint-local finance
plurality that conflicts with the fixed geography assignment; none of the 31
distinct, same-candidate valid controls has that priority inversion. This
explains why P1K4 passed its qualification sample: its 18 tie decisions
contained no priority-inversion endpoint.

This is discovery evidence from one corpus and the P1L1 expected-phenotype
labels. It does not qualify or authorize a resolver change.

## Frozen replay and control construction

- HotpotQA: 5,233,329 documents; corpus SHA-256
  `3e776d2343352f83341878202b8c49cc1ebe6e2ad4c2a77a21c116cafa229334`.
- Reconstructed 2,779 episodes and matched all 31 invalid
  `TIE_RESOLVED` episode keys against P1L1.
- Invalid episodes: 28 `bank_to_water`, 3 `bank_to_shore`.
- Valid tie-resolved pool: 146 episodes.
- Controls: 31 unique valid episodes, selected from the same candidate by
  nearest nomination document position without replacement.
- The matched controls include 2 keys in the P1K4 sample; none of the 31
  invalid keys appears in P1K4.

The three `bank_to_shore` controls are much farther away in corpus order than
the `bank_to_water` controls (median nomination-position gap 220,383 versus
29,449 documents). Treat candidate-specific comparisons as descriptive.
The 31 control episode keys are distinct, but the episode streams share
endpoints: 17 document IDs occur in both the invalid and control sets, and
adjacent invalid episodes share an endpoint in 8 `bank_to_water` links and 1
`bank_to_shore` link. The 31 pairs therefore are not 31 independent
observations; the counts below are descriptive, with no significance claim.

## What separates the groups

Every invalid episode and every matched valid control is mixed-marker and
same-field. The tie-resolved endpoint orientation also overlaps: 14 of 31
matched pairs have the same unique-endpoint side. Witness polarity and
opportunity delay overlap substantially. These properties do not explain the
failure by themselves.

The decisive route evidence is:

| Evidence | Invalid episodes | Matched valid controls |
| --- | ---: | ---: |
| Unique endpoint selects finance | 31/31 | 0/31 |
| Unique endpoint selects geography | 0/31 | 31/31 |
| Endpoint priority inversion | 31/31 | 0/31 |
| Tie endpoint count vector `[finance, geography, transport] = [1,1,0]` | 30/31 | 31/31 |
| Mixed-marker endpoints | 31/31 | 31/31 |
| Same field at both endpoints | 31/31 | 31/31 |

For the invalid set, the unique endpoint's family counts were
`[2,1,0]` in 25 cases, `[3,1,0]` in 4, and `[4,2,0]` in 2. The valid
controls mostly show the mirror pattern: `[1,2,0]` in 25 cases and
`[1,3,0]` in 4. The resolver follows whichever family wins at the unique
endpoint when the other endpoint's tie contains it.

The frozen marker lists make this tail especially revealing: `bank` belongs
to the finance family, while `water` and `shore` belong to geography. Their
candidate pair therefore supplies a finance/geography base tie in the local
marker counts. A post-hoc comparison of the already-recorded family counts
and marker masks found that 20/31 invalid finance advantages increase the
finance count without adding a new distinct finance marker bit at the unique
endpoint. That pattern occurs in 2/31 valid geography controls. This suggests
that repeated evidence for an already-present marker identity can tip the
endpoint plurality; the receipt does not identify which individual word was
repeated, so no more specific claim is made.

Marker-mask Hamming distance is 0 in 20/31 invalid episodes and 2/31 controls.
Side-mask distances overlap (median 1 in both groups), as do corpus-opportunity
delays (median 14,232 versus 12,596) and polarity (21 support, 9 abstain, 1
contradiction versus 19 support, 12 abstain). Those are weaker descriptions
than the finance/geography plurality inversion.

## P1K4 coverage gap

P1K4 sampled 18 tie decisions: 9 routed to geography, 4 to transport, and 5
abstained. None had a unique finance endpoint while geography markers were
also present. Its HotpotQA examples covered geography agreement and abstention
cases, but not the natural-tail direction in which repeated finance-family
counts win against a geography-bearing tie. P1L1 already established route
parity on the shared P1K4 keys; P1L2 identifies the missing structural class.

## Decision boundary

P1L2 supports this discovery hypothesis:

> In the `bank -> water/shore` mixed-marker tail, endpoint agreement can
> promote finance plurality created without a new distinct finance marker,
> even though the fixed assignment is geography.

This is a specific failure signature, not evidence that a generic
priority-inversion guard will generalize. No router, eligibility, authority,
retrieval, or serving behavior changed. Any candidate repair needs a separate
protocol and unopened qualification episodes before integration is retried.
