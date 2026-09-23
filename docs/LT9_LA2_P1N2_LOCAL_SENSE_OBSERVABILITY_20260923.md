# LT9-LA2 P1N2: candidate-conditioned local sense observability

Date: 2026-09-23
Status: **controlled diagnostic only; no router or authority promotion**

## Question

Can candidate-conditioned local lexical and spatial evidence distinguish a controlled local frame while ignoring candidate-token identity and remote topic perturbations? Separately, can a pairwise observer classify two contexts as same, different, or unknown without first assigning named global phenotypes?

This is an observability assay, not a claim of natural-language sense understanding. The stimuli are deterministic controlled contexts with frame labels defined by construction. No human sense labels, qrels, queries, corpus validity outcomes, or retrieval outputs are used.

## Frozen stimulus design

Three fixed candidate pairs are used as model selectors only: `bank/water`, `car/vehicle`, and `insurance/coverage`. Both candidate tokens appear in every variant of their pair. Their occurrences, identities, and any n-grams crossing them are forbidden from the local probe feature vector. Candidate identity may select a separate per-pair observer; it is not an input feature and does not define the target label.

Each candidate has two controlled local-frame classes mapped to existing family IDs. Twelve base groups per candidate are generated from a fixed, balanced local-cue inventory. Each group has eight variants:

1. Frame A, base local cues, one neutral ambient-topic marker.
2. Frame A, same local cues, three distinct markers from the opposing ambient topic.
3. Frame A, same local cues, five distinct markers from the opposing ambient topic.
4. Frame B local-cue swap, neutral ambient topic.
5. Frame B local cues, three opposing-topic markers.
6. Frame B local cues, five opposing-topic markers.
7. Local-cue removal, labelled unknown.
8. Conflicting A and B local cues plus a contradiction cue, labelled unknown.

The pair positions, local cue inventories, marker inventories, placement slots, field boundary schedule, and generation order are source-frozen. Topic interventions change only remote/far-side family markers. Local frame interventions change the near-pair cue set while keeping the candidate tokens and base ambient topic fixed. Cue-removal and conflict conditions are explicitly unknown; the probe is not rewarded for forcing a class.

Groups 0–7 per candidate are the fitting partition. Groups 8–11 are the held-out partition. All variants and pairwise comparisons from one base group remain on one side of this boundary. No thresholds, feature subsets, tree depth, or split are selected from held-out results.

## Frozen views

1. **Current family route:** distinct-marker family plurality over the endpoint window, including candidate-token votes; ties abstain.
2. **Context-only family route:** the same rule after excluding the two candidate positions and all exact occurrences of those candidate words.
3. **Candidate-conditioned local probe:** a separate depth-3 categorical decision tree per candidate pair, trained only on the fixed local feature inventory and groups 0–7. Features include noncandidate token identities, before/between/after roles, near/far position and distance to each endpoint, immediate neighbors, deterministic within-field bigrams/trigrams, support/contradiction cues, and field-boundary relations. Candidate tokens, relation ID, corpus/document identity, and ambient topic label are not row features.
4. **Pairwise compatibility probe:** a depth-3 categorical tree per pair, trained on context-pair features from groups 0–7. Its labels are SAME for two known equal frames, DIFFERENT for two known unequal frames, and UNKNOWN if either context is unknown. It receives only context overlap, role-specific overlap, local n-gram overlap, cue-presence, and field compatibility features. Pairwise lexical overlap is restricted to noncandidate tokens within three positions of either endpoint. No candidate token or corpus/document identifier is exposed.

The tree learner uses deterministic Gini reduction, categorical branches, minimum child size 2, and maximum depth 3. Ties in leaf class counts fail closed to UNKNOWN when UNKNOWN is tied; otherwise the lowest class ID wins. The procedure is fixed before held-out scoring.

## Primary held-out measurements

- **Topic invariance:** same predicted class for each base-vs-topic-swap and base-vs-amplification comparison, reported separately for Frame A and Frame B.
- **Sense/frame sensitivity:** distinct correct predictions for the A-base and B-base contexts in each held-out group.
- **Ambiguity calibration:** UNKNOWN prediction rate on cue-removal and conflicting-cue variants, with condition-level confusion.
- **Held-out classification:** accuracy and per-frame recall, macro-averaged first across base groups, then candidates.
- **Pairwise compatibility:** group-macro accuracy and class confusion for SAME / DIFFERENT / UNKNOWN.
- **Family controls:** route consistency and correctness for the current and context-only family views, on the same held-out stimuli.

All counts are reported by candidate and condition. The base group is the resampling/statistical unit; variant and Cartesian pair counts are not treated as independent observations.

## Interpretation boundary

Strong local-probe results show only that this controlled lexical feature construction makes the stipulated local frames observable. They do not qualify a natural phenotype router, establish human semantic correctness, or authorize lexical authority. Weak results falsify only this frozen surface-feature assay; they do not prove that syntax or richer semantics are necessary. Pairwise compatibility outperforming classification supports the simpler diagnostic formulation but is not an integration policy.

P1N2 does not read P1M2R validity receipts, P1N1 outcome receipts, qrels, queries, Webis-Touche2020, or any reserved qualification labels. It does not run natural learning, update authority, serve replacements, or evaluate retrieval.

## Decision

P1N2 is diagnostic only. Any successor observer, compatibility rule, natural corpus qualification, or memory integration requires a separately frozen protocol and unopened evidence. No result in this assay changes the current router or unblocks LA2-B.
