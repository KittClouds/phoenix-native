use bytemuck::{Pod, Zeroable};
use phoenix_scene_contract::{GraphSurface, GraphTopologyEmphasis, GraphViewState};
use phoenix_scene_product_index::{EdgeProductRecord, NodeProductRecord};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct NodeProductGpu {
    pub family_mask: [u32; 2],
    pub scope_mask: [u32; 2],
    pub review_mask: u32,
    pub enabled: u32,
    /// Set by the CPU visibility closure when a selected edge needs this node
    /// as a muted endpoint even though its primary lane is hidden.
    pub context_visible: u32,
    /// Display-only overlay bits: source-local (`SOURCE_SCOPE_*`) and route
    /// walk (`WALK_*`, glow in bits 16..24). Zero when both modes are off, so
    /// the word never changes admission or graph authority.
    pub overlay_flags: u32,
}

impl NodeProductGpu {
    pub const UNFILTERED: Self = Self {
        family_mask: [u32::MAX; 2],
        scope_mask: [u32::MAX; 2],
        review_mask: u32::MAX,
        enabled: 1,
        context_visible: 0,
        overlay_flags: 0,
    };
}

impl From<&NodeProductRecord> for NodeProductGpu {
    fn from(record: &NodeProductRecord) -> Self {
        Self {
            family_mask: split_u64(record.family_mask),
            scope_mask: split_u64(record.scope_mask),
            review_mask: record.review_mask,
            enabled: 1,
            context_visible: 0,
            overlay_flags: 0,
        }
    }
}

/// Source-local mode is active for this frame.
pub const SOURCE_SCOPE_ACTIVE: u32 = 1;
/// The node is explicitly bound to the anchor's verified source passages.
pub const SOURCE_SCOPE_MEMBER: u32 = 2;
/// The node is the source-local anchor.
pub const SOURCE_SCOPE_ANCHOR: u32 = 4;
/// A route walk is active for this frame.
pub const WALK_ACTIVE: u32 = 8;
/// Route member not yet reached.
pub const WALK_ROUTE: u32 = 16;
/// Route member already walked.
pub const WALK_VISITED: u32 = 32;
/// The walk's current node.
pub const WALK_CURRENT: u32 = 64;
/// Destination of the traversal in flight.
pub const WALK_NEXT: u32 = 128;
pub const WALK_GLOW_SHIFT: u32 = 16;
/// The Reader is speaking an object bound to this node (4B).
pub const READER_GLOW: u32 = 256;
/// Reader glow intensity lives in bits 24..32.
pub const READER_INTENSITY_SHIFT: u32 = 24;
/// Story timeline (4C): an object with no stored span; shown neutral.
pub const STORY_UNTIMED: u32 = 512;
/// Story bloom intensity (0-15) lives in bits 12..16.
pub const STORY_BLOOM_SHIFT: u32 = 12;

/// Display-only source-local scope: node ids resolved from stored provenance.
/// Nodes outside `members` stay resident and are ghosted, never removed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceScopeMask {
    pub anchor: graph_model::NodeId,
    /// Sorted, de-duplicated node ids.
    pub members: std::sync::Arc<[u64]>,
}

impl SourceScopeMask {
    #[must_use]
    pub fn flags_for(&self, node: graph_model::NodeId) -> u32 {
        let mut flags = SOURCE_SCOPE_ACTIVE;
        if self.members.binary_search(&node.0).is_ok() {
            flags |= SOURCE_SCOPE_MEMBER;
        }
        if node == self.anchor {
            flags |= SOURCE_SCOPE_ANCHOR | SOURCE_SCOPE_MEMBER;
        }
        flags
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
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GraphLensUniform {
    pub family_mask: [u32; 2],
    pub entity_family_mask: [u32; 2],
    pub topology_family_mask: [u32; 2],
    pub scope_mask: [u32; 2],
    pub relation_mask: [u32; 2],
    pub review_mask: u32,
    pub product_index_enabled: u32,
    pub focus_active: u32,
    pub dimmed_node_opacity: f32,
    pub dimmed_edge_opacity: f32,
    pub topology_emphasis: u32,
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
        focus_active: 0,
        dimmed_node_opacity: 1.0,
        dimmed_edge_opacity: 1.0,
        topology_emphasis: 0,
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
            focus_active: 0,
            dimmed_node_opacity: 1.0,
            dimmed_edge_opacity: 1.0,
            topology_emphasis: match view.topology_emphasis {
                GraphTopologyEmphasis::Off => 0,
                GraphTopologyEmphasis::Structure => 1,
                GraphTopologyEmphasis::Facts => 2,
                GraphTopologyEmphasis::Discourse => 3,
            } * u32::from(
                product_index_enabled && view.surface == GraphSurface::Atlas,
            ),
        }
    }

    #[must_use]
    pub fn with_focus(mut self, active: bool) -> Self {
        self.focus_active = if active { 1 } else { 0 };
        self.dimmed_node_opacity = if active { 0.14 } else { 1.0 };
        self.dimmed_edge_opacity = if active { 0.08 } else { 1.0 };
        self
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
        FamilyMask, GraphScope, GraphSurface, GraphTopologyEmphasis, RelationMask, ReviewMask,
        ScopeMask,
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

    #[test]
    fn topology_emphasis_requires_published_product_identity() {
        let view = GraphViewState {
            surface: GraphSurface::Atlas,
            topology_emphasis: GraphTopologyEmphasis::Facts,
            ..GraphViewState::default()
        };
        assert_eq!(GraphLensUniform::from_view(view, true).topology_emphasis, 2);
        assert_eq!(
            GraphLensUniform::from_view(view, false).topology_emphasis,
            0
        );
    }
}
