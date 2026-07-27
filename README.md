# Phoenix Native

`phoenix-native` is the clean-room root for the Rust-native Phoenix replacement. It is deliberately
self-contained: no Angular, Tauri, TypeScript, browser storage, legacy graph packet, or Phoenix
service is imported here. The directory can become an independent repository without changing
crate paths.

The first vertical slice is a host-neutral `wgpu` graph renderer plus a small `winit` proving
application. `winit` is demo glue only; `graph-render-wgpu` does not depend on it. The second slice
proves a GPUI shell and the graph renderer can live in one process as two coordinated native
top-level windows without copying pixels. The third slice introduces one resident
`Arc<PhoenixKernel>` as the authority shared by both windows. Cut 3 freezes the first exact
comparison cohort in `PhoenixSceneArchiveV1`, a page-addressable binary format opened with
`mmap2`. The editor cut vendors Velotype as local Rust source before extracting its reusable
GPUI editor from the upstream standalone application.

## Current status

The renderer currently provides:

- stable 64-bit node and edge identities with monotonic graph revisions;
- transactional snapshots and diffs—invalid updates do not partially mutate resident state;
- stable reusable slots and tombstones;
- coalesced partial GPU uploads and bind-group rebuilds only after buffer growth;
- GPU-instanced circular nodes and screen-space edge ribbons;
- orbit, pan, zoom, fit, reset, resize, and DPI conversion;
- asynchronous one-pixel `R32Uint` picking with a 256-byte aligned readback buffer;
- bounded asynchronous renderer events;
- deterministic synthetic fixtures and memory-mapped JSON fixture loading;
- CPU timings, allocation telemetry, a model-state benchmark, and a headless GPU smoke test.

The native shell currently provides:

- pinned GPUI 0.2.2 and GPUI Component 0.5.1 sources;
- left navigation and a durable hierarchical workspace tree;
- stable workspace IDs and atomic create, read, rename, and recursive-delete persistence;
- a right inspector, system-truth drawer, and live footer data;
- a real embedded Velotype editor surface created from the vendored local library;
- bounded focus and visibility commands to the independent graph window;
- one resident, immutable graph generation shared through the kernel;
- bounded, monotonically sequenced kernel commands and events;
- kernel-owned active document, manifold, style, runtime capabilities, cancellation, and shutdown;
- explicit archive-backed scene ingress with no generated-data fallback.

The native scene archive provides:

- one shared copy of node identity/style, topology, edge, label-priority, relation-mask, and
  palette-policy pages;
- independent positions, compact guides, and prepared straight/curved/bundled paths for all five
  manifolds;
- a stable generation ID, binary format version, per-page BLAKE3 hashes, and a cohort hash;
- bounded memory-mapped opening with directory verification at ingress and page verification once
  on first access;
- named fail-closed errors for missing, corrupt, oversized, unknown, and unsupported-version pages;
- no JSON graph payload, browser-packet translator, production fixture generator, or legacy
  fallback.

This is not yet Phoenix V2 feature parity. Labels, relation-family controls, five authoritative
manifold scene pages, guide geometry, route overlays, and prepared curved/bundled edges remain
explicit later slices. Nothing in this workspace silently reconstructs those features from a
legacy packet.

## Workspace

```text
phoenix-native/
  apps/graph-demo/              winit window, fixture adapter, controls
  apps/phoenix-shell-proof/     GPUI shell and coordinated graph-window consumers
  crates/graph-model/           IDs, visual records, snapshots, diffs, validation
  crates/gpui-animated-gradient-text/
                                standalone publishable GPUI gradient text
  crates/graph-render-wgpu/     resident scene, GPU resources, camera, picking, shaders
  crates/phoenix-app-core/      single resident kernel, queues, lifecycle, capabilities
  crates/phoenix-scene-archive/ mmap binary archive, freezer, frozen comparison cohort
  crates/phoenix-scene-contract/ immutable scene-generation and style contracts
  crates/phoenix-workspace/     durable workspace authority and CRUD
  vendor/velotype/              pinned local Velotype v0.7.0 source and assets
  docs/renderer-audit.md        source experiment audit and architectural decisions
```

## Run

`phoenix-shell` is the production/default application. The standalone `graph-demo`
is retained only as a historical renderer harness and is unreachable unless the
caller explicitly enables `standalone-harness`.

For graph-product development, use the checked launcher. It opens the atomic
`current.pspm` authority and refuses missing, corrupt, registry-only, empty, or
unindexed generations instead of presenting an unexplained empty canvas:

```powershell
.\scripts\launch-graph-shell.ps1 `
  -Binary 'C:\phoenix-bin\phoenix-shell.exe' `
  -ScenePublicationRoot 'C:\phoenix-bin\verified-scene\scene-publications-v1'
```

The workspace still comes from `%LOCALAPPDATA%\Phoenix\NativeShell`; only the
explicitly named graph generation comes from the development publication root.
Entity-only startup remains available by launching `phoenix-shell` normally.

PowerShell:

```powershell
cd 'C:\code land\clean-rust\phoenix-native'
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-cleanroom-graph'
cargo run --release -p graph-demo --features standalone-harness -- --fixture clustered --nodes 10000 --edges 50000 --seed 42
```

Controls:

- left click: select;
- left drag: orbit;
- Shift + left drag or middle drag: pan;
- wheel: zoom;
- `F`: fit graph;
- `R`: reset camera;
- `Escape`: clear selection;
- `U`: apply one deterministic 1% update;
- `Space`: toggle continuous 1% updates.

Input pointer coordinates at the reusable boundary are logical pixels. Resize dimensions are
physical pixels. The renderer applies the supplied scale factor once.

## Verification

All outputs stay on the scoped `D:` target:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-cleanroom-graph'
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p graph-render-wgpu --bench state_updates
cargo build --release -p graph-demo --features standalone-harness
```

The kernel cut has a focused fail-closed receipt:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-document-lease-cut5'
cargo test -p phoenix-scene-archive -p phoenix-workspace -p phoenix-scene-contract -p phoenix-app-core -p phoenix-shell
cargo clippy -p phoenix-scene-archive -p phoenix-workspace -p phoenix-scene-contract -p phoenix-app-core -p phoenix-shell --all-targets -- -D warnings
cargo run -p phoenix-shell -- --proof --scene-archive crates/phoenix-scene-archive/tests/fixtures/phoenix-comparison-v1.psa
cargo build --release -p phoenix-shell
```

The non-mutating Cut 5 soak uses the real default workspace and requires a
verified scene archive:

```powershell
cargo run --release -p phoenix-shell -- --soak --scene-archive crates/phoenix-scene-archive/tests/fixtures/phoenix-comparison-v1.psa
```

The vendored Velotype source is a local workspace member. Its Tree-sitter runtime and highlighter
are pinned to 0.25.10, matching the exact native `links` identity already used by
`gpui-component`. The unchanged upstream suite passed before and after this alignment. Build it
directly from the repository:

```powershell
$env:CARGO_TARGET_DIR = 'D:\phoenix-target-editor-cut4'
cargo check -p velotype
cargo test -p velotype
```

The remaining embedded extraction may not hide standalone workspace or persistence authority
behind a second runtime editor process.

Normal shell runs persist the workspace manifest at
`%LOCALAPPDATA%\Phoenix\NativeShell\workspace-v1.json`. Proof mode uses a disposable isolated
manifest and verifies one resident archive generation, lazy page verification, command/event
sequencing, create, reopen, rename, recursive delete, graph-window teardown, and clean coordinator
shutdown. Proof mode fails if `--scene-archive` is absent; it never substitutes generated data.
The Cut 5 receipt additionally proves that the editor is using embedded host mode and has no
direct standalone file authority. Phoenix opens each selected note as a kernel-owned lease with a
stable entry ID, document revision, BLAKE3 content hash, and raw Markdown content. Embedded
Velotype emits save intent; only the bounded kernel coordinator may atomically replace the
versioned binary document envelope. Stale leases, corrupt envelopes, folders, unsupported versions,
and oversized content fail closed. The proof reopens the committed envelope from durable storage
and verifies that a stale save cannot replace it.

The benchmark is intentionally model-only. It measures validation, stable-slot lookup, and
transactional mutation without confusing those costs with adapter, presentation, or GPU queue
timing. Live rendering performance must be measured in the native demo and eventually in the GPUI
host.

## Clean-room contracts

1. Revisions only move forward.
2. Add, update, and remove mutation lanes are disjoint.
3. Adds cannot overwrite an existing identity; updates and removals cannot target missing IDs.
4. Removing a node requires every incident edge to be removed or updated in the same diff.
5. Validation completes before any resident state changes.
6. A small diff never triggers a full upload unless a buffer must grow.
7. GPU identities are slots only inside the renderer; host-visible events resolve stable IDs.
8. Fixture parse failure is fatal. The demo does not substitute synthetic data silently.
9. The renderer owns no `winit` type and accepts a host-owned adapter/device/queue path.
10. Phoenix-owned source should stay narrowly partitioned; large upstream vendored files may
    remain intact until a behavior-preserving extraction gives them a natural module boundary.
11. The GPUI shell never owns or reconstructs renderer graph state.
12. Workspace persistence fails closed; corrupt or unsupported manifests are not replaced by a
    seeded workspace.
13. `PhoenixKernel` is passed as an `Arc`; there is no global mutable singleton.
14. Shell and graph windows consume the same resident generation and cannot publish stale
    generations.
15. The shell has no production synthetic scene path; proof and archive-backed runs require an
    explicit native archive.
16. Archive directory and page limits are checked before mapping or page decoding, and corrupt
    pages never fall through to another source.

## Known limits

- Picking redraws the integer target when requested. It is throttled and reads one pixel, but large
  scenes still need live GPU timing before the final picking schedule is frozen.
- Straight edges are the correctness baseline. Curves and bundles will use prepared path pages,
  not per-frame CPU tessellation.
- Slot compaction is intentionally absent. Reuse prevents ordinary growth; a measured
  fragmentation policy will be added only with an identity-preservation test.
- The demo title reports presentation cadence, not GPU timestamp-query duration.
- There is no claim of V2 feature parity or V3 performance parity yet. Those claims require a
  frozen legacy comparison fixture and live side-by-side measurements.
