use phoenix_memory_contract::ModelSemanticRoleV3;
use thiserror::Error;

pub const CUE_EXPLICIT_CORRECTION: u32 = 1 << 0;
pub const CUE_NEGATED_PROPOSITION: u32 = 1 << 1;
pub const CUE_TEMPORAL_QUALIFIER: u32 = 1 << 2;
pub const CUE_SCOPE_QUALIFIER: u32 = 1 << 3;
pub const CUE_UNCERTAINTY: u32 = 1 << 4;
pub const CUE_CURRENT_STATE: u32 = 1 << 5;
pub const CUE_PREVIOUS_STATE: u32 = 1 << 6;
pub const CUE_FUTURE_INTENTION: u32 = 1 << 7;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum MemoryEventV1 {
    Repetition = 1,
    Corroboration = 2,
    Elaboration = 3,
    Specialization = 4,
    Generalization = 5,
    TemporalUpdate = 6,
    ScopeUpdate = 7,
    PreferenceShift = 8,
    ExplicitCorrection = 9,
    Retraction = 10,
    Denial = 11,
    ConditionalStatement = 12,
    HypotheticalStatement = 13,
    PlannedState = 14,
    AbandonedPlan = 15,
    ApparentConflict = 16,
    HardConflict = 17,
    Unrelated = 18,
    Ambiguous = 19,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum NliRelationV1 {
    Entailment = 1,
    Contradiction = 2,
    Neutral = 3,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ScopeRelationV1 {
    Same = 1,
    Different = 2,
    Overlapping = 3,
    Unknown = 4,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum TemporalRelationV1 {
    CurrentOverCurrent = 1,
    LaterState = 2,
    HistoricalContext = 3,
    FutureState = 4,
    Unknown = 5,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SourceAuthorityV1 {
    SubjectExplicit = 1,
    CuratorConfirmed = 2,
    PinnedResult = 3,
    Inferred = 4,
    NonAuthoritative = 5,
}

impl SourceAuthorityV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::SubjectExplicit),
            2 => Some(Self::CuratorConfirmed),
            3 => Some(Self::PinnedResult),
            4 => Some(Self::Inferred),
            5 => Some(Self::NonAuthoritative),
            _ => None,
        }
    }

    const fn may_change_subject_truth(self) -> bool {
        matches!(self, Self::SubjectExplicit | Self::CuratorConfirmed)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum MemoryActionV1 {
    AddCandidate = 1,
    AddEvidence = 2,
    Elaborate = 3,
    CloseAndReplace = 4,
    Supersede = 5,
    RetainBoth = 6,
    OpenDispute = 7,
    PreserveHistorical = 8,
    CandidateOnly = 9,
    Ignore = 10,
    Defer = 11,
}

impl MemoryActionV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::AddCandidate),
            2 => Some(Self::AddEvidence),
            3 => Some(Self::Elaborate),
            4 => Some(Self::CloseAndReplace),
            5 => Some(Self::Supersede),
            6 => Some(Self::RetainBoth),
            7 => Some(Self::OpenDispute),
            8 => Some(Self::PreserveHistorical),
            9 => Some(Self::CandidateOnly),
            10 => Some(Self::Ignore),
            11 => Some(Self::Defer),
            _ => None,
        }
    }

    pub const fn requires_replacement_target(self) -> bool {
        matches!(self, Self::CloseAndReplace | Self::Supersede)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum PolicyReasonV1 {
    NewCompatibleMemory = 1,
    CorroboratingEvidence = 2,
    CompatibleElaboration = 3,
    AuthoritativeCorrection = 4,
    LaterStateTransition = 5,
    DistinctScope = 6,
    UnresolvedConflict = 7,
    HistoricalStatement = 8,
    NonCurrentModality = 9,
    UnrelatedObservation = 10,
    InsufficientConfidence = 11,
    InsufficientEvidence = 12,
    AmbiguousSemantics = 13,
}

impl PolicyReasonV1 {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::NewCompatibleMemory),
            2 => Some(Self::CorroboratingEvidence),
            3 => Some(Self::CompatibleElaboration),
            4 => Some(Self::AuthoritativeCorrection),
            5 => Some(Self::LaterStateTransition),
            6 => Some(Self::DistinctScope),
            7 => Some(Self::UnresolvedConflict),
            8 => Some(Self::HistoricalStatement),
            9 => Some(Self::NonCurrentModality),
            10 => Some(Self::UnrelatedObservation),
            11 => Some(Self::InsufficientConfidence),
            12 => Some(Self::InsufficientEvidence),
            13 => Some(Self::AmbiguousSemantics),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SemanticAdjudicationInputV1 {
    pub memory_event: MemoryEventV1,
    pub memory_event_score: f32,
    pub nli_relation: NliRelationV1,
    pub nli_score: f32,
    pub scope_relation: ScopeRelationV1,
    pub temporal_relation: TemporalRelationV1,
    pub source_authority: SourceAuthorityV1,
    pub semantic_cues: u32,
    pub evidence_count: u16,
    pub gliclass_role: ModelSemanticRoleV3,
    pub modernbert_role: ModelSemanticRoleV3,
}

impl SemanticAdjudicationInputV1 {
    pub fn observation_identity(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix.semantic-adjudication-observation/v1\0");
        hasher.update(&(self.memory_event as u16).to_le_bytes());
        hasher.update(&self.memory_event_score.to_bits().to_le_bytes());
        hasher.update(&(self.nli_relation as u16).to_le_bytes());
        hasher.update(&self.nli_score.to_bits().to_le_bytes());
        hasher.update(&(self.scope_relation as u16).to_le_bytes());
        hasher.update(&(self.temporal_relation as u16).to_le_bytes());
        hasher.update(&(self.source_authority as u16).to_le_bytes());
        hasher.update(&self.semantic_cues.to_le_bytes());
        hasher.update(&self.evidence_count.to_le_bytes());
        hasher.update(&(self.gliclass_role as u16).to_le_bytes());
        hasher.update(&(self.modernbert_role as u16).to_le_bytes());
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProposalV1 {
    pub action: MemoryActionV1,
    pub reason: PolicyReasonV1,
    pub close_existing_validity: bool,
    pub preserve_history: bool,
    pub requires_explicit_decision: bool,
}

impl PolicyProposalV1 {
    const fn new(action: MemoryActionV1, reason: PolicyReasonV1) -> Self {
        Self {
            action,
            reason,
            close_existing_validity: false,
            preserve_history: true,
            requires_explicit_decision: true,
        }
    }

    const fn closing(action: MemoryActionV1, reason: PolicyReasonV1) -> Self {
        Self {
            close_existing_validity: true,
            ..Self::new(action, reason)
        }
    }

    pub const fn is_constitutional(self) -> bool {
        let pair_is_valid = matches!(
            (self.action, self.reason),
            (
                MemoryActionV1::AddCandidate,
                PolicyReasonV1::NewCompatibleMemory
            ) | (
                MemoryActionV1::AddEvidence,
                PolicyReasonV1::CorroboratingEvidence
            ) | (
                MemoryActionV1::Elaborate,
                PolicyReasonV1::CompatibleElaboration
            ) | (
                MemoryActionV1::Supersede,
                PolicyReasonV1::AuthoritativeCorrection
            ) | (
                MemoryActionV1::CloseAndReplace,
                PolicyReasonV1::LaterStateTransition
            ) | (MemoryActionV1::RetainBoth, PolicyReasonV1::DistinctScope)
                | (
                    MemoryActionV1::OpenDispute,
                    PolicyReasonV1::UnresolvedConflict
                )
                | (
                    MemoryActionV1::PreserveHistorical,
                    PolicyReasonV1::HistoricalStatement
                )
                | (
                    MemoryActionV1::CandidateOnly,
                    PolicyReasonV1::NonCurrentModality
                )
                | (MemoryActionV1::Ignore, PolicyReasonV1::UnrelatedObservation)
                | (
                    MemoryActionV1::Defer,
                    PolicyReasonV1::InsufficientConfidence
                )
                | (MemoryActionV1::Defer, PolicyReasonV1::InsufficientEvidence)
                | (MemoryActionV1::Defer, PolicyReasonV1::AmbiguousSemantics)
        );
        let closure_is_valid = self.close_existing_validity
            == matches!(
                self.action,
                MemoryActionV1::Supersede | MemoryActionV1::CloseAndReplace
            );
        pair_is_valid
            && closure_is_valid
            && self.preserve_history
            && self.requires_explicit_decision
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AdjudicationError {
    #[error("GLiClass observations require the steerable semantic-observer lane")]
    InvalidGliclassLane,
    #[error("NLI observations require the dedicated ModernBERT-NLI lane")]
    InvalidModernbertLane,
    #[error("semantic and NLI scores must be finite and within [0, 1]")]
    InvalidScore,
}

#[derive(Clone, Copy, Debug)]
pub struct DeterministicAdjudicatorV1 {
    minimum_event_score: f32,
    minimum_nli_score: f32,
}

impl Default for DeterministicAdjudicatorV1 {
    fn default() -> Self {
        Self::new(0.75, 0.75).expect("static adjudication thresholds are valid")
    }
}

impl DeterministicAdjudicatorV1 {
    pub fn new(
        minimum_event_score: f32,
        minimum_nli_score: f32,
    ) -> Result<Self, AdjudicationError> {
        if !valid_score(minimum_event_score) || !valid_score(minimum_nli_score) {
            return Err(AdjudicationError::InvalidScore);
        }
        Ok(Self {
            minimum_event_score,
            minimum_nli_score,
        })
    }

    pub fn adjudicate(
        &self,
        input: SemanticAdjudicationInputV1,
    ) -> Result<PolicyProposalV1, AdjudicationError> {
        validate_input(input)?;
        if input.evidence_count == 0 {
            return Ok(PolicyProposalV1::new(
                MemoryActionV1::Defer,
                PolicyReasonV1::InsufficientEvidence,
            ));
        }
        if input.memory_event_score < self.minimum_event_score
            || input.nli_score < self.minimum_nli_score
        {
            return Ok(PolicyProposalV1::new(
                MemoryActionV1::Defer,
                PolicyReasonV1::InsufficientConfidence,
            ));
        }
        Ok(decide(input))
    }

    pub fn policy_identity(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix.deterministic-adjudicator/v1\0");
        hasher.update(&self.minimum_event_score.to_bits().to_le_bytes());
        hasher.update(&self.minimum_nli_score.to_bits().to_le_bytes());
        *hasher.finalize().as_bytes()
    }
}

fn validate_input(input: SemanticAdjudicationInputV1) -> Result<(), AdjudicationError> {
    if input.gliclass_role != ModelSemanticRoleV3::SteerableSemanticObserver {
        return Err(AdjudicationError::InvalidGliclassLane);
    }
    if input.modernbert_role != ModelSemanticRoleV3::DedicatedNliObserver {
        return Err(AdjudicationError::InvalidModernbertLane);
    }
    if !valid_score(input.memory_event_score) || !valid_score(input.nli_score) {
        return Err(AdjudicationError::InvalidScore);
    }
    Ok(())
}

const fn valid_score(value: f32) -> bool {
    value.is_finite() && value >= 0.0 && value <= 1.0
}

fn decide(input: SemanticAdjudicationInputV1) -> PolicyProposalV1 {
    use MemoryActionV1 as Action;
    use MemoryEventV1 as Event;
    use NliRelationV1 as Nli;
    use PolicyReasonV1 as Reason;
    use ScopeRelationV1 as Scope;
    use TemporalRelationV1 as Time;

    match (
        input.memory_event,
        input.nli_relation,
        input.scope_relation,
        input.temporal_relation,
    ) {
        (
            Event::ExplicitCorrection | Event::Retraction | Event::Denial,
            Nli::Contradiction,
            Scope::Same,
            _,
        ) if input.source_authority.may_change_subject_truth()
            && input.semantic_cues & CUE_EXPLICIT_CORRECTION != 0 =>
        {
            PolicyProposalV1::closing(Action::Supersede, Reason::AuthoritativeCorrection)
        }
        (
            Event::TemporalUpdate | Event::PreferenceShift,
            Nli::Contradiction,
            Scope::Same,
            Time::LaterState,
        ) if input.source_authority.may_change_subject_truth()
            && input.semantic_cues & CUE_TEMPORAL_QUALIFIER != 0 =>
        {
            PolicyProposalV1::closing(Action::CloseAndReplace, Reason::LaterStateTransition)
        }
        (Event::ScopeUpdate, _, Scope::Different, _) => {
            PolicyProposalV1::new(Action::RetainBoth, Reason::DistinctScope)
        }
        (Event::Repetition | Event::Corroboration, Nli::Entailment, _, _) => {
            PolicyProposalV1::new(Action::AddEvidence, Reason::CorroboratingEvidence)
        }
        (
            Event::Elaboration | Event::Specialization | Event::Generalization,
            Nli::Entailment | Nli::Neutral,
            _,
            _,
        ) => PolicyProposalV1::new(Action::Elaborate, Reason::CompatibleElaboration),
        (_, _, _, Time::HistoricalContext) => {
            PolicyProposalV1::new(Action::PreserveHistorical, Reason::HistoricalStatement)
        }
        (
            Event::ConditionalStatement
            | Event::HypotheticalStatement
            | Event::PlannedState
            | Event::AbandonedPlan,
            _,
            _,
            _,
        ) => PolicyProposalV1::new(Action::CandidateOnly, Reason::NonCurrentModality),
        (Event::ApparentConflict | Event::HardConflict, Nli::Contradiction, _, _) => {
            PolicyProposalV1::new(Action::OpenDispute, Reason::UnresolvedConflict)
        }
        (Event::Unrelated, _, _, _) => {
            PolicyProposalV1::new(Action::Ignore, Reason::UnrelatedObservation)
        }
        (Event::Ambiguous, _, _, _) => {
            PolicyProposalV1::new(Action::Defer, Reason::AmbiguousSemantics)
        }
        (_, Nli::Neutral, Scope::Different, _) => {
            PolicyProposalV1::new(Action::AddCandidate, Reason::NewCompatibleMemory)
        }
        _ => PolicyProposalV1::new(Action::OpenDispute, Reason::UnresolvedConflict),
    }
}
