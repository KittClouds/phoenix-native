use crate::NativeSceneCompilerError;
use glam::Vec3;
use phoenix_scene_archive::PositionRecord;
use phoenix_scene_contract::{CapsRole, CAPS_KLEIN_BOUND, CAPS_WORLD_SCALE};
use std::f32::consts::PI;

const GOLDEN_ANGLE: f32 = 2.399_963_1;
const MIN_CAP_APERTURE: f32 = 0.055;
const CAP_RING_STEP: f32 = 0.032;
const MAX_CAP_APERTURE: f32 = 0.30;
const ROOT_CAP_MIN_APERTURE: f32 = 0.85;
const ROOT_CAP_MAX_APERTURE: f32 = PI - 0.12;
const ROOT_CAP_SATURATION: f32 = 6.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapsNode {
    pub stable_id: u64,
    pub role: CapsRole,
    pub parent_slot: Option<u32>,
    pub sibling_rank: u32,
    pub sibling_count: u32,
    pub membership_count: u16,
}

pub fn layout(nodes: &[CapsNode]) -> Result<Vec<PositionRecord>, NativeSceneCompilerError> {
    validate(nodes)?;
    let mut directions = vec![Vec3::ZERO; nodes.len()];

    // Roles are processed from abstract outer shells to concrete inner shells.
    // A parent can therefore appear later in node-slot order without forcing
    // pointer chasing, recursion, or a graph-sized temporary object model.
    for role in CapsRole::ALL {
        for (slot, node) in nodes.iter().enumerate() {
            if node.role != role {
                continue;
            }
            directions[slot] = match node.parent_slot {
                Some(parent_slot) => child_direction(
                    node,
                    nodes[parent_slot as usize],
                    directions[parent_slot as usize],
                ),
                None => root_direction(node),
            };
        }
    }

    directions
        .into_iter()
        .zip(nodes)
        .enumerate()
        .map(|(slot, (direction, node))| {
            let klein = lorentz_to_klein(direction, node.role.klein_radius());
            if !klein.is_finite() || klein.length() >= CAPS_KLEIN_BOUND {
                return Err(NativeSceneCompilerError::CapsProjectionInvalid { slot });
            }
            Ok(PositionRecord {
                position: (klein * CAPS_WORLD_SCALE).to_array(),
            })
        })
        .collect()
}

fn validate(nodes: &[CapsNode]) -> Result<(), NativeSceneCompilerError> {
    for (slot, node) in nodes.iter().enumerate() {
        if node.stable_id == 0 {
            return Err(NativeSceneCompilerError::CapsZeroIdentity { slot });
        }
        if node.sibling_count == 0 || node.sibling_rank >= node.sibling_count {
            return Err(NativeSceneCompilerError::CapsSiblingRange {
                slot,
                rank: node.sibling_rank,
                count: node.sibling_count,
            });
        }
        let Some(parent_slot) = node.parent_slot else {
            continue;
        };
        let parent = nodes.get(parent_slot as usize).ok_or(
            NativeSceneCompilerError::CapsParentOutOfRange {
                slot,
                parent: parent_slot,
            },
        )?;
        if parent_slot as usize == slot || parent.role >= node.role {
            return Err(NativeSceneCompilerError::CapsParentRole {
                slot,
                parent: parent_slot,
            });
        }
    }
    Ok(())
}

fn root_direction(node: &CapsNode) -> Vec3 {
    match node.role {
        CapsRole::Document => Vec3::Y,
        CapsRole::Episode => {
            // A slight stable tilt makes all three spatial axes legible from
            // the default camera while keeping the shell origin authoritative.
            Vec3::new(0.24 + signed_unit(node.stable_id, 0) * 0.05, 0.31, 0.92).normalize()
        }
        _ => fibonacci_direction(node.sibling_rank, node.sibling_count, node.stable_id),
    }
}

fn child_direction(node: &CapsNode, parent: CapsNode, parent_direction: Vec3) -> Vec3 {
    let center = parent_direction.normalize_or_zero();
    let fallback = root_direction(&parent);
    let center = if center.length_squared() > 0.5 {
        center
    } else {
        fallback
    };
    if parent.role <= CapsRole::Episode {
        return root_cap_direction(node, parent, center);
    }
    let (tangent_u, tangent_v) = tangent_basis(center);
    let ring = integer_ring(node.sibling_rank);
    let ambiguity = f32::from(node.membership_count.saturating_sub(1).min(8)) * 0.012;
    let aperture =
        (MIN_CAP_APERTURE + ring as f32 * CAP_RING_STEP + ambiguity).min(MAX_CAP_APERTURE);
    let stable_phase = signed_unit(parent.stable_id, 4) * PI;
    let angle = node.sibling_rank as f32 * GOLDEN_ANGLE
        + stable_phase
        + signed_unit(node.stable_id, 8) * 0.08;
    let tangent = tangent_u * angle.cos() + tangent_v * angle.sin();
    (center * aperture.cos() + tangent * aperture.sin()).normalize()
}

fn root_cap_direction(node: &CapsNode, parent: CapsNode, center: Vec3) -> Vec3 {
    // Document and episode descendants define the global CAPS chart. A
    // fixed narrow child aperture turns every real document into one dense
    // lobe regardless of node count, leaving the orthogonal Klein sections
    // as decorative scenery. Grow the spherical cap toward the full chart as
    // siblings accumulate, while retaining the parent axis and chronological
    // rank as stable semantic coordinates.
    let count = node.sibling_count.max(1) as f32;
    let coverage = count / (count + ROOT_CAP_SATURATION);
    let aperture =
        ROOT_CAP_MIN_APERTURE + coverage * (ROOT_CAP_MAX_APERTURE - ROOT_CAP_MIN_APERTURE);
    let ordinal = (node.sibling_rank as f32 + 0.5) / count;
    let cos_theta = 1.0 - ordinal * (1.0 - aperture.cos());
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let (tangent_u, tangent_v) = tangent_basis(center);
    let stable_phase = signed_unit(parent.stable_id, 4) * PI;
    let angle = node.sibling_rank as f32 * GOLDEN_ANGLE
        + stable_phase
        + signed_unit(node.stable_id, 8) * 0.08;
    let tangent = tangent_u * angle.cos() + tangent_v * angle.sin();
    (center * cos_theta + tangent * sin_theta).normalize()
}

fn tangent_basis(direction: Vec3) -> (Vec3, Vec3) {
    let reference = if direction.y.abs() < 0.88 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = direction.cross(reference).normalize();
    let v = u.cross(direction).normalize();
    (u, v)
}

fn fibonacci_direction(rank: u32, count: u32, stable_id: u64) -> Vec3 {
    let count = count.max(1) as f32;
    let y = 1.0 - 2.0 * (rank as f32 + 0.5) / count;
    let radial = (1.0 - y * y).max(0.0).sqrt();
    let phase = rank as f32 * GOLDEN_ANGLE + signed_unit(stable_id, 12) * 0.12;
    Vec3::new(phase.cos() * radial, y, phase.sin() * radial).normalize()
}

fn lorentz_to_klein(direction: Vec3, klein_radius: f32) -> Vec3 {
    // Build the H3 point on the unit hyperboloid, then project it into the
    // Klein ball. Keeping the construction explicit prevents a Euclidean
    // shell shortcut from quietly replacing the geometry later.
    let radius = klein_radius.clamp(0.0, CAPS_KLEIN_BOUND);
    let rho = radius.atanh();
    let time = rho.cosh();
    let spatial = direction.normalize_or_zero() * rho.sinh();
    spatial / time
}

fn integer_ring(rank: u32) -> u32 {
    // Ring capacities are 1, 6, 12, 18...; this loop is bounded by sibling
    // rank and runs once at publication time, never in the renderer.
    if rank == 0 {
        return 0;
    }
    let mut ring = 1_u32;
    let mut capacity = 1_u32;
    while capacity.saturating_add(ring.saturating_mul(6)) <= rank {
        capacity = capacity.saturating_add(ring.saturating_mul(6));
        ring = ring.saturating_add(1);
    }
    ring
}

fn signed_unit(stable_id: u64, lane: u8) -> f32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.native.caps-layout/v1\0");
    hasher.update(&stable_id.to_le_bytes());
    hasher.update(&[lane]);
    let mut raw = [0_u8; 4];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..4]);
    (u32::from_le_bytes(raw) as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        stable_id: u64,
        role: CapsRole,
        parent_slot: Option<u32>,
        sibling_rank: u32,
        sibling_count: u32,
    ) -> CapsNode {
        CapsNode {
            stable_id,
            role,
            parent_slot,
            sibling_rank,
            sibling_count,
            membership_count: 1,
        }
    }

    #[test]
    fn lorentz_projection_respects_role_shells_and_parent_caps() {
        let nodes = [
            node(1, CapsRole::Episode, None, 0, 1),
            node(2, CapsRole::Chunk, Some(0), 0, 2),
            node(3, CapsRole::Chunk, Some(0), 1, 2),
            node(4, CapsRole::Entity, Some(1), 0, 2),
            node(5, CapsRole::Entity, Some(1), 1, 2),
        ];
        let positions = layout(&nodes).unwrap_or_else(|error| panic!("{error}"));
        let radii = positions
            .iter()
            .map(|position| Vec3::from_array(position.position).length())
            .collect::<Vec<_>>();
        assert!((radii[0] - CapsRole::Episode.world_radius()).abs() < 0.001);
        assert!((radii[1] - CapsRole::Chunk.world_radius()).abs() < 0.001);
        assert!((radii[3] - CapsRole::Entity.world_radius()).abs() < 0.001);
        let chunk = Vec3::from_array(positions[1].position).normalize();
        for entity in &positions[3..] {
            assert!(chunk.dot(Vec3::from_array(entity.position).normalize()) > 0.93);
        }
    }

    #[test]
    fn malformed_parent_and_sibling_contracts_fail_closed() {
        let bad_parent = [node(1, CapsRole::Entity, Some(4), 0, 1)];
        assert!(matches!(
            layout(&bad_parent),
            Err(NativeSceneCompilerError::CapsParentOutOfRange { .. })
        ));
        let bad_order = [
            node(1, CapsRole::Entity, None, 0, 1),
            node(2, CapsRole::Chunk, Some(0), 0, 1),
        ];
        assert!(matches!(
            layout(&bad_order),
            Err(NativeSceneCompilerError::CapsParentRole { .. })
        ));
        let bad_sibling = [node(1, CapsRole::Episode, None, 1, 1)];
        assert!(matches!(
            layout(&bad_sibling),
            Err(NativeSceneCompilerError::CapsSiblingRange { .. })
        ));
    }

    #[test]
    fn integer_rings_are_bounded_and_monotonic() {
        let mut previous = 0;
        for rank in 0..10_000 {
            let ring = integer_ring(rank);
            assert!(ring >= previous);
            previous = ring;
        }
        assert!(previous < 64);
    }

    #[test]
    fn projection_is_bitwise_deterministic() {
        let nodes = [
            node(7, CapsRole::Episode, None, 0, 1),
            node(8, CapsRole::Chunk, Some(0), 0, 1),
            node(9, CapsRole::Entity, Some(1), 0, 1),
        ];
        assert_eq!(
            layout(&nodes).unwrap_or_else(|error| panic!("{error}")),
            layout(&nodes).unwrap_or_else(|error| panic!("{error}"))
        );
    }

    #[test]
    fn episode_children_occupy_the_global_caps_chart() {
        const CHUNKS: u32 = 128;
        let mut nodes = Vec::with_capacity(CHUNKS as usize + 1);
        nodes.push(node(1, CapsRole::Episode, None, 0, 1));
        for rank in 0..CHUNKS {
            nodes.push(node(
                10 + u64::from(rank),
                CapsRole::Chunk,
                Some(0),
                rank,
                CHUNKS,
            ));
        }
        let positions = layout(&nodes).unwrap_or_else(|error| panic!("{error}"));
        let directions = positions[1..]
            .iter()
            .map(|position| Vec3::from_array(position.position).normalize())
            .collect::<Vec<_>>();
        let centroid = directions.iter().copied().sum::<Vec3>() / CHUNKS as f32;
        assert!(
            centroid.length() < 0.14,
            "global chart collapsed into a lobe: {centroid:?}"
        );
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let (minimum, maximum) = directions.iter().map(|direction| direction.dot(axis)).fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
            assert!(minimum < -0.70, "negative axis coverage {minimum}");
            assert!(maximum > 0.70, "positive axis coverage {maximum}");
        }
    }

    #[test]
    fn local_descendants_remain_inside_their_parent_cap() {
        let nodes = [
            node(1, CapsRole::Episode, None, 0, 1),
            node(2, CapsRole::Chunk, Some(0), 0, 1),
            node(3, CapsRole::Entity, Some(1), 0, 3),
            node(4, CapsRole::Entity, Some(1), 1, 3),
            node(5, CapsRole::Entity, Some(1), 2, 3),
        ];
        let positions = layout(&nodes).unwrap_or_else(|error| panic!("{error}"));
        let chunk = Vec3::from_array(positions[1].position).normalize();
        for entity in &positions[2..] {
            assert!(chunk.dot(Vec3::from_array(entity.position).normalize()) > 0.95);
        }
    }

    #[test]
    fn node_slot_permutation_cannot_change_stable_caps_geometry() {
        let canonical = [
            node(7, CapsRole::Episode, None, 0, 1),
            node(8, CapsRole::Chunk, Some(0), 0, 1),
            node(9, CapsRole::Entity, Some(1), 0, 2),
            node(10, CapsRole::Entity, Some(1), 1, 2),
        ];
        let permuted = [
            node(10, CapsRole::Entity, Some(3), 1, 2),
            node(7, CapsRole::Episode, None, 0, 1),
            node(9, CapsRole::Entity, Some(3), 0, 2),
            node(8, CapsRole::Chunk, Some(1), 0, 1),
        ];
        let mut expected = canonical
            .iter()
            .zip(layout(&canonical).unwrap_or_else(|error| panic!("{error}")))
            .map(|(node, position)| (node.stable_id, position))
            .collect::<Vec<_>>();
        let mut actual = permuted
            .iter()
            .zip(layout(&permuted).unwrap_or_else(|error| panic!("{error}")))
            .map(|(node, position)| (node.stable_id, position))
            .collect::<Vec<_>>();
        expected.sort_unstable_by_key(|(stable_id, _)| *stable_id);
        actual.sort_unstable_by_key(|(stable_id, _)| *stable_id);
        assert_eq!(actual, expected);
    }
}
