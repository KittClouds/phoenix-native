# Phoenix native status handoff — 2026-07-26

## Executive verdict

Phoenix native now has a substantial clean-room application, resident renderer,
scene archive/index contracts, atomic publisher, kernel-owned controls, typed CAPS
layout, labels/interaction layers, and a fail-closed exact-parity lock.

It is **not yet production-cut-over**.

The decisive remaining boundary is the producer:

- the running native application still opens a prepared native fixture generation;
- the legacy Rust Atlas engine has now been exercised against the exact Shortrun B
  bytes in an isolated copied store;
- that legacy result is not yet converted into a versioned
  `PhoenixGraphGenerationV1`, compiled into `.psa` + `.pspi`, and atomically
  published through the clean-room kernel;
- exact Angular/native parity is therefore still a recorded STOP, not a release.

Do not describe this as “Phoenix V2 parity complete.” The correct description is:

> The native rendering, authority, storage, control, and proof substrate is largely
> built and green. The exact real-data producer bridge and final semantic parity
> lock remain.

## Repository and live state

- Workspace: `C:\code land\clean-rust`
- Branch: `codex/phoenix-native-cleanroom-cut3`
- HEAD: `8d4fcec9` — `Advance Phoenix graph product and native shell`
- That commit is present on `origin/codex/phoenix-native-cleanroom-cut3`.
- The current worktree also contains uncommitted follow-on CAPS, compiler,
  publisher, native Atlas-control, and camera changes. Preserve them.
- The current full-scene native executable is running from:
  `C:\phoenix-bin\atlas-control-e2e-v2-20260726\phoenix-shell-atlas-control-e2e-v2.exe`
- It was launched with `--require-full-scene` and an explicit publication root.
- Its current generation is a native visual/semantic fixture:
  392 nodes, 841 edges, 180 verified entity mappings.
- It is **not** the Shortrun B graph.

Current focused verification, run after the uncommitted follow-on work:

- 89 Rust tests passed across `graph-render-wgpu`, `graph-model`,
  `phoenix-app-core`, `phoenix-scene-compiler`, `phoenix-scene-contract`, and
  `phoenix-scene-publisher`.
- Focused Clippy passed with `-D warnings`, including `phoenix-shell`.
- Target artifacts remained on `D:\phoenix-target-atlas-control-e2e`.

## Status by remapped native cut

### Cut 1 — resident five-manifold engine

Status: **implemented substrate; fixture performance proof passes; final cohort gate open**

Done:

- `ResidentScene` owns an `Arc<PhoenixSceneArchiveV1>`, not an
  `Arc<GraphSnapshot>`.
- Shared identities/topology/edges are opened from the memory-mapped archive.
- All five manifold position inventories are verified on ingress.
- Active guide/path pages are bounded by a named hot-page budget and fail closed.
- Manifold changes are bounded kernel `SetManifold` commands.
- The renderer performs packed position updates without rebuilding graph identity
  or topology.
- `ManifoldSwitchReceipt` telemetry exists.
- Stable GPU slots, buffer generations, and allocation capacities are checked.

Recorded fixture proof on 10,000 nodes / 50,000 edges:

- 200 manifold switches completed.
- Warm switch CPU p95: 5.426 ms in proof, 4.154 ms in soak.
- Switch-to-present p95: 5.917 ms in proof, 4.697 ms in soak.
- GPU allocation bytes and buffer generations remained stable.
- 1,000 interaction updates had CPU p95 349 µs in proof and 286 µs in soak.
- Resize-present p95 remained below 16.7 ms, though recorded maxima exceeded it.

Still open:

- repeat the performance, allocation, and process-memory plateau gates on the exact
  final Shortrun B native generation;
- record a general interactive-frame p95, not only switch/resize receipts;
- rerun the full live proof after the current uncommitted renderer/CAPS changes.

### Cut 2 — scene product index and native view state

Status: **substantially implemented**

Done:

- Immutable, mmap-backed `PhoenixSceneProductIndexV1`.
- Cryptographic archive binding.
- UTF-8 label slab and offsets.
- Node/edge family, scope, review, and relation masks.
- Inspector/provenance references.
- Verified `EntityId ↔ NodeId` mappings.
- Kernel-owned `GraphViewState`.
- Lens updates write bounded masks/uniforms rather than rebuilding arrays.
- Stale, corrupt, mismatched, and oversized artifacts fail closed.

Contract difference to resolve:

- the implemented `GraphViewState` has authority, surface, lens, scope, reviews,
  relations, and manifold;
- the originally proposed typed `ProjectionProfile` field is not present;
- add it only if Shell/Multi still has a real product requirement.

### Cut 3 — real native scene publication

Status: **publisher complete; final backend producer not connected**

Done:

- `ScenePublicationStore` atomically publishes one `.psa` archive plus one `.pspi`
  product index and then replaces `current.pspm`.
- Generation monotonicity and archive/index receipt hashes are verified.
- Cold reopen binds the exact pair.
- Registry-only publication uses stable canonical IDs.
- Registry-only publication cannot demote an existing full generation.
- The kernel owns the writable production publisher handle.
- Explicit read-only publication-root startup is available for development.
- Production synthetic fallback is excluded.

Still open:

- define and freeze `PhoenixGraphGenerationV1`;
- adapt the qualified backend output into that contract;
- compile the exact real graph generation into archive/index pages;
- make `PhoenixKernel::rebuild_active_scene` use that producer instead of the
  current narrow `compile_active_document` path;
- prove cold restart of the exact real generation;
- remove the development fixture from final production startup.

### Cut 4 — redesigned GPUI graph controls

Status: **implemented; final live UX acceptance open**

Done:

- `Entities | Atlas` surface choice.
- Five-manifold selector.
- Entities, Structure, Facts, and Discourse lenses.
- Independent Accepted and Proposed review toggles.
- Relation-family controls.
- Global, Narrative, Note, and Compare scopes.
- Fit and Reset actions.
- Kernel-owned bounded commands and validation.
- New Atlas-control state machine exposes one valid next action and never claims a
  publication after a failed build.

Still open:

- finish live visual/interaction acceptance on the final real generation;
- verify Compare identity behavior with real multi-scope data;
- decide whether a typed Hybrid projection profile is still needed.

### Cut 5 — labels, styles, and interaction parity

Status: **native layers implemented; exact semantic/visual parity open**

Done:

- Bounded label selection/collision grid.
- Prepared straight, curved, and bundled path layers.
- Guide styling flags.
- Kernel-persisted highlight palette.
- Hover, picking, selection, bounded route search, and neighborhood interaction.
- Relation/lens filtering through GPU product records and a compact uniform.
- Verified Atlas-row/graph-node cross-selection.
- Stable-capacity interaction stress proof.
- Camera behavior now follows the Angular interaction contract more closely:
  panned offset survives orbit, vertical tilt direction is aligned, wheel zoom
  tracks the pointer on zoom-in, and right/middle drag pans.

Still open:

- exact label/color/guide/path parity on Shortrun B;
- live hover/orbit/route proof on every manifold with the final producer;
- process-memory and GPU-memory plateau proof under the final label population;
- a final camera interaction pass in the running application after the current
  camera patch.

### Cut 6 — exact parity and release lock

Status: **lock implemented; release result is STOP**

Done:

- Frozen Shortrun B cohort metadata.
- Exact note identity, version, text lengths, and hashes.
- Angular binary/repository identity.
- Per-manifold IDs, topology, positions, colors, labels, and guide-page hashes.
- Fail-closed `phoenix-release-lock` executable.
- Zero JSON graph freight, zero runtime fallback, and compile-time legacy-adapter
  exclusion gates.
- Separate live proof and soak receipts.

Frozen Shortrun B identity:

- Note ID: `05dbbd93-e0f7-4fb2-937e-92a2352dcbae`
- Title: `Shortrun B`
- Stored Markdown: 154,105 UTF-16 code units / 159,402 UTF-8 bytes
- Markdown SHA-256:
  `9beafe8bd23317e118d218d99a21a21d53ed61371d6fbf60d76b0f0eedc9b8c7`
- Footer: 26,198 words / 152,000 characters after line-break removal
- Angular generation: `fnv64-a88cd38f42d7035c`

Current exact-lock result:

- 7 gates pass.
- 14 gates stop.
- Angular itself does not expose one stable identity/topology inventory across all
  five manifolds:
  - HOPF: 3,663 nodes / 21,795 edges
  - Hybrid, CAPS, Transit, Siegel: 642 nodes / 9,219 edges
- The current native fixture is not the Angular cohort.
- Exact node/edge IDs, topology, colors, and all five position pages do not match.
- Archive-level note ID/version/text-hash binding is still absent.
- Angular does not provide canonical model, family/review/scope, label, or
  guide/path parity contracts.

No fixture performance result overrides those semantic stops.

## CAPS redesign status

Status: **analytical H3-style kernel and native guide presentation implemented;
authority contract and final semantic producer incomplete**

Done:

- Typed roles: Document, Episode, Chunk, Evidence, Event, Fact, Entity, Memory.
- Explicit stable ID, parent slot, sibling rank/count, and membership count.
- Deterministic analytical Lorentz-to-Klein position generation.
- Strict nested semantic radii inside a bounded Klein ball.
- Tangent-cap sibling packing with bounded apertures/rings.
- Input-order determinism and malformed-parent/sibling failures.
- Local descendants remain inside parent caps.
- Three orthogonal circles per semantic shell.
- Cap-boundary rings and a concentration axis.
- Prepared CAPS guides are built during publication, not in the renderer.
- Guide/path styling is driven by bounded flags.
- CAPS geometry/property tests pass.
- A dedicated CAPS construction benchmark exists.

Differences from the proposed contract:

- there is no explicit `CapsAuthority` enum yet;
- `StructuralH3` is effectively the implemented behavior but is not recorded as a
  first-class receipt;
- `LearnedLorentzH4` is not implemented, which is correct until a verified model
  publishes real H4 coordinates;
- `CapsNode` does not currently carry the proposed `cap_id`, `confidence`, and
  `ambiguity` fields; membership count is the current compact overlap signal.

Still open:

- introduce an explicit authority receipt before claiming structural vs learned
  geometry;
- bind real backend roles, parents, memberships, confidence, and ambiguity;
- benchmark the requested 25,000-node / 150,000-edge cohort and record p95;
- run the live CAPS interaction/plateau gates on the final real graph.

## Backend compatibility bridge: work completed

The bridge is intentionally isolated at the producer boundary. No legacy Angular
or graph-packet authority has entered `phoenix-native`.

Completed:

- The exact saved Shortrun B Markdown was exported from an isolated copied
  Phoenix store.
- Its SHA-256 matches the frozen release-lock hash exactly.
- The helper refuses the live Phoenix Desktop store path.
- Both helper executables built in:
  `D:\phoenix-target-legacy-cohort\release\examples`
- The scan ran against:
  `D:\phoenix-angular-cohort-root-20260726`
- The result is:
  `D:\phoenix-angular-cohort-root-20260726\analysis.json`

First isolated forced-scan result:

- 1 document processed, 0 skipped.
- 4,467 dynamic mentions.
- 2,361 sentences.
- 395 dynamic chunks and 3,282 lens chunk hints.
- 2,054 graph nodes and 3,072 graph edges.
- 15,108 evidence receipts and dataset examples.
- 159 candidate suggestions.
- 102 alias proposals.
- 4,636 identity nodes, 10,717 identity edges, 831 identity receipts.
- 0 relation candidates.
- 0 embeddings emitted.

Important limitations:

- GLiNER attached successfully.
- GLiClass kind arbitration did not run because
  `gliclass-instruct-onnx-v2\encoder.onnx` was missing.
- The exporter’s generic `relation:list` request returned zero entities.
- A separately captured live registry contains 179 entities, so canonical
  registry export/reconciliation is not solved.
- The forced scan wrote new segments into the isolated copied store. That is safe
  for the live app but means a fresh copy is required for a clean determinism
  rerun.
- `analysis.json` is an audit artifact. It must not become production JSON graph
  freight.

## Required next work, in order

### 1. Close and freeze the producer audit

- Export the 179 canonical entities through a verified Rust store command.
- Resolve or explicitly provision the missing GLiClass model.
- Record exact model hashes and runtime identities.
- Repeat the forced scan from a fresh copy.
- Compare graph IDs, counts, evidence, candidates, and receipts across two clean
  runs.
- Prove that the live store was never mutated.

### 2. Define `PhoenixGraphGenerationV1`

The contract should include:

- generation and cohort identity;
- document ID, revision, content hash, and source-span coordinate contract;
- producer binary and model identities;
- stable node and edge IDs;
- compact shared topology;
- families, scopes, review states, and relation masks;
- verified `EntityId ↔ NodeId` mappings;
- evidence/provenance references;
- typed CAPS roles, parents, memberships, and authority;
- stage receipts.

This is graph truth. Five manifold positions, guides, and prepared paths remain
scene-compiler outputs.

### 3. Build the process-isolated adapter

- Keep legacy types and storage outside `phoenix-native`.
- Convert qualified legacy results into `PhoenixGraphGenerationV1`.
- Use a bounded binary/process boundary; do not adopt the Angular packet or a
  production JSON graph payload.
- Fail closed on missing models, stale document identity, corrupt output, unknown
  roles, or unstable IDs.

### 4. Connect the clean-room producer

Replace the temporary path:

```text
PhoenixKernel::rebuild_active_scene
  -> compile_active_document
```

with:

```text
PhoenixKernel::rebuild_active_scene
  -> native producer coordinator
  -> PhoenixGraphGenerationV1
  -> scene compiler
  -> atomic .psa + .pspi publication
  -> resident renderer
```

### 5. Run the final lock

- Reopen the exact published generation in a fresh process.
- Verify all graph/product/scene hashes.
- Run 200 manifold switches, interaction stress, resize/collapse, DPI,
  multi-monitor, minimize/maximize, device/surface loss, corruption rejection,
  memory pressure, and cold restart.
- Record process and GPU memory plateaus.
- Require zero fallback and zero production JSON graph freight.
- Compile legacy adapters out.
- Accept only the live Phoenix native application as final authority.

## Guardrails

- Do not port Angular orchestration or packet formats into the native app.
- Do not let Overgraph or legacy store types leak into kernel, renderer, or scene
  contracts.
- Do not infer identity from labels.
- Keep candidate semantics separate from asserted GraphTruth until promotion.
- Do not invent H4 coordinates or silently claim learned CAPS authority.
- Do not replace a valid full generation with registry-only output.
- Do not call fixture proof “exact parity.”
- Preserve the current dirty worktree and the running native application.
