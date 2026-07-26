# Renderer experiment audit

Audit date: 2026-07-25.

Source inspected:
`C:\Users\shuga\.gemini\antigravity\scratch\graph-renderer-poc`.

Only source, manifests, shader intent, and tests were carried forward. The scratch `target/`
directory and its build artifacts were excluded.

## Confirmed experiment strengths

- The crate split correctly kept `winit` outside the renderer.
- Stable IDs, revisioned snapshots/diffs, instanced nodes, screen-space edge quads, and integer
  picking were the right primitives.
- The demo already used event-driven redraw rather than a permanent maximum-speed loop.
- Deterministic fixture topologies covered useful density and shape extremes.
- Production source files were already below the 800-line repository limit.

## Confirmed faults in the source experiment

| Fault | Consequence | Clean-room resolution |
|---|---|---|
| Diff validation cloned the complete node set | Graph-sized allocation on every small update | Predicates over resident `hashbrown` maps; extra edge scan only when nodes are removed |
| Mutation lanes were not checked for duplicates/conflicts | Adds could overwrite IDs; unknown updates/removals were ignored | Named add/update/remove existence and disjoint-lane guards |
| Node removal did not require incident-edge action | Existing edges could address tombstones or a reused node slot | Fail-closed incident-edge contract, including updated-edge escape |
| GPU state mutated during an incompletely validated diff | Late failure could leave partial state | Full validation before mutation |
| Every diff recreated node and edge bind groups | Avoidable driver allocations and CPU work | Buffer generation tracking; bindings refresh only on growth |
| Each changed slot used a separate queue write | 1% changes produced hundreds of calls | Sorted/deduplicated dirty slots and contiguous range coalescing |
| Fit-to-view cloned all `NodeVisual` records | Graph-sized temporary allocation | Iterator-based bounds calculation over resident nodes |
| Picking copied with `bytes_per_row=256` into a 4-byte buffer | Invalid copy layout | Fixed 256-byte staging allocation |
| Picking marked mapping active before callback completion | Read could occur before mapping became ready | Explicit idle/pending/ready/failed atomic state machine |
| Integer picking cleared with a floating `0xFFFFFFFF` sentinel | Fragile conversion and unnecessary sentinel complexity | Zero clear; shader emits slot plus one |
| Click picking started on pointer press | Orbit gestures could also select | Selection begins on release only below the drag threshold |
| Asynchronous hover/selection had no host event delivery | Host could not observe completed GPU picks | Bounded event queue and `drain_events` |
| Animation repeatedly derived changes from the initial snapshot | Positions did not accumulate correctly | Demo derives each update from authoritative resident state |
| JSON failure silently substituted a synthetic graph | False success and cohort drift | Fatal fixture error with context |
| Revision increment saturated | Maximum revision could repeat and be stale | Checked revision increment |

## Architectural decisions

`SceneState` is the authoritative renderer-resident CPU index. It has no GPU or window dependency,
so identity, revision, deletion, and parity tests run everywhere. `GpuScene` is a projection:
packed node/edge arrays plus grow-only buffers. It cannot invent topology.

A graph diff follows this sequence:

```text
host diff
  -> complete semantic validation
  -> stable-slot mutation
  -> dirty-slot sort/deduplicate
  -> packed CPU range refresh
  -> contiguous queue writes
  -> bind refresh only if a buffer grew
```

The public construction boundary supports both a standalone device and a host-supplied
adapter/device/queue. The latter is the future GPUI integration seam.

## Performance evidence

The debug-profile state benchmark on this machine processed 100 updates, each touching 1% of a
25,000-node / 150,000-edge scene, in 30.838 ms total (0.308 ms per update). The release-profile
result was 1.4765 ms total (0.015 ms per update). These are not render-frame claims.

The release demo was then exercised in the native Windows application with the deterministic
10,000-node / 50,000-edge clustered fixture:

- initial snapshot rendered and entered event-driven idle;
- orbit visibly changed the 3D view;
- a manual 1% update completed without replacing the scene;
- integer GPU picking resolved stable `NodeId(3281)`;
- continuous 1% updates presented at 58 FPS over a 2.2-second observation;
- stopping updates returned to event-driven idle;
- maximize/resize reconfigured the surface and preserved aspect.

The first live attempt exposed a fatal picking lifecycle bug: mapping began before the command
buffer that copied into the staging buffer was submitted. `wgpu` correctly rejected a queue
submission using the mapped buffer. The fixed order is encode copy, submit, begin asynchronous map,
poll readiness, read four bytes, and unmap. The complete live sequence above passed after that
change.

The 58 FPS title is presentation cadence while the demo also applies a 1% diff every frame. It is
not GPU timestamp-query evidence and is not yet a V3 parity claim.

## Parity work still open

The clean-room renderer intentionally does not copy legacy packet contracts. Before parity claims,
freeze one comparison manifest containing exact node/edge IDs, positions, visual kinds/flags,
relation visibility, manifold guide pages, label priority, camera state, and expected interactions.

Likely next vertical slices:

1. branded packed scene-page format with shared identity/edge pages;
2. prepared straight/curve/bundle path pages and GPU policy masks;
3. GPU text/label atlas with bounded priority and collision policy;
4. all five manifold page adapters and guide geometry;
5. fixed-capacity hover/selection/route overlays;
6. GPUI render host and side-by-side legacy parity harness.

No slice may gain a production legacy fallback. Unsupported capabilities return an explicit error
until their native page exists.
