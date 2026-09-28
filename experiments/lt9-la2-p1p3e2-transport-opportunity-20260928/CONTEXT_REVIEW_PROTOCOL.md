# P1P3E2 context-review packet contract

**Frozen:** 2026-09-28, before any packet judgments

The packet set contains all 47 retained qrels counterpart candidates, including top-10 controls, top-100 misses, rank-11–100 rows, and rows whose query exceeds the frozen QPS group bound. Reviewers do not see that status or any qrels, rank, dataset, document, query, split, or candidate-inventory metadata. Packet order is a stable hash order rather than a relation grouping.

Each packet presents one directed lexical relation and the local source-side query context paired with a target-side document context. The source and target forms are masked inside both excerpts and replaced with `[SOURCE]` and `[TARGET]`; the pair is shown separately so the reviewer knows which relation is under review. Context windows include up to 12 word tokens on each side of an occurrence. Up to three occurrences are retained on each side.

Use exactly these judgments:

- `SAME`: the two uses are compatible for this directed lexical relation and would support transporting the query-side source term to the document-side counterpart.
- `DIFFERENT`: the contexts clearly use incompatible senses or roles.
- `UNKNOWN`: the local excerpts are insufficient, ambiguous, or conflicting.

Edit only `judgment` in `judgments-template.json`. Keep packet IDs and order unchanged. The packet is an engineering review instrument: qrels relevance does not imply `SAME`, and absence from qrels is not a `DIFFERENT` label. Record reviewer provenance separately when returning judgments. One reviewer or a model-proxy pass is sufficient to advance this engineering diagnostic; it must be named accurately and must not be represented as independent-human consensus or formal P1P3 qualification.

This packet set contains naturally occurring qrels counterpart contexts, not an intentionally balanced class sample. Its labels are useful for discovery and support accounting only. A separate acquisition pass is required for hard-negative and UNKNOWN coverage before fitting a selective gate.
