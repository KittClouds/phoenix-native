use bytemuck::{Pod, Zeroable};
use phoenix_scene_contract::GraphViewState;
use phoenix_scene_product_index::{EdgeProductRecord, NodeProductRecord};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct NodeProductGpu {
    pub family_mask: [u32; 2],
    pub scope_mask: [u32; 2],
    pub review_mask: u32,
    pub enabled: u32,
    pub _padding: [u32; 2],
}

impl NodeProductGpu {
    pub const UNFILTERED: Self = Self {
        family_mask: [u32::MAX; 2],
        scope_mask: [u32::MAX; 2],
        review_mask: u32::MAX,
        enabled: 1,
        _padding: [0; 2],
    };
}

impl From<&NodeProductRecord> for NodeProductGpu {
    fn from(record: &NodeProductRecord) -> Self {
        Self {
            family_mask: split_u64(record.family_mask),
            scope_mask: split_u64(record.scope_mask),
            review_mask: record.review_mask,
            enabled: 1,
            _padding: [0; 2],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct EdgeProductGpu {
    pub family_mask: [u32; 2],
    pub scope_mask: [u32; 2],
    pub relation_mask: [u32; 2],
    pub review_mask: u32,
    pub enabled: u32,
}

impl EdgeProductGpu {
    pub const UNFILTERED: Self = Self {
        family_mask: [u32::MAX; 2],
        scope_mask: [u32::MAX; 2],
        relation_mask: [u32::MAX; 2],
        review_mask: u32::MAX,
        enabled: 1,
    };
}

impl From<&EdgeProductRecord> for EdgeProductGpu {
    fn from(record: &EdgeProductRecord) -> Self {
        Self {
            family_mask: split_u64(record.family_mask),
            scope_mask: split_u64(record.scope_mask),
            relation_mask: split_u64(record.relation_mask),
            review_mask: record.review_mask,
            enabled: 1,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct GraphLensUniform {
    pub family_mask: [u32; 2],
    pub entity_family_mask: [u32; 2],
    pub topology_family_mask: [u32; 2],
    pub scope_mask: [u32; 2],
    pub relation_mask: [u32; 2],
    pub review_mask: u32,
    pub product_index_enabled: u32,
    pub _padding: [u32; 4],
}

impl GraphLensUniform {
    pub const UNFILTERED: Self = Self {
        family_mask: [u32::MAX; 2],
        entity_family_mask: [u32::MAX; 2],
        topology_family_mask: [u32::MAX; 2],
        scope_mask: [u32::MAX; 2],
        relation_mask: [u32::MAX; 2],
        review_mask: u32::MAX,
        product_index_enabled: 0,
        _padding: [0; 4],
    };

    #[must_use]
    pub fn from_view(view: GraphViewState, product_index_enabled: bool) -> Self {
        Self {
            family_mask: split_u64(view.family_mask().0),
            entity_family_mask: split_u64(view.entity_families.0),
            topology_family_mask: split_u64(view.topology_families.0),
            scope_mask: split_u64(view.scope_mask().0),
            relation_mask: split_u64(view.relations.0),
            review_mask: view.reviews.0,
            product_index_enabled: u32::from(product_index_enabled),
            _padding: [0; 4],
        }
    }
}

const _: () = {
    assert!(size_of::<NodeProductGpu>() == 32);
    assert!(size_of::<EdgeProductGpu>() == 32);
    assert!(size_of::<GraphLensUniform>() == 64);
};

const fn split_u64(value: u64) -> [u32; 2] {
    [value as u32, (value >> 32) as u32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_contract::{
        FamilyMask, GraphScope, GraphSurface, RelationMask, ReviewMask, ScopeMask,
    };

    #[test]
    fn view_masks_split_without_losing_high_bits() {
        let view = GraphViewState {
            surface: GraphSurface::Atlas,
            families: FamilyMask::DISCOURSE,
            scope: GraphScope::Note,
            relations: RelationMask(0xaaaa_bbbb_cccc_dddd),
            reviews: ReviewMask::PROPOSED,
            ..GraphViewState::default()
        };
        let uniform = GraphLensUniform::from_view(view, true);
        assert_eq!(uniform.family_mask, [FamilyMask::DISCOURSE.0 as u32, 0]);
        assert_eq!(
            uniform.entity_family_mask,
            [FamilyMask::ENTITY_LANES.0 as u32, 0]
        );
        assert_eq!(
            uniform.topology_family_mask,
            [
                FamilyMask::TOPOLOGY_LANES.0 as u32,
                (FamilyMask::TOPOLOGY_LANES.0 >> 32) as u32,
            ]
        );
        assert_eq!(uniform.scope_mask, [ScopeMask::NOTE.0 as u32, 0]);
        assert_eq!(uniform.relation_mask, [0xcccc_dddd, 0xaaaa_bbbb]);
        assert_eq!(uniform.review_mask, 2);
        assert_eq!(uniform.product_index_enabled, 1);
    }
}
