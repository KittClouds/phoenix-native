//! Clean-room Hopf-fibration layout kernel for Phoenix Native.
//!
//! Every displayed fiber is the stereographic projection of an exact phase
//! orbit in `S3`. Graph hierarchy chooses the base point on `S2`; stable node
//! identity chooses the phase on that fiber. The renderer receives only packed
//! positions and sampled guides, so no manifold math runs in the frame loop.

pub use phoenix_scene_contract::CapsRole;
use std::f32::consts::{PI, TAU};
use thiserror::Error;

pub const HOPF_LAYOUT_CONTRACT: &str = "phoenix.native.hopf-fibration/v1";
pub const HOPF_WORLD_SCALE: f32 = 2.0;
pub const HOPF_BASE_SPHERE_RADIUS: f32 = 3.6;
pub const HOPF_FIBER_SAMPLES: usize = 65;
pub const MAX_HOPF_FIBERS: usize = 96;
pub const MIN_HOPF_FIBERS: usize = 12;

const GOLDEN_ANGLE: f32 = PI * (3.0 - 2.236_068);
const MAX_BASE_Z: f32 = 0.92;
const MIN_BASE_Z: f32 = -0.72;
const SOUTH_POLE_EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HopfNode {
    pub stable_id: u64,
    /// Stable `VisualNodeKind` discriminant. Each kind owns a disjoint fiber
    /// band so low-volume semantic products cannot disappear beneath the
    /// structural majority.
    pub semantic_slot: u16,
    pub role: CapsRole,
    pub parent_slot: Option<u32>,
    pub sibling_rank: u32,
    pub sibling_count: u32,
    pub degree: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HopfPoint {
    pub position: [f32; 3],
    pub base_direction: [f32; 3],
    pub fiber_slot: u16,
    pub phase: f32,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HopfLayoutError {
    #[error("Hopf node slot {slot} has reserved identity zero")]
    ZeroIdentity { slot: usize },
    #[error("Hopf node slot {slot} has parent slot {parent} outside the node page")]
    ParentOutOfRange { slot: usize, parent: u32 },
    #[error("Hopf node slot {slot} cannot descend from parent slot {parent}")]
    ParentRole { slot: usize, parent: u32 },
    #[error("Hopf node slot {slot} has an invalid sibling range")]
    SiblingRange { slot: usize },
    #[error("Hopf node slot {slot} produced a non-finite projection")]
    NonFinite { slot: usize },
}

/// Compile a complete node page with hierarchy-stable fiber assignment.
pub fn layout(nodes: &[HopfNode]) -> Result<Vec<HopfPoint>, HopfLayoutError> {
    validate(nodes)?;
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let anchors = anchor_ids(nodes);
    let mut semantic_slots = nodes
        .iter()
        .map(|node| node.semantic_slot)
        .collect::<Vec<_>>();
    semantic_slots.sort_unstable();
    semantic_slots.dedup();
    let fibers = fiber_count(nodes.len());

    nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| {
            let semantic_rank = semantic_slots
                .binary_search(&node.semantic_slot)
                .expect("semantic slot was collected from the same node page");
            let band_start = semantic_rank * fibers / semantic_slots.len();
            let band_end = ((semantic_rank + 1) * fibers / semantic_slots.len())
                .max(band_start + 1)
                .min(fibers);
            let band_width = band_end - band_start;
            let anchor_bucket = stable_hash(anchors[slot]) as usize % band_width;
            let fiber_slot = band_start + anchor_bucket;
            let base_direction = base_direction(fiber_slot, fibers);
            let phase = node_phase(node);
            let position = fiber_point(base_direction, phase);
            if position.iter().any(|value| !value.is_finite()) {
                return Err(HopfLayoutError::NonFinite { slot });
            }
            Ok(HopfPoint {
                position,
                base_direction,
                fiber_slot: fiber_slot as u16,
                phase,
            })
        })
        .collect()
}

/// Number of sampled fibers used for a graph generation.
#[must_use]
pub const fn fiber_count(node_count: usize) -> usize {
    if node_count == 0 {
        return 0;
    }
    let scaled = node_count.div_ceil(64);
    let lower = if node_count < MIN_HOPF_FIBERS {
        node_count
    } else {
        MIN_HOPF_FIBERS
    };
    if scaled < lower {
        lower
    } else if scaled > MAX_HOPF_FIBERS {
        MAX_HOPF_FIBERS
    } else {
        scaled
    }
}

/// Deterministic spherical-Fibonacci sample on a pole-safe band of `S2`.
#[must_use]
pub fn base_direction(slot: usize, count: usize) -> [f32; 3] {
    let count = count.max(1);
    let t = (slot.min(count - 1) as f32 + 0.5) / count as f32;
    let z = MAX_BASE_Z + (MIN_BASE_Z - MAX_BASE_Z) * t;
    let radius = (1.0 - z * z).max(0.0).sqrt();
    let azimuth = slot as f32 * GOLDEN_ANGLE;
    [radius * azimuth.cos(), radius * azimuth.sin(), z]
}

/// One exact Hopf fiber sample after `S3 -> R3` stereographic projection.
#[must_use]
pub fn fiber_point(base: [f32; 3], phase: f32) -> [f32; 3] {
    let [x, y, z] = normalize(base);
    let z1 = ((1.0 + z) * 0.5).max(0.0).sqrt();
    let (z2_re, z2_im) = if z1 > SOUTH_POLE_EPSILON {
        (x / (2.0 * z1), y / (2.0 * z1))
    } else {
        (1.0, 0.0)
    };
    let (sin_phase, cos_phase) = phase.sin_cos();
    let x1 = z1 * cos_phase;
    let x2 = z1 * sin_phase;
    let x3 = z2_re * cos_phase - z2_im * sin_phase;
    let x4 = z2_re * sin_phase + z2_im * cos_phase;
    let inverse = 1.0 / (1.0 - x4).max(SOUTH_POLE_EPSILON);
    // Rotate the projected axes so the fiber family reads clearly in the
    // graph camera's default orientation.
    [
        x1 * inverse * HOPF_WORLD_SCALE,
        x3 * inverse * HOPF_WORLD_SCALE,
        x2 * inverse * HOPF_WORLD_SCALE,
    ]
}

#[must_use]
pub fn sphere_point(latitude: f32, longitude: f32) -> [f32; 3] {
    let radius = latitude.cos() * HOPF_BASE_SPHERE_RADIUS;
    [
        radius * longitude.cos(),
        latitude.sin() * HOPF_BASE_SPHERE_RADIUS,
        radius * longitude.sin(),
    ]
}

fn validate(nodes: &[HopfNode]) -> Result<(), HopfLayoutError> {
    for (slot, node) in nodes.iter().enumerate() {
        if node.stable_id == 0 {
            return Err(HopfLayoutError::ZeroIdentity { slot });
        }
        if node.sibling_count == 0 || node.sibling_rank >= node.sibling_count {
            return Err(HopfLayoutError::SiblingRange { slot });
        }
        if let Some(parent_slot) = node.parent_slot {
            let Some(parent) = nodes.get(parent_slot as usize) else {
                return Err(HopfLayoutError::ParentOutOfRange {
                    slot,
                    parent: parent_slot,
                });
            };
            if parent_slot as usize == slot || parent.role >= node.role {
                return Err(HopfLayoutError::ParentRole {
                    slot,
                    parent: parent_slot,
                });
            }
        }
    }
    Ok(())
}

fn anchor_ids(nodes: &[HopfNode]) -> Vec<u64> {
    nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| {
            if is_fiber_anchor(node.role) || node.parent_slot.is_none() {
                return node.stable_id;
            }
            let mut cursor = slot;
            loop {
                let current = nodes[cursor];
                if is_fiber_anchor(current.role) || current.parent_slot.is_none() {
                    break current.stable_id;
                }
                cursor = current.parent_slot.expect("validated parent") as usize;
            }
        })
        .collect()
}

const fn is_fiber_anchor(role: CapsRole) -> bool {
    matches!(
        role,
        CapsRole::Document | CapsRole::Chapter | CapsRole::Episode | CapsRole::Entity
    )
}

fn node_phase(node: &HopfNode) -> f32 {
    let sibling = (node.sibling_rank as f32 + 0.5) / node.sibling_count.max(1) as f32;
    let stable = stable_unit(node.stable_id ^ u64::from(node.semantic_slot).rotate_left(17));
    let hierarchy = node.role as u8 as f32 * 0.173;
    let degree = (node.degree as f32 + 1.0).ln() * 0.013;
    (TAU * (sibling * 0.42 + stable * 0.58) + hierarchy + degree).rem_euclid(TAU)
}

fn normalize([x, y, z]: [f32; 3]) -> [f32; 3] {
    let inverse = 1.0 / (x * x + y * y + z * z).sqrt().max(SOUTH_POLE_EPSILON);
    [x * inverse, y * inverse, z * inverse]
}

fn stable_hash(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    value
}

fn stable_unit(value: u64) -> f32 {
    (stable_hash(value) >> 40) as f32 / ((1_u32 << 24) - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(stable_id: u64, role: CapsRole, parent_slot: Option<u32>) -> HopfNode {
        HopfNode {
            stable_id,
            semantic_slot: role as u16,
            role,
            parent_slot,
            sibling_rank: 0,
            sibling_count: 1,
            degree: 3,
        }
    }

    #[test]
    fn projected_fiber_round_trips_to_one_base_point() {
        let base = base_direction(31, MAX_HOPF_FIBERS);
        for sample in 0..HOPF_FIBER_SAMPLES - 1 {
            let phase = sample as f32 * TAU / (HOPF_FIBER_SAMPLES - 1) as f32;
            let recovered = hopf_base_from_projected(fiber_point(base, phase));
            for axis in 0..3 {
                assert!((recovered[axis] - base[axis]).abs() < 2.0e-4);
            }
        }
    }

    #[test]
    fn same_semantic_descendants_share_their_authoritative_anchor_fiber() {
        let mut nodes = [
            node(1, CapsRole::Document, None),
            node(2, CapsRole::Chapter, Some(0)),
            node(3, CapsRole::Paragraph, Some(1)),
            node(4, CapsRole::Sentence, Some(2)),
            node(5, CapsRole::Episode, Some(0)),
            node(6, CapsRole::Event, Some(4)),
        ];
        nodes[2].semantic_slot = nodes[1].semantic_slot;
        nodes[3].semantic_slot = nodes[1].semantic_slot;
        nodes[5].semantic_slot = nodes[4].semantic_slot;
        let points = layout(&nodes).expect("layout");
        assert_eq!(points[1].fiber_slot, points[2].fiber_slot);
        assert_eq!(points[2].fiber_slot, points[3].fiber_slot);
        assert_eq!(points[4].fiber_slot, points[5].fiber_slot);
        assert_ne!(points[1].fiber_slot, points[4].fiber_slot);
    }

    #[test]
    fn semantic_kinds_receive_disjoint_fiber_bands() {
        let mut nodes = Vec::with_capacity(48);
        for slot in 0..48_u64 {
            nodes.push(HopfNode {
                stable_id: slot + 1,
                semantic_slot: if slot < 24 { 40 } else { 44 },
                role: if slot < 24 {
                    CapsRole::Fact
                } else {
                    CapsRole::Memory
                },
                parent_slot: None,
                sibling_rank: 0,
                sibling_count: 1,
                degree: 1,
            });
        }
        let points = layout(&nodes).expect("layout");
        let first_max = points[..24]
            .iter()
            .map(|point| point.fiber_slot)
            .max()
            .expect("first semantic band");
        let second_min = points[24..]
            .iter()
            .map(|point| point.fiber_slot)
            .min()
            .expect("second semantic band");
        assert!(first_max < second_min);
    }

    #[test]
    fn shortrun_scale_layout_is_finite_and_bounded() {
        let nodes = (0..6_867)
            .map(|slot| HopfNode {
                stable_id: slot as u64 + 1,
                semantic_slot: (slot % 8) as u16,
                role: CapsRole::Entity,
                parent_slot: None,
                sibling_rank: 0,
                sibling_count: 1,
                degree: (slot % 64) as u32,
            })
            .collect::<Vec<_>>();
        let points = layout(&nodes).expect("layout");
        assert_eq!(fiber_count(nodes.len()), MAX_HOPF_FIBERS);
        assert!(points.iter().all(|point| point
            .position
            .iter()
            .all(|value| value.is_finite() && value.abs() < 40.0)));
    }

    fn hopf_base_from_projected([x, y, z]: [f32; 3]) -> [f32; 3] {
        let x = x / HOPF_WORLD_SCALE;
        let x2 = z / HOPF_WORLD_SCALE;
        let x3 = y / HOPF_WORLD_SCALE;
        let norm_sq = x * x + x2 * x2 + x3 * x3;
        let denominator = norm_sq + 1.0;
        let s3 = [
            2.0 * x / denominator,
            2.0 * x2 / denominator,
            2.0 * x3 / denominator,
            (norm_sq - 1.0) / denominator,
        ];
        [
            2.0 * (s3[0] * s3[2] + s3[1] * s3[3]),
            2.0 * (s3[0] * s3[3] - s3[1] * s3[2]),
            s3[0] * s3[0] + s3[1] * s3[1] - s3[2] * s3[2] - s3[3] * s3[3],
        ]
    }
}
