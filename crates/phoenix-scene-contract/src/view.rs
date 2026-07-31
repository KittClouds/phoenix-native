use crate::{GraphGeneration, Manifold};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SceneAuthority {
    Unavailable,
    Archive {
        generation: GraphGeneration,
        cohort_hash: [u8; 32],
        product_index_hash: Option<[u8; 32]>,
    },
}

impl SceneAuthority {
    #[must_use]
    pub const fn generation(self) -> Option<GraphGeneration> {
        match self {
            Self::Unavailable => None,
            Self::Archive { generation, .. } => Some(generation),
        }
    }

    #[must_use]
    pub const fn product_index_hash(self) -> Option<[u8; 32]> {
        match self {
            Self::Unavailable => None,
            Self::Archive {
                product_index_hash, ..
            } => product_index_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphSurface {
    #[default]
    Entities,
    Atlas,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphLens {
    #[default]
    Entities,
    Structure,
    Facts,
    Discourse,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphScope {
    #[default]
    Global,
    Narrative,
    Note,
    Compare,
}

impl GraphScope {
    #[must_use]
    pub const fn mask(self) -> ScopeMask {
        match self {
            Self::Global => ScopeMask::ALL,
            Self::Narrative => ScopeMask::NARRATIVE,
            Self::Note => ScopeMask::NOTE,
            Self::Compare => ScopeMask::COMPARE,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ScopeMask(pub u64);

impl ScopeMask {
    pub const NOTE: Self = Self(1 << 0);
    pub const REGISTRY: Self = Self(1 << 1);
    pub const NARRATIVE: Self = Self(1 << 2);
    pub const COMPARE: Self = Self(1 << 3);
    pub const ALL: Self = Self(u64::MAX);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct FamilyMask(pub u64);

impl FamilyMask {
    pub const ENTITIES: Self = Self((1 << 8) - 1);
    pub const STRUCTURE: Self = Self(1 << 8);
    pub const FACTS: Self = Self(1 << 9);
    pub const DISCOURSE: Self = Self(1 << 10);
    pub const ALL: Self =
        Self(Self::ENTITIES.0 | Self::STRUCTURE.0 | Self::FACTS.0 | Self::DISCOURSE.0);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[must_use]
    pub const fn toggled(self, other: Self) -> Self {
        Self(self.0 ^ other.0)
    }

    #[must_use]
    pub const fn is_valid_selection(self) -> bool {
        self.0 != 0 && self.0 & !Self::ALL.0 == 0
    }
}

impl Default for FamilyMask {
    fn default() -> Self {
        Self::ALL
    }
}

impl GraphLens {
    #[must_use]
    pub const fn family_mask(self) -> FamilyMask {
        match self {
            Self::Entities => FamilyMask::ENTITIES,
            Self::Structure => FamilyMask::STRUCTURE,
            Self::Facts => FamilyMask::FACTS,
            Self::Discourse => FamilyMask::DISCOURSE,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ReviewMask(pub u32);

impl ReviewMask {
    pub const ACCEPTED: Self = Self(1);
    pub const PROPOSED: Self = Self(2);
    pub const REJECTED: Self = Self(4);
    pub const DEFERRED: Self = Self(8);
    pub const SUPERSEDED: Self = Self(16);
    pub const VISIBLE: Self = Self(Self::ACCEPTED.0 | Self::PROPOSED.0);
    pub const ALL: Self =
        Self(Self::VISIBLE.0 | Self::REJECTED.0 | Self::DEFERRED.0 | Self::SUPERSEDED.0);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn toggled(self, other: Self) -> Self {
        Self(self.0 ^ other.0)
    }

    #[must_use]
    pub const fn is_visible_selection(self) -> bool {
        self.0 != 0 && self.0 & !Self::VISIBLE.0 == 0
    }
}

/// A generation-local review classification update for one existing edge.
///
/// This changes only the renderer's product mask. It cannot add an edge or
/// promote candidate topology.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphReviewOverride {
    pub edge_id: u64,
    pub review_mask: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum RelationFamily {
    CoOccurrence,
    Observation,
    Communication,
    Causal,
    Temporal,
    Structural,
    Identity,
    Relationship,
    Event,
    MemoryState,
}

impl RelationFamily {
    pub const ALL: [Self; 10] = [
        Self::CoOccurrence,
        Self::Observation,
        Self::Communication,
        Self::Causal,
        Self::Temporal,
        Self::Structural,
        Self::Identity,
        Self::Relationship,
        Self::Event,
        Self::MemoryState,
    ];

    #[must_use]
    pub const fn mask(self) -> RelationMask {
        RelationMask(1 << self as u8)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct RelationMask(pub u64);

impl RelationMask {
    pub const ALL: Self = Self((1 << RelationFamily::ALL.len()) - 1);

    #[must_use]
    pub const fn contains(self, family: RelationFamily) -> bool {
        self.0 & family.mask().0 != 0
    }

    #[must_use]
    pub const fn toggled(self, family: RelationFamily) -> Self {
        Self(self.0 ^ family.mask().0)
    }

    #[must_use]
    pub const fn is_valid_selection(self) -> bool {
        self.0 != 0 && self.0 & !Self::ALL.0 == 0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GraphViewState {
    pub authority: SceneAuthority,
    pub surface: GraphSurface,
    /// Combined topology lanes selected by the Style Hub.
    ///
    /// `lens` remains as a compatibility name for a preferred lane, but it no
    /// longer owns visibility. The renderer consumes this mask directly.
    #[serde(default)]
    pub families: FamilyMask,
    pub lens: GraphLens,
    pub scope: GraphScope,
    pub reviews: ReviewMask,
    pub relations: RelationMask,
    pub manifold: Manifold,
}

impl GraphViewState {
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            authority: SceneAuthority::Unavailable,
            surface: GraphSurface::Entities,
            families: FamilyMask::ALL,
            lens: GraphLens::Entities,
            scope: GraphScope::Global,
            reviews: ReviewMask::VISIBLE,
            relations: RelationMask::ALL,
            manifold: Manifold::Hybrid,
        }
    }

    #[must_use]
    pub const fn for_archive(
        generation: GraphGeneration,
        cohort_hash: [u8; 32],
        product_index_hash: Option<[u8; 32]>,
    ) -> Self {
        Self {
            authority: SceneAuthority::Archive {
                generation,
                cohort_hash,
                product_index_hash,
            },
            ..Self::unavailable()
        }
    }

    #[must_use]
    pub const fn family_mask(self) -> FamilyMask {
        match self.surface {
            GraphSurface::Entities => FamilyMask::ENTITIES,
            GraphSurface::Atlas => self.families,
        }
    }

    #[must_use]
    pub const fn scope_mask(self) -> ScopeMask {
        self.scope.mask()
    }

    #[must_use]
    pub const fn requires_product_index(self) -> bool {
        !matches!(self.authority, SceneAuthority::Unavailable)
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.families.is_valid_selection()
            && self.reviews.is_visible_selection()
            && self.relations.is_valid_selection()
    }
}

impl Default for GraphViewState {
    fn default() -> Self {
        Self::unavailable()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphAction {
    Fit,
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_is_entities_without_authority() {
        let view = GraphViewState::default();
        assert_eq!(view.surface, GraphSurface::Entities);
        assert_eq!(view.family_mask(), FamilyMask::ENTITIES);
        assert_eq!(view.authority, SceneAuthority::Unavailable);
        assert!(view.is_valid());
    }

    #[test]
    fn atlas_lenses_are_disjoint_named_bits_and_combine_without_allocation() {
        let masks = [
            GraphLens::Entities.family_mask(),
            GraphLens::Structure.family_mask(),
            GraphLens::Facts.family_mask(),
            GraphLens::Discourse.family_mask(),
        ];
        for (index, mask) in masks.iter().enumerate() {
            for other in &masks[index + 1..] {
                assert_eq!(mask.0 & other.0, 0);
            }
        }
        let combined = FamilyMask::STRUCTURE
            .toggled(FamilyMask::FACTS)
            .toggled(FamilyMask::ENTITIES);
        assert!(combined.contains(FamilyMask::STRUCTURE));
        assert!(combined.contains(FamilyMask::FACTS));
        assert!(combined.intersects(FamilyMask::ENTITIES));
        assert!(combined.is_valid_selection());
    }

    #[test]
    fn review_and_relation_toggles_never_need_allocation() {
        assert_eq!(
            ReviewMask::VISIBLE.toggled(ReviewMask::PROPOSED),
            ReviewMask::ACCEPTED
        );
        assert!(RelationMask::ALL
            .toggled(RelationFamily::Temporal)
            .contains(RelationFamily::Causal));
        assert!(!RelationMask::ALL
            .toggled(RelationFamily::Temporal)
            .contains(RelationFamily::Temporal));
    }
}
