//! Clean-room Hybrid manifold kernel for Phoenix Native.
//!
//! The kernel compiles a graph generation into a bounded Poincare ball.  A
//! node's semantic lane chooses a boundary prototype, while hierarchy depth
//! controls radial commitment.  The renderer receives only packed positions;
//! it never evaluates manifold math on the frame path.

use glam::Vec3;
pub use phoenix_scene_contract::CapsRole;
use std::f32::consts::PI;
use thiserror::Error;

pub const HYBRID_LAYOUT_CONTRACT: &str = "phoenix.native.hybrid-busemann/v1";
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

/// Compile a complete node page in one role-ordered hierarchy pass.
///
/// Radius comes only from the native hierarchy role. Parent direction defines
/// the center of each child cap, siblings receive stable golden-angle packing,
/// and semantic lane contributes only a small tangent bias inside that cap.
/// The renderer receives the final contiguous page and performs no hierarchy
/// work on the frame path.
pub fn layout(nodes: &[HybridNode]) -> Result<Vec<HybridPoint>, HybridLayoutError> {
    validate(nodes)?;
    let mut directions = vec![Vec3::ZERO; nodes.len()];

    for role in CapsRole::ALL {
        for (slot, node) in nodes.iter().enumerate() {
            if node.role != role {
                continue;
            }
            directions[slot] = match node.parent_slot {
                Some(parent) => {
                    child_direction(node, nodes[parent as usize], directions[parent as usize])
                }
                None => root_direction(node),
            };
        }
    }

    nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| {
            let radius = hierarchy_radius(node.role) + within_band_offset(node);
            let unit_position = directions[slot] * radius;
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

fn root_direction(node: &HybridNode) -> Vec3 {
    let center = node.lane.prototype();
    if node.sibling_count <= 1 {
        return center;
    }
    packed_cap_direction(node, center, root_cap_aperture(node.sibling_count), 0)
}

fn child_direction(node: &HybridNode, parent: HybridNode, parent_direction: Vec3) -> Vec3 {
    let center = parent_direction.normalize_or_zero();
    let center = if center.length_squared() > 0.5 {
        center
    } else {
        parent.lane.prototype()
    };
    packed_cap_direction(
        node,
        center,
        child_cap_aperture(parent.role, node.sibling_count),
        parent.stable_id,
    )
}

fn packed_cap_direction(node: &HybridNode, center: Vec3, aperture: f32, parent_id: u64) -> Vec3 {
    let count = node.sibling_count.max(1) as f32;
    let ordinal = (node.sibling_rank as f32 + 0.5) / count;
    let cos_theta = 1.0 - ordinal * (1.0 - aperture.cos());
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let (tangent_u, tangent_v) = tangent_basis(center);
    let phase =
        node.sibling_rank as f32 * 2.399_963_1 + stable_signed(parent_id ^ node.stable_id, 4) * PI;
    let sibling_tangent = tangent_u * phase.cos() + tangent_v * phase.sin();
    let lane = node.lane.prototype();
    let lane_tangent = (lane - center * lane.dot(center)).normalize_or_zero();
    let tangent = (sibling_tangent * 0.84 + lane_tangent * 0.16)
        .try_normalize()
        .unwrap_or(sibling_tangent);
    (center * cos_theta + tangent * sin_theta).normalize()
}

fn tangent_basis(direction: Vec3) -> (Vec3, Vec3) {
    let reference = if direction.y.abs() < 0.88 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent_u = direction.cross(reference).normalize();
    let tangent_v = tangent_u.cross(direction).normalize();
    (tangent_u, tangent_v)
}

fn root_cap_aperture(sibling_count: u32) -> f32 {
    population_aperture(sibling_count, 0.12, 0.42)
}

fn child_cap_aperture(parent_role: CapsRole, sibling_count: u32) -> f32 {
    let maximum = if parent_role <= CapsRole::Episode {
        0.52
    } else {
        0.24
    };
    population_aperture(sibling_count, 0.035, maximum)
}

fn population_aperture(sibling_count: u32, minimum: f32, maximum: f32) -> f32 {
    let coverage = ((sibling_count.max(1) as f32).ln_1p() / 8.0).clamp(0.0, 1.0);
    minimum + (maximum - minimum) * coverage
}

fn within_band_offset(node: &HybridNode) -> f32 {
    let stable = stable_signed(node.stable_id, 2) * 0.010;
    let degree = ((node.degree as f32 + 1.0).ln() * 0.0015).min(0.008);
    (stable + degree).clamp(-0.012, 0.014)
}

fn stable_signed(stable_id: u64, lane: u64) -> f32 {
    let mut value = stable_id ^ lane.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    let unit = (value >> 40) as f32 / ((1_u32 << 24) - 1) as f32;
    unit * 2.0 - 1.0
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
    fn semantic_roots_share_a_compact_front_hemisphere() {
        let nodes =
            HybridLane::ALL.map(|lane| node(lane as u64 + 10, lane, CapsRole::Entity, None));
        let points = layout(&nodes).unwrap_or_else(|error| panic!("Hybrid layout: {error}"));
        assert!(points.iter().all(|point| point.position[2] > 0.0));
        for left in 0..points.len() {
            for right in left + 1..points.len() {
                assert_ne!(points[left].position, points[right].position);
                let left = Vec3::from_array(points[left].unit_position).normalize();
                let right = Vec3::from_array(points[right].unit_position).normalize();
                assert!(left.dot(right) > 0.80);
            }
        }
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
