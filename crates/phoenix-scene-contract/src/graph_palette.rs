use crate::{EntityFamily, RelationFamily, ReviewMask, VisualNodeDescriptor, VisualNodeKind};
use serde::{Deserialize, Serialize};

pub const GRAPH_PALETTE_COLOR_COUNT: usize = 39;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum GraphColorKey {
    Entities,
    Structure,
    Facts,
    Discourse,
    Characters,
    Locations,
    Networks,
    Creatures,
    Npcs,
    Factions,
    Events,
    Concepts,
    OtherEntities,
    Documents,
    Episodes,
    Chapters,
    Paragraphs,
    Sentences,
    Chunks,
    Evidence,
    EventFacts,
    RelationshipFacts,
    TemporalFacts,
    CausalFacts,
    MemoryStateFacts,
    IdentityDiscourse,
    ContextualDiscourse,
    CoOccurrenceEdges,
    ObservationEdges,
    CommunicationEdges,
    CausalEdges,
    TemporalEdges,
    StructuralEdges,
    IdentityEdges,
    RelationshipEdges,
    EventEdges,
    MemoryStateEdges,
    AcceptedEdges,
    ProposedEdges,
}

impl GraphColorKey {
    pub const ALL: [Self; GRAPH_PALETTE_COLOR_COUNT] = [
        Self::Entities,
        Self::Structure,
        Self::Facts,
        Self::Discourse,
        Self::Characters,
        Self::Locations,
        Self::Networks,
        Self::Creatures,
        Self::Npcs,
        Self::Factions,
        Self::Events,
        Self::Concepts,
        Self::OtherEntities,
        Self::Documents,
        Self::Episodes,
        Self::Chapters,
        Self::Paragraphs,
        Self::Sentences,
        Self::Chunks,
        Self::Evidence,
        Self::EventFacts,
        Self::RelationshipFacts,
        Self::TemporalFacts,
        Self::CausalFacts,
        Self::MemoryStateFacts,
        Self::IdentityDiscourse,
        Self::ContextualDiscourse,
        Self::CoOccurrenceEdges,
        Self::ObservationEdges,
        Self::CommunicationEdges,
        Self::CausalEdges,
        Self::TemporalEdges,
        Self::StructuralEdges,
        Self::IdentityEdges,
        Self::RelationshipEdges,
        Self::EventEdges,
        Self::MemoryStateEdges,
        Self::AcceptedEdges,
        Self::ProposedEdges,
    ];

    #[must_use]
    pub const fn slot(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn entity_family(self) -> Option<EntityFamily> {
        match self {
            Self::Characters => Some(EntityFamily::Character),
            Self::Locations => Some(EntityFamily::Location),
            Self::Networks => Some(EntityFamily::Network),
            Self::Creatures => Some(EntityFamily::Creature),
            Self::Npcs => Some(EntityFamily::Npc),
            Self::Factions => Some(EntityFamily::Network),
            Self::Events => Some(EntityFamily::Event),
            Self::Concepts => Some(EntityFamily::Concept),
            Self::OtherEntities => Some(EntityFamily::Other),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct GraphPalette {
    node_colors: [[f32; 4]; 27],
    edge_colors: [[f32; 4]; 12],
}

impl Default for GraphPalette {
    fn default() -> Self {
        Self {
            node_colors: [
                rgba(0x2f80ff),
                rgba(0xe03b78),
                rgba(0xff7733),
                rgba(0x9a68ff),
                rgba(0x2450e6),
                rgba(0x00c48c),
                rgba(0x22d3ee),
                rgba(0xf59e0b),
                rgba(0xa855f7),
                rgba(0x17b7a3),
                rgba(0xe35216),
                rgba(0x00a896),
                rgba(0x71817b),
                rgba(0x3d8cf5),
                rgba(0xb852f0),
                rgba(0x6652eb),
                rgba(0xa34ddb),
                rgba(0xd14cad),
                rgba(0xf04482),
                rgba(0x35c7d9),
                rgba(0xfb6f26),
                rgba(0xe84fa8),
                rgba(0xf4df23),
                rgba(0xff5964),
                rgba(0x22d36f),
                rgba(0x9858f5),
                rgba(0x35c7d9),
            ],
            edge_colors: [
                rgba(0x58a6ff),
                rgba(0x8bd5ff),
                rgba(0x42d6b5),
                rgba(0xff5964),
                rgba(0xf4df23),
                rgba(0x8b93a7),
                rgba(0x9858f5),
                rgba(0xe84fa8),
                rgba(0xfb6f26),
                rgba(0x22d36f),
                rgba(0x42f5b3),
                rgba(0xf4c95d),
            ],
        }
    }
}

impl GraphPalette {
    #[must_use]
    pub const fn color(self, key: GraphColorKey) -> [f32; 4] {
        let slot = key.slot();
        if slot < self.node_colors.len() {
            self.node_colors[slot]
        } else {
            self.edge_colors[slot - self.node_colors.len()]
        }
    }

    pub fn set_color(&mut self, key: GraphColorKey, color: [f32; 4]) {
        let slot = key.slot();
        if slot < self.node_colors.len() {
            self.node_colors[slot] = color;
        } else {
            self.edge_colors[slot - self.node_colors.len()] = color;
        }
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.node_colors
            .iter()
            .chain(&self.edge_colors)
            .flatten()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
    }

    #[must_use]
    pub const fn node_key(
        descriptor: VisualNodeDescriptor,
        node_kind: u16,
    ) -> Option<GraphColorKey> {
        if matches!(descriptor.kind, VisualNodeKind::EntityNetwork)
            && node_kind == crate::EntityKind::Faction as u16
        {
            return Some(GraphColorKey::Factions);
        }
        match descriptor.kind {
            VisualNodeKind::EntityCharacter => Some(GraphColorKey::Characters),
            VisualNodeKind::EntityLocation => Some(GraphColorKey::Locations),
            VisualNodeKind::EntityNetwork => Some(GraphColorKey::Networks),
            VisualNodeKind::EntityCreature => Some(GraphColorKey::Creatures),
            VisualNodeKind::EntityNpc => Some(GraphColorKey::Npcs),
            VisualNodeKind::EntityEvent => Some(GraphColorKey::Events),
            VisualNodeKind::EntityConcept => Some(GraphColorKey::Concepts),
            VisualNodeKind::EntityOther => Some(GraphColorKey::OtherEntities),
            VisualNodeKind::Document => Some(GraphColorKey::Documents),
            VisualNodeKind::Episode => Some(GraphColorKey::Episodes),
            VisualNodeKind::Chapter => Some(GraphColorKey::Chapters),
            VisualNodeKind::Paragraph => Some(GraphColorKey::Paragraphs),
            VisualNodeKind::Sentence => Some(GraphColorKey::Sentences),
            VisualNodeKind::Chunk => Some(GraphColorKey::Chunks),
            VisualNodeKind::Evidence => Some(GraphColorKey::Evidence),
            VisualNodeKind::EventFact => Some(GraphColorKey::EventFacts),
            VisualNodeKind::RelationshipFact => Some(GraphColorKey::RelationshipFacts),
            VisualNodeKind::TemporalFact => Some(GraphColorKey::TemporalFacts),
            VisualNodeKind::CausalFact => Some(GraphColorKey::CausalFacts),
            VisualNodeKind::MemoryStateFact => Some(GraphColorKey::MemoryStateFacts),
            VisualNodeKind::IdentityDiscourse => Some(GraphColorKey::IdentityDiscourse),
            VisualNodeKind::ContextualDiscourse => Some(GraphColorKey::ContextualDiscourse),
            VisualNodeKind::Unknown => None,
        }
    }

    #[must_use]
    pub const fn edge_key(relation_mask: u64, review_mask: u32) -> Option<GraphColorKey> {
        let relations = [
            GraphColorKey::CoOccurrenceEdges,
            GraphColorKey::ObservationEdges,
            GraphColorKey::CommunicationEdges,
            GraphColorKey::CausalEdges,
            GraphColorKey::TemporalEdges,
            GraphColorKey::StructuralEdges,
            GraphColorKey::IdentityEdges,
            GraphColorKey::RelationshipEdges,
            GraphColorKey::EventEdges,
            GraphColorKey::MemoryStateEdges,
        ];
        let mut slot = 0;
        while slot < RelationFamily::ALL.len() {
            if relation_mask & (1_u64 << slot) != 0 {
                return Some(relations[slot]);
            }
            slot += 1;
        }
        if review_mask & ReviewMask::ACCEPTED.0 != 0 {
            Some(GraphColorKey::AcceptedEdges)
        } else if review_mask & ReviewMask::PROPOSED.0 != 0 {
            Some(GraphColorKey::ProposedEdges)
        } else {
            None
        }
    }
}

const fn rgba(value: u32) -> [f32; 4] {
    [
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{describe_node, FamilyMask};

    fn node_key(family_mask: u64) -> Option<GraphColorKey> {
        GraphPalette::node_key(describe_node(family_mask), 0)
    }

    #[test]
    fn specific_product_keys_win_over_broad_lanes() {
        assert_eq!(
            node_key(FamilyMask::STRUCTURE.0 | FamilyMask::CHAPTERS.0),
            Some(GraphColorKey::Chapters)
        );
        assert_eq!(
            node_key(FamilyMask::ENTITIES.0 | FamilyMask::NPCS.0),
            Some(GraphColorKey::Npcs)
        );
    }

    #[test]
    fn compiler_real_fact_masks_keep_their_primary_palette_key() {
        for (mask, key) in [
            (FamilyMask::EVENT_FACTS, GraphColorKey::EventFacts),
            (
                FamilyMask::RELATIONSHIP_FACTS,
                GraphColorKey::RelationshipFacts,
            ),
            (FamilyMask::TEMPORAL_FACTS, GraphColorKey::TemporalFacts),
            (FamilyMask::CAUSAL_FACTS, GraphColorKey::CausalFacts),
            (
                FamilyMask::MEMORY_STATE_FACTS,
                GraphColorKey::MemoryStateFacts,
            ),
        ] {
            assert_eq!(
                node_key(
                    FamilyMask::FACTS.0
                        | mask.0
                        | FamilyMask::CHARACTERS.0
                        | FamilyMask::LOCATIONS.0,
                ),
                Some(key)
            );
        }
    }

    #[test]
    fn compiler_real_discourse_masks_keep_their_primary_palette_key() {
        for (mask, key) in [
            (
                FamilyMask::IDENTITY_DISCOURSE,
                GraphColorKey::IdentityDiscourse,
            ),
            (
                FamilyMask::CONTEXTUAL_DISCOURSE,
                GraphColorKey::ContextualDiscourse,
            ),
        ] {
            assert_eq!(
                node_key(
                    FamilyMask::DISCOURSE.0
                        | mask.0
                        | FamilyMask::CHARACTERS.0
                        | FamilyMask::NETWORKS.0,
                ),
                Some(key)
            );
        }
    }

    #[test]
    fn relation_keys_win_over_review_keys() {
        assert_eq!(
            GraphPalette::edge_key(RelationFamily::Temporal.mask().0, ReviewMask::ACCEPTED.0,),
            Some(GraphColorKey::TemporalEdges)
        );
    }
}
