# LT9-LA2 P1M1 multi-member corpus intake addendum

Date: 2026-09-23

This addendum resolves the official archive layout before any P1M1 structural screen outcome is read. It does not change the frozen corpus order, structural floors, episode definitions, router, or decision gates in `LT9_LA2_P1M1_EXCLUSIVE_PLURALITY_PROTOCOL.md`.

## CQADupStack archive layout

The official CQADupStack archive contains twelve forum-level `corpus.jsonl` members. P1M1 treats the archive as the preassigned corpus unit and constructs one deterministic input in ascending ordinal archive-member path order:

1. `cqadupstack/android/corpus.jsonl`
2. `cqadupstack/english/corpus.jsonl`
3. `cqadupstack/gaming/corpus.jsonl`
4. `cqadupstack/gis/corpus.jsonl`
5. `cqadupstack/mathematica/corpus.jsonl`
6. `cqadupstack/physics/corpus.jsonl`
7. `cqadupstack/programmers/corpus.jsonl`
8. `cqadupstack/stats/corpus.jsonl`
9. `cqadupstack/tex/corpus.jsonl`
10. `cqadupstack/unix/corpus.jsonl`
11. `cqadupstack/webmasters/corpus.jsonl`
12. `cqadupstack/wordpress/corpus.jsonl`

Only these corpus members are extracted/read. Their UTF-8 JSONL bytes are concatenated in the order above, with one LF separator after each member; blank lines are ignored by the frozen loader. The result is screened as one corpus with global document-index shards, matching the frozen dataset-level role assignment. No qrels, queries, or relevance labels are accessed.

## Other frozen inputs

Climate-FEVER contains one `climate-fever/corpus.jsonl` member and is passed directly to the screen. Natural Questions remains the next fallback under the already frozen corpus order.

## Boundary

This handling rule was fixed after archive member names were inspected but before the P1M1 label-blind structural screen was run. It uses archive layout only and does not inspect candidate validity, expected phenotype, support/contradiction outcome, or retrieval relevance.
