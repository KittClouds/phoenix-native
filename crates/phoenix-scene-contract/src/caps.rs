//! Stable semantic and visual constants for the native CAPS projection.
//!
//! CAPS is a containment-and-overlap view. It deliberately does not infer
//! hierarchy from labels, colors, or entity families. Producers must assign a
//! role and parent explicitly before invoking the layout kernel.

pub const CAPS_LAYOUT_CONTRACT: &str = "phoenix.native.caps-lorentz-klein/v3";
pub const CAPS_WORLD_SCALE: f32 = 40.0;
pub const CAPS_KLEIN_BOUND: f32 = 0.96;

pub const DOCUMENT_NODE_KIND: u16 = 999;
pub const EPISODE_NODE_KIND: u16 = 1_000;
pub const CHUNK_NODE_KIND: u16 = 1_001;
pub const EVIDENCE_NODE_KIND: u16 = 1_002;
pub const CHAPTER_NODE_KIND: u16 = 1_003;
pub const PARAGRAPH_NODE_KIND: u16 = 1_004;
pub const SENTENCE_NODE_KIND: u16 = 1_005;
pub const EVENT_NODE_KIND: u16 = 1_006;
pub const RELATIONSHIP_FACT_NODE_KIND: u16 = 1_007;
pub const TEMPORAL_MIDPOINT_NODE_KIND: u16 = 1_008;
pub const CAUSAL_MIDPOINT_NODE_KIND: u16 = 1_009;
pub const MEMORY_STATE_NODE_KIND: u16 = 1_010;
pub const IDENTITY_MIDPOINT_NODE_KIND: u16 = 1_011;
pub const CONTEXTUAL_MIDPOINT_NODE_KIND: u16 = 1_012;

pub const GUIDE_FLAG_SHELL: u32 = 1;
pub const GUIDE_FLAG_CAP_BOUNDARY: u32 = 2;
pub const GUIDE_FLAG_CONCENTRATION_AXIS: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum CapsRole {
    Document = 0,
    Chapter = 1,
    Paragraph = 2,
    Sentence = 3,
    Episode = 4,
    Chunk = 5,
    Evidence = 6,
    Event = 7,
    Fact = 8,
    Discourse = 9,
    Entity = 10,
    Memory = 11,
}

impl CapsRole {
    pub const ALL: [Self; 12] = [
        Self::Document,
        Self::Chapter,
        Self::Paragraph,
        Self::Sentence,
        Self::Episode,
        Self::Chunk,
        Self::Evidence,
        Self::Event,
        Self::Fact,
        Self::Discourse,
        Self::Entity,
        Self::Memory,
    ];

    /// Midpoint of the role's volumetric depth interval in the Klein ball.
    ///
    /// CAPS v3 no longer constrains every role to a zero-thickness Euclidean
    /// sphere. The midpoint remains useful for reference guides and legacy
    /// callers, while layout uses [`Self::klein_depth_range`].
    pub const fn klein_radius(self) -> f32 {
        let [near, far] = self.klein_depth_range();
        (near + far) * 0.5
    }

    /// Inclusive role interval, ordered from the center toward the boundary.
    /// Stable identity selects a point inside the interval at publication
    /// time, giving CAPS genuine interior volume without runtime relaxation.
    pub const fn klein_depth_range(self) -> [f32; 2] {
        match self {
            Self::Document => [0.88, 0.93],
            Self::Chapter => [0.81, 0.87],
            Self::Paragraph => [0.74, 0.80],
            Self::Sentence => [0.67, 0.73],
            Self::Episode => [0.60, 0.66],
            Self::Chunk => [0.51, 0.59],
            Self::Evidence => [0.43, 0.50],
            Self::Event => [0.36, 0.42],
            Self::Fact => [0.29, 0.35],
            Self::Discourse => [0.23, 0.28],
            Self::Entity => [0.15, 0.22],
            Self::Memory => [0.08, 0.14],
        }
    }

    pub const fn world_radius(self) -> f32 {
        self.klein_radius() * CAPS_WORLD_SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_radii_are_strictly_nested_and_inside_the_klein_ball() {
        for pair in CapsRole::ALL.windows(2) {
            assert!(pair[0].klein_radius() > pair[1].klein_radius());
            assert!(pair[0].klein_depth_range()[0] > pair[1].klein_depth_range()[1]);
        }
        assert!(CapsRole::Document.klein_radius() < CAPS_KLEIN_BOUND);
        assert!(CapsRole::Memory.klein_radius() > 0.0);
    }

    #[test]
    fn every_role_owns_nonzero_klein_volume() {
        for role in CapsRole::ALL {
            let [near, far] = role.klein_depth_range();
            assert!(near > 0.0);
            assert!(far > near);
            assert!(far < CAPS_KLEIN_BOUND);
        }
    }
}
