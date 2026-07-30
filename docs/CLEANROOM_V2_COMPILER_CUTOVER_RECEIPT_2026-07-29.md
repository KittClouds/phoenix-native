# Clean-room V2 compiler cutover receipt

Date: 2026-07-29

## Result

`PhoenixGraphGenerationV2` is now the production scene authority.

The kernel rebuild route is:

```text
active document lease
  -> bounded native analysis
  -> verified V2 document/entity/story generations
  -> exact ReviewCatalog
  -> compile_graph_generation_v2
  -> immutable .psa + .pspi publication
  -> V2 compiler-authority sidecar
  -> one mmap-backed ResidentScene
```

Reviewed publication uses the same V2 compiler route. It first writes a new
immutable reviewed V2 generation through the decision ledger, then compiles
and publishes that generation atomically. It does not mutate an existing scene
or archive.

## Enforced authority

- A production full-scene publication requires a V2 compile receipt, verified
  V2 source generation, and exact review catalog.
- The compiler-authority sidecar binds the compiler contract, V2 source hash,
  scene generation, archive hash, and product-index hash.
- Startup rejects full scenes whose sidecar is missing, corrupt, stale, or
  mismatched.
- Installing a scene also installs the exact V2 source generation and review
  catalog in kernel state.
- New analysis invalidates prior V2 source authority before publication.
- Generic `Related` NLI remains contextual evidence and is not converted into
  a typed accepted relationship.
- User tags enter the V2 entity generation through stable IDs; no label-based
  identity merge is used.

## V1 disposition

The V1 scene compiler modules and public API are compiled only for tests or the
explicit `legacy-v1-fixture` feature. Its benchmark and visual-fixture example
also require that feature. A compile-fail documentation gate proves the V1 API
is absent with production default features.

The older V1 analysis artifact remains readable as an intermediate legacy
analysis/recovery format. It is not a production scene compiler or resident
scene authority.

The gated V1 fixture compiler does not manufacture an `Episode 1`. Without
verified episode authority, its chunks attach directly to the document.

## Verification

All commands used `D:\phoenix-target-v2-cutover`.

- `cargo fmt --all -- --check`: passed.
- Focused unit, contract, restart, corruption, and compile-fail gates: 58
  passed.
- Focused Clippy for the semantic review, entity producer, scene compiler, and
  app core with warnings denied: passed.
- All-target checks for app core and the scene compiler: passed.
- Downstream `phoenix-shell` and `phoenix-analysis-proof` checks: passed.
- Production caller census found no app-core use of
  `compile_active_document`, `compile_graph_generation`, or
  `NativeSceneCompilerInput`.

The Phoenix application was not launched or restarted for this cut. This
receipt proves build-time and contract cutover, not live cohort parity,
interaction latency, or soak performance.

## Rollback

Revert this cutover as one source change. Existing immutable V1 and V2
artifacts remain readable and untouched. There is no runtime compatibility
switch and no silent fallback from V2 to V1.
