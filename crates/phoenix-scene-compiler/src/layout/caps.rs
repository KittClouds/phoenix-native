use crate::NativeSceneCompilerError;
use glam::Vec3;
use phoenix_scene_archive::PositionRecord;
use phoenix_scene_contract::{CapsRole, VisualNodeKind, CAPS_KLEIN_BOUND, CAPS_WORLD_SCALE};
use std::f32::consts::PI;

const GOLDEN_ANGLE: f32 = 2.399_963_1;
const SUPER_ROOT_APERTURE: f32 = PI - 0.12;
const CAP_MARGIN: f32 = 0.006;
const MAX_LAYOUT_GUIDES: usize = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapsNode {
    pub stable_id: u64,
    pub role: CapsRole,
    pub semantic_kind: VisualNodeKind,
    pub parent_slot: Option<u32>,
    pub sibling_rank: u32,
    pub sibling_count: u32,
    pub membership_count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapsGuide {
    pub stable_id: u64,
    pub center: [f32; 3],
    pub aperture: f32,
    pub radius: f32,
    pub role: CapsRole,
    pub weight: u32,
}

#[derive(Debug, PartialEq)]
pub struct CapsLayout {
    pub positions: Vec<PositionRecord>,
    pub guides: Vec<CapsGuide>,
}

#[derive(Clone, Copy, Debug)]
struct CapFrame {
    center: Vec3,
    aperture: f32,
}

impl Default for CapFrame {
    fn default() -> Self {
        Self {
            center: Vec3::Z,
            aperture: 0.0,
        }
    }
}

struct ChildIndex {
    offsets: Vec<usize>,
    slots: Vec<usize>,
}

impl ChildIndex {
    fn build(nodes: &[CapsNode]) -> Self {
        let mut offsets = vec![0_usize; nodes.len() + 1];
        for node in nodes {
            if let Some(parent) = node.parent_slot {
                offsets[parent as usize + 1] += 1;
            }
        }
        for slot in 1..offsets.len() {
            offsets[slot] += offsets[slot - 1];
        }
        let mut cursors = offsets[..nodes.len()].to_vec();
        let mut slots = vec![0_usize; offsets[nodes.len()]];
        for (slot, node) in nodes.iter().enumerate() {
            if let Some(parent) = node.parent_slot {
                let cursor = &mut cursors[parent as usize];
                slots[*cursor] = slot;
                *cursor += 1;
            }
        }
        for parent in 0..nodes.len() {
            slots[offsets[parent]..offsets[parent + 1]].sort_unstable_by_key(|slot| {
                (
                    nodes[*slot].semantic_kind as u8,
                    stable_hash(nodes[*slot].stable_id, 19),
                    nodes[*slot].stable_id,
                )
            });
        }
        Self { offsets, slots }
    }

    fn children(&self, parent: usize) -> &[usize] {
        &self.slots[self.offsets[parent]..self.offsets[parent + 1]]
    }
}

pub fn layout(nodes: &[CapsNode]) -> Result<Vec<PositionRecord>, NativeSceneCompilerError> {
    layout_with_guides(nodes).map(|layout| layout.positions)
}

/// Build a deterministic nested-cap mosaic in the Klein ball.
///
/// A virtual super-root allocates disjoint chart regions to authoritative
/// roots. Every parent then allocates subtree-weighted child caps in its local
/// tangent disk. Role controls a nonzero depth interval instead of an exact
/// Euclidean sphere, so the graph occupies the interior volume of H3.
pub fn layout_with_guides(nodes: &[CapsNode]) -> Result<CapsLayout, NativeSceneCompilerError> {
    validate(nodes)?;
    if nodes.is_empty() {
        return Ok(CapsLayout {
            positions: Vec::new(),
            guides: Vec::new(),
        });
    }

    let children = ChildIndex::build(nodes);
    let weights = subtree_weights(nodes)?;
    let mut frames = vec![CapFrame::default(); nodes.len()];
    let mut roots = nodes
        .iter()
        .enumerate()
        .filter_map(|(slot, node)| node.parent_slot.is_none().then_some(slot))
        .collect::<Vec<_>>();
    roots.sort_unstable_by_key(|slot| {
        (
            nodes[*slot].semantic_kind as u8,
            stable_hash(nodes[*slot].stable_id, 23),
            nodes[*slot].stable_id,
        )
    });

    let super_root = CapFrame {
        center: Vec3::new(0.18, 0.31, 0.93).normalize(),
        aperture: SUPER_ROOT_APERTURE,
    };
    allocate_group(&roots, super_root, nodes, &weights, &mut frames, true);

    // Parent roles are strictly ordered before child roles by contract. This
    // gives a cache-linear hierarchy pass without recursion or pointer trees.
    for role in CapsRole::ALL {
        for (parent, node) in nodes.iter().enumerate() {
            if node.role != role {
                continue;
            }
            let child_slots = children.children(parent);
            if !child_slots.is_empty() {
                allocate_group(
                    child_slots,
                    frames[parent],
                    nodes,
                    &weights,
                    &mut frames,
                    false,
                );
            }
        }
    }

    let positions = nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| {
            let depth = volumetric_depth(*node);
            let klein = lorentz_to_klein(frames[slot].center, depth);
            if !klein.is_finite() || klein.length() >= CAPS_KLEIN_BOUND {
                return Err(NativeSceneCompilerError::CapsProjectionInvalid { slot });
            }
            Ok(PositionRecord {
                position: (klein * CAPS_WORLD_SCALE).to_array(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut guide_slots = (0..nodes.len())
        .filter(|slot| !children.children(*slot).is_empty())
        .collect::<Vec<_>>();
    guide_slots.sort_unstable_by(|left, right| {
        weights[*right]
            .cmp(&weights[*left])
            .then_with(|| nodes[*left].stable_id.cmp(&nodes[*right].stable_id))
    });
    guide_slots.truncate(MAX_LAYOUT_GUIDES);
    let guides = guide_slots
        .into_iter()
        .map(|slot| CapsGuide {
            stable_id: nodes[slot].stable_id,
            center: frames[slot].center.to_array(),
            aperture: frames[slot].aperture,
            radius: Vec3::from_array(positions[slot].position).length(),
            role: nodes[slot].role,
            weight: u32::try_from(weights[slot]).unwrap_or(u32::MAX),
        })
        .collect();

    Ok(CapsLayout { positions, guides })
}

fn validate(nodes: &[CapsNode]) -> Result<(), NativeSceneCompilerError> {
    for (slot, node) in nodes.iter().enumerate() {
        if node.stable_id == 0 {
            return Err(NativeSceneCompilerError::CapsZeroIdentity { slot });
        }
        if semantic_role(node.semantic_kind) != Some(node.role) {
            return Err(NativeSceneCompilerError::CapsSemanticRole {
                slot,
                role: node.role,
                kind: node.semantic_kind,
            });
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

const fn semantic_role(kind: VisualNodeKind) -> Option<CapsRole> {
    match kind {
        VisualNodeKind::EntityCharacter
        | VisualNodeKind::EntityLocation
        | VisualNodeKind::EntityNetwork
        | VisualNodeKind::EntityCreature
        | VisualNodeKind::EntityNpc
        | VisualNodeKind::EntityEvent
        | VisualNodeKind::EntityConcept
        | VisualNodeKind::EntityOther => Some(CapsRole::Entity),
        VisualNodeKind::Document => Some(CapsRole::Document),
        VisualNodeKind::Episode => Some(CapsRole::Episode),
        VisualNodeKind::Chapter => Some(CapsRole::Chapter),
        VisualNodeKind::Paragraph => Some(CapsRole::Paragraph),
        VisualNodeKind::Sentence => Some(CapsRole::Sentence),
        VisualNodeKind::Chunk => Some(CapsRole::Chunk),
        VisualNodeKind::Evidence => Some(CapsRole::Evidence),
        VisualNodeKind::EventFact => Some(CapsRole::Event),
        VisualNodeKind::RelationshipFact
        | VisualNodeKind::TemporalFact
        | VisualNodeKind::CausalFact => Some(CapsRole::Fact),
        VisualNodeKind::MemoryStateFact => Some(CapsRole::Memory),
        VisualNodeKind::IdentityDiscourse | VisualNodeKind::ContextualDiscourse => {
            Some(CapsRole::Discourse)
        }
        VisualNodeKind::Unknown => None,
    }
}

fn subtree_weights(nodes: &[CapsNode]) -> Result<Vec<u64>, NativeSceneCompilerError> {
    let mut weights = vec![1_u64; nodes.len()];
    for role in CapsRole::ALL.into_iter().rev() {
        for (slot, node) in nodes.iter().enumerate() {
            if node.role != role {
                continue;
            }
            if let Some(parent) = node.parent_slot {
                weights[parent as usize] =
                    weights[parent as usize].checked_add(weights[slot]).ok_or(
                        NativeSceneCompilerError::RangeOverflow("CAPS subtree weight"),
                    )?;
            }
        }
    }
    Ok(weights)
}

fn allocate_group(
    slots: &[usize],
    parent: CapFrame,
    nodes: &[CapsNode],
    weights: &[u64],
    frames: &mut [CapFrame],
    root_group: bool,
) {
    if slots.is_empty() {
        return;
    }
    let total = slots
        .iter()
        .map(|slot| weights[*slot] as f64)
        .sum::<f64>()
        .max(1.0);
    let mut cumulative = 0.0_f64;
    for (ordinal, slot) in slots.iter().copied().enumerate() {
        let weight = weights[slot] as f64;
        let share = (weight / total) as f32;
        let aperture = child_aperture(
            parent.aperture,
            nodes[slot].role,
            share,
            slots.len(),
            root_group,
        );
        let available = (parent.aperture - aperture - CAP_MARGIN).max(0.0);
        let midpoint = ((cumulative + weight * 0.5) / total) as f32;
        let ambiguity = f32::from(nodes[slot].membership_count.saturating_sub(1).min(8));
        let radial = if slots.len() == 1 {
            0.0
        } else {
            (midpoint.sqrt() * available * (0.92 + ambiguity * 0.008)).min(available)
        };
        let angle = ordinal as f32 * GOLDEN_ANGLE
            + stable_signed(nodes[slot].stable_id, 31) * PI
            + nodes[slot].sibling_rank as f32 * 0.013;
        frames[slot] = CapFrame {
            center: cap_direction(parent.center, radial, angle),
            aperture,
        };
        cumulative += weight;
    }
}

fn child_aperture(parent: f32, role: CapsRole, share: f32, count: usize, root_group: bool) -> f32 {
    let role_limit = role_aperture_limit(role);
    if count == 1 {
        return (parent * if root_group { 0.92 } else { 0.58 }).min(role_limit);
    }
    let scale = if root_group { 0.74 } else { 0.68 };
    let desired = parent * share.sqrt() * scale;
    let maximum_factor = if root_group {
        0.46 + share * 0.42
    } else {
        0.30 + share * 0.34
    };
    let minimum = (parent * 0.018).clamp(0.004, 0.024);
    desired
        .clamp(minimum, parent * maximum_factor)
        .min(role_limit)
}

const fn role_aperture_limit(role: CapsRole) -> f32 {
    match role {
        CapsRole::Document => 2.72,
        CapsRole::Chapter => 0.72,
        CapsRole::Paragraph => 0.46,
        CapsRole::Sentence => 0.30,
        CapsRole::Episode => 0.82,
        CapsRole::Chunk => 0.50,
        CapsRole::Evidence => 0.32,
        CapsRole::Event => 0.30,
        CapsRole::Fact => 0.28,
        CapsRole::Discourse => 0.26,
        CapsRole::Entity => 0.24,
        CapsRole::Memory => 0.18,
    }
}

fn cap_direction(center: Vec3, offset: f32, angle: f32) -> Vec3 {
    if offset <= f32::EPSILON {
        return center;
    }
    let (u, v) = tangent_basis(center);
    let tangent = u * angle.cos() + v * angle.sin();
    (center * offset.cos() + tangent * offset.sin()).normalize()
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

fn volumetric_depth(node: CapsNode) -> f32 {
    let [near, far] = node.role.klein_depth_range();
    let stable = stable_unit(node.stable_id, 41);
    let membership = f32::from(node.membership_count.saturating_sub(1).min(8)) / 8.0;
    let fill = (0.10 + stable * 0.78 + membership * 0.08).min(0.96);
    let (semantic_band, semantic_band_count) = semantic_depth_band(node.semantic_kind, node.role);
    let band_width = (far - near) / f32::from(semantic_band_count);
    near + band_width * (f32::from(semantic_band) + fill)
}

const fn semantic_depth_band(kind: VisualNodeKind, role: CapsRole) -> (u8, u8) {
    match role {
        CapsRole::Fact => match kind {
            VisualNodeKind::RelationshipFact => (0, 3),
            VisualNodeKind::TemporalFact => (1, 3),
            VisualNodeKind::CausalFact => (2, 3),
            _ => (0, 1),
        },
        CapsRole::Discourse => match kind {
            VisualNodeKind::IdentityDiscourse => (0, 2),
            VisualNodeKind::ContextualDiscourse => (1, 2),
            _ => (0, 1),
        },
        CapsRole::Entity => match kind {
            VisualNodeKind::EntityCharacter => (0, 8),
            VisualNodeKind::EntityLocation => (1, 8),
            VisualNodeKind::EntityNetwork => (2, 8),
            VisualNodeKind::EntityCreature => (3, 8),
            VisualNodeKind::EntityNpc => (4, 8),
            VisualNodeKind::EntityEvent => (5, 8),
            VisualNodeKind::EntityConcept => (6, 8),
            VisualNodeKind::EntityOther => (7, 8),
            _ => (0, 1),
        },
        _ => (0, 1),
    }
}

fn lorentz_to_klein(direction: Vec3, klein_radius: f32) -> Vec3 {
    // Construct a point on the H3 hyperboloid and project it into the Klein
    // ball. Geodesics remain straight in this chart, which is CAPS' visual
    // signature and deliberately differs from Hybrid's horospheres.
    let radius = klein_radius.clamp(0.0, CAPS_KLEIN_BOUND);
    let rho = radius.atanh();
    let time = rho.cosh();
    let spatial = direction.normalize_or_zero() * rho.sinh();
    spatial / time
}

fn stable_hash(stable_id: u64, lane: u64) -> u64 {
    let mut value = stable_id ^ lane.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn stable_unit(stable_id: u64, lane: u64) -> f32 {
    (stable_hash(stable_id, lane) >> 40) as f32 / ((1_u32 << 24) - 1) as f32
}

fn stable_signed(stable_id: u64, lane: u64) -> f32 {
    stable_unit(stable_id, lane) * 2.0 - 1.0
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
        let semantic_kind = match role {
            CapsRole::Document => VisualNodeKind::Document,
            CapsRole::Chapter => VisualNodeKind::Chapter,
            CapsRole::Paragraph => VisualNodeKind::Paragraph,
            CapsRole::Sentence => VisualNodeKind::Sentence,
            CapsRole::Episode => VisualNodeKind::Episode,
            CapsRole::Chunk => VisualNodeKind::Chunk,
            CapsRole::Evidence => VisualNodeKind::Evidence,
            CapsRole::Event => VisualNodeKind::EventFact,
            CapsRole::Fact => VisualNodeKind::RelationshipFact,
            CapsRole::Discourse => VisualNodeKind::IdentityDiscourse,
            CapsRole::Entity => VisualNodeKind::EntityOther,
            CapsRole::Memory => VisualNodeKind::MemoryStateFact,
        };
        CapsNode {
            stable_id,
            role,
            semantic_kind,
            parent_slot,
            sibling_rank,
            sibling_count,
            membership_count: 1,
        }
    }

    #[test]
    fn layout_uses_role_volumes_and_nested_parent_caps() {
        let nodes = [
            node(1, CapsRole::Document, None, 0, 1),
            node(2, CapsRole::Chapter, Some(0), 0, 1),
            node(3, CapsRole::Paragraph, Some(1), 0, 2),
            node(4, CapsRole::Paragraph, Some(1), 1, 2),
            node(5, CapsRole::Sentence, Some(2), 0, 1),
        ];
        let result = layout_with_guides(&nodes).unwrap_or_else(|error| panic!("{error}"));
        for (node, position) in nodes.iter().zip(&result.positions) {
            let radius = Vec3::from_array(position.position).length() / CAPS_WORLD_SCALE;
            let [near, far] = node.role.klein_depth_range();
            assert!((near..=far).contains(&radius));
        }
        assert_eq!(result.guides.len(), 3);
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
        let mut bad_semantic = node(3, CapsRole::Fact, None, 0, 1);
        bad_semantic.semantic_kind = VisualNodeKind::IdentityDiscourse;
        assert!(matches!(
            layout(&[bad_semantic]),
            Err(NativeSceneCompilerError::CapsSemanticRole { .. })
        ));
        let bad_sibling = [node(1, CapsRole::Episode, None, 1, 1)];
        assert!(matches!(
            layout(&bad_sibling),
            Err(NativeSceneCompilerError::CapsSiblingRange { .. })
        ));
    }

    #[test]
    fn fact_and_discourse_subtypes_occupy_distinct_semantic_depths() {
        let mut relationship = node(21, CapsRole::Fact, None, 0, 3);
        relationship.semantic_kind = VisualNodeKind::RelationshipFact;
        let mut temporal = node(22, CapsRole::Fact, None, 1, 3);
        temporal.semantic_kind = VisualNodeKind::TemporalFact;
        let mut causal = node(23, CapsRole::Fact, None, 2, 3);
        causal.semantic_kind = VisualNodeKind::CausalFact;
        let mut identity = node(31, CapsRole::Discourse, None, 0, 2);
        identity.semantic_kind = VisualNodeKind::IdentityDiscourse;
        let mut contextual = node(32, CapsRole::Discourse, None, 1, 2);
        contextual.semantic_kind = VisualNodeKind::ContextualDiscourse;
        let result = layout(&[relationship, temporal, causal, identity, contextual])
            .unwrap_or_else(|error| panic!("semantic CAPS: {error}"));
        let radii = result
            .iter()
            .map(|position| Vec3::from_array(position.position).length())
            .collect::<Vec<_>>();
        assert!(radii[0] < radii[1] && radii[1] < radii[2]);
        assert!(radii[3] < radii[4]);
    }

    #[test]
    fn one_role_occupies_radial_volume_instead_of_a_shell() {
        let mut nodes = Vec::with_capacity(129);
        nodes.push(node(1, CapsRole::Document, None, 0, 1));
        for rank in 0..128 {
            nodes.push(node(
                10 + u64::from(rank),
                CapsRole::Chapter,
                Some(0),
                rank,
                128,
            ));
        }
        let positions = layout(&nodes).unwrap_or_else(|error| panic!("{error}"));
        let mut radii = positions[1..]
            .iter()
            .map(|position| Vec3::from_array(position.position).length())
            .collect::<Vec<_>>();
        radii.sort_unstable_by(f32::total_cmp);
        assert!(radii.last().unwrap() - radii.first().unwrap() > 1.5);
    }

    #[test]
    fn document_chart_uses_positive_and_negative_axes() {
        const CHILDREN: u32 = 256;
        let mut nodes = Vec::with_capacity(CHILDREN as usize + 1);
        nodes.push(node(1, CapsRole::Document, None, 0, 1));
        for rank in 0..CHILDREN {
            nodes.push(node(
                10 + u64::from(rank),
                CapsRole::Chapter,
                Some(0),
                rank,
                CHILDREN,
            ));
        }
        let positions = layout(&nodes).unwrap_or_else(|error| panic!("{error}"));
        let directions = positions[1..]
            .iter()
            .map(|position| Vec3::from_array(position.position).normalize())
            .collect::<Vec<_>>();
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let (minimum, maximum) = directions.iter().map(|direction| direction.dot(axis)).fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
            assert!(minimum < -0.55, "negative axis coverage {minimum}");
            assert!(maximum > 0.55, "positive axis coverage {maximum}");
        }
    }

    #[test]
    fn child_cap_descriptors_remain_inside_their_parent() {
        let nodes = [
            node(1, CapsRole::Document, None, 0, 1),
            node(2, CapsRole::Chapter, Some(0), 0, 2),
            node(3, CapsRole::Chapter, Some(0), 1, 2),
            node(4, CapsRole::Paragraph, Some(1), 0, 2),
            node(5, CapsRole::Paragraph, Some(1), 1, 2),
        ];
        validate(&nodes).unwrap();
        let children = ChildIndex::build(&nodes);
        let weights = subtree_weights(&nodes).unwrap();
        let mut frames = vec![CapFrame::default(); nodes.len()];
        allocate_group(
            &[0],
            CapFrame {
                center: Vec3::Z,
                aperture: SUPER_ROOT_APERTURE,
            },
            &nodes,
            &weights,
            &mut frames,
            true,
        );
        for parent in 0..nodes.len() {
            let slots = children.children(parent);
            if !slots.is_empty() {
                allocate_group(slots, frames[parent], &nodes, &weights, &mut frames, false);
                for child in slots {
                    let angle = frames[parent]
                        .center
                        .dot(frames[*child].center)
                        .clamp(-1.0, 1.0)
                        .acos();
                    assert!(angle + frames[*child].aperture <= frames[parent].aperture + 1.0e-4);
                }
            }
        }
    }

    #[test]
    fn node_slot_permutation_cannot_change_stable_caps_geometry() {
        let canonical = [
            node(7, CapsRole::Document, None, 0, 1),
            node(8, CapsRole::Chapter, Some(0), 0, 1),
            node(9, CapsRole::Paragraph, Some(1), 0, 2),
            node(10, CapsRole::Paragraph, Some(1), 1, 2),
        ];
        let permuted = [
            node(10, CapsRole::Paragraph, Some(3), 1, 2),
            node(7, CapsRole::Document, None, 0, 1),
            node(9, CapsRole::Paragraph, Some(3), 0, 2),
            node(8, CapsRole::Chapter, Some(1), 0, 1),
        ];
        let mut expected = canonical
            .iter()
            .zip(layout(&canonical).unwrap())
            .map(|(node, position)| (node.stable_id, position))
            .collect::<Vec<_>>();
        let mut actual = permuted
            .iter()
            .zip(layout(&permuted).unwrap())
            .map(|(node, position)| (node.stable_id, position))
            .collect::<Vec<_>>();
        expected.sort_unstable_by_key(|(stable_id, _)| *stable_id);
        actual.sort_unstable_by_key(|(stable_id, _)| *stable_id);
        assert_eq!(actual, expected);
    }
}
