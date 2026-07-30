use crate::{
    CausalRelation, EpisodeFamily, EventKind, MemoryStateKind, RelationshipKind, TemporalRelation,
};
use phoenix_graph_generation_v2::CandidateId;
use phoenix_semantic_lens::{
    validate_definition, CandidateOrigin, CoreSemanticClass, EndpointMask, LensCodeDefinition,
    LensIdentity, SemanticLensDefinition, SemanticLensError,
};

pub const STORY_LENS_NAMESPACE: &str = "phoenix.story/v1";
pub const STORY_CANDIDATE_NAMESPACE: &str = "phoenix.story-candidate/v1";

const ENTITY: EndpointMask = EndpointMask::ENTITY;
const STORY_CONTEXT: EndpointMask = EndpointMask::CHUNK
    .union(EndpointMask::ENTITY)
    .union(EndpointMask::OCCURRENCE)
    .union(EndpointMask::GROUPING);
const SEMANTIC_ENDPOINT: EndpointMask = STORY_CONTEXT;

const STORY_CODES: [LensCodeDefinition<'static>; 29] = [
    relation(1, "relationship.communicates_with"),
    relation(2, "relationship.supports"),
    relation(3, "relationship.opposes"),
    relation(4, "relationship.owns"),
    relation(5, "relationship.located_in"),
    relation(6, "relationship.participates_in"),
    relation(7, "relationship.knows"),
    occurrence(1, "occurrence.action"),
    occurrence(2, "occurrence.encounter"),
    occurrence(3, "occurrence.transfer"),
    occurrence(4, "occurrence.decision"),
    occurrence(5, "occurrence.state_change"),
    grouping(1, "grouping.scene"),
    grouping(2, "grouping.sequence"),
    grouping(3, "grouping.conflict"),
    grouping(4, "grouping.transition"),
    temporal(1, "temporal.before"),
    temporal(2, "temporal.after"),
    temporal(3, "temporal.simultaneous"),
    temporal(4, "temporal.during"),
    influence(1, "influence.causes"),
    influence(2, "influence.enables"),
    influence(3, "influence.prevents"),
    influence(4, "influence.motivates"),
    state(1, "state.knows"),
    state(2, "state.believes"),
    state(3, "state.remembers"),
    state(4, "state.wants"),
    state(5, "state.decides"),
];

pub fn story_lens_definition() -> SemanticLensDefinition<'static> {
    SemanticLensDefinition {
        namespace: STORY_LENS_NAMESPACE,
        version: 1,
        configuration_hash: *blake3::hash(b"phoenix.story/v1/default-config").as_bytes(),
        codes: &STORY_CODES,
    }
}

pub fn story_lens_identity() -> Result<LensIdentity, SemanticLensError> {
    validate_definition(&story_lens_definition())
}

pub const fn story_semantic_code(class: CoreSemanticClass, local_code: u16) -> u32 {
    ((class as u32) << 16) | local_code as u32
}

pub fn story_candidate_origin(
    candidate_id: CandidateId,
    class: CoreSemanticClass,
    local_code: u16,
) -> Result<CandidateOrigin, SemanticLensError> {
    CandidateOrigin::from_identity(
        story_lens_identity()?,
        story_semantic_code(class, local_code),
        class,
        candidate_id,
    )
}

pub const fn relationship_code(kind: RelationshipKind) -> u32 {
    story_semantic_code(CoreSemanticClass::Relation, kind as u16)
}

pub const fn event_code(kind: EventKind) -> u32 {
    story_semantic_code(CoreSemanticClass::Occurrence, kind as u16)
}

pub const fn episode_code(family: EpisodeFamily) -> u32 {
    story_semantic_code(CoreSemanticClass::Grouping, family as u16)
}

pub const fn temporal_code(relation: TemporalRelation) -> u32 {
    story_semantic_code(CoreSemanticClass::TemporalConstraint, relation as u16)
}

pub const fn causal_code(relation: CausalRelation) -> u32 {
    story_semantic_code(CoreSemanticClass::Influence, relation as u16)
}

pub const fn memory_state_code(kind: MemoryStateKind) -> u32 {
    story_semantic_code(CoreSemanticClass::AttributedState, kind as u16)
}

const fn relation(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::Relation,
        local,
        stable_name,
        ENTITY,
        ENTITY,
    )
}

const fn occurrence(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::Occurrence,
        local,
        stable_name,
        STORY_CONTEXT,
        EndpointMask::NONE,
    )
}

const fn grouping(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::Grouping,
        local,
        stable_name,
        EndpointMask::GROUPING,
        EndpointMask::CHUNK.union(EndpointMask::OCCURRENCE),
    )
}

const fn temporal(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::TemporalConstraint,
        local,
        stable_name,
        SEMANTIC_ENDPOINT,
        SEMANTIC_ENDPOINT,
    )
}

const fn influence(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::Influence,
        local,
        stable_name,
        SEMANTIC_ENDPOINT,
        SEMANTIC_ENDPOINT,
    )
}

const fn state(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    code(
        CoreSemanticClass::AttributedState,
        local,
        stable_name,
        ENTITY,
        STORY_CONTEXT,
    )
}

const fn code(
    class: CoreSemanticClass,
    local: u16,
    stable_name: &'static str,
    source_endpoints: EndpointMask,
    target_endpoints: EndpointMask,
) -> LensCodeDefinition<'static> {
    LensCodeDefinition {
        code: story_semantic_code(class, local),
        stable_name,
        class,
        source_endpoints,
        target_endpoints,
        flags: 0,
    }
}
