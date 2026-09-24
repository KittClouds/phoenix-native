# LT9-LA2 P1P2 label-side reliability

This diagnostic preserves the three sealed P1O2 judgments per packet and
reports reviewer agreement, consensus-derived disagreement, vote entropy,
fit/holdout and overlap-stratum summaries, and triangles whose three edges are
unanimously labeled.

It performs no feature analysis, model fit, authority update, or retrieval run.
The private-ledger join is restricted to packet/candidate IDs, split, overlap
band, and occurrence node IDs so triangle membership can be reconstructed.

Rater order is R0 author, R1 Luna A, R2 Luna B. R1 and R2 are separate agent
contexts from the same model family; R0 is the experiment author. This is a
proxy-review diagnostic, not independent-human reliability evidence. A
two-of-three split is `DISPUTED` and maps to consensus `UNKNOWN`; it is not
resolved by majority vote. Triangle patterns are descriptive and do not
assume compatibility is transitive.

The executable accepts the following paths, in order:

```text
packets author-review luna-a-review luna-b-review private-ledger
comparison-receipt p1o2-analysis rubric pre-review-root validation-receipt
sufficiency-receipt this-Cargo.toml fresh-output-directory
```

Outputs are written only to a previously nonexistent directory. The receipt
binds the inputs, source, manifest, lockfile, and executable hashes.
