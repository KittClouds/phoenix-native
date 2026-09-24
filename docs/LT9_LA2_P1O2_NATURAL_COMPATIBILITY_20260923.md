# LT9-LA2 P1O2: Natural Pairwise Compatibility Observability

Date: 2026-09-23  
Status: **prospective blind natural-context acquisition; diagnostic only**

## Question and scope

Can frozen local surface-context evidence distinguish human-judged compatibility between natural occurrence contexts for lexical relations that met P1O1's frozen `MIXED` criterion? P1O2 tests observability only. It does not qualify a natural-memory router, update authority, alter serving, evaluate retrieval, or unblock LA2-B.

Pairwise labels remain pairwise observations. No equivalence-class assumption, transitive closure, union-find, or clustering is allowed.

## Conditional candidate cohort

The five P1O1 `MIXED` candidates are frozen in `experiments/lt9-la2-p1o2/candidate-cohort.json`: `allotment ↔ apportioning`, `save ↔ spare`, `leaning ↔ tilt`, `occlusive ↔ stop`, and `publication ↔ publishing`. This cohort was selected conditionally from the author-reviewed P1O1 result. P1O2 therefore is not an independent estimate of relation-level variability or population prevalence. The candidate list is fixed before P1O2 occurrence scanning; P1O2 labels/features do not select or replace candidates.

## Corpus and freshness boundary

Use the exact 13-corpus, text-only P1O1 roster and verify every source hash. This is additional document-level discovery evidence, not an unopened-corpus qualification. Before retaining contexts, exclude document content hashes in the P1N3, P1N4, and P1O1 private packet ledgers. The scan reads only title/text corpus fields and those specific document-hash fields needed for exclusion. It does not read queries, qrels, retrieval output, model scores, authority outcomes, or prior human labels. All other reserved qualification corpora remain unopened.

An occurrence node is one deterministic exact single-token appearance of one cohort lemma in one source document/field. Retain at most one occurrence per lemma/document. Display up to 12 ASCII-alphanumeric tokens on either side, marking the focal token. Candidate-token occurrences are excluded from all compatibility features; the candidate pair selects the observer only.

## Frozen sample geometry

For each of the five candidates, attempt to acquire 12 occurrence contexts: six for each lemma, each from a distinct document, with no document repeated between the two lemmas. Partition them into two disjoint six-node graphs, each containing three contexts per lemma. Every graph must span at least three corpora, have at least four noncandidate local structural signatures, at least 24 unique noncandidate local token identities, and token-Jaccard interquartile range of at least 0.10 across its 15 edges. All 15 unordered edges in each graph become review packets, for a maximum of 30 packets per candidate and 150 total.

Sampling order, graph assignment, and packet IDs are deterministic and frozen before labels. For each graph, use the inherited P1O1 selector's hash-ranked distinct-document triplets and its exact frozen graph-diversity calculations. The first eligible graph is fit; remove all six of its source-document hashes from that candidate's occurrence pools, then select the first eligible graph for holdout. Context instances, source documents, and graphs are disjoint across this boundary. If a candidate cannot meet the frozen structural requirements in the fixed corpus roster, mark it `UNDERPOWERED`; do not replace it, relax requirements, or search another corpus. Because graph edges share endpoint contexts, per-edge counts are descriptive; the candidate relation and disjoint graph are the grouping units.

The first pre-review implementation attempt produced zero packets because it selected only the first hash-ranked occurrences before checking graph diversity. It exposed no packet to a reviewer and no label was assigned. That empty attempt is preserved as an implementation-shortfall receipt; it is not P1O2 evidence. The final pre-review root binds the corrected deterministic triplet-search implementation above. No criterion, cohort, source roster, or label-dependent choice changed.

For each graph, exact noncandidate token Jaccard ranks its 15 edges. Edges at or below the frozen lower-quartile value are `low`; edges at or above the upper-quartile value are `high`; remaining edges are `middle`. Quartile ties remain in the corresponding band. These are label-blind overlap strata, not predicted hard-positive/negative labels.

## Blind human review

The reviewer receives only `packets.json`, `rubric.md`, and `judgments-template.json`. The packet shows the candidate pair and two natural excerpts with a focal occurrence; it omits source corpus/document IDs, split, overlap, feature vectors, sampling strata, previous labels, ranks, scores, and authority state. The reviewer edits only `judgment` and leaves IDs and text unchanged.

- `SAME`: the two occurrences express compatible contextual uses of the displayed candidate relation.
- `DIFFERENT`: the contexts clearly express incompatible uses relevant to that relation.
- `UNKNOWN`: local evidence is insufficient or genuinely ambiguous.

Shared words alone do not establish `SAME`. The experiment author may review, but the receipt must say `AUTHOR_REVIEW`, not independent review.

## Pre-feature sufficiency gate

Validate packet IDs, exact row count, labels, and unchanged visible fields before joining private features. Then report counts by candidate and fit/holdout graph. A candidate may enter its descriptive model analysis only if each graph has at least eight determinate labels and at least three `SAME` and three `DIFFERENT` judgments. Otherwise that candidate is `UNDERPOWERED`; pooled counts do not rescue it. Report `UNKNOWN` counts independently. No feature joins or model fits occur before this gate is sealed.

## Frozen diagnostic feature views and probe

For each endpoint context, exclude every occurrence of either candidate lemma. Extract noncandidate lexical identities, before/after roles relative to the focal position, near/far bins, within-field bigrams/trigrams that do not cross a candidate token, support/contradiction cue flags, field kind, and structural counts.

1. `P_full_local`: shared exact token identities, role-tagged identities, bigram/trigram identities, their frozen Jaccard summaries, and structural/cue fields.
2. `P_no_exact_overlap`: only structural/cue fields; remove shared identity features and all token/ngram Jaccard summaries.

Candidate identity selects a separate observer but is never a feature. Corpus/document IDs, candidate-token presence, P1O1 labels, old phenotype families, qrels, queries, ranking output, and authority outcomes are prohibited.

After the label and sufficiency seals, fit one deterministic per-candidate three-class CART for each view on that candidate's fit graph only: Gini, max depth 3, minimum 3 fit rows per child, unweighted classes, thresholds enumerated only from fit values, lexicographic feature-name then lower-threshold tie break, and leaf ties predict `UNKNOWN`. No holdout choice, tuning, or feature selection. If a candidate's fit data has fewer than two classes or no legal split, report the frozen fit-majority/constant diagnostic and mark its inferential result underpowered rather than substituting another method.

## Primary report

Report per candidate and macro summaries, never only pooled edge metrics:

- holdout confusion counts and observed-class balanced accuracy;
- `SAME`, `DIFFERENT`, and `UNKNOWN` recall;
- false-SAME rate, `P(predicted SAME | human DIFFERENT)`;
- `UNKNOWN → SAME` and `UNKNOWN → DIFFERENT` counts;
- low-overlap SAME recall and high-overlap DIFFERENT recall;
- `P_full_local` versus `P_no_exact_overlap`;
- human label counts and sufficiency status by candidate and graph;
- all observed fully labeled triangles, with no transitivity enforcement.

The high-overlap `DIFFERENT` cell is the principal safety diagnostic; low-overlap `SAME` tests beyond-overlap generalization. A positive result remains discovery-only and does not qualify compatibility-gated witness ownership.

## Freeze and stop boundary

The pre-review root binds the protocol, sources, corpus roster, candidate cohort, exclusion hashes, packets, rubric, template, private feature ledger, and executable. The only reviewer-editable field is `judgment`. After review, seal/validate labels and sufficiency before joining features. P1O2 does not run natural learning, authority updates, retrieval, or serving. Fresh external qualification is required before any memory integration; LA2-B remains blocked.
