//! Stable semantic and visual constants for the native CAPS projection.
//!
//! CAPS is a containment-and-overlap view. It deliberately does not infer
//! hierarchy from labels, colors, or entity families. Producers must assign a
//! role and parent explicitly before invoking the layout kernel.

pub const CAPS_LAYOUT_CONTRACT: &str = "phoenix.native.caps-lorentz-klein/v2";
pub const CAPS_WORLD_SCALE: f32 = 40.0;
pub const CAPS_KLEIN_BOUND: f32 = 0.96;

pub const DOCUMENT_NODE_KIND: u16 = 999;
pub const EPISODE_NODE_KIND: u16 = 1_000;
pub const CHUNK_NODE_KIND: u16 = 1_001;
pub const EVIDENCE_NODE_KIND: u16 = 1_002;

pub const GUIDE_FLAG_SHELL: u32 = 1;
pub const GUIDE_FLAG_CAP_BOUNDARY: u32 = 2;
pub const GUIDE_FLAG_CONCENTRATION_AXIS: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum CapsRole {
    Document = 0,
    Episode = 1,
    Chunk = 2,
    Evidence = 3,
    Event = 4,
    Fact = 5,
    Entity = 6,
    Memory = 7,
}

impl CapsRole {
    pub const ALL: [Self; 8] = [
        Self::Document,
        Self::Episode,
        Self::Chunk,
        Self::Evidence,
        Self::Event,
        Self::Fact,
        Self::Entity,
        Self::Memory,
    ];

    /// Radius in the Klein ball. Outer bands are more abstract; inner bands
    /// are more concrete or state-like.
    pub const fn klein_radius(self) -> f32 {
        match self {
            Self::Document => 0.90,
            Self::Episode => 0.78,
            Self::Chunk => 0.67,
            Self::Evidence => 0.55,
            Self::Event => 0.50,
            Self::Fact => 0.45,
            Self::Entity => 0.39,
            Self::Memory => 0.26,
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
        }
        assert!(CapsRole::Document.klein_radius() < CAPS_KLEIN_BOUND);
        assert!(CapsRole::Memory.klein_radius() > 0.0);
    }
}
