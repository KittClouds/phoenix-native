# LT9-LA2 P1O1: Relation-Variability Census Results

Date: 2026-09-23
Status: **discovery-only descriptive census; author-reviewed**

## Frozen result

The 120 human labels were validated against the sealed packet set before private sampling metadata was joined. The reviewer was the experiment author, so these labels are human judgments but not independent judgments. The canonical judgment file and validation receipt are bound in the external acquisition artifacts; packet labels were not modified in place.

| Candidate relation | SAME | DIFFERENT | UNKNOWN | Frozen category |
|---|---:|---:|---:|---|
| `concentrate ↔ reduce` | 2 | 13 | 0 | DIFFERENT_DOMINANT / SAME_NOT_OBSERVED |
| `allotment ↔ apportioning` | 10 | 5 | 0 | MIXED |
| `save ↔ spare` | 4 | 11 | 0 | MIXED |
| `aspect ↔ facet` | 3 | 12 | 0 | DIFFERENT_DOMINANT / SAME_NOT_OBSERVED |
| `leaning ↔ tilt` | 4 | 6 | 5 | MIXED |
| `consecutive ↔ successive` | 15 | 0 | 0 | SAME_DOMINANT / DIFFERENT_NOT_OBSERVED |
| `occlusive ↔ stop` | 4 | 11 | 0 | MIXED |
| `publication ↔ publishing` | 4 | 11 | 0 | MIXED |

Across all candidates the totals are 46 SAME, 69 DIFFERENT, and 5 UNKNOWN. There were 160 within-graph triangles (20 per relation). No sampled triangle had exactly two SAME edges and one DIFFERENT edge. This is only a property of these small selected graphs; it is not evidence that compatibility is transitive.

## Interpretation and boundary

The five MIXED relations satisfy the preregistered descriptive criterion and trigger the conditional P1O2 observability experiment. The other three relations are not called stable: their unsampled or unobserved contexts remain unknown. Candidate relations were selected for P1O1 using a frozen label-blind proposal process, but P1O2's five-relation cohort is selected conditionally on P1O1's author-reviewed labels. P1O2 therefore estimates neither population prevalence nor independent confirmation of the P1O1 classifications.

P1O1 trained no observer, updated no authority, and ran no retrieval. Pairwise labels are not transitive by assumption; no clustering or closure is permitted. LA2-B remains blocked.

## Sealed input identity

- Review mode: `AUTHOR_REVIEW`
- Judgment SHA-256: `3410156a41e93302146619c91d10852d8f36dbf1e1be7b37d2e5d3ba40288dbb`
- Validation receipt SHA-256: `4b92867430d5e418978d0bb67c4f8ae2baa0744303cd1dd2f89b51954d636065`
- Pre-review root SHA-256: `66879622b9fdf516460fe6c7d94c8bd831c6dd288c3485e74ab7e4df21631ff5`
- Joined descriptive analysis SHA-256: `9998916c72cd0a0e594dbea57fbdbd8c118618198a2586c3de1cde0f6ebc3ba2`

The private packet ledger and raw judgment file stay outside the repository. The committed summary contains only aggregate counts and the already disclosed candidate list.
