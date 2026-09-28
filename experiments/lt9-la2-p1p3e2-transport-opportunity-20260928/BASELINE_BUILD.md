# P1P3E2 frozen BM25F/QPS baseline build

The ranking pass uses a standalone copy of the `phoenix-lexical-qps` source from the sealed P1P3E1 isolated harness at `D:\phoenix-evals\lt9-la2p1p3e1-20260928-isolated-v2\qps`. This avoids invoking the main TTS workspace and binds the QPS implementation independently of later repository edits. The copy and runner binary hashes are recorded in `P1P3E2_ARTIFACT_MANIFEST.json`.

Configuration:

- title field: weight `2.5`, `b=0.35`
- body field: weight `1.0`, `b=0.75`
- proximity, order, phrase, and segment residuals: `0`
- maximum query groups: `128`
- candidate pool cap: `256`
- serving candidate depth: top `100`

Each corpus is built and released one at a time. The corpus row ordinal becomes the QPS external document ID, matching the frozen opportunity scanner. Search uses the offline evidence API with the same bounded top-100 candidate selection. Any target whose exact returned rank is over 100, or absent from those candidates, is classified as `MISSED_TOP100`. A `QueryTooLarge` error is kept as `QUERY_UNSUPPORTED_TOO_LARGE`; queries are not truncated or altered.

Build and test on the target volume, then invoke the resulting executable from the test workspace:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-overgraph'
cargo test --release --manifest-path 'D:\phoenix-evals\lt9-la2-p1p3e2-qps-ranker-20260928\Cargo.toml'
cargo build --release --manifest-path 'D:\phoenix-evals\lt9-la2-p1p3e2-qps-ranker-20260928\Cargo.toml'
```

```powershell
& 'D:\phoenix-target-overgraph\release\p1p3e2-qps-baseline.exe' `
  'D:\phoenix-native-cleanroom-cut4-20260920\experiments\lt9-la2-p1p3e2-transport-opportunity-20260928\cohort.json' `
  'D:\phoenix-evals\lt9-la2-p1p3e2-expanded-final-20260928\opportunity-screen-receipt.json' `
  'D:\phoenix-evals\lt9-la2-p1p3e2-expanded-final-20260928\qrels-counterpart-candidates.jsonl' `
  'D:\phoenix-evals\lt9-la2-p1p3e2-qps-ranks-run3-20260928'
```

The release test passed 1/1. A second complete run using the same ranker logic produced a byte-identical ranked JSONL. No model, compatibility feature, qrels label join, lexical authority, or serving route participates in this baseline pass.
