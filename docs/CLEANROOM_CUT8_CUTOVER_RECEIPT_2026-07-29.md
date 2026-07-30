# Clean-room Cut 8 — production V2 cutover receipt

Date: 2026-07-29

## Authority

Phoenix Native production rebuilds now have one route:

```text
active document lease
  -> bounded native model producer
  -> PhoenixProducerCoordinatorV1 artifacts
  -> clean-room document/entity/story producers
  -> PhoenixGraphGenerationV2
  -> V2 scene compiler
  -> atomic PSA/PSPI publication
  -> resident renderer
```

`PhoenixGraphGenerationV2` is the production graph authority. The kernel and
shell have no normal dependency on the V1 graph-generation crate or the
detached legacy bridge. The V1 compiler is available only to tests, explicitly
feature-gated fixture examples, and benchmarks.

## Fail-closed cutover locks

- `apps/phoenix-legacy-bridge` is excluded from the production workspace and is
  a standalone diagnostic utility with no kernel dependency or publication
  authority.
- The shell's `legacy-graph-adapter` feature is an architecture tripwire that
  always emits `PHOENIX_LEGACY_GRAPH_ADAPTER_FORBIDDEN`.
- The V1 scene compiler exports are absent unless tests or the explicit
  `legacy-v1-fixture` feature are selected.
- The production graph pipeline crates do not depend on `serde_json`; the
  release manifest requires JSON graph freight and fallback counts to remain
  zero.
- A fixture-built run cannot be frozen as a production release manifest because
  it has no verified production analysis generation.

## Restart authority

Every full scene publication has a fixed-size compiler-authority sidecar that
binds the PSA/PSPI hashes to the exact V2 source-generation hash. Production
startup:

1. opens the atomic scene publication manifest;
2. verifies the compiler-authority sidecar;
3. opens the exact mmap-backed `.pgg2` by generation hash;
4. verifies document and registry authority;
5. reconstructs only the immutable review index;
6. installs the same V2 generation and scene as the single resident authority.

The published document takes startup precedence over an unrelated previously
active note. A changed source document may leave the old scene visible for
inspection, but its stale Atlas run is not restored as current and its review
authority cannot act on the changed document.

Corrupt or missing compiler authority, mismatched V2 pages, oversized authority
inventories, or missing exact generations fail closed. There is no runtime
compatibility switch.

## Review publication

Reviewed publication now installs and records the exact reviewed V2 generation,
not its pre-review source generation. Its review catalog is regenerated against
that immutable generation, so subsequent actions and cold restart share one
hash-bound authority.

## Verification

All build and test output for this cut is directed to
`D:\phoenix-target-v2-cutover`.

The focused restart, corruption, release-lock, bounded coordinator, V2 compiler,
producer, review, and architecture tests must pass before this receipt is
complete. The production dependency tree must contain V2 but neither the V1
graph-generation crate nor `phoenix-legacy-bridge`. Enabling the legacy shell
feature must fail compilation.

Recorded result:

- 114 unit and integration tests passed.
- The production-only V2 compiler compile-fail documentation test passed.
- Clippy passed for all affected targets with warnings denied.
- The shell's forbidden legacy feature failed compilation with the named
  `PHOENIX_LEGACY_GRAPH_ADAPTER_FORBIDDEN` error.
- The normal shell dependency tree contains V2 and contains neither the V1
  graph-generation crate nor `phoenix-legacy-bridge`.
- The optimized 25,000-node CAPS benchmark recorded 12.9861 ms median,
  15.7047 ms p95, and 16.6732 ms max against its existing 20 ms p95 gate.

Cut 9 live application, interaction, performance, memory, cancellation,
device-loss, and exact-cohort evidence is deliberately deferred.
