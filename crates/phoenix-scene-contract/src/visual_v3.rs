//! Typed visual roles for the resident graph product.
//!
//! The archive remains a compact V1 byte format.  These enums describe the
//! meaning carried by its product masks and style metadata without asking the
//! renderer to infer semantics from colors or labels.

use crate::FamilyMask;

pub const VISUAL_GRAPH_CONTRACT_V3: &str = "phoenix.native.visual-graph-contract/v3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VisualNodeLane {
    Entities = 1,
    Structure = 2,
    Facts = 3,
    Discourse = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VisualNodeKind {
    EntityCharacter = 1,
    EntityLocation = 2,
    EntityNetwork = 3,
    EntityCreature = 4,
    EntityNpc = 5,
    EntityEvent = 6,
    EntityConcept = 7,
    EntityOther = 8,
    Document = 20,
    Episode = 21,
    Chapter = 22,
    Paragraph = 23,
    Sentence = 24,
    Chunk = 25,
    Evidence = 26,
    EventFact = 40,
    RelationshipFact = 41,
    TemporalFact = 42,
    CausalFact = 43,
    MemoryStateFact = 44,
    IdentityDiscourse = 60,
    ContextualDiscourse = 61,
    Unknown = 255,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VisualEdgeKind {
    Structural = 1,
    CoOccurrence = 2,
    Observation = 3,
    Communication = 4,
    Authority = 5,
    Relationship = 6,
    Identity = 7,
    Event = 8,
    Temporal = 9,
    Causal = 10,
    MemoryState = 11,
    Candidate = 12,
    Unknown = 255,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisualNodeDescriptor {
    pub lane: VisualNodeLane,
    pub kind: VisualNodeKind,
    pub detail_mask: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisualEdgeDescriptor {
    pub kind: VisualEdgeKind,
    pub relation_mask: u64,
}

#[must_use]
pub const fn describe_node(family_mask: u64) -> VisualNodeDescriptor {
    let kind = if family_mask & FamilyMask::DOCUMENTS.0 != 0 {
        VisualNodeKind::Document
    } else if family_mask & FamilyMask::EPISODES.0 != 0 {
        VisualNodeKind::Episode
    } else if family_mask & FamilyMask::CHAPTERS.0 != 0 {
        VisualNodeKind::Chapter
    } else if family_mask & FamilyMask::PARAGRAPHS.0 != 0 {
        VisualNodeKind::Paragraph
    } else if family_mask & FamilyMask::SENTENCES.0 != 0 {
        VisualNodeKind::Sentence
    } else if family_mask & FamilyMask::CHUNKS.0 != 0 {
        VisualNodeKind::Chunk
    } else if family_mask & FamilyMask::EVIDENCE.0 != 0 {
        VisualNodeKind::Evidence
    } else if family_mask & FamilyMask::EVENT_FACTS.0 != 0 {
        VisualNodeKind::EventFact
    } else if family_mask & FamilyMask::RELATIONSHIP_FACTS.0 != 0 {
        VisualNodeKind::RelationshipFact
    } else if family_mask & FamilyMask::TEMPORAL_FACTS.0 != 0 {
        VisualNodeKind::TemporalFact
    } else if family_mask & FamilyMask::CAUSAL_FACTS.0 != 0 {
        VisualNodeKind::CausalFact
    } else if family_mask & FamilyMask::MEMORY_STATE_FACTS.0 != 0 {
        VisualNodeKind::MemoryStateFact
    } else if family_mask & FamilyMask::IDENTITY_DISCOURSE.0 != 0 {
        VisualNodeKind::IdentityDiscourse
    } else if family_mask & FamilyMask::CONTEXTUAL_DISCOURSE.0 != 0 {
        VisualNodeKind::ContextualDiscourse
    } else if family_mask & FamilyMask::CHARACTERS.0 != 0 {
        VisualNodeKind::EntityCharacter
    } else if family_mask & FamilyMask::LOCATIONS.0 != 0 {
        VisualNodeKind::EntityLocation
    } else if family_mask & FamilyMask::NETWORKS.0 != 0 {
        VisualNodeKind::EntityNetwork
    } else if family_mask & FamilyMask::CREATURES.0 != 0 {
        VisualNodeKind::EntityCreature
    } else if family_mask & FamilyMask::NPCS.0 != 0 {
        VisualNodeKind::EntityNpc
    } else if family_mask & FamilyMask::EVENTS.0 != 0 {
        VisualNodeKind::EntityEvent
    } else if family_mask & FamilyMask::CONCEPTS.0 != 0 {
        VisualNodeKind::EntityConcept
    } else if family_mask & FamilyMask::OTHER_ENTITIES.0 != 0 {
        VisualNodeKind::EntityOther
    } else {
        VisualNodeKind::Unknown
    };
    let lane = match kind {
        VisualNodeKind::EntityCharacter
        | VisualNodeKind::EntityLocation
        | VisualNodeKind::EntityNetwork
        | VisualNodeKind::EntityCreature
        | VisualNodeKind::EntityNpc
        | VisualNodeKind::EntityEvent
        | VisualNodeKind::EntityConcept
        | VisualNodeKind::EntityOther => VisualNodeLane::Entities,
        VisualNodeKind::EventFact
        | VisualNodeKind::RelationshipFact
        | VisualNodeKind::TemporalFact
        | VisualNodeKind::CausalFact
        | VisualNodeKind::MemoryStateFact => VisualNodeLane::Facts,
        VisualNodeKind::IdentityDiscourse | VisualNodeKind::ContextualDiscourse => {
            VisualNodeLane::Discourse
        }
        _ => VisualNodeLane::Structure,
    };
    let detail_mask = family_mask & FamilyMask::TOPOLOGY_LANES.0;
    VisualNodeDescriptor {
        lane,
        kind,
        detail_mask,
    }
}

#[must_use]
pub const fn describe_edge(relation_mask: u64) -> VisualEdgeDescriptor {
    let kind = if relation_mask & (1 << 5) != 0 {
        VisualEdgeKind::Structural
    } else if relation_mask & (1 << 0) != 0 {
        VisualEdgeKind::CoOccurrence
    } else if relation_mask & (1 << 1) != 0 {
        VisualEdgeKind::Observation
    } else if relation_mask & (1 << 2) != 0 {
        VisualEdgeKind::Communication
    } else if relation_mask & (1 << 6) != 0 {
        VisualEdgeKind::Identity
    } else if relation_mask & (1 << 7) != 0 {
        VisualEdgeKind::Relationship
    } else if relation_mask & (1 << 8) != 0 {
        VisualEdgeKind::Event
    } else if relation_mask & (1 << 9) != 0 {
        VisualEdgeKind::MemoryState
    } else if relation_mask & (1 << 3) != 0 {
        VisualEdgeKind::Causal
    } else if relation_mask & (1 << 4) != 0 {
        VisualEdgeKind::Temporal
    } else {
        VisualEdgeKind::Unknown
    };
    VisualEdgeDescriptor {
        kind,
        relation_mask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_bits_take_precedence_over_broad_lanes() {
        let descriptor = describe_node(FamilyMask::FACTS.0 | FamilyMask::CAUSAL_FACTS.0);
        assert_eq!(descriptor.lane, VisualNodeLane::Facts);
        assert_eq!(descriptor.kind, VisualNodeKind::CausalFact);
    }

    #[test]
    fn entity_lanes_remain_individual() {
        let descriptor = describe_node(FamilyMask::ENTITIES.0 | FamilyMask::NPCS.0);
        assert_eq!(descriptor.lane, VisualNodeLane::Entities);
        assert_eq!(descriptor.kind, VisualNodeKind::EntityNpc);
    }

    #[test]
    fn relation_masks_decode_without_color_inference() {
        assert_eq!(describe_edge(1 << 3).kind, VisualEdgeKind::Causal);
        assert_eq!(describe_edge(1 << 5).kind, VisualEdgeKind::Structural);
    }
}
