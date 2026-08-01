use phoenix_graph_generation_v2::{
    CandidateId, ChunkId, ContextualEvidenceRecord, EntityId, EpisodeId, EventId, EvidenceId,
    VerifiedGraphGenerationV2,
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RelationshipKind {
    CommunicatesWith = 1,
    Supports = 2,
    Opposes = 3,
    Owns = 4,
    LocatedIn = 5,
    ParticipatesIn = 6,
    Knows = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum EventKind {
    Action = 1,
    Encounter = 2,
    Transfer = 3,
    Decision = 4,
    StateChange = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum EpisodeFamily {
    Scene = 1,
    Sequence = 2,
    Conflict = 3,
    Transition = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum TemporalRelation {
    Before = 1,
    After = 2,
    Simultaneous = 3,
    During = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum CausalRelation {
    Causes = 1,
    Enables = 2,
    Prevents = 3,
    Motivates = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum MemoryStateKind {
    Knows = 1,
    Believes = 2,
    Remembers = 3,
    Wants = 4,
    Decides = 5,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SemanticEndpoint {
    Entity(EntityId),
    Event(EventId),
    Episode(EpisodeId),
    Chunk(ChunkId),
}

impl SemanticEndpoint {
    pub(crate) const fn raw(self) -> u64 {
        match self {
            Self::Entity(id) => id.0,
            Self::Event(id) => id.0,
            Self::Episode(id) => id.0,
            Self::Chunk(id) => id.0,
        }
    }

    pub(crate) const fn tag(self) -> u32 {
        match self {
            Self::Entity(_) => 1,
            Self::Event(_) => 2,
            Self::Episode(_) => 3,
            Self::Chunk(_) => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EpisodeMember {
    Chunk(ChunkId),
    Event(EventId),
}

impl EpisodeMember {
    pub(crate) const fn raw(self) -> u64 {
        match self {
            Self::Chunk(id) => id.0,
            Self::Event(id) => id.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RelationshipCandidateInput<'a> {
    pub candidate_id: CandidateId,
    pub source_entity_id: EntityId,
    pub target_entity_id: EntityId,
    pub source_evidence_id: EvidenceId,
    pub target_evidence_id: EvidenceId,
    pub additional_evidence_ids: &'a [EvidenceId],
    pub relation: RelationshipKind,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct EventCandidateInput<'a> {
    pub event_id: EventId,
    pub label: &'a str,
    pub label_start: u32,
    pub label_end: u32,
    pub evidence_ids: &'a [EvidenceId],
    pub kind: EventKind,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct EpisodeMembershipInput<'a> {
    pub member: EpisodeMember,
    pub evidence_ids: &'a [EvidenceId],
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct EpisodeCandidateInput<'a> {
    pub episode_id: EpisodeId,
    pub label: &'a str,
    pub label_start: u32,
    pub label_end: u32,
    pub evidence_ids: &'a [EvidenceId],
    pub memberships: &'a [EpisodeMembershipInput<'a>],
    pub ordinal: u32,
    pub family: EpisodeFamily,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TemporalCandidateInput<'a> {
    pub candidate_id: CandidateId,
    pub source: SemanticEndpoint,
    pub target: SemanticEndpoint,
    pub evidence_ids: &'a [EvidenceId],
    pub relation: TemporalRelation,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct CausalCandidateInput<'a> {
    pub candidate_id: CandidateId,
    pub cause: SemanticEndpoint,
    pub effect: SemanticEndpoint,
    pub evidence_ids: &'a [EvidenceId],
    pub relation: CausalRelation,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryStateCandidateInput<'a> {
    pub candidate_id: CandidateId,
    pub subject_entity_id: EntityId,
    pub context: SemanticEndpoint,
    pub key: &'a str,
    pub value: &'a str,
    pub evidence_ids: &'a [EvidenceId],
    pub kind: MemoryStateKind,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum ProducerRegistration<'a, T> {
    Unsupported {
        producer_id: &'a str,
    },
    Deterministic {
        producer_id: &'a str,
        rules: &'a [T],
    },
}

impl<T> ProducerRegistration<'_, T> {
    pub(crate) const fn producer_id(&self) -> &str {
        match self {
            Self::Unsupported { producer_id } | Self::Deterministic { producer_id, .. } => {
                producer_id
            }
        }
    }

    pub(crate) const fn is_supported(&self) -> bool {
        matches!(self, Self::Deterministic { .. })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StoryRegistrations<'a> {
    pub relationships: ProducerRegistration<'a, RelationshipCandidateInput<'a>>,
    pub events: ProducerRegistration<'a, EventCandidateInput<'a>>,
    pub episodes: ProducerRegistration<'a, EpisodeCandidateInput<'a>>,
    pub temporal: ProducerRegistration<'a, TemporalCandidateInput<'a>>,
    pub causal: ProducerRegistration<'a, CausalCandidateInput<'a>>,
    pub memory_state: ProducerRegistration<'a, MemoryStateCandidateInput<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ModelIdentityInput<'a> {
    pub name: &'a str,
    pub runtime: &'a str,
    pub artifact_uri: &'a str,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
pub struct ModelScoreInput {
    /// The same content-bound candidate key used by deterministic production.
    pub candidate_id: CandidateId,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ModelRankingBatch<'a> {
    pub model: ModelIdentityInput<'a>,
    pub scores: &'a [ModelScoreInput],
}

#[derive(Clone, Copy, Debug)]
pub struct StoryProducerInput<'a> {
    pub text: &'a str,
    pub source: &'a VerifiedGraphGenerationV2,
    pub registrations: StoryRegistrations<'a>,
    /// Context-only co-occurrence evidence. This never becomes accepted topology.
    pub contextual_evidence: &'a [ContextualEvidenceRecord],
    pub model_ranking: Option<ModelRankingBatch<'a>>,
    pub producer_binary_hash: [u8; 32],
    pub published_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryPublicationReceipt {
    pub path: PathBuf,
    pub previous_generation_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub candidate_authority_hash: [u8; 32],
    pub relationship_count: u32,
    pub event_count: u32,
    pub episode_count: u32,
    pub membership_count: u32,
    pub temporal_count: u32,
    pub causal_count: u32,
    pub memory_state_count: u32,
    pub contextual_evidence_count: u32,
    pub evidence_binding_count: u32,
    pub model_ranked_count: u32,
    /// One bit per story product in registration order. Set means unsupported.
    pub unsupported_mask: u16,
}
