# LT9-LA2-P1L3 Marker Identity Attribution — Discovery Receipt

Date: 2026-09-23

Branch: `codex/phoenix-native-p1l3-marker-attribution-20260923`

## Result

P1L3 supports marker multiplicity as a substantial contributor to the HotpotQA
tail, but it does not explain or repair the entire routing failure. Distinct
marker voting removes many invalid routes, yet one contaminated authority
compartment remains. A single finance marker (`bank`) accounts for nearly all
repeated finance surplus on inversion endpoints, leaving a lexicon-specific
marker defect as a live alternative.

The run is discovery-only. No candidate policy was promoted, no external labels
were opened, no natural-learning state was changed, and retrieval remained dark.

## Baseline parity and counterfactual

The frozen HotpotQA corpus hash matched. The P1L3 replay reproduced the LA2-B
source hash and full stream counts exactly:

| Measure | Frozen baseline | Distinct-marker vote |
| --- | ---: | ---: |
| Events | 2,790 | 2,790 |
| Episodes | 2,779 | 2,779 |
| Routed episodes | 2,659 | 2,652 |
| Abstained episodes | 120 | 127 |
| Tie-resolved episodes | 177 | 166 |
| Invalid routed episodes | 36 | 11 |
| Invalid authority compartments | 2 | 1 |

Of 36 baseline invalid episodes, distinct-marker voting routes one to its
expected family, abstains on 24, and still routes 11 incorrectly. Of 2,623
baseline-valid episodes, it preserves 2,614 and loses 9 to abstention; none is
changed to a wrong family. There are 33 new abstentions overall, 26 episodes
that were previously abstained but become routed, and 60 total route-decision
changes.

The invalid compartment `bank_to_shore@finance` is prevented. The other,
`bank_to_water@finance`, persists; no new invalid compartment is introduced.
This fails the future zero-invalid-authority safety requirement and therefore
does not qualify distinct-marker voting for use.

## Marker attribution

There are 20 episodes matching the frozen structural signature: raw
tie-resolved route, inversion at the unique endpoint, and a repeated-identity
count gain despite identical winner-family identity masks at both endpoints.
They are concentrated in `bank_to_water` (17) and `bank_to_shore` (3).

Across raw priority-inversion endpoints, `bank` contributes 25 of the 27
finance winning repeated-occurrence surplus, or 92.6%. `loan` and `insurance`
contribute one each. This concentration is a strong warning against treating
the result as a universal counting-semantics defect. The 11 remaining invalid
episodes after deduplication are the second falsifier: identity multiplicity
is contributory, not sufficient.

## Label-blind corpus preflight

The frozen suitability floor was at least 8 risk-signature episodes across at
least 2 candidate relations. The six remaining local corpora were screened in
the predeclared order using only `corpus.jsonl` text; none met the floor except
Webis-Touche2020's 2 episodes across 1 relation, which remains underpowered.

After preregistering fresh-corpus order as DBPedia-Entity then CQADupStack,
DBPedia-Entity was the first eligible screen result, with 9 risk-signature
episodes across 2 relations (`bank_to_shore`: 4, `bank_to_water`: 5). The
preflight read no qrels, queries, or validity labels. The official BEIR archive
checksum matched; only `dbpedia-entity/corpus.jsonl` was extracted for this
screen. The archived non-corpus members were not opened.

DBPedia-Entity meets only the label-blind state-space coverage floor. That does
not qualify a router. It is the preregistered first corpus for an independent
qualification attempt with the policy frozen before any outcome review.
CQADupStack was not acquired or screened because the earlier candidate met the
floor.

## Verification and hashes

- `cargo test --release`: 2/2 passed.
- Release binary built to the D: target and tested through the C: junction.
- HotpotQA replay receipt matched across two runs:
  `4b62564a5a77db6d134bc09c6b835107c52b5607c636a903647f9874a62e3df7`.
- Seven-corpus label-blind screen receipt matched across two runs:
  `ec267453a7870411803301d0823e2e71003b4087475b43cc3155c5bc6549b41b`.
- DBPedia-Entity corpus JSONL SHA-256:
  `abd0a993f69b0abc2a4a5367695bceb081f0c0efd22d65b9986d4ac6ff8baf95`.

The release uses a standalone Cargo harness because the checked-out workspace
still names the absent `crates/phoenix-tts-native` member. That remains an
environment/workspace limitation, not an experimental failure.
