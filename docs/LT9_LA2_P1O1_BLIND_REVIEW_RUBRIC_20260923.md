# P1O1 context-graph review

For each packet, read the candidate word pair and both excerpts. Assign exactly one label:

- `SAME`: the contexts show compatible contextual uses of the displayed lexical relation.
- `DIFFERENT`: the contexts clearly show incompatible contextual uses relevant to that relation.
- `UNKNOWN`: the excerpts do not provide enough evidence, or the relationship between these uses is ambiguous.

Judge only the excerpts shown. Shared words do not by themselves establish `SAME`. Do not assume the relation is globally stable or that compatibility must be transitive. Review each pair independently, even when packets share an excerpt. Modify only the `judgment` value; preserve packet IDs, word pair, and context text exactly. Use the JSON strings `"SAME"`, `"DIFFERENT"`, or `"UNKNOWN"`.
