# LT9-LA2 P1N4: Targeted Natural Compatibility Acquisition

Date: 2026-09-23
Status: **blind acquisition only; no labels, model fitting, authority, or retrieval**

## Purpose

P1N3 is frozen. P1N4 acquires a fresh set of natural occurrence-context pairs aimed at underrepresented diagnostic cells: high lexical overlap with structural divergence, short/low-evidence ambiguity, and low-overlap variation. It is an acquisition study, not a new model result.

The candidate relations remain `bank → water`, `car → vehicle`, and `insurance → coverage`. P1N4 reuses the frozen 12-corpus text roster but excludes every document and every exact candidate-masked local-context template used in P1N3. This is fresh occurrence-level evidence within the same corpus cohort, not external-corpus qualification.

## Frozen sampling strata

For each candidate and corpus, retain at most 96 deterministic hash-reservoir occurrence contexts after excluding P1N3 documents. Candidate-pair contexts must come from different documents in the same corpus. No source document may appear in two P1N4 packets.

Each corpus/candidate pair pool is partitioned by deterministic rank into overlap quartiles using exact Jaccard over non-candidate local token identities. The five mutually exclusive strata, applied in this precedence order, are:

1. `ambiguity_low_evidence`: both endpoints are at or below the within-corpus/candidate lower quartile for non-candidate local-token count and excerpt word count, with at most one exact shared local token and no shared bigram or trigram.
2. `high_overlap_structural_divergence`: pair lies in the top overlap quartile and top structural-divergence quartile within that high-overlap pool. Structural divergence is the sum of absolute before/between/after token-count differences, local-token-count difference, distance-bin difference, support-cue mismatch, contradiction-cue mismatch, and field-kind mismatch.
3. `high_overlap_control`: remaining pairs in the top overlap quartile.
4. `low_overlap`: pairs in the bottom overlap quartile not already assigned to ambiguity.
5. `ordinary_middle`: all remaining pairs.

The fixed target is eight packets per candidate per stratum (40 per candidate, 120 total), with a maximum of two selected packets per corpus/candidate/stratum. All selection is deterministic and label-blind. Quota shortfalls are reported without relaxing rules, substituting another stratum, or searching outcomes.

The strata enrich for likely difficult examples; they do not assign or imply human labels. Human `SAME`, `DIFFERENT`, and `UNKNOWN` judgments remain the only outcome. No `UNKNOWN` quota is imposed.

## Reviewer firewall

The reviewer receives only `blind-review/packets.json`, `blind-review/rubric.md`, and `blind-review/judgments-template.json`. The packets disclose the lexical pair and two natural excerpts, but not corpus/document identity, overlap, stratum, features, model output, previous labels, authority state, or retrieval result. To make review and return easier, the judgments template contains only the opaque packet ID and a blank judgment field for each item; the reviewer fills that small file and leaves `packets.json` unchanged.

P1N3 packets, judgments, and source documents are not modified or re-reviewed by this acquisition. P1N3 document and masked-template hashes are consulted only to exclude reused evidence; P1N3 labels, outcomes, and evaluator outputs are not read by the sampler.

## Post-review sufficiency and analysis boundary

After all packets are labeled, validate IDs and immutable content before inspecting features. Report `SAME`, `DIFFERENT`, and `UNKNOWN` counts by candidate and sampling stratum. A candidate is eligible for a candidate-specific compatibility analysis only if it has at least 12 `SAME` and 12 `DIFFERENT` labels overall and at least four `DIFFERENT` labels in the high-overlap structural-divergence stratum. This is a predeclared minimum, not a claim of statistical power; candidates below it remain underpowered. `UNKNOWN` is reported as observed and is not reinterpreted.

The primary safety outcome is false-`SAME` on human `DIFFERENT`, especially in the high-overlap structural-divergence stratum. Hard-positive, low-overlap `SAME` and ambiguity-stratum `UNKNOWN` are separate diagnostics. Any later fitting must use a separately frozen plan and grouped splits; this acquisition alone does not authorize fitting or memory integration.

Pairwise compatibility remains non-transitive. Do not use union-find, connected components, or semantic closure. Grouping exact masked templates, if used later for leakage control, has no semantic role.

## Scope exclusions

No qrels, query data, expected phenotype labels, P1N3 outcome artifacts, ranking output, retrieval quality, lexical authority, serving behavior, or credit-router changes are used or permitted. P1N4 cannot unblock LA2-B by itself.
