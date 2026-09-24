# LT9-LA2 P1O1: Relation-Variability Census

Date: 2026-09-23
Status: **label-blind intake and context-graph discovery only**

## Question

Do candidate lexical relations differ in how often natural occurrence contexts are compatible, incompatible, or unresolved? This is an anatomy study. It does not train an observer, create authority, update memory, evaluate retrieval, or qualify serving behavior.

The study tests the relation-level distinction:

```text
context-stable / context-sensitive / unresolved
```

It does not call a candidate context-stable merely because no incompatible context was sampled.

## Candidate frame

Candidate proposals are generated from Princeton WordNet 3.0 synsets. The intake uses only pairs of distinct, single-token alphabetic lemmas occurring in one synset. It does not read glosses, example sentences, sense-frequency metadata, prior Phoenix labels, or old authority outcomes. WordNet supplies a reproducible proposal frame, not P1O1 compatibility labels.

Before corpus text is scanned, candidate-pair identities receive a deterministic SHA-256 priority from the frozen P1O1 salt. Previously studied candidate lemmas and a fixed stop list are excluded. The resulting candidate order is independent of natural-context labels.

## Discovery text and freshness boundary

The text-only source is the frozen P1N3 12-corpus discovery roster, with its corpus order and source hashes unchanged, plus the locally available FiQA corpus appended as a new LT9 discovery source. Webis-Touche2020, NQ, HotpotQA, and all other reserved qualification material are excluded. The intake reads only corpus document title/text fields; it does not read queries, qrels, prior phenotype labels, or authority outcomes. FiQA query/qrel files and all retrieval outputs remain closed.

To make the occurrence evidence fresh relative to human review, document hashes appearing in the P1N3 and P1N4 private packet ledgers are excluded before candidate occurrences are retained. The P1N3/P1N4 label files and feature fields are not inputs. FiQA contributes new LT9 context text, while all other sources contribute only document-level contexts not shown in the P1N3/P1N4 human packets. P1O1 remains discovery evidence, not external-corpus qualification.

## Label-blind occurrence and context graph

An occurrence node is one exact single-token appearance of one candidate lemma in one source document/field. At most one node per candidate lemma and document is retained. Its display excerpt contains up to 12 ASCII-alphanumeric tokens on either side of the focal occurrence. Sampling and context-diversity features exclude both candidate lemmas.

For each eligible candidate relation, freeze six occurrence nodes: three for each lemma, each from a distinct source document. Each lemma must be represented in at least two corpus IDs, and the six nodes must span at least three corpus IDs. Candidate cohort selection uses only occurrence availability and label-blind local-context diversity.

The reviewer receives the complete graph over the six nodes (all 15 pairwise edges). This deliberately permits shared nodes and triangles. Pair labels remain non-transitive observations; no union-find, clustering, or closure is allowed.

For intake eligibility, the six selected contexts must have at least four distinct local structural signatures, at least 24 unique non-candidate local token identities in aggregate, and an interquartile range of at least 0.10 among the 15 exact-token Jaccard similarities. The lower and upper quartiles supply low- and high-overlap graph edges. These are candidate-intake criteria only; no human label or predicted sense is involved.

The first eight eligible candidate pairs in the frozen hash order form the review cohort. If fewer than eight qualify from the fixed WordNet proposal pool, keep the smaller cohort and report the shortfall; do not relax criteria or inspect outcomes to select replacements.

## Blind review

The reviewer receives only opaque packets, the short rubric, and a judgment template. Packets show the lexical pair and two natural excerpts with focal terms, but hide corpus/document identity, candidate source, overlap, structural signatures, and all sampling metadata. Reviewer edits only `judgment` to `SAME`, `DIFFERENT`, or `UNKNOWN`.

`SAME` means the two occurrences express compatible contextual uses of the displayed candidate relation. `DIFFERENT` means the contexts clearly express incompatible uses relevant to that relation. `UNKNOWN` means local evidence is insufficient or ambiguous. Shared words alone do not establish `SAME`.

The experiment author may review the packets, but that must be recorded as author review, not independent review.

## Frozen descriptive classification

Each candidate is classified only after judgments are validated, using the following descriptive categories:

- `MIXED`: at least four `SAME` and four `DIFFERENT` edges; each label spans at least three distinct occurrence nodes and at least two corpus IDs.
- `SAME_DOMINANT_DIFFERENT_NOT_OBSERVED`: at least 10 `SAME`, fewer than four `DIFFERENT`, and at least 12 determinate edges. This is not a claim of global stability.
- `DIFFERENT_DOMINANT_SAME_NOT_OBSERVED`: symmetric descriptive category.
- `UNDERPOWERED`: fewer than 12 determinate edges, or no category's frozen evidence floor is met.

Report `UNKNOWN` counts and all labeled triangles. Edge counts are not independent observations; candidate relation is the census unit, with the full context graph shown as descriptive anatomy.

## Freeze and stop boundary

Before packets are reviewed, seal the WordNet source hash, candidate generator, corpus roster and hashes, prior-review document-hash exclusions, selected candidate cohort, occurrence nodes, graph edges, packets, rubric, and empty judgment template. Review labels are validated and sealed before joining private graph metadata.

P1O1 does not fit P1N3/P1N4 probes or any new observer. Only candidates descriptively classified `MIXED` can motivate a separately preregistered P1O2 compatibility-observability study. P1O1 itself does not qualify compatibility-gated witness ownership and does not unblock LA2-B.
