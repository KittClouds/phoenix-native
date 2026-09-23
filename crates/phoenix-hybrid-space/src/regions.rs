//! Equal-area spherical treemap. Dense slots, O(n log n) sorting/partitioning,
//! O(n) scratch; no per-node allocation, iteration, force simulation or RNG.
use super::HybridNode;
use glam::Vec3;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug)]
struct Region {
    longitude: [f32; 2],
    height: [f32; 2],
}

impl Region {
    const WORLD: Self = Self {
        longitude: [-PI, PI],
        height: [-1.0, 1.0],
    };

    fn direction(self) -> Vec3 {
        let phi = (self.longitude[0] + self.longitude[1]) * 0.5;
        let y = (self.height[0] + self.height[1]) * 0.5;
        let horizontal = (1.0 - y * y).max(0.0).sqrt();
        Vec3::new(horizontal * phi.cos(), y, horizontal * phi.sin())
    }

    fn split(self, fraction: f32) -> (Self, Self) {
        let mut left = self;
        let mut right = self;
        // Longitude × height has constant area measure on the unit sphere.
        // Normalize spans before choosing the longer side to avoid thin strips.
        if (self.longitude[1] - self.longitude[0]) / TAU >= (self.height[1] - self.height[0]) * 0.5
        {
            let cut = self.longitude[0] + (self.longitude[1] - self.longitude[0]) * fraction;
            left.longitude[1] = cut;
            right.longitude[0] = cut;
        } else {
            let cut = self.height[0] + (self.height[1] - self.height[0]) * fraction;
            left.height[1] = cut;
            right.height[0] = cut;
        }
        (left, right)
    }
}

pub(super) fn directions(nodes: &[HybridNode]) -> Vec<Vec3> {
    let count = nodes.len();
    // The synthetic root occupies slot count. Children are contiguous in CSR.
    let mut children: Vec<usize> = (0..count).collect();
    children.sort_unstable_by_key(|&i| {
        (
            nodes[i].parent_slot.map_or(count, |p| p as usize),
            nodes[i].lane,
            nodes[i].role,
            nodes[i].stable_id,
        )
    });
    let mut offsets = vec![0_usize; count + 2];
    for node in nodes {
        offsets[node.parent_slot.map_or(count, |p| p as usize) + 1] += 1;
    }
    for i in 1..offsets.len() {
        offsets[i] += offsets[i - 1];
    }
    let mut weights = vec![1_u64; count];
    // Validated roles strictly increase along containment edges.
    for role in super::CapsRole::ALL.into_iter().rev() {
        for (i, node) in nodes.iter().enumerate().filter(|(_, n)| n.role == role) {
            if let Some(parent) = node.parent_slot {
                weights[parent as usize] += weights[i];
            }
        }
    }
    // Prefix weights make partition lookup logarithmic even for skewed trees.
    let mut prefix = Vec::with_capacity(count + 1);
    prefix.push(0_u64);
    for &i in &children {
        prefix.push(prefix.last().copied().unwrap() + weights[i]);
    }
    let mut regions = vec![Region::WORLD; count];
    partition(
        &children,
        &prefix,
        offsets[count],
        offsets[count + 1],
        Region::WORLD,
        &mut regions,
    );
    for role in super::CapsRole::ALL {
        for (i, _) in nodes.iter().enumerate().filter(|(_, n)| n.role == role) {
            partition(
                &children,
                &prefix,
                offsets[i],
                offsets[i + 1],
                regions[i],
                &mut regions,
            );
        }
    }
    regions.into_iter().map(Region::direction).collect()
}

fn partition(
    children: &[usize],
    prefix: &[u64],
    start: usize,
    end: usize,
    region: Region,
    output: &mut [Region],
) {
    if start == end {
        return;
    }
    if end - start == 1 {
        output[children[start]] = region;
        return;
    }
    let total = prefix[end] - prefix[start];
    let half = prefix[start] + total / 2;
    let relative = prefix[start + 1..end].partition_point(|&w| w < half);
    let split = (start + 1 + relative).min(end - 1);
    let fraction = (prefix[split] - prefix[start]) as f32 / total as f32;
    let (left, right) = region.split(fraction);
    partition(children, prefix, start, split, left, output);
    partition(children, prefix, split, end, right, output);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapsRole, HybridLane};

    fn node(id: u64, role: CapsRole, parent: Option<u32>) -> HybridNode {
        HybridNode {
            stable_id: id,
            lane: HybridLane::Structure,
            role,
            parent_slot: parent,
            sibling_rank: 0,
            sibling_count: 1,
            degree: 0,
        }
    }

    #[test]
    fn broad_siblings_fill_both_hemispheres_and_all_axes() {
        let mut nodes = vec![node(1, CapsRole::Document, None)];
        for id in 2..258 {
            nodes.push(node(id, CapsRole::Chapter, Some(0)));
        }
        let points = directions(&nodes);
        for axis in 0..3 {
            assert!(points.iter().any(|p| p[axis] < -0.7));
            assert!(points.iter().any(|p| p[axis] > 0.7));
        }
        assert!(points.iter().all(|p| (p.length() - 1.0).abs() < 1e-5));
    }

    #[test]
    fn slot_permutation_preserves_positions_by_identity() {
        let nodes = [
            node(1, CapsRole::Document, None),
            node(20, CapsRole::Chapter, Some(0)),
            node(30, CapsRole::Chapter, Some(0)),
            node(40, CapsRole::Paragraph, Some(1)),
        ];
        let a = directions(&nodes);
        let mut permuted = [nodes[3], nodes[2], nodes[0], nodes[1]];
        permuted[0].parent_slot = Some(3);
        permuted[1].parent_slot = Some(2);
        permuted[3].parent_slot = Some(2);
        let b = directions(&permuted);
        assert_eq!([a[3], a[2], a[0], a[1]], b.as_slice());
    }

    #[test]
    fn partition_area_matches_population_and_children_stay_inside() {
        let mut regions = [Region::WORLD; 3];
        partition(
            &[0, 1, 2],
            &[0, 1, 4, 10],
            0,
            3,
            Region::WORLD,
            &mut regions,
        );
        for (r, weight) in regions.iter().zip([1.0, 3.0, 6.0]) {
            let area = (r.longitude[1] - r.longitude[0]) * (r.height[1] - r.height[0]);
            assert!((area / (2.0 * TAU) - weight / 10.0).abs() < 1e-6);
            assert!(r.longitude[0] >= -PI && r.longitude[1] <= PI);
            assert!(r.height[0] >= -1.0 && r.height[1] <= 1.0);
        }
    }
}
