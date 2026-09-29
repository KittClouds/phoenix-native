# P1P3E2 synthetic seed bank

**Status:** `SYNTHETIC_ENGINEERING_BANK_BUILT`
**Date:** 2026-09-28
**Lane:** separate engineering branch; formal E2-C remains unchanged.

The custom bank is built from pinned SWORDS v1.1 development and CoInCo
development data. It contains **481,985** context-specific substitution
observations over **10,288** masked context occurrences. Joining reciprocal
substitution evidence across distinct contexts produced **75,296** paired
engineering examples across **6,984 directed relations**. A second copy of
each definite pair has both contexts erased to teach the gate a concrete
no-evidence abstention case.

| Split | SAME | DIFFERENT | UNKNOWN | Total |
|---|---:|---:|---:|---:|
| TRAIN | 10,126 | 46,190 | 57,344 | 113,660 |
| DEV | 1,110 | 9,256 | 10,418 | 20,784 |
| TEST | 806 | 7,808 | 8,678 | 17,292 |

`UNKNOWN` contains **75,296 generated no-local-evidence rows** and **1,144
seed-unsure/mixed rows**. The source observations were split by normalized
context identity before pairing; the validator checked all 151,736 rows and
found zero context groups crossing splits.

The labels preserve a quality distinction that matters for fitting:

* `SWORDS_TRUE_VOTE` / `SWORDS_FALSE_VOTE` come from explicit context-specific
  votes.
* `COINCO_TRUE_IMPLICIT` records substitutes produced by annotators.
* `COINCO_FALSE_IMPLICIT_WEAK_NEGATIVE` means the substitute was not elicited;
  it is **not** an explicit rejection.

Of the 63,254 `DIFFERENT` paired rows, **3,886** include an explicit SWORDS
false-vote endpoint. The remaining rows are driven by at least one CoInCo
weak-negative endpoint. `label_strength` and both endpoint evidence types are
retained so the engineering fit can down-weight weak negatives. This bank is
therefore useful training material, not a direct human-labeled cross-context
transport benchmark.

The source checkout is pinned at
`p-lambda/swords@04ca75370d0ce098a7f4db68240fc8e79a4f7b3b`. Upstream declares
SWORDS, CoInCo, and MASC content CC-BY-3.0-US and documents the per-context
substitute/vote format in the [SWORDS repository](https://github.com/p-lambda/swords).
The generated corpus stays outside Git at:

`D:\phoenix-evals\p1p3e2-synthetic-transport-bank-20260928\bank-v3`

The copied [build receipt](build-receipt.json) binds the source archive hashes,
generator hash, generated-file hashes, row counts, and validation result. The
compressed artifacts total about 55 MB.

No model was fitted, no QPS retrieval lane was run, and no authority or serving
state changed. The next engineering step is to fit the small selective gate
using `label_strength` and the endpoint provenance, then connect its decisions
to the existing QPS lanes. The TEST partition is an internal seed-bank check,
not external qualification.
