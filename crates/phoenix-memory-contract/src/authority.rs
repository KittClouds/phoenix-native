pub const SOURCE_FLAG_COMPLETE: u16 = 1;
pub const TIME_UNBOUNDED: i64 = i64::MAX;
pub const TIME_UNKNOWN: i64 = i64::MIN + 1;
pub const TIMEZONE_OFFSET_UNKNOWN: i32 = i32::MIN;

pub const TEMPORAL_FLAG_SOURCE_TIME: u32 = 1 << 0;
pub const TEMPORAL_FLAG_ASSERTED_TIME: u32 = 1 << 1;
pub const TEMPORAL_FLAG_OCCURRENCE_TIME: u32 = 1 << 2;
pub const TEMPORAL_FLAG_OBSERVED_TIME: u32 = 1 << 3;
pub const TEMPORAL_FLAG_EXPLICIT_TEXT: u32 = 1 << 4;
pub const TEMPORAL_FLAG_NORMALIZED: u32 = 1 << 5;
pub const TEMPORAL_FLAG_UNCERTAIN: u32 = 1 << 6;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum TemporalPrecisionV1 {
    Unknown = 1,
    Instant = 2,
    Minute = 3,
    Hour = 4,
    Day = 5,
    Month = 6,
    Year = 7,
    Interval = 8,
    Relative = 9,
    Ordinal = 10,
}

impl TemporalPrecisionV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Unknown),
            2 => Some(Self::Instant),
            3 => Some(Self::Minute),
            4 => Some(Self::Hour),
            5 => Some(Self::Day),
            6 => Some(Self::Month),
            7 => Some(Self::Year),
            8 => Some(Self::Interval),
            9 => Some(Self::Relative),
            10 => Some(Self::Ordinal),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum TemporalSubjectKindV1 {
    SemanticCandidate = 1,
    Event = 2,
    Episode = 3,
    Turn = 4,
}

impl TemporalSubjectKindV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::SemanticCandidate),
            2 => Some(Self::Event),
            3 => Some(Self::Episode),
            4 => Some(Self::Turn),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum TemporalBindingRoleV1 {
    Primary = 1,
    Member = 2,
    Context = 3,
}

impl TemporalBindingRoleV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Primary),
            2 => Some(Self::Member),
            3 => Some(Self::Context),
            _ => None,
        }
    }
}

/// Semantic model duties are explicit. A dedicated NLI observer is not a
/// fallback for steerable classification, and a steerable classifier is not
/// silently substituted for the dedicated NLI lane.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ModelSemanticRoleV3 {
    SteerableSemanticObserver = 1,
    DedicatedNliObserver = 2,
    Other = 3,
}

impl ModelSemanticRoleV3 {
    pub const MASK: u32 = 0xff;

    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::SteerableSemanticObserver),
            2 => Some(Self::DedicatedNliObserver),
            3 => Some(Self::Other),
            _ => None,
        }
    }

    pub const fn from_flags(flags: u32) -> Option<Self> {
        Self::from_raw((flags & Self::MASK) as u16)
    }

    pub const fn flags(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum VocabularyPackKindV3 {
    Core = 1,
    Lens = 2,
}

impl VocabularyPackKindV3 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Core),
            2 => Some(Self::Lens),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum CandidateEndpointRoleV3 {
    Subject = 1,
    Object = 2,
    Participant = 3,
    Context = 4,
}

impl CandidateEndpointRoleV3 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Subject),
            2 => Some(Self::Object),
            3 => Some(Self::Participant),
            4 => Some(Self::Context),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SourceKind {
    WorkspaceDocument = 1,
    Conversation = 2,
}

impl SourceKind {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::WorkspaceDocument),
            2 => Some(Self::Conversation),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ParticipantRole {
    System = 1,
    User = 2,
    Assistant = 3,
    Tool = 4,
    Other = 5,
}

impl ParticipantRole {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::System),
            2 => Some(Self::User),
            3 => Some(Self::Assistant),
            4 => Some(Self::Tool),
            5 => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ContentUnitKind {
    Document = 1,
    Chapter = 2,
    Paragraph = 3,
    Sentence = 4,
    DynamicChunk = 5,
    Span = 6,
    Turn = 7,
    TurnSubchunk = 8,
}

impl ContentUnitKind {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Document),
            2 => Some(Self::Chapter),
            3 => Some(Self::Paragraph),
            4 => Some(Self::Sentence),
            5 => Some(Self::DynamicChunk),
            6 => Some(Self::Span),
            7 => Some(Self::Turn),
            8 => Some(Self::TurnSubchunk),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum AuthoritySubjectKind {
    SemanticCandidate = 1,
    AcceptedFact = 2,
    Decision = 3,
    Supersession = 4,
}

impl AuthoritySubjectKind {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::SemanticCandidate),
            2 => Some(Self::AcceptedFact),
            3 => Some(Self::Decision),
            4 => Some(Self::Supersession),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ProducerProductV3 {
    SourceStructure = 1,
    ContentUnitsAndChunks = 2,
    MentionsAndEvidence = 3,
    CanonicalEntityBindings = 4,
    IdentityCoreference = 5,
    ClaimsAttributes = 6,
    Relationships = 7,
    EventsTemporal = 8,
    Causality = 9,
    StateBelief = 10,
    CorrectionsSupersession = 11,
    GoalsProcedures = 12,
    ContextualEvidence = 13,
}

impl ProducerProductV3 {
    pub const ALL: [Self; 13] = [
        Self::SourceStructure,
        Self::ContentUnitsAndChunks,
        Self::MentionsAndEvidence,
        Self::CanonicalEntityBindings,
        Self::IdentityCoreference,
        Self::ClaimsAttributes,
        Self::Relationships,
        Self::EventsTemporal,
        Self::Causality,
        Self::StateBelief,
        Self::CorrectionsSupersession,
        Self::GoalsProcedures,
        Self::ContextualEvidence,
    ];

    pub const fn from_raw(raw: u16) -> Option<Self> {
        if raw == 0 || raw > Self::ALL.len() as u16 {
            return None;
        }
        Some(Self::ALL[(raw - 1) as usize])
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ProducerStateV3 {
    Produced = 1,
    DurableVerified = 2,
    Unsupported = 3,
    Cancelled = 4,
    Failed = 5,
}

impl ProducerStateV3 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Produced),
            2 => Some(Self::DurableVerified),
            3 => Some(Self::Unsupported),
            4 => Some(Self::Cancelled),
            5 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SemanticCandidateFamilyV3 {
    Identity = 1,
    Coreference = 2,
    Claim = 3,
    Attribute = 4,
    Relationship = 5,
    Event = 6,
    Temporal = 7,
    Causal = 8,
    State = 9,
    Belief = 10,
    Correction = 11,
    Supersession = 12,
    Goal = 13,
    Procedure = 14,
    ContextualEvidence = 15,
}

impl SemanticCandidateFamilyV3 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Identity),
            2 => Some(Self::Coreference),
            3 => Some(Self::Claim),
            4 => Some(Self::Attribute),
            5 => Some(Self::Relationship),
            6 => Some(Self::Event),
            7 => Some(Self::Temporal),
            8 => Some(Self::Causal),
            9 => Some(Self::State),
            10 => Some(Self::Belief),
            11 => Some(Self::Correction),
            12 => Some(Self::Supersession),
            13 => Some(Self::Goal),
            14 => Some(Self::Procedure),
            15 => Some(Self::ContextualEvidence),
            _ => None,
        }
    }
}
