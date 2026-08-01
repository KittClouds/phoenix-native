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
    // Bits 0..7 are the original V1 family slots.  Keep them readable so
    // frozen archives continue to open, but give the native renderer a
    // second, unambiguous set of entity-kind lanes.  The high lanes are
    // deliberately outside the legacy range so a new kind can never alias a
    // top-level Structure/Facts/Discourse bit.
    pub const CHARACTERS: Self = Self(1 << 16);
    pub const LOCATIONS: Self = Self(1 << 17);
    pub const NETWORKS: Self = Self(1 << 18);
    pub const CREATURES: Self = Self(1 << 19);
    pub const NPCS: Self = Self(1 << 20);
    pub const EVENTS: Self = Self(1 << 21);
    pub const CONCEPTS: Self = Self(1 << 22);
    pub const OTHER_ENTITIES: Self = Self(1 << 23);
    pub const ENTITY_LANES: Self = Self(
        Self::CHARACTERS.0
            | Self::LOCATIONS.0
            | Self::NETWORKS.0
            | Self::CREATURES.0
            | Self::NPCS.0
            | Self::EVENTS.0
            | Self::CONCEPTS.0
            | Self::OTHER_ENTITIES.0,
    );
    // Native topology-detail lanes. These bits describe what a product is,
    // independently of the broad renderer lane above. They let the Style Hub
    // and shaders address real compiler products without inferring semantics
    // from colors, labels, or node kinds at interaction time.
    pub const DOCUMENTS: Self = Self(1 << 24);
    pub const EPISODES: Self = Self(1 << 25);
    pub const CHUNKS: Self = Self(1 << 26);
    pub const EVIDENCE: Self = Self(1 << 27);
    pub const CHAPTERS: Self = Self(1 << 28);
    pub const PARAGRAPHS: Self = Self(1 << 29);
    pub const SENTENCES: Self = Self(1 << 30);
    pub const EVENT_FACTS: Self = Self(1 << 31);
    pub const RELATIONSHIP_FACTS: Self = Self(1 << 32);
    pub const TEMPORAL_FACTS: Self = Self(1 << 33);
    pub const CAUSAL_FACTS: Self = Self(1 << 34);
    pub const MEMORY_STATE_FACTS: Self = Self(1 << 35);
    pub const IDENTITY_DISCOURSE: Self = Self(1 << 36);
    pub const CONTEXTUAL_DISCOURSE: Self = Self(1 << 37);
    pub const STRUCTURE_LANES: Self = Self(
        Self::DOCUMENTS.0
            | Self::EPISODES.0
            | Self::CHUNKS.0
            | Self::EVIDENCE.0
            | Self::CHAPTERS.0
            | Self::PARAGRAPHS.0
            | Self::SENTENCES.0,
    );
    pub const FACT_LANES: Self = Self(
        Self::EVENT_FACTS.0
            | Self::RELATIONSHIP_FACTS.0
            | Self::TEMPORAL_FACTS.0
            | Self::CAUSAL_FACTS.0
            | Self::MEMORY_STATE_FACTS.0,
    );
    pub const DISCOURSE_LANES: Self =
        Self(Self::IDENTITY_DISCOURSE.0 | Self::CONTEXTUAL_DISCOURSE.0);
    pub const TOPOLOGY_LANES: Self =
        Self(Self::STRUCTURE_LANES.0 | Self::FACT_LANES.0 | Self::DISCOURSE_LANES.0);
    // Broad topology selection and granular entity-kind selection are two
    // independent axes.  Do not fold `ENTITY_LANES` into this mask: doing so
    // makes the broad Entities toggle invert individual kind choices.
    pub const ENTITIES: Self = Self((1 << 8) - 1);
    pub const STRUCTURE: Self = Self(1 << 8);
    pub const FACTS: Self = Self(1 << 9);
    pub const DISCOURSE: Self = Self(1 << 10);
    pub const ALL: Self =
        Self(Self::ENTITIES.0 | Self::STRUCTURE.0 | Self::FACTS.0 | Self::DISCOURSE.0);

    #[must_use]
    pub const fn entity_lane(family: crate::EntityFamily) -> Self {
        match family {
            crate::EntityFamily::Character => Self::CHARACTERS,
            crate::EntityFamily::Location => Self::LOCATIONS,
            crate::EntityFamily::Organization | crate::EntityFamily::Network => Self::NETWORKS,
            crate::EntityFamily::Creature => Self::CREATURES,
            crate::EntityFamily::Npc => Self::NPCS,
            crate::EntityFamily::Event => Self::EVENTS,
            crate::EntityFamily::Concept => Self::CONCEPTS,
            crate::EntityFamily::Item => Self::CREATURES,
            crate::EntityFamily::Structure | crate::EntityFamily::Other => Self::OTHER_ENTITIES,
        }
    }

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

    #[must_use]
    pub const fn is_valid_entity_selection(self) -> bool {
        self.0 != 0 && self.0 & !Self::ENTITY_LANES.0 == 0
    }

    #[must_use]
    pub const fn is_valid_topology_selection(self) -> bool {
        self.0 != 0 && self.0 & !Self::TOPOLOGY_LANES.0 == 0
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
    /// Entity-kind lanes are an orthogonal refinement of the broad topology
    /// lanes. Keeping this separate prevents a kind click from changing the
    /// active Entities/Atlas surface or disabling unrelated structure.
    #[serde(default = "default_entity_families")]
    pub entity_families: FamilyMask,
    /// Structure, fact, and discourse product subtypes selected by the Style
    /// Hub. Products from older archives carry no detail bits and therefore
    /// remain readable; verified V2 publications carry exact subtype lanes.
    #[serde(default = "default_topology_families")]
    pub topology_families: FamilyMask,
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
            entity_families: FamilyMask::ENTITY_LANES,
            topology_families: FamilyMask::TOPOLOGY_LANES,
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
            && self.entity_families.is_valid_entity_selection()
            && self.topology_families.is_valid_topology_selection()
            && self.reviews.is_visible_selection()
            && self.relations.is_valid_selection()
    }

    /// Migrates the short-lived shell-state encoding that placed granular
    /// entity bits in the broad `families` field.  Published scene authority
    /// is unaffected; this only repairs persisted view preferences.
    pub fn normalize_legacy_family_masks(&mut self) {
        let legacy_entity_lanes = self.families.0 & FamilyMask::ENTITY_LANES.0;
        if legacy_entity_lanes != 0 {
            self.entity_families = FamilyMask(
                (self.entity_families.0 | legacy_entity_lanes) & FamilyMask::ENTITY_LANES.0,
            );
            self.families = FamilyMask(self.families.0 & FamilyMask::ALL.0);
        }
    }
}

const fn default_entity_families() -> FamilyMask {
    FamilyMask::ENTITY_LANES
}

const fn default_topology_families() -> FamilyMask {
    FamilyMask::TOPOLOGY_LANES
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
        assert_eq!(view.entity_families, FamilyMask::ENTITY_LANES);
        assert_eq!(view.topology_families, FamilyMask::TOPOLOGY_LANES);
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
    fn granular_entity_lanes_are_disjoint_from_broad_entities() {
        let lanes = [
            FamilyMask::CHARACTERS,
            FamilyMask::LOCATIONS,
            FamilyMask::NETWORKS,
            FamilyMask::CREATURES,
            FamilyMask::NPCS,
        ];
        for (index, lane) in lanes.iter().enumerate() {
            assert!(!FamilyMask::ENTITIES.intersects(*lane));
            for other in &lanes[index + 1..] {
                assert!(!lane.intersects(*other));
            }
        }
        assert_eq!(
            FamilyMask::entity_lane(crate::EntityFamily::Character),
            FamilyMask::CHARACTERS
        );
        assert_eq!(
            FamilyMask::entity_lane(crate::EntityFamily::Location),
            FamilyMask::LOCATIONS
        );
        assert_eq!(
            FamilyMask::entity_lane(crate::EntityFamily::Network),
            FamilyMask::NETWORKS
        );
        assert_eq!(
            FamilyMask::entity_lane(crate::EntityFamily::Creature),
            FamilyMask::CREATURES
        );
        assert_eq!(
            FamilyMask::entity_lane(crate::EntityFamily::Npc),
            FamilyMask::NPCS
        );
        assert!(FamilyMask::ENTITY_LANES.is_valid_entity_selection());
        assert!(!FamilyMask::ALL.is_valid_entity_selection());
    }

    #[test]
    fn legacy_entity_bits_migrate_out_of_the_broad_family_mask() {
        let mut view = GraphViewState {
            families: FamilyMask(
                FamilyMask::ENTITIES.0
                    | FamilyMask::STRUCTURE.0
                    | FamilyMask::CHARACTERS.0
                    | FamilyMask::LOCATIONS.0,
            ),
            entity_families: FamilyMask::NPCS,
            ..GraphViewState::default()
        };

        view.normalize_legacy_family_masks();

        assert_eq!(
            view.families,
            FamilyMask(FamilyMask::ENTITIES.0 | FamilyMask::STRUCTURE.0)
        );
        assert!(view.entity_families.contains(FamilyMask::CHARACTERS));
        assert!(view.entity_families.contains(FamilyMask::LOCATIONS));
        assert!(view.entity_families.contains(FamilyMask::NPCS));
        assert!(view.is_valid());
    }

    #[test]
    fn topology_detail_lanes_are_disjoint_and_complete() {
        let lanes = [
            FamilyMask::DOCUMENTS,
            FamilyMask::EPISODES,
            FamilyMask::CHUNKS,
            FamilyMask::EVIDENCE,
            FamilyMask::EVENT_FACTS,
            FamilyMask::RELATIONSHIP_FACTS,
            FamilyMask::TEMPORAL_FACTS,
            FamilyMask::CAUSAL_FACTS,
            FamilyMask::MEMORY_STATE_FACTS,
            FamilyMask::IDENTITY_DISCOURSE,
            FamilyMask::CONTEXTUAL_DISCOURSE,
        ];
        for (slot, lane) in lanes.iter().enumerate() {
            assert!(FamilyMask::TOPOLOGY_LANES.contains(*lane));
            for other in lanes.iter().skip(slot + 1) {
                assert!(!lane.intersects(*other));
            }
        }
        assert!(FamilyMask::TOPOLOGY_LANES.is_valid_topology_selection());
        assert!(!FamilyMask::ALL.is_valid_topology_selection());
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
