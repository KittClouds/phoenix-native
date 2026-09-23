# LT9-LA2 P1N3: Natural Pairwise Context Compatibility

Date: 2026-09-23
Status: **blinded natural-context acquisition; diagnostic only**

## Question

Can candidate-conditioned local context evidence distinguish natural pairs of occurrence contexts that humans judge `SAME`, `DIFFERENT`, or `UNKNOWN`, including low-overlap SAME and high-overlap DIFFERENT cases?

P1N3 tests pairwise context compatibility. It does not predict named finance/geography/transport phenotypes, update lexical authority, serve replacements, or evaluate retrieval.

## Frozen candidate and corpus cohort

The candidate relation selectors are the three pairs used in P1N2: `bank → water`, `car → vehicle`, and `insurance → coverage`. Candidate identity may select a separate diagnostic observer but is never an input feature. The candidate words are visible to the independent reviewer because they are needed to interpret each context; their occurrences and all n-grams containing them are excluded from model features.

The natural text pool is the complete, fixed 12-corpus P1N1 label-blind corpus roster, in its frozen order: CQADupStack, ArguAna, NFCorpus, Quora, SciDocs, TREC-COVID, SciFact, LoTTE Writing, LoTTE Recreation, LoTTE Science, LoTTE Technology, and LoTTE Lifestyle. P1N3 uses only each corpus JSONL's document text and title. It does not read P1N1/P1M2R outcome receipts, expected-family labels, qrels, queries, authority state, or retrieval output. The inherited corpus roster is a discovery source, not a fresh qualification set.

## Natural occurrence extraction and sampling

An occurrence is the nearest same-field appearance of both words in a candidate pair, at most 24 ASCII-alphanumeric tokens apart. At most one occurrence per candidate and document is retained. The context shown to the reviewer is a natural source-field excerpt extending up to 12 tokens on either side of the candidate pair. No corpus ID, document ID, rank, score, family label, sampling stratum, or provenance is included in the review packet.

For each candidate and corpus, occurrence contexts are capped with a deterministic hash-reservoir of 64 documents. All cross-document pairs within that candidate/corpus pool are scored by exact-token Jaccard over local non-candidate tokens. The lowest and highest within-corpus quartiles define low- and high-overlap sampling pools. The frozen target is 16 low-overlap and 16 high-overlap context pairs per candidate, sampled deterministically with no document/context reused anywhere in the set and at most four selected pairs from any one corpus per candidate/overlap stratum. If the full target cannot be met, the shortfall is reported; no outcome-informed substitution or corpus search is allowed after labels are viewed.

The overlap strata are label-blind. Their eventual human labels populate the four natural cells:

| Non-candidate lexical overlap | Human label | Diagnostic cell |
|---|---|---|
| high | SAME | easy positive |
| low | SAME | hard positive |
| low | DIFFERENT | easy negative |
| high | DIFFERENT | hard negative |

The sampling process does not claim in advance which label any selected pair will receive. `UNKNOWN` is a valid independent judgment, not a failed label.

## Human review contract

The reviewer receives only the opaque packet file and a short rubric. Labels are assigned from the two natural contexts and displayed candidate word pair, without corpus/document identity, old phenotype labels, family markers, overlap strata, model outputs, scores, ranks, qrels, or expected outcomes.

- `SAME`: both contexts express a compatible contextual use of the displayed lexical relation.
- `DIFFERENT`: the contexts clearly express incompatible uses/senses relevant to that relation.
- `UNKNOWN`: the local text does not establish compatibility or incompatibility, or is genuinely ambiguous.

The reviewer changes only the `judgment` field. They do not infer a label from mere shared words, and leave packet IDs and context text unchanged.

## Feature firewall and views

After the human labels are frozen, a deterministic, candidate-conditioned diagnostic may compare:

1. `P_full_local`: exact non-candidate lexical identity overlap; endpoint-relative before/between/after token roles; immediate-neighbor and distance bins; exact within-field bigram/trigram overlap; support/contradiction cues; and field compatibility.
2. `P_no_exact_overlap`: the same structural, role, position, cue, and field evidence with exact shared token identities, role-tagged shared identities, and exact shared n-gram overlap removed.
3. The prior named-family views as diagnostic controls only; they do not determine labels and remain off the critical path.

Candidate words themselves, candidate ID as a row feature, corpus/document IDs, old phenotype labels, qrels, queries, authority outcomes, ranks, scores, and retrieval results are prohibited from all compatibility feature vectors. Candidate identity can only choose a per-relation observer outside its feature vector.

## Split and statistical unit

The packet sampler uses unique documents and unique context instances globally. Before labels are opened, it assigns whole packet/context-template connected groups to a fixed fit/holdout partition. Exact normalized masked-context templates are grouped within each candidate-specific observer, because each relation uses a separate probe. No context instance, source document, or exact masked local template may cross that observer's boundary. Templates shared across different candidate observers do not cross model boundaries. If these constraints materially reduce the target count, the reduced count is reported instead of relaxing the split.

Human judgments are the outcome labels. Candidate relation and context-template groups, not individual Cartesian feature rows, are the generalization units. Report per-candidate and macro results; do not let multiple pairs from one context inflate the effective sample size.

## Primary measurements

- SAME recall and DIFFERENT recall.
- UNKNOWN rate and confusion/calibration against human UNKNOWN.
- False-SAME rate: `P(predicted SAME | human DIFFERENT)`.
- Hard-positive accuracy: low overlap + human SAME.
- Hard-negative accuracy: high overlap + human DIFFERENT.
- `P_full_local` versus `P_no_exact_overlap`, on the frozen holdout only.
- Named-family controls, clearly separated from pairwise compatibility results.

The diagnostic learner, if run, is a frozen shallow per-candidate probe trained only on the fit partition. No threshold or feature selection may use holdout judgments. If a candidate/class is too sparse to fit or score meaningfully, report it as underpowered.

Pairwise `SAME` judgments are not assumed transitive. Do not convert pairwise results to connected components, union-find compartments, or transitive closure. After labels are available, report observed labeled triangles where all three pairwise comparisons exist, including `A SAME B`, `B SAME C`, `A DIFFERENT C`. Such patterns remain pairwise observations and are not repaired by the probe. The implementation's template grouping is only an evaluation-split leakage control and has no role in semantic compatibility or architecture.

## Interpretation and stopping boundary

P1N3 can establish natural human-label observability for this frozen text/candidate sample only. It cannot qualify an authority router, alter natural learning, promote a phenotype representation, or unblock LA2-B. A positive result requires a later separately preregistered qualification on unopened contexts/corpora. A weak result falsifies this frozen surface-context assay only; it does not prove richer syntax or semantics are required.

The sequence is:

`blind human judgments → validation and authority-free seal → frozen fit/holdout probe → observability verdict → fresh external qualification → only then memory integration`.

No lexical authority, retrieval, or serving changes are permitted in P1N3.
