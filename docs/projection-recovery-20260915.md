# Native projection recovery

Requested scope: recover the original CAPS and Siegel/Finsler visual language;
replace Transit with a stacked radial view inspired by the former native Siegel.
Preserve graph topology and the native packed-page renderer.

## Evidence and design

The original Angular references used a curated Embed target set. The current
Shortrun native publication contains 6,941 nodes and 9,736 edges, including
paragraphs and sentences. This cut keeps all products; visual comparison must
account for that inventory difference.

The retired native Siegel formula used global node ordinal for angle and radius,
and degree for height. Native Transit was an ordinal-based horizontal strip.
The candidate replaces both in the full native compiler with batch layouts:

- Siegel: eleven ordered semantic bands, parent-coherent neighborhoods, bounded
  depth, and lane guides. This is a semantic display chart. It does not claim
  restoration of the separate Siegel matrix/Finsler distance producer.
- Transit: twelve hierarchy-role levels with concentric radial tracks, stable
  parent/kind/identity ordering. These are hierarchy levels, not chapter-time
  levels; source-order semantics are not available in this layout input.
- CAPS: nested angular containment with thin semantic shells and ordered child
  rings. Guide count is bounded; eight reference shells each have three planes.
- Guides: readable opacity and width independent of the topology edge-opacity
  ceiling. Edge styling and inventory are preserved.

Geometry is compiled once into position/guide pages. No per-frame force layout
or model inference is introduced. Layout identity enters the projection digest.

## Qualification log

- First unit pass: renderer 56, scene compiler 22, scene contract 35, publisher
  7 tests passed (120 total). Includes slot-permutation invariance, invalid-parent
  rejection, containment, finite positions, camera/lens/picking contracts.
- Optimized benchmark, 25,000 nodes / 40 measured trials after 3 warmups:
  CAPS median 5.371 ms / p95 6.356 ms; Siegel 0.265 / 0.320 ms;
  Transit 0.761 / 0.928 ms. These are layout-only timings on this machine.
- `projection_parity` example compares actual before/after publications. It
  requires identical identities, topology, node/edge styles across every view;
  only CAPS, Siegel and Transit position pages may change.
- First release candidate SHA-256:
  `CE6303CBDF8B590678A316C49F20139FA752BA53180C91A06F5F372F7993A720`.
  UI Build graph produced generation 8 from saved analysis. The publication
  parity check passed for all six manifolds. New guide strokes: CAPS 49,
  Transit 24, Siegel 11. The untouched three position pages matched exactly.
- Generation 8 UI request to first graph frame submission: 244.469 ms;
  renderer projection 15.603 ms; frame CPU encode/submit 0.456 ms. This is one
  reused-analysis run, not an inference benchmark or a percentile guarantee.
- Viewed CAPS, Siegel, Transit in the real app. The expected ring/fan, band,
  and stacked radial compositions appeared. A second refinement adds explicit
  Full detail / Overview controls and stronger reference guides.
- Final projection candidate SHA-256:
  `98B1A9A78D5070FF3D80A94619F790A25D7F954CE9D28E9488762CB07DC9195E`.
  Generation 9 parity passed against generation 7: all 6,941 identities and
  9,736 edges/styles preserved in all six views; Hybrid/Torus/Hopf positions
  unchanged. Final app captures show CAPS, Transit and Siegel; the Overview
  toggle visibly changes the label and paragraph/sentence visibility.
  Evidence: `D:/phoenix-projections-20260915/live-overview-check.png` and
  `generation9-parity.log`. This is visual layout acceptance; the original
  matrix/Finsler metric producer remains a separate outstanding integration.

## Hybrid edge follow-up

The active Hybrid and Siegel styles use bundled paths. Their former ports scaled
each endpoint toward the world origin by 0.58 and added slot-derived Z jitter
of up to 2.5 world units. This creates long excursions for nearby nodes far from
the origin. The replacement uses an endpoint-relative X/Z corridor and ordered
Y interpolation. Every point stays inside the endpoints' axis-aligned box;
routes reverse symmetrically and translate with their nodes. Four points per
edge and the packed-page renderer are retained. Projection contract v2 forces
fresh publication identity. Regression tests cover distant short edges,
coincident nodes, reversal, translation and forward progress.

The `edge_route_audit` example checks actual archived endpoints and counts
out-of-bounds routes. Generation 10 had 9,736/9,736 Hybrid routes outside their
endpoint boxes (maximum axis excess 14.840260); Siegel had 8,683. Generation 11,
built through the live app, has zero overshoots and zero detached endpoints in
both views. All six topology/style inventories still match generation 7.
Publisher tests: 9 passed. Shell Overview test: 1 passed.
Bounded-route executable SHA-256:
`E8C302005D21AFDA216F703AC1CE99857597EFE3DFBC87D8AB4A46E410C39ADA`.

### Direct Hybrid edges

The user confirmed overshoot was gone but identified visible elbows. The
four-point bundled polyline retained two intermediate turns. Hybrid now selects
the existing StraightPaths page: exactly two node endpoints and one rendered
segment per edge, versus three previously. Siegel retains its bounded bundled
route. This selection applies to existing publications without reanalysis.
The archive audit now requires exactly two points for every active Hybrid path.
Direct-edge build succeeded, SHA-256
`C26DCC46E5F1642EEFD00B31B310749C852A9D11931418C40D1345291C8067A7`.
The generation 11 archive passes the two-point Hybrid assertion with zero
overshoot/detachment. App-core and scene-contract tests passed (91 total).
The user confirmed the kink fix worked in the live app.

## Artifact locations

- Build target: `D:/phoenix-target-atlas-runtime-shell-20260813`.
- C: junction: `C:/phoenix-bin/gliner25-shell-20260914`.
- Rollback binary and generation 7 archive/index/manifest:
  `D:/phoenix-projections-20260915/baseline`.
- Live workspace: `D:/phoenix-gliner25-live-20260914/shortrun/workspace.json`.

Do not present this checkpoint as full visual or mathematical qualification.
