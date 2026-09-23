# Phoenix Native product branch scope — 2026-09-23

Branch: `codex/phoenix-native-product-reader-manifolds-20260923`

This branch starts from `codex/phoenix-native-cleanroom-cut3` and carries the
Phoenix product changes present in the shared checkout: native Reader and
Supertonic/Breeze providers, shell and editor integration, manifold projection
and renderer work, and their focused tests, tools, and qualification notes.
The mixed workspace manifest and lockfile were resolved for this product tree.

The active QPS/lexical and memory-lock science changes from the later mixed
snapshot are excluded. The baseline QPS crate remains because it is an existing
application dependency; no experimental QPS changes were replayed. The
memory-lock proof apps are not workspace members on this branch.

The GLiNER2.5 worker and analysis-bridge source lives under the separate
`clean-rust/rust-native` tree. This repository contains its integration notes,
not a copy of that external source. A combined packaged release still requires
an exact bridge/model dependency pin and live acceptance of the same binary.

The shared `clean-rust` checkout and other agents' branches were left intact.
This branch records product source, not final narrator-quality or packaged-app
qualification.
