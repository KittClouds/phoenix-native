//! Static semantic charts. No simulation, locks, or allocations in the node loop.
use super::caps::{validate, CapsNode};
use crate::NativeSceneCompilerError;
use phoenix_scene_archive::PositionRecord;
use phoenix_scene_contract::{
    siegel_band, siegel_band_center, transit_layer_radius, transit_layer_y, CapsRole,
};
use std::f32::consts::TAU;

pub fn siegel(nodes: &[CapsNode]) -> Result<Vec<PositionRecord>, NativeSceneCompilerError> {
    validate(nodes)?;
    let mut positions = Vec::with_capacity(nodes.len());
    for node in nodes {
        let center = siegel_band_center(siegel_band(node.semantic_kind));
        let anchor = node
            .parent_slot
            .map_or(node.stable_id, |p| nodes[p as usize].stable_id);
        // Shared parents occupy coherent neighborhoods within each semantic band.
        // Identity-based local spread avoids insertion-order spirals and survives
        // slot permutations. It is a display fallback, not fabricated matrix data.
        positions.push(PositionRecord {
            position: [
                center[0] + signed(node.stable_id, 1) * 1.8,
                center[1] + signed(anchor, 2) * 1.2 + signed(node.stable_id, 3) * 0.7,
                signed(anchor, 4) * 9.0 + signed(node.stable_id, 5) * 3.4,
            ],
        });
    }
    Ok(positions)
}

pub fn transit(nodes: &[CapsNode]) -> Result<Vec<PositionRecord>, NativeSceneCompilerError> {
    validate(nodes)?;
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_unstable_by_key(|&slot| {
        let node = nodes[slot];
        (
            node.role,
            node.parent_slot.map(|p| nodes[p as usize].stable_id),
            node.semantic_kind as u8,
            node.stable_id,
        )
    });
    let mut positions = vec![PositionRecord { position: [0.0; 3] }; nodes.len()];
    let mut first = 0;
    for role in CapsRole::ALL {
        let end = first + order[first..].partition_point(|&slot| nodes[slot].role == role);
        let count = end - first;
        // Bounded ring population gives dense layers multiple concentric tracks.
        let tracks = count.div_ceil(160).max(1);
        let per_track = count.div_ceil(tracks).max(1);
        for (rank, &slot) in order[first..end].iter().enumerate() {
            let track = rank / per_track;
            let track_count = per_track.min(count - track * per_track);
            let phase = (rank % per_track) as f32 / track_count as f32 * TAU;
            let radius =
                transit_layer_radius(role) * (1.0 - 0.44 * track as f32 / tracks.max(2) as f32);
            positions[slot].position = [
                radius * phase.cos(),
                transit_layer_y(role),
                radius * phase.sin(),
            ];
        }
        first = end;
    }
    Ok(positions)
}

fn signed(id: u64, salt: u64) -> f32 {
    let mut v = id ^ salt.wrapping_mul(0x9e3779b97f4a7c15);
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^= v >> 31;
    (v >> 40) as f32 / ((1_u32 << 24) - 1) as f32 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_contract::VisualNodeKind;

    fn fixture() -> Vec<CapsNode> {
        let mut nodes = vec![CapsNode {
            stable_id: 1,
            role: CapsRole::Document,
            semantic_kind: VisualNodeKind::Document,
            parent_slot: None,
            sibling_rank: 0,
            sibling_count: 1,
            membership_count: 1,
        }];
        for i in 0..500 {
            nodes.push(CapsNode {
                stable_id: i + 2,
                role: CapsRole::Chapter,
                semantic_kind: VisualNodeKind::Chapter,
                parent_slot: Some(0),
                sibling_rank: i as u32,
                sibling_count: 500,
                membership_count: 1,
            });
        }
        nodes
    }

    #[test]
    fn charts_preserve_inventory_and_semantic_separation() {
        let nodes = fixture();
        let bands = siegel(&nodes).unwrap();
        let rings = transit(&nodes).unwrap();
        assert_eq!(bands.len(), nodes.len());
        assert_eq!(rings.len(), nodes.len());
        assert!(
            bands[0].position[0]
                < bands[1..]
                    .iter()
                    .map(|p| p.position[0])
                    .fold(f32::INFINITY, f32::min)
        );
        for (node, point) in nodes.iter().zip(&rings) {
            assert_eq!(point.position[1], transit_layer_y(node.role));
            assert!(point.position.iter().all(|v| v.is_finite()));
            let r = point.position[0].hypot(point.position[2]);
            assert!(r <= transit_layer_radius(node.role) + 1.0e-4);
        }
    }

    #[test]
    fn node_slot_permutation_preserves_both_charts() {
        let nodes = fixture();
        let mut reversed = nodes.clone();
        reversed[1..].reverse();
        for layout in [siegel, transit] {
            let expected = layout(&nodes).unwrap();
            let mut actual = layout(&reversed).unwrap();
            actual[1..].reverse();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn invalid_parents_fail_before_layout() {
        let mut nodes = fixture();
        nodes[1].parent_slot = Some(u32::MAX);
        assert!(siegel(&nodes).is_err());
        assert!(transit(&nodes).is_err());
    }
}
