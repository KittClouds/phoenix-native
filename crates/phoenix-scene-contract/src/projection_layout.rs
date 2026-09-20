//! Coordinates shared by the compiler and its prepared visual guides.
//! These describe display charts, not a claim of metric-preserving embedding.
use crate::{CapsRole, VisualNodeKind};

pub const PROJECTION_LAYOUT_CONTRACT: &str =
    "phoenix.native.projection-layout/2026-09-15-v4-containment-atlas";
pub const SIEGEL_BAND_COUNT: usize = 11;
pub const TRANSIT_LAYER_COUNT: usize = 12;

pub const fn siegel_band(kind: VisualNodeKind) -> usize {
    match kind {
        VisualNodeKind::Document => 0,
        VisualNodeKind::Chapter | VisualNodeKind::Episode => 1,
        VisualNodeKind::Chunk | VisualNodeKind::Paragraph | VisualNodeKind::Sentence => 2,
        VisualNodeKind::EventFact | VisualNodeKind::EntityEvent => 3,
        VisualNodeKind::EntityLocation => 4,
        VisualNodeKind::EntityCharacter => 5,
        VisualNodeKind::EntityNetwork
        | VisualNodeKind::EntityCreature
        | VisualNodeKind::EntityNpc
        | VisualNodeKind::EntityConcept
        | VisualNodeKind::EntityOther => 6,
        VisualNodeKind::MemoryStateFact => 7,
        VisualNodeKind::RelationshipFact
        | VisualNodeKind::TemporalFact
        | VisualNodeKind::CausalFact => 8,
        VisualNodeKind::Evidence => 9,
        VisualNodeKind::IdentityDiscourse
        | VisualNodeKind::ContextualDiscourse
        | VisualNodeKind::Unknown => 10,
    }
}

pub fn siegel_band_center(band: usize) -> [f32; 3] {
    let depth = band.min(SIEGEL_BAND_COUNT - 1) as f32;
    [-26.0 + depth * 5.2, 22.0 - depth * 4.4, 0.0]
}

pub fn transit_layer_y(role: CapsRole) -> f32 {
    27.5 - role as u8 as f32 * 5.0
}

pub fn transit_layer_radius(role: CapsRole) -> f32 {
    // A gently widening stack makes every floor visible in a three-quarter view.
    13.0 + role as u8 as f32 * 0.75
}
