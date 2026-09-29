# Synthetic lexical transport bank (engineering lane)

This is a separate engineering lane. It does not revise, reopen, or claim to
qualify the frozen P1P3/E2-C experiment. Its purpose is to build a usable
training bank from existing contextual lexical-substitution data and put a
gate into the QPS transport path.

## Seed sources

The first bank uses the SWORDS v1.1 development release and the CoInCo
development release packaged in the SWORDS repository. SWORDS supplies
context-specific substitute judgments (`TRUE`, `FALSE`, `UNSURE`); CoInCo
supplies human-produced substitutes (`TRUE_IMPLICIT`) and candidates that were
not produced by annotators (`FALSE_IMPLICIT`). The latter is a weak negative,
not an explicit rejection, and remains marked as such in every generated row.

The repository declares SWORDS, CoInCo, and MASC content CC-BY-3.0-US. Source
attribution and the pinned repository revision are recorded in the build
receipt. WiC is not included in this app-capable first pass because its public
license is noncommercial. SCWS and the other listed resources can be added
after their exact redistribution/use terms are checked; this does not block the
current build.

## What the custom bank means

For a directed relation `x -> y`, a left seed is a natural occurrence of `x`
with a judgment about whether `y` substitutes in that context. A right seed is
a natural occurrence of `y` with a judgment about whether `x` substitutes
there. We join different contexts in the same direction:

* `SAME`: both endpoints have positive substitution evidence.
* `DIFFERENT`: either endpoint has rejection evidence.
* `UNKNOWN`: neither endpoint rejects, and at least one seed judgment is
  unsure or mixed.
* `SYNTHETIC_UNKNOWN`: both endpoint contexts are erased, leaving no local
  evidence. This is a generated abstention example, not a natural semantic
  label.

The candidate terms are replaced with `[FOCAL]` in the context strings so a
gate can learn from context rather than merely seeing the answer words. The
relation remains explicit in each row. Direction, source dataset, judgment
type, vote counts, and confidence are retained. We do not claim the joined
labels are direct human judgments of cross-document transport; they are
programmatic engineering labels derived from contextual substitution evidence.

Context text is stored once in `contexts.jsonl`; observations and paired bank
rows refer to compact context IDs. Rows are deterministically partitioned by
context identity (70/15/15) before pairing, so an occurrence cannot cross the
TRAIN/DEV/TEST boundary. The split is for practical model development, not a
formal external qualification claim.

## Build

The source checkout and generated artifacts live outside Git under
`D:\phoenix-evals\lexical-transport-seeds-20260928`. From the repository root:

```powershell
python experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/build_seed_bank.py `
  --swords-root D:\phoenix-evals\lexical-transport-seeds-20260928\swords `
  --output D:\phoenix-evals\p1p3e2-synthetic-transport-bank-20260928\bank-v1
```

The builder refuses to overwrite an existing output directory. It uses only
Python's standard library. The generated receipt hashes both source archives,
the pinned upstream revision, the generator, and all output files.

## Scope

This bank is training/debug material for an engineering selective gate. The
natural E2 retrieval-opportunity examples remain useful for downstream QPS
sanity checks, but are not represented as a fresh qualification set. No
authority learning or production serving is part of this artifact.

## Weighted gate and QPS application run

The completed engineering result is in
[`WEIGHTED_TRANSPORT_ENGINEERING_REPORT.md`](WEIGHTED_TRANSPORT_ENGINEERING_REPORT.md).
The bank used for fitting is the validated `bank-v3` artifact at
`D:\phoenix-evals\p1p3e2-synthetic-transport-bank-20260928\bank-v3`.
The fit script requires Python and NumPy. The optional frozen 230M readout
also requires PyTorch, Transformers, a CUDA-capable device, and the pinned
`LiquidAI/LFM2.5-230M-Base` revision recorded in its receipt.

```powershell
python experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/train_weighted_gate.py `
  D:/phoenix-evals/p1p3e2-synthetic-transport-bank-20260928/bank-v3 `
  D:/phoenix-evals/p1p3e2-weighted-gate-v2-20260928

$env:CARGO_TARGET_DIR = 'D:/phoenix-target-overgraph'
cargo test --release --manifest-path `
  experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/qps_transport_harness/Cargo.toml
cargo build --release --manifest-path `
  experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/qps_transport_harness/Cargo.toml
```

The isolated QPS package referenced by that Cargo manifest is the same frozen
source copy used by the E2 baseline ranker. The corpus replay consumes the
17 reviewed E2 opportunity rows and writes a new directory; the runner
refuses to overwrite an existing one.

```powershell
& D:/phoenix-target-overgraph/release/p1p3e2-weighted-qps-transport.exe `
  experiments/lt9-la2-p1p3e2-transport-opportunity-20260928/cohort.json `
  D:/phoenix-evals/lt9-la2-p1p3e2-context-bank-final-20260928/transport-opportunity-bank.jsonl `
  D:/phoenix-evals/p1p3e2-weighted-qps-search-v2-20260928

python experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/score_qps_lanes.py `
  D:/phoenix-evals/p1p3e2-weighted-gate-v2-20260928/weighted-gate.json `
  D:/phoenix-evals/p1p3e2-weighted-qps-search-v2-20260928/qps-transport-searches.jsonl `
  D:/phoenix-evals/lt9-la2-p1p3e2-context-bank-final-20260928 `
  experiments/lt9-la2-p1p3e2-transport-opportunity-20260928/cohort.json `
  D:/phoenix-evals/p1p3e2-weighted-qps-lanes-v2-20260928
```

The checked-in `results/` files are compact model/receipt artifacts. They
exclude raw BEIR text and pretrained model weights; the full-corpus hashes
and model revision are recorded so the result remains auditable.

The optional 230M comparator uses the exact pinned local checkpoint and a
deterministic bounded subset of the same bank:

```powershell
python experiments/lt9-la2-p1p3e2-synthetic-transport-20260928/lfm230_readout.py `
  D:/phoenix-evals/p1p3e2-synthetic-transport-bank-20260928/bank-v3 `
  D:/phoenix-models/lfm2.5-230m-base-9d2be55 `
  D:/phoenix-evals/lt9-la2-p1p3e2-context-bank-final-20260928 `
  D:/phoenix-evals/p1p3e2-weighted-qps-search-v2-20260928/qps-transport-searches.jsonl `
  D:/phoenix-evals/p1p3e2-lfm230-readout-20260928
```

Use a new output directory for any replay. The checked-in receipt is the
result of the original run; replaying it must not overwrite those artifacts.
