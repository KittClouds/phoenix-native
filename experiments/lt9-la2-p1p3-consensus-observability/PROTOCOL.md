# LT9-LA2 P1P3: Prospective consensus observability

**Frozen:** 2026-09-24  
**Status:** acquisition protocol; no P1P3 labels or feature joins yet  
**Question:** can a fixed shallow observer predict only the natural contextual-compatibility judgments that three independent reviewers reproduce?

## Target and review firewall

The unit is a pair of natural occurrence contexts for one displayed candidate lexical relation. Reviewers see only the candidate pair, the two focal-marked excerpts, and a three-label rubric. They do not see document/corpus IDs, fit/holdout assignment, overlap stratum, marker/features, model predictions, expected relation classes, or other reviewers' labels. Candidate words are visible to clarify the relation being judged; they are not eligible as model context features.

Three genuinely independent human reviewers label the same frozen packets independently. The experiment author does not count as an independent reviewer. Luna/model reviews may be retained only as a separate proxy diagnostic and cannot substitute for a missing human review.

Individual labels are `SAME`, `DIFFERENT`, or `UNKNOWN`. Review each edge independently; do not impose transitivity or consistency across a triangle. The prospective target is:

```text
all three SAME       -> consensus SAME
all three DIFFERENT  -> consensus DIFFERENT
every other vote vector -> consensus UNKNOWN
```

This includes unanimous `UNKNOWN` and every disagreement pattern. Preserve those as separate subtypes in the label receipt: unanimous human-unknown versus reviewer disagreement. Do not majority-vote disputed edges into a semantic target. Consensus `UNKNOWN` is distinct from an observer's own abstention for insufficient machine evidence.

## Label-blind acquisition

The candidate proposal pool uses the existing deterministic WordNet-3.0 pair generator (which caps its proposal list at 8,192 pairs), then reorders proposals with a P1P3-specific salt. Exclude every lemma found in the frozen P1O1 exclusion list or in prior P1N3, P1N4, P1O1, and P1O2 public packet candidate pairs. The cohort is the first four eligible, lexically disjoint candidate pairs in that order. A pair is selected only by text-only event availability and the same frozen structural-diversity requirements used for natural context graphs; no labels, qrels, retrieval outcomes, old expected-family labels, or model predictions participate.

For each candidate, acquire two six-occurrence context graphs with 15 pair edges each. The graphs are document-disjoint, and the 12 occurrence documents are also unique across selected candidates. Each graph must span at least three corpora, have at least four local structural signatures and 24 noncandidate context tokens, and pass the frozen context-overlap dispersion requirement (`Q3 - Q1 >= 0.10` over the 15 pairwise token Jaccards). Candidate terms and all prior reviewed document hashes are excluded from the new candidate/context pool.

The fixed target is four candidates × two graphs × 15 edges = 120 physical context pairs. Each reviewer receives a separately salted opaque ID and independently shuffled order for the same 120 pairs. If fewer than four candidates meet the label-blind acquisition contract, the acquisition is marked shortfall; do not lower criteria after seeing which pairs were selected. Fit/holdout role is assigned to the two document-disjoint graphs by a deterministic candidate-specific bit before labels are reviewed.

Within each graph, noncandidate context-token Jaccard ranks the 15 edges. Use the existing P1O2 quartile rule: sorted positions 3 and 11 are Q1/Q3; values `<=Q1` are `low`, `>=Q3` are `high`, and the rest are `middle`. This private sampling stratum is not shown to reviewers. The acquisition stores no model feature vectors.

## Validation and sufficiency gates

1. Validate every review independently: exact packet-ID set, one allowed label per packet, no duplicates or omissions. Seal each original review before comparing reviews.
2. Compute consensus labels from the frozen unanimity rule. Before joining private sampling metadata, require at least 12 consensus edges in each class overall, at least 4 consensus edges per class in each aggregate fit and holdout graph, and at least three candidate relations with 4 consensus `SAME` and 4 consensus `DIFFERENT` edges each. A candidate below either per-relation floor remains `UNDERPOWERED`; pooled counts do not rescue it.
3. Only if gate 2 passes, join the minimal private sampling metadata and check the hard cells: at least 8 consensus `SAME` edges in `low` overlap and at least 8 consensus `DIFFERENT` edges in `high` overlap, with at least 2 of each in holdout. Failure stops before feature extraction or fitting.
4. Only after both gates pass may feature representations be materialized and joined.

All packet, class, and stratum counts are reported by candidate and fit/holdout graph. Candidate relations are not pooled to hide an underpowered candidate.

## Frozen observability comparison

Use one frozen depth-3 CART probe for every representation. Candidate ID selects no output label and is not a direct feature; fit and holdout contain different occurrence nodes and documents. Compare:

1. P1O2 surface/local baseline features.
2. Ordered local context structure, preserving endpoint-relative position and n-grams.
3. Syntactic/relational context features, using a frozen parser/configuration if available; if no deterministic parser is available, mark this arm `NOT_RUN` rather than silently replacing it with another surface view.

No threshold search, feature expansion after holdout, authority update, lexical transport, ranking, or retrieval is part of P1P3. The same probe, depth, split, target, and evaluation code apply to all runnable representations.

## Primary measures

The operational measure is `ALLOW precision`:

```text
P(consensus SAME | predicted SAME)
```

Report false-SAME separately for consensus `DIFFERENT` and consensus `UNKNOWN`, plus SAME recall, DIFFERENT recall, UNKNOWN recall, UNKNOWN→SAME, UNKNOWN→DIFFERENT, and class support. Report the two hard cells separately: low-overlap consensus SAME and high-overlap consensus DIFFERENT. Do not present pooled accuracy without candidate-level denominators.

For triangles, use only edges with unanimous human labels. Report every observed binary pattern (`SSS`, `DDD`, and all permutations of `SDD` and `SSD`) by candidate and split; do not perform closure, clustering, or union-find. `SSD`/`SDS`/`DSS` (two SAME, one DIFFERENT) are the equivalence-transitivity-violating patterns if SAME is interpreted as an equivalence relation, but their presence or absence does not force that architecture.

## Stopping boundary

If the label sufficiency gate fails, P1P3 is `UNDERPOWERED` and no feature joins or fits run. If only proxy reviews are available, report a proxy diagnostic and do not call it the primary P1P3 result. A positive P1P3 result still requires fresh external observer qualification followed by a memory-only witness-authorization experiment before any LA2-B, retrieval, or serving work.
