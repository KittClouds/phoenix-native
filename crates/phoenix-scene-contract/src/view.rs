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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct GraphScope(pub u64);

impl GraphScope {
    pub const ALL: Self = Self(u64::MAX);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct FamilyMask(pub u64);

impl FamilyMask {
    pub const ALL: Self = Self(u64::MAX);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ReviewMask(pub u32);

impl ReviewMask {
    pub const ACCEPTED: Self = Self(1);
    pub const PROPOSED: Self = Self(2);
    pub const REJECTED: Self = Self(4);
    pub const ALL: Self = Self(Self::ACCEPTED.0 | Self::PROPOSED.0 | Self::REJECTED.0);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct RelationMask(pub u64);

impl RelationMask {
    pub const ALL: Self = Self(u64::MAX);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ProjectionProfile(pub u16);

impl ProjectionProfile {
    pub const DEFAULT: Self = Self(0);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GraphViewState {
    pub authority: SceneAuthority,
    pub scope: GraphScope,
    pub families: FamilyMask,
    pub reviews: ReviewMask,
    pub relations: RelationMask,
    pub manifold: Manifold,
    pub profile: ProjectionProfile,
}

impl GraphViewState {
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            authority: SceneAuthority::Unavailable,
            scope: GraphScope::ALL,
            families: FamilyMask::ALL,
            reviews: ReviewMask::ALL,
            relations: RelationMask::ALL,
            manifold: Manifold::Hybrid,
            profile: ProjectionProfile::DEFAULT,
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
    pub const fn is_unfiltered(self) -> bool {
        self.scope.0 == u64::MAX
            && self.families.0 == u64::MAX
            && self.reviews.0 == ReviewMask::ALL.0
            && self.relations.0 == u64::MAX
    }
}

impl Default for GraphViewState {
    fn default() -> Self {
        Self::unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_is_unfiltered_but_has_no_authority() {
        let view = GraphViewState::default();
        assert!(view.is_unfiltered());
        assert_eq!(view.authority, SceneAuthority::Unavailable);
    }

    #[test]
    fn review_mask_names_are_stable_bits() {
        assert_eq!(ReviewMask::ACCEPTED.0, 1);
        assert_eq!(ReviewMask::PROPOSED.0, 2);
        assert_eq!(ReviewMask::REJECTED.0, 4);
        assert_eq!(ReviewMask::ALL.0, 7);
    }
}
