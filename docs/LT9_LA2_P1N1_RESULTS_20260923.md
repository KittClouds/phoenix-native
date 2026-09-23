# LT9-LA2 P1N1: topic–sense observability results

Date: 2026-09-23

Status: **diagnostic result; no router qualified**

P1N1 shows that the frozen broad-family marker route is strongly affected by both candidate-token votes and ambient-family markers. The current phenotype representation has not demonstrated that it tracks lexical-relation sense independently of candidate identity and surrounding topic cues.

## Frozen execution and scope

The protocol and executable were frozen before the completed runs. Twelve P1M2R text corpora were reused in their frozen order for a label-blind feature assay. The earlier P1M2R outcome receipt was not read. The cohort is already outcome-exposed, so these measurements are not prospective and cannot qualify a router or select a future corpus.

The runner processed 5,485 candidate endpoint events, including 3,876 endpoints from the nine declared fixed-sense relations and 138 bank_to_water ambient templates. Relation-family correctness below means agreement with the predeclared class attached to the candidate identity; it is not human sense annotation.

Two independent executions produced identical 5,289,742-byte outputs (SHA-256 e2282d7d41c87a7523cdae1a637735675beb4816ee6b74dd1156b5036983eb5f). The compressed receipt round-trips to that hash. All eight unit tests passed. The release binary hash is ecb98ad59a6aa1f56ccca2694ee34aadfa61f47289903d57509233bd08615f70.

Scope flags stayed false: no expected-context outcomes, P1M2R outcome receipt, qrels, queries, or reserved qualification labels were read; no router was selected; no natural authority was updated; no retrieval or ranking ran.

## Candidate-token self-vote

Removing exactly one source-token vote and one target-token vote from each endpoint changed the assigned route for **2,972 / 3,876 (76.7%)** fixed-sense endpoints. It moved **2,872 (74.1%)** to abstention. The route matched the predeclared relation-family map for 3,864 endpoints with candidate votes present, versus 900 after those votes were removed.

That contrast is dominated by the representation design: many candidate words are themselves members of the family marker lists. The high all-marker agreement is therefore not independent evidence that ambient context identifies relation sense. For example, candidate-token removal alone caused 1,063 abstentions in each direction of car ↔ vehicle and 145 of 167 insurance → coverage endpoints.

The bucket-only probe reinforces this: the between bucket, which includes the candidate endpoints, matched the all-marker family route on 3,867 of 3,876 endpoints. The before and after buckets routed only 447 and 387 endpoints, respectively, and abstained on most cases.

## Ambient-marker perturbation

Each intervention cell begins from the same natural endpoint vector and adds unused, noncandidate marker identities from a foreign family. N below counts repeated intervention cells across foreign families and placements, not independent documents.

| View | Injected distinct identities | Cells | Route unchanged | Baseline matches declared family | Variant matches declared family | Abstain entered |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| All markers | 1 | 23,256 | 22,896 (98.5%) | 23,184 | 22,860 | 324 |
| All markers | 3 | 23,256 | 300 (1.3%) | 23,184 | 300 | 3,240 |
| Candidate tokens removed | 1 | 23,256 | 1,131 (4.9%) | 5,400 | 723 | 4,953 |
| Candidate tokens removed | 3 | 23,256 | 300 (1.3%) | 5,400 | 0 | 90 |

Three distinct markers from a foreign family generally take over the route, including when the original lexical relation is held fixed. With candidate tokens removed, a foreign marker set commonly turns an unknown endpoint into a confident but nonmatching family route. The exact identities selected for each intervention condition are included in the receipt.

For the same marker identities, all 72 matched before / between / after condition sets had identical route transitions. This is consistent with the current router using the aggregate distinct-marker mask and ignoring those location buckets. The out-of-window no-op control was stable in all 15,504 cells.

Distinct-marker voting was invariant to adding the same marker identity at multiplicity 1, 2, or 4. The raw-occurrence diagnostic was not: its stable-route count fell from 8,138 at multiplicity 1 to 2,021 at 2 and 186 at 4 across the corresponding 15,504 intervention cells.

## Paired bank sense contrast

The same 138 natural ambient vectors were reused for bank → shore and bank → lender.

With candidate votes removed, both candidate variants always received the same route: finance in 13 templates, geography in 36, and abstention in 89. With candidate votes included, the two routes differed in 130 templates, but both matched their predeclared relation families in only **37 / 138 (26.8%)**. The candidate-inclusive route matched the declared family for bank → shore in 40 templates and bank → lender in 135.

Thus the route distinction mostly appears when the candidate tokens themselves vote. This synthetic pairing does not establish that natural context exposes the relation sense.

## Candidate-stream heterogeneity census

Across the reused text cohort, the frozen streams contained 5,485 endpoint events: 3,969 uniquely routed and 1,516 abstained. There were 6 phenotype switches across 3,812 adjacent uniquely routed pairs (0.16%), and no A → B → A patterns across 3,729 uniquely routed triples. This cohort did not exercise a strongly heterogeneous candidate-stream regime; the census is descriptive, not a future corpus-selection gate.

## Interpretation

P1N1 supports the category-error concern in this controlled assay: the current broad marker vote is sensitive to ambient marker identity and is heavily driven by candidate-token self-votes. It does not establish a qualified natural-language sense router. The paired bank contrast is particularly clear about the limitation: candidate identity often changes the route, while holding the ambient feature vector fixed does not.

Plurality, tie handling, and credit routing remain unchanged. P1N1 authorizes no natural learner or serving change. The next research question is whether a candidate-conditioned local relation-sense representation is observable from evidence other than the candidate tokens themselves; any such representation needs a separate frozen assay and unopened qualification evidence.

## Artifacts

- Protocol: docs/LT9_LA2_P1N1_TOPIC_SENSE_INVARIANCE_20260923.md
- Pre-run manifest: experiments/lt9-la2-p1n1/pre-run-manifest-20260923.json
- Compressed result: experiments/lt9-la2-p1n1/p1n1-result-20260923.json.gz
- Artifact seal: experiments/lt9-la2-p1n1/artifact-seal-20260923.json