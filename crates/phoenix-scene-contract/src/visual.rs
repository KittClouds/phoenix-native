//! Stable visual hierarchy metadata shared by the compiler and GPU consumer.
//!
//! The archive's V1 node-style record is intentionally kept at 24 bytes.  The
//! upper four bits below the interaction flags are reserved for a compact
//! visual role.  This is presentation metadata only: it never changes node
//! identity, topology, positions, review state, or product-lens semantics.

/// First bit used by the visual role in `NodeStyleRecord::flags`.
pub const VISUAL_ROLE_SHIFT: u16 = 8;
/// Four bits leave the interaction flags at bits 12..15 untouched.
pub const VISUAL_ROLE_MASK: u16 = 0x0f00;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum VisualRole {
    /// A normal product node with the base sphere treatment.
    #[default]
    Ordinary = 0,
    /// Document or episode roots that orient the scene.
    Root = 1,
    /// Entity/evidence anchors that deserve a readable body and aura.
    Anchor = 2,
    /// A high-degree node promoted by the deterministic compiler pass.
    Hub = 3,
    /// Reserved for an explicit producer-provided medoid.
    Medoid = 4,
    /// Reserved for an explicit producer-provided centroid.
    Centroid = 5,
}

impl VisualRole {
    pub const ALL: [Self; 6] = [
        Self::Ordinary,
        Self::Root,
        Self::Anchor,
        Self::Hub,
        Self::Medoid,
        Self::Centroid,
    ];

    /// A stable label/picking priority.  Larger roles receive earlier labels.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Ordinary => 0,
            Self::Anchor => 1,
            Self::Root => 2,
            Self::Hub => 3,
            Self::Centroid => 4,
            Self::Medoid => 5,
        }
    }

    /// Multiplier used by the analytic sphere surface.  Edge widths do not
    /// consume this value and therefore remain stable as nodes grow.
    #[must_use]
    pub const fn surface_scale(self) -> f32 {
        match self {
            Self::Ordinary => 1.0,
            Self::Anchor => 1.22,
            Self::Root => 1.72,
            Self::Hub => 1.48,
            Self::Medoid => 1.95,
            Self::Centroid => 1.82,
        }
    }

    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Root,
            2 => Self::Anchor,
            3 => Self::Hub,
            4 => Self::Medoid,
            5 => Self::Centroid,
            _ => Self::Ordinary,
        }
    }
}

/// Replace only the visual-role band, retaining producer and interaction bits.
#[must_use]
pub const fn with_visual_role(flags: u16, role: VisualRole) -> u16 {
    (flags & !VISUAL_ROLE_MASK) | ((role as u16) << VISUAL_ROLE_SHIFT)
}

#[must_use]
pub const fn visual_role(flags: u16) -> VisualRole {
    VisualRole::from_u8(((flags & VISUAL_ROLE_MASK) >> VISUAL_ROLE_SHIFT) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_round_trip_preserves_non_visual_flags() {
        let flags = 0x80f3;
        let packed = with_visual_role(flags, VisualRole::Hub);
        assert_eq!(visual_role(packed), VisualRole::Hub);
        assert_eq!(packed & !VISUAL_ROLE_MASK, flags & !VISUAL_ROLE_MASK);
    }

    #[test]
    fn interaction_band_is_outside_visual_band() {
        assert_eq!(VISUAL_ROLE_MASK & 0xf000, 0);
    }

    #[test]
    fn reserved_roles_have_stronger_surface_scale() {
        assert!(VisualRole::Medoid.surface_scale() > VisualRole::Anchor.surface_scale());
        assert!(VisualRole::Centroid.surface_scale() > VisualRole::Hub.surface_scale());
    }
}
