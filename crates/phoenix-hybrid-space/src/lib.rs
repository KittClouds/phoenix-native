//! Clean-room Hybrid manifold kernel for Phoenix Native.
//!
//! Hierarchy roles select radial shells; population-weighted branch regions
//! occupy the sphere. This is a display chart, not a confidence embedding.
//! All geometry is compiled once into packed position pages.

use glam::Vec3;
pub use phoenix_scene_contract::CapsRole;
mod regions;
use thiserror::Error;

pub const HYBRID_LAYOUT_CONTRACT: &str = "phoenix.native.hybrid-population-regions/v2";
pub const HYBRID_WORLD_RADIUS: f32 = 34.0;
pub const HYBRID_BALL_BOUND: f32 = 0.965;
const EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum HybridLane {
    Structure = 0,
    Facts = 1,
    Discourse = 2,
    Entities = 3,
}

impl HybridLane {
    pub const ALL: [Self; 4] = [
        Self::Structure,
        Self::Facts,
        Self::Discourse,
        Self::Entities,
    ];

    #[must_use]
    pub fn prototype(self) -> Vec3 {
        let offset = match self {
            Self::Structure => Vec3::new(-0.10, 0.18, 1.0),
            Self::Facts => Vec3::new(-0.30, -0.02, 1.0),
            Self::Discourse => Vec3::new(0.30, 0.02, 1.0),
            Self::Entities => Vec3::new(0.08, -0.26, 1.0),
        };
        offset.normalize()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HybridNode {
    pub stable_id: u64,
    pub lane: HybridLane,
    /// The native model's authoritative containment role.
    pub role: CapsRole,
    pub parent_slot: Option<u32>,
    pub sibling_rank: u32,
    pub sibling_count: u32,
    pub degree: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HybridPoint {
    pub position: [f32; 3],
    pub unit_position: [f32; 3],
    pub radius: f32,
    pub lane: HybridLane,
    pub busemann_score: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Horosphere {
    pub center: [f32; 3],
    pub radius: f32,
    pub lane: HybridLane,
    pub tau: f32,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HybridLayoutError {
    #[error("Hybrid node slot {slot} has a zero stable identity")]
    ZeroIdentity { slot: usize },
    #[error("Hybrid node slot {slot} has parent slot {parent} outside the node page")]
    ParentOutOfRange { slot: usize, parent: u32 },
    #[error("Hybrid node slot {slot} has parent slot {parent} at an invalid hierarchy role")]
    ParentRole { slot: usize, parent: u32 },
    #[error("Hybrid node slot {slot} has an invalid sibling range")]
    SiblingRange { slot: usize },
    #[error("Hybrid node slot {slot} produced a non-finite projection")]
    NonFinite { slot: usize },
}

/// Hierarchy-first equal-area spherical regions. Radius encodes containment
/// role, never confidence; direction encodes the population-weighted branch.
/// Busemann scores remain diagnostic only and do not drive this layout.
pub fn layout(nodes: &[HybridNode]) -> Result<Vec<HybridPoint>, HybridLayoutError> {
    validate(nodes)?;
    let directions = regions::directions(nodes);
    nodes
        .iter()
        .zip(directions)
        .enumerate()
        .map(|(slot, (node, direction))| {
            let radius = hierarchy_radius(node.role);
            let unit_position = direction * radius;
            let world_position = unit_position * HYBRID_WORLD_RADIUS;
            if !world_position.is_finite() {
                return Err(HybridLayoutError::NonFinite { slot });
            }
            Ok(HybridPoint {
                position: world_position.to_array(),
                unit_position: unit_position.to_array(),
                radius,
                lane: node.lane,
                busemann_score: busemann_score(unit_position, node.lane.prototype()),
            })
        })
        .collect()
}
/// Poincare unit-ball Busemann score.
///
/// `B_p(x) = ln(||p - x||^2 / (1 - ||x||^2))`; lower values mean stronger
/// commitment toward the boundary prototype.
#[must_use]
pub fn busemann_score(point: Vec3, prototype: Vec3) -> f32 {
    let point_norm_sq = point.length_squared().min(1.0 - EPSILON);
    let prototype = prototype.normalize_or_zero();
    let numerator = (prototype - point).length_squared().max(EPSILON);
    let denominator = (1.0 - point_norm_sq).max(EPSILON);
    (numerator / denominator).ln()
}

/// Euclidean representation of a Busemann level set tangent to the unit ball.
#[must_use]
pub fn horosphere(lane: HybridLane, tau: f32) -> Horosphere {
    let a = tau.exp();
    let radius = a / (1.0 + a);
    let center = lane.prototype() / (1.0 + a);
    Horosphere {
        center: (center * HYBRID_WORLD_RADIUS).to_array(),
        radius: radius * HYBRID_WORLD_RADIUS,
        lane,
        tau,
    }
}

fn validate(nodes: &[HybridNode]) -> Result<(), HybridLayoutError> {
    for (slot, node) in nodes.iter().enumerate() {
        if node.stable_id == 0 {
            return Err(HybridLayoutError::ZeroIdentity { slot });
        }
        if let Some(parent_slot) = node.parent_slot {
            let Some(parent) = nodes.get(parent_slot as usize) else {
                return Err(HybridLayoutError::ParentOutOfRange {
                    slot,
                    parent: parent_slot,
                });
            };
            if parent_slot as usize == slot || parent.role >= node.role {
                return Err(HybridLayoutError::ParentRole {
                    slot,
                    parent: parent_slot,
                });
            }
        }
        if node.sibling_count == 0 || node.sibling_rank >= node.sibling_count {
            return Err(HybridLayoutError::SiblingRange { slot });
        }
    }
    Ok(())
}

#[must_use]
pub const fn hierarchy_radius(role: CapsRole) -> f32 {
    const RADII: [f32; CapsRole::ALL.len()] = [
        0.18, 0.24, 0.31, 0.38, 0.46, 0.56, 0.64, 0.71, 0.78, 0.82, 0.87, 0.92,
    ];
    RADII[role as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(stable_id: u64, lane: HybridLane, role: CapsRole, parent: Option<u32>) -> HybridNode {
        HybridNode {
            stable_id,
            lane,
            role,
            parent_slot: parent,
            sibling_rank: 0,
            sibling_count: 1,
            degree: 3,
        }
    }

    #[test]
    fn hierarchy_is_nested_inside_the_bounded_ball() {
        let points = layout(&[
            node(1, HybridLane::Structure, CapsRole::Document, None),
            node(2, HybridLane::Structure, CapsRole::Chapter, Some(0)),
            node(3, HybridLane::Structure, CapsRole::Paragraph, Some(1)),
        ])
        .unwrap_or_else(|error| panic!("Hybrid layout: {error}"));
        assert!(points[0].radius < points[1].radius);
        assert!(points[1].radius < points[2].radius);
        assert!(points.iter().all(|point| point.radius <= HYBRID_BALL_BOUND));
    }

    #[test]
    fn authoritative_role_radii_are_strictly_nested() {
        for roles in CapsRole::ALL.windows(2) {
            assert!(hierarchy_radius(roles[0]) < hierarchy_radius(roles[1]));
        }
        assert!(hierarchy_radius(CapsRole::Memory) < HYBRID_BALL_BOUND);
    }

    #[test]
    fn parent_first_caps_work_when_parent_slots_appear_later() {
        let nodes = [
            node(2, HybridLane::Structure, CapsRole::Chapter, Some(1)),
            node(1, HybridLane::Structure, CapsRole::Document, None),
        ];
        let points = layout(&nodes).unwrap_or_else(|error| panic!("Hybrid layout: {error}"));
        let child = Vec3::from_array(points[0].unit_position).normalize();
        let parent = Vec3::from_array(points[1].unit_position).normalize();
        assert!(child.dot(parent) > 0.98);
    }

    #[test]
    fn busemann_commitment_strengthens_toward_a_prototype() {
        let prototype = HybridLane::Entities.prototype();
        let weak = busemann_score(prototype * 0.25, prototype);
        let strong = busemann_score(prototype * 0.88, prototype);
        assert!(strong < weak);
    }

    #[test]
    fn horosphere_is_tangent_to_the_world_shell() {
        for lane in HybridLane::ALL {
            for tau in [-3.0, -2.0, -1.0, 0.0] {
                let sphere = horosphere(lane, tau);
                let center = Vec3::from_array(sphere.center).length();
                assert!((center + sphere.radius - HYBRID_WORLD_RADIUS).abs() < 0.001);
            }
        }
    }

    #[test]
    fn layout_is_bitwise_deterministic() {
        let nodes = [
            node(42, HybridLane::Facts, CapsRole::Document, None),
            node(99, HybridLane::Entities, CapsRole::Entity, Some(0)),
        ];
        let first = layout(&nodes).unwrap_or_else(|error| panic!("first layout: {error}"));
        let second = layout(&nodes).unwrap_or_else(|error| panic!("second layout: {error}"));
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_invalid_parent_and_sibling_ranges() {
        let mut bad_parent = node(1, HybridLane::Facts, CapsRole::Fact, Some(4));
        assert!(matches!(
            layout(&[bad_parent]),
            Err(HybridLayoutError::ParentOutOfRange { .. })
        ));
        bad_parent.parent_slot = None;
        bad_parent.sibling_count = 0;
        assert!(matches!(
            layout(&[bad_parent]),
            Err(HybridLayoutError::SiblingRange { .. })
        ));

        let invalid_roles = [
            node(1, HybridLane::Facts, CapsRole::Fact, None),
            node(2, HybridLane::Facts, CapsRole::Evidence, Some(0)),
        ];
        assert!(matches!(
            layout(&invalid_roles),
            Err(HybridLayoutError::ParentRole { .. })
        ));
    }
}
