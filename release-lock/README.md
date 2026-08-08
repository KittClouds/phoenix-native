# Phoenix exact-parity release lock

`phoenix-release-lock` is a fail-closed comparison tool. It does not transform
an Angular packet into a native scene and it has no compatibility fallback.
The JSON file in this directory is metadata describing a frozen comparison
cohort; graph freight remains in the memory-mapped `.psa` and `.pspi` files.

The first frozen live cohort is:

- note `05dbbd93-e0f7-4fb2-937e-92a2352dcbae` (`Shortrun B`);
- stored Markdown: 154,105 UTF-16 code units, 159,402 UTF-8 bytes;
- live plain text: 154,024 UTF-16 code units, 159,321 UTF-8 bytes;
- footer analytics: 26,198 words and 152,000 characters after line-break
  removal;
- Angular packet generation `fnv64-a88cd38f42d7035c`.

The 154,105/152,000 display discrepancy is therefore not document drift.
154,105 is the stored Markdown length. The footer derives 152,000 from the
154,024-character live plain text after removing 2,024 line-break characters.

The captured Angular generation is not one stable topology across all five
manifolds. HOPF contains 3,663 nodes and 21,795 edges; Hybrid, Caps, Transit,
and Siegel contain 642 nodes and 9,219 edges. The release lock records the
exact identity, topology, position, color, label-freight, and guide-page hashes
instead of concealing this mismatch.

Run from the workspace root:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-cut6-release-lock'
cargo run --release -p phoenix-release-lock -- `
  --manifest release-lock\shortrun-b-angular-2026-07-26.json `
  --scene-archive C:\path\generation.psa `
  --scene-product-index C:\path\generation.pspi
```

Exit codes:

- `0`: every parity and architecture gate passed;
- `1`: artifacts were valid, but at least one release gate stopped;
- `2`: the manifest or native artifact could not be verified.

The lock computes the same dual-lane byte hash used by Angular V2 packets for
native node IDs, edge IDs, slot topology, positions, and RGBA8 colors. It also
opens and binds the native product index, verifies all six native manifold
inventories, and records archive/index generation and BLAKE3 identities.

Known STOP conditions in the initial run are intentionally explicit:

- the frozen Angular manifolds do not share identities/topology;
- the current native visual fixture is 10,000 nodes / 50,000 edges and is not
  the live Shortrun B cohort;
- `PhoenixSceneArchiveV1` does not yet bind note ID/version/text hash;
- Angular does not emit model identities or canonical family/review/scope
  pages in the V2 scene packet;
- Angular detail strings are not a canonical node-label page;
- Angular and native guides/prepared paths lack a shared canonical digest.

No performance result can override a semantic STOP. Live proof/soak receipts
remain necessary for switch, frame, interaction, resize, DPI, lifecycle, and
allocation gates once the exact cohort passes this lock.
