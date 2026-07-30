# Clean-room Cut 9 release and performance lock

Date: 2026-07-29

Status: PASS for the frozen Phoenix Native cohort, with the explicit hardware
limits recorded below.

## Frozen authority

| Identity | Frozen value |
|---|---|
| Workspace | `C:\Users\shuga\AppData\Local\Phoenix\NativeShell\workspace-v1.json` |
| Document | `Shortrun B`, stable ID `7`, revision `1`, workspace revision `4` |
| Document hash | `dae0a8b09fcc733284a4aad2bc4f66641f259ab4a40b5a974b760396f46fe422` |
| Native document length | `159,412` characters |
| Published generation | `4` |
| Registry / NER revision | `4` |
| Graph-generation hash | `58e3641495ca18aa0e1be70b36d0a629db1cd881ffc8f92e630192fb43a44e06` |
| Scene archive hash | `c69e965d9598682e86472c45a1721070dbf0226bda0c1baaef2fa8362affe203` |
| Product-index hash | `2ec6ebe6c1a14b35e9faf4704e9bb0dd1532156c45f619f5ba0f6fe248e60cfb` |
| Atlas run receipt | `9a4eb959032c5f6b68ed188b8528fc8bd3b1e698ab28401d9d4227dcd4db585b` |
| Semantic digest | `5f1650d2c9186ecfd062a21c3408aca5200bfd4e1cb575602463d72cb05361fb` |
| Release binary | `C:\phoenix-bin\native-cut9-release-20260729\phoenix-shell-cut9.exe` |
| Release binary SHA-256 | `75A18926FC175D7EB22DE0A7B52B6C41AF50B97A07DD5FFB880E12ED7C6F6B01` |
| Producer binary | `C:\phoenix-bin\native-cut6-interaction-20260728\phoenix-analysis-bridge.exe` |
| Producer SHA-256 | `07F4A8D96C904A3D067CD2E457407A6509CDA6D87AD6FEB8D01D59D0123EE648` |
| NER model | `D:\hf-models\gliner-bi-base-v2.0-onnx` |
| NLI model | `D:\phoenix-models\modernbert-base-nli-onnx` |

The editor status bar reports `26,182 words / 152,009 chars`. That display count
is not the kernel authority length above; the release manifest freezes the
kernel document bytes and hash so the two measurement semantics cannot be
silently substituted.

## Exact cohort

| Product | Count |
|---|---:|
| Dynamic chunks | 393 |
| Sentences | 2,361 |
| Canonical entities | 65 |
| Evidence mentions | 1,101 |
| Candidate-only NLI adjudications | 57 |
| Receipt-backed promotions | 0 |
| Scene nodes | 4,936 |
| Scene edges | 5,971 |

The editor paint projection requested and applied `1,100` highlights with zero
unmapped paint requests. Paint spans are intentionally not the evidence
authority: valid overlapping or otherwise non-paintable evidence remains in the
1,101-record authority rather than being deleted to make the visual count match.

## Deterministic production proof

Two isolated fresh-process replays produced identical:

- chunk, sentence, entity, mention, NLI, promotion, node, and edge counts;
- structural-product hash;
- semantic-coordinator hash;
- NLI artifact hash;
- scene archive hash;
- semantic digest.

The complete analysis-envelope hashes differed because they include measured
timings and run-receipt telemetry. Those volatile fields are not treated as
semantic drift.

Cold restart reopened generation `4` and verified the same archive and product
index without rebuilding or selecting an older generation.

## Release soak

| Gate | Evidence | Result |
|---|---:|---|
| Warm manifold-switch CPU p95 | `0.738 ms` | PASS, limit `8 ms` |
| Manifold-switch present p95 | `1.654 ms` | PASS |
| UI interaction CPU p95 | `0.136 ms` | PASS, limit `16.7 ms` |
| Resize-to-present p95 | `15.690 ms` | PASS, limit `16.7 ms` |
| Resize-to-present max | `17.230 ms` | Informational maximum; p95 contract passes |
| Manifold switches | 200 | PASS |
| Drawer visibility cycles | 200 | PASS |
| Resize cycles | 80 | PASS |
| Interaction updates | 1,000 | PASS |
| Hover/picking | 1 probe / 1 hover | PASS |
| Private memory | `395,382,784` to `400,252,928` bytes | PASS, `+4,870,144` bytes |
| GPU allocation | `1,310,720` bytes, unchanged | PASS |
| Kernel command/event queue high-water | `1 / 1` | PASS |
| Graph queue high-water | `2` | PASS |
| Fallback count | `0` | PASS |
| JSON graph freight | `0` | PASS |
| Resident generations | `1` | PASS |
| Frame readback / texture copy | `0 / 0` | PASS |
| Renderer/surface/device recovery | `2 created / 2 dropped`, zero live after proof | PASS |

The release benchmark also passed the 25,000-node CAPS gate at `15.5494 ms`
p95 and the 200-switch benchmark at `0.057 ms` p95. Debug-profile benchmark
timings are not substituted for release evidence.

## Failure, cancellation, and authority gates

The workspace test suite covers:

- deterministic generation and stable IDs;
- exact source, chunk, entity, mention, and evidence bindings;
- candidate-only semantics and receipt-backed promotion;
- idempotent review decisions, stale-decision rejection, and restart recovery;
- bounded cancellation and queue saturation;
- corrupt, stale, unsupported, and oversized artifact rejection;
- immutable generation publication and manifest recovery;
- one resident scene authority and zero production fixture fallback.

`cargo test --workspace --lib --tests` passed. Formatting passed. Clippy passed
with warnings denied for the release shell and renderer surfaces.

## Live Phoenix Native authority

The exact frozen binary was launched with the frozen workspace, publication
root, producer, model roots, full-scene requirement, and release-manifest
verification. The live application showed:

- `Shortrun B`;
- 65 canonical Atlas entities;
- automatic entity highlighting;
- the resident CAPS graph;
- working drawer, manifold, fit, hover, and embedded-child clipping.

The live window was moved across all three connected displays and returned to
the primary display. The embedded graph remained clipped and rendered on every
display. All three displays report 120 DPI, so this machine cannot honestly
prove a mixed-DPI transition.

## Legacy quarantine

`phoenix-shell` has no dependency on `phoenix-legacy-bridge`. Production also
contains a compile-time rejection for `legacy-graph-adapter`, and architecture
tests reject legacy registration and JSON graph freight.

The legacy bridge remains only as a quarantined diagnostic and old-artifact
inspection harness. It has no kernel dependency, release dependency, or
publication authority.

The previous V1 publication root was preserved at:

`C:\Users\shuga\AppData\Local\Phoenix\NativeShell\scene-publications-v1.pre-cut9-v1-20260729`

There is no runtime compatibility switch.

## Evidence

- Immutable manifest:
  `D:\phoenix-cut9-20260729\live-release-manifest-final2-cut9.pnrm`
- Manifest SHA-256:
  `97526A25ADB29FB79C02177BCC8D1224DEAD8D61966DAFC54134A4ABA2DBE4ED`
- Soak log:
  `D:\phoenix-cut9-20260729\live-soak-final2-cut9.log`
- Soak-log SHA-256:
  `7C27EC2EE66519C6FFD35898EB29D4526EE29703A98A42B2FB4AB246597AD056`
- Cold-restart log:
  `D:\phoenix-cut9-20260729\live-cold-restart-final2-cut9.log`
- Cold-restart-log SHA-256:
  `36A09C0877DE6B218B3994C19F9FFDF13FB26ABAAC6B139E79FA8A035C6FF920`

## Honest limits

- Device recovery was proven by controlled destruction and recreation of the
  renderer, surface, and device while retaining the same resident generation.
  A physical GPU removal or driver reset was not forced.
- Corruption, oversized-page, cancellation, and queue-pressure tests operate on
  temporary test artifacts and bounded test queues; the live authority was not
  deliberately corrupted.
- Multi-monitor hosting was exercised, but mixed-DPI behavior remains unproven
  because every connected display reports 120 DPI.

These limits do not weaken the measured release gates; they delimit the hardware
faults that were not safely inducible on this machine.
