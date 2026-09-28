# P1P3E2 lexical transport context review

For each packet, compare the query-side use of the source form with the document-side use of its counterpart. Decide whether transporting the source query term to retrieve the document-side counterpart would preserve the relevant lexical meaning in these local contexts.

- `SAME`: the two uses are compatible for this directed lexical relation.
- `DIFFERENT`: the contexts clearly use the terms in incompatible senses or roles.
- `UNKNOWN`: the snippets are too short, ambiguous, or conflicting to decide.

Judge only the displayed local contexts. The candidate forms are replaced by `[SOURCE]` and `[TARGET]` inside the snippets; the relation being evaluated is shown separately. Do not infer an answer from the corpus, query intent, document relevance, rank, or any external metadata. `UNKNOWN` is a valid result when local evidence is insufficient.

Edit only the `judgment` field in `judgments-template.json`. Keep packet IDs and order unchanged; use exactly `SAME`, `DIFFERENT`, or `UNKNOWN`.

This packet set is an engineering review aid. Qrels relevance identifies a retrieval opportunity candidate but does not certify lexical compatibility. It is not formal P1P3 qualification.
