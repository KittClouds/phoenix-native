# QPS V3 grouped semantic-curation cut

Date: 2026-08-08

Status: grouped packet and pilot pass; Phase 5 remains fail-closed

## Purpose

The pairwise review queue repeated the same query and annotated positive for
each hard negative. This cut groups those existing pair identities by query so
one semantic pass can compare the annotated positive against three to five
same-query challengers. Every challenger remains an independent decision and
no bundle creates authority without explicit semantic adjudication.

## Frozen packet

```text
C:\benchmarks\phoenix-qps-v3-20260804\semantic-review-bundles-v1-1400.json
```

- contract: `phoenix.qps.semantic-review-bundle-batch/v1`
- SHA-256: `e82cb48f07e84931ddcbbfe717a0afe1b93b1f6ad513ac5a5fcb00506b05ae50`
- query bundles: `1,400`
- pair judgments available: `5,490`
- V2 disagreements: `4,289`
- all eleven failure classes projected above `100`: yes
- authoritative evidence created by packet generation: no

The generator recursively excludes the complete-context review batch and the
expanded 1,100-pair browser batch. It reads current Phase 5 class counts and
selects deficit-first before deterministic V2-disagreement impact, LoCoMo
transfer value, and stable identity.

## Audit

The release audit is:

```text
C:\benchmarks\phoenix-qps-v3-20260804\semantic-review-bundles-v1-1400-release-audit.json
```

- audit SHA-256: `716ee5969c1d24b8a020fb92bd773825b60c287ab9bd95fd5b10b780c4e514db`
- unique bundles/queries: `1,400`
- unique pair identities: `5,490`
- bundles with three to five challengers: `1,400`
- semantic decisions emitted by audit: no

## Verified pilot

The first three deficit-prioritized bundles were semantically reviewed under
`agent_curated_with_user_authorization`. Eleven decisive pair judgments were
submitted and all eleven appended with zero revisions or duplicate
applications.

Current checkpoint:

```text
C:\benchmarks\phoenix-qps-v3-20260804\review-checkpoints\b1a1edc5be7c-f1152b7c65e3
```

- Phase 4: verified
- active judgments: `263`
- unique queries: `107`
- independent sources: `223`
- explicit or curator-confirmed: `263`
- Phase 5: false, as required
- promotion judgment deficit: `4,737`
- promotion unique-query deficit: `893`

## Binary and verification

Release binary:

```text
C:\phoenix-bin\qps-v3-bundles-20260808\phoenix-v3-independent-data.exe
```

SHA-256:
`aa622efedb716ccd13ecae6a856307e0f355022067cf7cb66c686e473f8b2bd8`

Verification completed on `D:\phoenix-target-qps-v3-bundles`:

- grouped and existing independent-data tests: `22 passed`
- focused QPS split tests: `2 passed`
- warnings-denied Clippy: passed
- optimized release build: passed
- debug and release bundle audits: byte-identical

## Boundary

The remaining 5,479 packet pairs are candidates, not judgments. They become
training evidence only after an explicit per-challenger semantic decision.
Missing or uncertain decisions remain absent; they are never inferred from the
benchmark label, V2 position, feature vector, or packet selection policy.
