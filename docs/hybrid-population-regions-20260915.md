# Hybrid population regions

## Meaning

Hybrid is a hierarchy-first spherical display chart. Radial shells encode the
producer's containment role, not model confidence. Angular regions encode
branches; their area is proportional to subtree population. Semantic lanes and
roles group siblings within the branch, with stable identity as the tie-breaker.
Direct edges show connectivity, not a claim of hyperbolic distance.

The prior layout deliberately kept prototypes within 15.7–33.5 degrees of one
another and recursively narrowed their caps. That caused the off-center stack.
The replacement partitions the full longitude/height domain. Longitude times
height is an equal-area parameterization of the unit sphere. Children partition
their parent's region without overlapping siblings. A single child inherits its
parent region. Stable identity ordering makes slot permutation irrelevant;
population changes can legitimately resize affected regions.

## Implementation

- Dense CSR child storage; one sort, role-ordered subtree counting, weighted
  binary partitions, and contiguous output. No force solver, per-node heap
  objects, random jitter, or frame-time layout.
- Role radius is exact. Degree does not masquerade as confidence or depth.
- Only occupied hierarchy shells are drawn, plus the enclosing sphere. The
  Busemann math remains available as a diagnostic API; its confidence-style
  guides are no longer rendered for a hierarchy-only placement.
- Hybrid Fit content frames visible-node bounds. Other manifold fitting stays
  unchanged. The general Reset control retains its existing camera behavior.
- Hybrid continues to select two-endpoint straight paths.
- Projection identity changes to v3-hybrid-regions, forcing a new layout receipt.

## Qualification

Kernel tests cover both hemispheres/all axes, permutation invariance, weighted
area, bounds, hierarchy validation, determinism, and finite coordinates.
Publisher tests require guides to match occupied role radii. Existing renderer
tests cover content-centered camera fitting.

Optimized synthetic 25,000-node hierarchy, 3 warmups and 40 samples:
median 4.332 ms, p95 5.655 ms. This is layout-only on this machine, not an
end-to-end graph build time. No claim of improved extraction throughput.

Previous generation and executable are preserved at
`D:/phoenix-projections-20260915/before-hybrid-redesign`.
Release build succeeded; SHA-256:
`5FEAB8C93C0C6CC1406FD51EDCABD3ABF16AC6C7275182747A2CDEA5E310AF7A`.
Live UI Build graph published generation 12. All 6,941 nodes, 9,736 edges and
styles match generation 11. Only Hybrid positions changed; the other five
position pages match byte-for-byte. Hybrid has 27 shell guides.

Before: all nodes were in positive Z; centroid (-1.559, 3.023, 17.416).
After: octant populations [871, 877, 864, 881, 868, 855, 863, 862]; centroid
(-0.150, 3.053, 0.738). Axis bounds are (-30.470, -29.236, -23.761) to
(26.257, 29.236, 30.144). Maximum radius 31.280 remains within the world sphere.
These are occupancy measures, not semantic-accuracy scores.

Hybrid's 9,736 active paths each have exactly two endpoints, zero detachment and
zero overshoot. Test suite: 56 renderer + 8 renderer integration + 10 Hybrid +
9 publisher tests passed (83 total). Live app inspection confirmed the broad
spherical hierarchy and Fit content control. The app remains open on Hybrid.

One reused-analysis UI build took 318.255 ms from request to first frame
submission, including 14.436 ms renderer projection and 0.537 ms frame
submission. This is not fresh extraction or a percentile claim.
Receipts: `D:/phoenix-projections-20260915/hybrid-redesign-parity.log`,
`hybrid-occupancy-after.log`, `hybrid-redesign-edges.log`, `hybrid-stderr.log`.

## Scope limits

This does not connect the retired confidence/ambiguity producer or recover the
original Siegel/Finsler metric. Angular proximity reflects hierarchy and family
grouping, not measured embedding similarity. Role shells are not a calibrated
hyperbolic metric. A future confidence interior needs explicit producer data.
