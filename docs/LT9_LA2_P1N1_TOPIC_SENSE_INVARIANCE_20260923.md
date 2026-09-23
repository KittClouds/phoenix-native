# LT9-LA2 P1N1: topic–sense observability assay

Date: 2026-09-23

Branch: `codex/phoenix-native-p1n1-topic-sense-20260923`

Status: frozen before P1N1 variants or candidate-stream heterogeneity results
are generated.

## Question and scope

P1M2R is closed as outcome-underpowered. Its twelve-corpus outcome receipt is
not an input to this assay. P1N1 asks whether the existing broad-family marker
representation measures ambient topic when the lexical relation's sense is
held fixed, and whether it distinguishes relation sense when ambient marker
evidence is held fixed.

The primary paired tests are:

```text
topic markers change, lexical relation fixed  -> phenotype should stay fixed
lexical relation sense changes, ambient cues fixed -> phenotype should change
```

This is a controlled observability assay, not a natural-corpus qualification.
It does not change or select a router, train a learner, open Webis-Touche2020,
or run retrieval, ranking, serving, queries, or qrels. P1M2R's result,
receipts, and outcome-underpowered disposition remain immutable.

## Frozen inputs and outcome separation

Use only the P1M2R normalized corpus rows identified by the label-blind screen
receipt SHA-256
`bc32d310b757e8919b45de9c058ebce59892de15da2c5f11527ee800d26b18a0` and the
normalization receipt SHA-256
`b8f87432ea9aa12edc545f7f4a07b3b9389628f76eeceae690aed1da559ad80b`. Verify
each normalized corpus hash against that frozen screen before loading its
natural event features. Preserve the frozen twelve-corpus order. The screen
and corpus text are reused only as natural feature seeds; P1N1 must not read
the P1M2R outcome replay, its expected-context results, BEIR qrels/queries, or
LoTTE QAS data. P1M2R outcomes have already been opened, so P1N1 measurements
from these texts are **not** prospective and must not select or qualify a
future corpus.

The source phenotype implementation is frozen at the P1M2R router contract:
distinct-marker plurality, unique-winner routing, and abstention on ties.
There is no learned weight, threshold, fallback, or router mutation. The
protocol's semantic classes are declared by lexical relation identity, not
derived from the route being tested. There are nine fixed-sense relations; `bank_to_water` is a separate ambient-template relation:

| Relation identities | Declared fixed sense |
| --- | --- |
| `car_to_vehicle`, `vehicle_to_car`, `engine_to_motor` | transport |
| `insurance_to_coverage`, `credit_to_loan`, `loan_to_debt`, `stock_to_bond` | finance/insurance |
| `bank_to_shore` | geography |
| `bank_to_lender` | finance/insurance |

These are assay definitions, not corpus judgments. Use all natural endpoint
events for the nine fixed-sense relations, without filtering by route,
actionability, credit outcome, or P1M2R validity. Use natural
`bank_to_water` endpoint events only as fixed ambient-context templates for
the paired synthetic `bank_to_shore` / `bank_to_lender` sense contrast. If a
predeclared relation has no natural seed, report it as absent; do not replace
it after inspecting route results.

## Causal interventions

For each fixed-sense natural endpoint event, retain an unmodified baseline and
create the following deterministic feature-level variants. The base natural
feature vector is the unit; variants change only the named marker evidence.

1. **Candidate-token inclusion:** compare the existing all-marker view with a
   context-only view that subtracts exactly one source-token and one
   target-token occurrence when those tokens are members of a marker family.
   Preserve any additional occurrences of the same words.
2. **Ambient-family perturbation:** starting from the natural baseline and
   from its context-only view, add one or three distinct marker identities
   from each family other than the relation's declared sense family. Exclude
   the candidate source and target strings from injected identities. Select
   eligible identities in the frozen family-catalogue order, skipping any
   identity already present in that family's mask; record the ordered
   identities selected for each variant.
3. **Placement:** apply the same injected identities separately to the
   existing `before`, `between`, and `after` feature buckets. Also report
   each bucket-only route as descriptive evidence. No bucket is privileged
   or selected as a new router.
4. **Multiplicity:** add the same eligible foreign marker identity with
   multiplicity 1, 2, and 4 in the `before` bucket. Compare current distinct-marker routing with a
   raw-occurrence route reported only as a diagnostic.
5. **Nearby/distant:** represent a nearby distractor by the local feature
   addition above. Represent a distant, out-of-window distractor as absent
   from the feature vector, matching the current eight-token local extractor.
   The outside-window case is an explicit no-op control, not a new distance
   feature.

For each variant report the baseline and variant phenotype, exact route
stability, abstention transitions, route correctness against the frozen
relation-identity class, and whether candidate-token removal changed the
assignment. Do not tune variant counts or marker identities.

## Paired relation-sense contrast

For every natural `bank_to_water` endpoint template, subtract only the
template's exact `bank` and `water` candidate-token votes to obtain one fixed
ambient marker vector. Clone that same ambient vector into two candidate
variants: `bank_to_shore` (geography) and `bank_to_lender`
(finance/insurance). Add each variant's exact candidate-token votes for the
all-marker view; keep them absent for the context-only view.

Report the paired routes for both views. The predeclared sense-discrimination
receipt is whether the two candidate identities produce distinct routes and
whether each route matches its declared class. This paired counterfactual is
synthetic around a natural ambient context; it does not claim a natural
human-judged sense label.

## Label-blind candidate-stream heterogeneity census

Alongside the assay, compute these descriptive metrics from the same frozen
corpus text, without reading any expected-context field or P1M2R outcome:

- candidate event-stream length;
- endpoint unique-route counts by family and endpoint abstention count;
- number of distinct routed phenotype families and Shannon entropy over
  routed endpoint-family frequencies;
- consecutive endpoint phenotype-switch count/rate, using only adjacent
  event pairs where both events have a unique route;
- `A→B→A` triple count, using only adjacent uniquely routed triples;
- minority-phenotype endpoint count (all routed events outside the modal
  family); and
- hard-abstain unique-pair route counts by candidate relation.

This census is a method/calibration receipt over an already outcome-exposed
cohort. It is not a corpus-selection screen, qualification gate, or evidence
for a relation-sense policy. Future prospective screens may adopt these
definitions in a separately frozen protocol.

## Interpretation and stopping rule

Report exact counts and rates for all predeclared cells, including absent
seeds and abstentions. Interpret topic sensitivity as descriptive evidence
that family-marker identity can alter routing under a fixed relation.
Interpret failure to separate the paired bank senses as evidence that the
current representation does not expose that distinction under fixed ambient
features. Neither result authorizes a repair, feature gate, threshold, corpus
qualification, or serving change. Any candidate-conditioned representation
is a separate future branch requiring its own protocol and unopened
qualification evidence.

The final receipt must bind this protocol, the frozen P1M2R screen and
normalization receipts, every normalized corpus hash, executable/source
hashes, all intervention definitions and counts, and a deterministic rerun
hash. It must state:

```text
expected_context_outcomes_read=false
p1m2r_outcome_receipt_read=false
qrels_or_queries_opened=false
reserved_qualification_labels_opened=false
router_selected=false
retrieval_or_ranking_run=false
prospective_corpus_selection=false
```
