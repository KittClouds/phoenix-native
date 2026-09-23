# LT9-LA2 P1N3 acquisition receipt

Date: 2026-09-23
Branch: `codex/phoenix-native-p1n3-natural-pairwise-20260923`
Status: **blinded packet acquisition complete; independent judgments pending**

The P1N3 sampler scanned the frozen 12-corpus text-only roster and verified each corpus SHA-256 before emitting review material. It did not open qrels, queries, earlier outcome receipts, expected-family labels, or retrieval outputs. The corpus pool is a discovery source and is not a qualification set.

The fixed target was met: **96 opaque packets**, with 16 low-overlap and 16 high-overlap pairs for each of `bank → water`, `car → vehicle`, and `insurance → coverage`. The packets contain contexts from 192 unique source documents. The frozen grouped split contains **67 fit packets and 29 holdout packets**; exact masked-template groups stay together within each candidate-specific observer, and source documents and context instances are globally disjoint. No candidate-word feature leak crosses the firewall; the sampler assigned zero judgments.

The label-blind roster census is preserved in the acquisition receipt. Occurrence counts varied sharply by candidate and corpus, but every candidate met both overlap-band quotas without extending the cohort or selecting by semantic outcomes. The sample therefore contains deliberately selected low/high non-candidate overlap strata; it does not claim that either stratum is SAME or DIFFERENT. Human labels will reveal which natural hard-positive and hard-negative cells were actually populated.

Two independent release replays produced byte-identical packets, rubric, private ledger, pre-review receipt, and pre-review root. Five unit tests pass. The release binary was built on the target drive and copied to the C: test path with matching SHA-256. No compatibility probe was fit or scored because the human labels have not arrived.

## Sealed artifacts

The review-only files are stored outside the repository at:

- `D:\phoenix-evals\lt9-la2-p1n3-20260923\blind-review\packets.json`
- `D:\phoenix-evals\lt9-la2-p1n3-20260923\blind-review\rubric.md`

The provenance ledger stays private at `D:\phoenix-evals\lt9-la2-p1n3-20260923\private-ledger.json`; it must not be provided to the reviewer. The committed root receipt binds the packet hash `2aef17f0c88794311c2fc713167bd41e38ab8ca9307fa46df0e42dcfe84b77aa`, rubric hash `bf676fb2f56df2f7bbedbfeb52f5f6223ba9446ef4e58d2a9314316263659d10`, and private-ledger hash `f4a2022b8eddcbc39bc95eedffaffe3c8c42779ef20c4c098f13f518802601a0`.

Only the two review files belong in the blind review. The reviewer changes only `judgment` to `SAME`, `DIFFERENT`, or `UNKNOWN`. No hypothesis, overlap stratum, corpus/document identity, feature, source receipt, or model result accompanies them.

## Interpretation boundary

P1N3 has produced no semantic result yet. It has produced a sealed natural-context sample. Pairwise SAME judgments are not assumed transitive: after judgments arrive, labeled triangles may be reported as observed, but no union-find, connected-component, or transitive-closure operation will convert them into architecture. No lexical authority or serving changes are permitted.
