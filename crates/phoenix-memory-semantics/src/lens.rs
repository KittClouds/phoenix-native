#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensSet(u8);

impl LensSet {
    pub const NONE: Self = Self(0);
    pub const NARRATIVE: Self = Self(1);
    pub const CONVERSATION: Self = Self(1 << 1);
    pub const DOCUMENT: Self = Self(1 << 2);
    pub const ALL: Self = Self(Self::NARRATIVE.0 | Self::CONVERSATION.0 | Self::DOCUMENT.0);

    pub const fn contains(self, lens: Self) -> bool {
        self.0 & lens.0 == lens.0
    }
}

pub trait VocabularyRelation {
    fn stable_name(self) -> &'static str;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreRelation {
    IdentityAlias,
    Attribute,
    Preference,
    Relationship,
    EventParticipation,
    TemporalOrder,
    Duration,
    Cause,
    Enablement,
    Knowledge,
    Belief,
    State,
    Commitment,
    Goal,
    Procedure,
    Correction,
    Conflict,
    Supersession,
    RepeatedEvidence,
}

impl VocabularyRelation for CoreRelation {
    fn stable_name(self) -> &'static str {
        match self {
            Self::IdentityAlias => "core.identity_alias",
            Self::Attribute => "core.attribute",
            Self::Preference => "core.preference",
            Self::Relationship => "core.relationship",
            Self::EventParticipation => "core.event_participation",
            Self::TemporalOrder => "core.temporal_order",
            Self::Duration => "core.duration",
            Self::Cause => "core.cause",
            Self::Enablement => "core.enablement",
            Self::Knowledge => "core.knowledge",
            Self::Belief => "core.belief",
            Self::State => "core.state",
            Self::Commitment => "core.commitment",
            Self::Goal => "core.goal",
            Self::Procedure => "core.procedure",
            Self::Correction => "core.correction",
            Self::Conflict => "core.conflict",
            Self::Supersession => "core.supersession",
            Self::RepeatedEvidence => "core.repeated_evidence",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NarrativeRelation {
    SceneMembership,
    EpisodeMembership,
    ConflictParticipation,
    CharacterState,
}

impl VocabularyRelation for NarrativeRelation {
    fn stable_name(self) -> &'static str {
        match self {
            Self::SceneMembership => "narrative.scene_membership",
            Self::EpisodeMembership => "narrative.episode_membership",
            Self::ConflictParticipation => "narrative.conflict_participation",
            Self::CharacterState => "narrative.character_state",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationRelation {
    SessionMembership,
    Speaker,
    Preference,
    Commitment,
    Correction,
}

impl VocabularyRelation for ConversationRelation {
    fn stable_name(self) -> &'static str {
        match self {
            Self::SessionMembership => "conversation.session_membership",
            Self::Speaker => "conversation.speaker",
            Self::Preference => "conversation.preference",
            Self::Commitment => "conversation.commitment",
            Self::Correction => "conversation.correction",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentRelation {
    SectionMembership,
    Claim,
    Citation,
    Definition,
    Revision,
}

impl VocabularyRelation for DocumentRelation {
    fn stable_name(self) -> &'static str {
        match self {
            Self::SectionMembership => "document.section_membership",
            Self::Claim => "document.claim",
            Self::Citation => "document.citation",
            Self::Definition => "document.definition",
            Self::Revision => "document.revision",
        }
    }
}
