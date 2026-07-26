use serde::{Deserialize, Serialize};

use crate::EntityFamily;

/// The explicit entity kinds offered by Phoenix's selection toolbar.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[repr(u16)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Character = 1,
    Location = 2,
    Npc = 3,
    Faction = 4,
    Event = 5,
    Concept = 6,
    Custom = 255,
}

impl EntityKind {
    pub const TOOLBAR: [Self; 7] = [
        Self::Character,
        Self::Location,
        Self::Npc,
        Self::Faction,
        Self::Event,
        Self::Concept,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Character => "Character",
            Self::Location => "Location",
            Self::Npc => "NPC",
            Self::Faction => "Faction",
            Self::Event => "Event",
            Self::Concept => "Concept",
            Self::Custom => "Custom",
        }
    }

    pub const fn family(self) -> EntityFamily {
        match self {
            Self::Character | Self::Npc => EntityFamily::Character,
            Self::Location => EntityFamily::Location,
            Self::Faction => EntityFamily::Organization,
            Self::Event => EntityFamily::Event,
            Self::Concept => EntityFamily::Concept,
            Self::Custom => EntityFamily::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_taxonomy_matches_the_phoenix_contract() {
        assert_eq!(
            EntityKind::TOOLBAR.map(EntityKind::label),
            [
                "Character",
                "Location",
                "NPC",
                "Faction",
                "Event",
                "Concept",
                "Custom",
            ]
        );
        assert_eq!(EntityKind::Npc.family(), EntityFamily::Character);
        assert_eq!(EntityKind::Faction.family(), EntityFamily::Organization);
    }
}
