use crate::{
    CandidateEvidenceBindingRecord, CandidateId, CandidateStatus, CanonicalBindingKind,
    CanonicalEntityBindingRecord, CausalCandidateRecord, ChunkRecord, ContextualEvidenceRecord,
    EntityRecord, EpisodeMemberKind, EpisodeMembershipRecord, EpisodeRecord, EventRecord,
    EvidenceRecord, IdentityCandidateRecord, MemoryStateCandidateRecord, MentionRecord, PageKind,
    StructuralEdgeRecord, TemporalCandidateRecord, TypedRelationshipCandidateRecord,
    VerifiedGraphGenerationV2,
};
use hashbrown::{HashMap, HashSet};
use thiserror::Error;

pub const ENDPOINT_SOURCE_SHIFT: u32 = 8;
pub const ENDPOINT_TARGET_SHIFT: u32 = 12;
const ENDPOINT_TAG_MASK: u32 = 0x0f;

macro_rules! tagged_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident = $value:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(u16)]
        pub enum $name { $($variant = $value),+ }

        impl $name {
            pub const fn from_raw(raw: u16) -> Option<Self> {
                match raw { $($value => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

tagged_enum! {
    /// Frozen source-containment relations emitted by the native document producer.
    pub enum StructuralRelationKind {
        DocumentContainsChapter = 1,
        ChapterContainsParagraph = 2,
        ParagraphContainsSentence = 3,
        DocumentContainsDynamicChunk = 4
    }
}

tagged_enum! {
    /// Directional temporal truth. Values 1-4 preserve the V2 byte contract.
    pub enum TemporalRelationKind {
        Before = 1,
        After = 2,
        Simultaneous = 3,
        During = 4,
        Contains = 5,
        Starts = 6,
        Finishes = 7,
        Overlaps = 8,
        RecursAfter = 9,
        Supersedes = 10
    }
}

tagged_enum! {
    /// Directional causal truth. Values 1-4 preserve the V2 byte contract.
    pub enum CausalRelationKind {
        DirectCause = 1,
        EnablingCondition = 2,
        Prevention = 3,
        Motivation = 4,
        Explanation = 5,
        Consequence = 6
    }
}

tagged_enum! {
    /// Endpoint tags packed into semantic candidate flags.
    pub enum SemanticEndpointKind {
        Entity = 1,
        Event = 2,
        Episode = 3,
        DynamicChunk = 4
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TopologyNodeKind {
    Document,
    Chapter,
    Paragraph,
    Sentence,
    DynamicChunk,
    Span,
    Entity,
    Evidence,
    Event,
    Episode,
}

/// Dense, one-pass validation view over a verified packed generation.
///
/// This does not create a second graph. It is an ID-to-kind index used while
/// compiling to prove that every edge endpoint exists and has the producer-
/// declared type. It is dropped after compilation.
pub struct VerifiedTopologyV2 {
    node_kinds: HashMap<u64, TopologyNodeKind>,
    source_node_ids: HashSet<u64>,
}

impl VerifiedTopologyV2 {
    pub fn open(generation: &VerifiedGraphGenerationV2) -> Result<Self, TopologyValidationError> {
        let documents = page::<crate::DocumentRecord>(generation, PageKind::Documents)?;
        let [document] = documents else {
            return Err(TopologyValidationError::DocumentCount(documents.len()));
        };
        let chapters = page::<crate::ChapterRecord>(generation, PageKind::Chapters)?;
        let paragraphs = page::<crate::ParagraphRecord>(generation, PageKind::Paragraphs)?;
        let sentences = page::<crate::SentenceRecord>(generation, PageKind::Sentences)?;
        let chunks = page::<ChunkRecord>(generation, PageKind::Chunks)?;
        let spans = page::<crate::SpanRecord>(generation, PageKind::Spans)?;
        let entities = page::<EntityRecord>(generation, PageKind::Entities)?;
        let mentions = page::<MentionRecord>(generation, PageKind::Mentions)?;
        let evidence = page::<EvidenceRecord>(generation, PageKind::Evidence)?;
        let events = page::<EventRecord>(generation, PageKind::Events)?;
        let episodes = page::<EpisodeRecord>(generation, PageKind::Episodes)?;

        let capacity = 1
            + chapters.len()
            + paragraphs.len()
            + sentences.len()
            + chunks.len()
            + spans.len()
            + entities.len()
            + evidence.len()
            + events.len()
            + episodes.len();
        let mut node_kinds = HashMap::with_capacity(capacity);
        let mut source_node_ids = HashSet::with_capacity(
            1 + chapters.len() + paragraphs.len() + sentences.len() + chunks.len() + spans.len(),
        );
        insert(&mut node_kinds, document.id, TopologyNodeKind::Document)?;
        source_node_ids.insert(document.id);
        for (id, kind) in chapters
            .iter()
            .map(|r| (r.id, TopologyNodeKind::Chapter))
            .chain(
                paragraphs
                    .iter()
                    .map(|r| (r.id, TopologyNodeKind::Paragraph)),
            )
            .chain(sentences.iter().map(|r| (r.id, TopologyNodeKind::Sentence)))
            .chain(
                chunks
                    .iter()
                    .map(|r| (r.id, TopologyNodeKind::DynamicChunk)),
            )
            .chain(spans.iter().map(|r| (r.id, TopologyNodeKind::Span)))
        {
            insert(&mut node_kinds, id, kind)?;
            source_node_ids.insert(id);
        }
        for (id, kind) in entities
            .iter()
            .map(|r| (r.id, TopologyNodeKind::Entity))
            .chain(evidence.iter().map(|r| (r.id, TopologyNodeKind::Evidence)))
            .chain(events.iter().map(|r| (r.id, TopologyNodeKind::Event)))
            .chain(episodes.iter().map(|r| (r.id, TopologyNodeKind::Episode)))
        {
            insert(&mut node_kinds, id, kind)?;
        }

        validate_source_edges(page(generation, PageKind::StructuralEdges)?, &node_kinds)?;
        let mention_ids = validate_mentions(mentions, evidence, &node_kinds)?;
        validate_evidence(evidence, &node_kinds)?;
        validate_entity_bindings(generation, &node_kinds)?;
        validate_contextual_evidence(generation, &node_kinds, &mention_ids)?;
        validate_semantics(generation, &node_kinds, evidence)?;

        Ok(Self {
            node_kinds,
            source_node_ids,
        })
    }

    pub fn kind_of(&self, id: u64) -> Option<TopologyNodeKind> {
        self.node_kinds.get(&id).copied()
    }

    pub fn is_source_node(&self, id: u64) -> bool {
        self.source_node_ids.contains(&id)
    }

    pub fn require(
        &self,
        id: u64,
        expected: TopologyNodeKind,
    ) -> Result<(), TopologyValidationError> {
        require_kind(&self.node_kinds, id, expected)
    }
}

fn validate_source_edges(
    edges: &[StructuralEdgeRecord],
    kinds: &HashMap<u64, TopologyNodeKind>,
) -> Result<(), TopologyValidationError> {
    for edge in edges {
        let relation = StructuralRelationKind::from_raw(edge.relation).ok_or(
            TopologyValidationError::UnknownStructuralRelation(edge.relation),
        )?;
        let expected = match relation {
            StructuralRelationKind::DocumentContainsChapter => {
                (TopologyNodeKind::Document, TopologyNodeKind::Chapter)
            }
            StructuralRelationKind::ChapterContainsParagraph => {
                (TopologyNodeKind::Chapter, TopologyNodeKind::Paragraph)
            }
            StructuralRelationKind::ParagraphContainsSentence => {
                (TopologyNodeKind::Paragraph, TopologyNodeKind::Sentence)
            }
            StructuralRelationKind::DocumentContainsDynamicChunk => {
                (TopologyNodeKind::Document, TopologyNodeKind::DynamicChunk)
            }
        };
        require_kind(kinds, edge.source_id, expected.0)?;
        require_kind(kinds, edge.target_id, expected.1)?;
    }
    Ok(())
}

fn validate_mentions(
    mentions: &[MentionRecord],
    evidence: &[EvidenceRecord],
    kinds: &HashMap<u64, TopologyNodeKind>,
) -> Result<HashSet<u64>, TopologyValidationError> {
    let evidence_ids: HashSet<u64> = evidence.iter().map(|record| record.id).collect();
    let mut mention_ids = HashSet::with_capacity(mentions.len());
    for mention in mentions {
        if mention.id == 0 || !mention_ids.insert(mention.id) {
            return Err(TopologyValidationError::DuplicateId(mention.id));
        }
        require_kind(kinds, mention.entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, mention.chunk_id, TopologyNodeKind::DynamicChunk)?;
        if !evidence_ids.contains(&mention.evidence_id) {
            return Err(TopologyValidationError::MissingEndpoint(
                mention.evidence_id,
            ));
        }
    }
    Ok(mention_ids)
}

fn validate_entity_bindings(
    generation: &VerifiedGraphGenerationV2,
    kinds: &HashMap<u64, TopologyNodeKind>,
) -> Result<(), TopologyValidationError> {
    for record in
        page::<CanonicalEntityBindingRecord>(generation, PageKind::CanonicalEntityBindings)?
    {
        require_kind(kinds, record.source_entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.canonical_entity_id, TopologyNodeKind::Entity)?;
        match CanonicalBindingKind::from_raw(record.kind) {
            Some(CanonicalBindingKind::Direct)
                if record.decision_id == 0
                    && record.source_entity_id == record.canonical_entity_id => {}
            Some(CanonicalBindingKind::CoordinatorDecision) if record.decision_id != 0 => {}
            Some(_) => return Err(TopologyValidationError::InvalidCanonicalBinding),
            None => {
                return Err(TopologyValidationError::UnknownCanonicalBinding(
                    record.kind,
                ))
            }
        }
    }
    Ok(())
}

fn validate_contextual_evidence(
    generation: &VerifiedGraphGenerationV2,
    kinds: &HashMap<u64, TopologyNodeKind>,
    mention_ids: &HashSet<u64>,
) -> Result<(), TopologyValidationError> {
    for record in page::<ContextualEvidenceRecord>(generation, PageKind::ContextualEvidence)? {
        require_kind(kinds, record.source_entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.target_entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.chunk_id, TopologyNodeKind::DynamicChunk)?;
        if !mention_ids.contains(&record.source_mention_id) {
            return Err(TopologyValidationError::MissingMention(
                record.source_mention_id,
            ));
        }
        if !mention_ids.contains(&record.target_mention_id) {
            return Err(TopologyValidationError::MissingMention(
                record.target_mention_id,
            ));
        }
    }
    Ok(())
}

fn validate_evidence(
    evidence: &[EvidenceRecord],
    kinds: &HashMap<u64, TopologyNodeKind>,
) -> Result<(), TopologyValidationError> {
    for record in evidence {
        require_kind(kinds, record.entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.chunk_id, TopologyNodeKind::DynamicChunk)?;
    }
    Ok(())
}

fn validate_semantics(
    generation: &VerifiedGraphGenerationV2,
    kinds: &HashMap<u64, TopologyNodeKind>,
    evidence: &[EvidenceRecord],
) -> Result<(), TopologyValidationError> {
    let evidence_ids: HashSet<u64> = evidence.iter().map(|record| record.id).collect();
    let bindings =
        page::<CandidateEvidenceBindingRecord>(generation, PageKind::CandidateEvidenceBindings)?;

    for record in
        page::<TypedRelationshipCandidateRecord>(generation, PageKind::TypedRelationshipCandidates)?
    {
        validate_status(record.status)?;
        require_kind(kinds, record.source_entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.target_entity_id, TopologyNodeKind::Entity)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            Some(record.candidate_id),
        )?;
    }
    for record in page::<IdentityCandidateRecord>(generation, PageKind::IdentityCandidates)? {
        validate_status(record.status)?;
        require_kind(kinds, record.left_entity_id, TopologyNodeKind::Entity)?;
        require_kind(kinds, record.right_entity_id, TopologyNodeKind::Entity)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            Some(record.candidate_id),
        )?;
    }
    for record in page::<EventRecord>(generation, PageKind::Events)? {
        validate_status(record.status)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            None,
        )?;
    }
    for record in page::<EpisodeRecord>(generation, PageKind::Episodes)? {
        validate_status(record.status)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            None,
        )?;
    }
    for record in page::<EpisodeMembershipRecord>(generation, PageKind::EpisodeMemberships)? {
        validate_status(record.status)?;
        require_kind(kinds, record.episode_id, TopologyNodeKind::Episode)?;
        let member = EpisodeMemberKind::from_raw(record.member_kind).ok_or(
            TopologyValidationError::UnknownEpisodeMember(record.member_kind),
        )?;
        require_kind(
            kinds,
            record.member_id,
            match member {
                EpisodeMemberKind::Chunk => TopologyNodeKind::DynamicChunk,
                EpisodeMemberKind::Event => TopologyNodeKind::Event,
            },
        )?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            None,
        )?;
    }
    for record in page::<TemporalCandidateRecord>(generation, PageKind::TemporalCandidates)? {
        validate_status(record.status)?;
        TemporalRelationKind::from_raw(record.relation).ok_or(
            TopologyValidationError::UnknownTemporalRelation(record.relation),
        )?;
        validate_encoded_endpoint(kinds, record.flags, ENDPOINT_SOURCE_SHIFT, record.source_id)?;
        validate_encoded_endpoint(kinds, record.flags, ENDPOINT_TARGET_SHIFT, record.target_id)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            Some(record.candidate_id),
        )?;
    }
    for record in page::<CausalCandidateRecord>(generation, PageKind::CausalCandidates)? {
        validate_status(record.status)?;
        CausalRelationKind::from_raw(record.relation).ok_or(
            TopologyValidationError::UnknownCausalRelation(record.relation),
        )?;
        validate_encoded_endpoint(kinds, record.flags, ENDPOINT_SOURCE_SHIFT, record.cause_id)?;
        validate_encoded_endpoint(kinds, record.flags, ENDPOINT_TARGET_SHIFT, record.effect_id)?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            Some(record.candidate_id),
        )?;
    }
    for record in page::<MemoryStateCandidateRecord>(generation, PageKind::MemoryStateCandidates)? {
        validate_status(record.status)?;
        require_kind(kinds, record.subject_id, TopologyNodeKind::Entity)?;
        validate_encoded_endpoint(
            kinds,
            record.flags,
            ENDPOINT_TARGET_SHIFT,
            record.context_id,
        )?;
        validate_bindings(
            bindings,
            &evidence_ids,
            record.evidence_start,
            record.evidence_count,
            Some(record.candidate_id),
        )?;
    }
    Ok(())
}

fn validate_encoded_endpoint(
    kinds: &HashMap<u64, TopologyNodeKind>,
    flags: u32,
    shift: u32,
    id: u64,
) -> Result<(), TopologyValidationError> {
    let raw = ((flags >> shift) & ENDPOINT_TAG_MASK) as u16;
    let endpoint = SemanticEndpointKind::from_raw(raw)
        .ok_or(TopologyValidationError::UnknownEndpointKind(raw))?;
    let expected = match endpoint {
        SemanticEndpointKind::Entity => TopologyNodeKind::Entity,
        SemanticEndpointKind::Event => TopologyNodeKind::Event,
        SemanticEndpointKind::Episode => TopologyNodeKind::Episode,
        SemanticEndpointKind::DynamicChunk => TopologyNodeKind::DynamicChunk,
    };
    require_kind(kinds, id, expected)
}

fn validate_bindings(
    bindings: &[CandidateEvidenceBindingRecord],
    evidence_ids: &HashSet<u64>,
    start: u32,
    count: u32,
    candidate_id: Option<CandidateId>,
) -> Result<(), TopologyValidationError> {
    if count == 0 {
        return Err(TopologyValidationError::EmptyEvidenceBinding);
    }
    let start = start as usize;
    let end = start
        .checked_add(count as usize)
        .ok_or(TopologyValidationError::EvidenceRange)?;
    let rows = bindings
        .get(start..end)
        .ok_or(TopologyValidationError::EvidenceRange)?;
    for row in rows {
        if candidate_id.is_some_and(|id| row.candidate_id != id) {
            return Err(TopologyValidationError::CandidateEvidenceMismatch);
        }
        if !evidence_ids.contains(&row.evidence_id) {
            return Err(TopologyValidationError::MissingEvidence(row.evidence_id));
        }
    }
    Ok(())
}

fn validate_status(raw: u16) -> Result<(), TopologyValidationError> {
    CandidateStatus::from_raw(raw)
        .map(|_| ())
        .ok_or(TopologyValidationError::UnknownCandidateStatus(raw))
}

fn insert(
    kinds: &mut HashMap<u64, TopologyNodeKind>,
    id: u64,
    kind: TopologyNodeKind,
) -> Result<(), TopologyValidationError> {
    if id == 0 || kinds.insert(id, kind).is_some() {
        return Err(TopologyValidationError::DuplicateId(id));
    }
    Ok(())
}

fn require_kind(
    kinds: &HashMap<u64, TopologyNodeKind>,
    id: u64,
    expected: TopologyNodeKind,
) -> Result<(), TopologyValidationError> {
    match kinds.get(&id).copied() {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(TopologyValidationError::WrongEndpointKind {
            id,
            expected,
            actual,
        }),
        None => Err(TopologyValidationError::MissingEndpoint(id)),
    }
}

fn page<T: bytemuck::Pod>(
    generation: &VerifiedGraphGenerationV2,
    page: PageKind,
) -> Result<&[T], TopologyValidationError> {
    generation
        .typed_page(page)
        .map_err(|_| TopologyValidationError::InvalidPage(page))
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TopologyValidationError {
    #[error("topology page {0:?} has an invalid packed layout")]
    InvalidPage(PageKind),
    #[error("topology contains {0} document rows; expected exactly one")]
    DocumentCount(usize),
    #[error("topology identity {0} is zero or duplicated across authority classes")]
    DuplicateId(u64),
    #[error("topology endpoint {0} does not exist")]
    MissingEndpoint(u64),
    #[error("topology endpoint {id} has kind {actual:?}; expected {expected:?}")]
    WrongEndpointKind {
        id: u64,
        expected: TopologyNodeKind,
        actual: TopologyNodeKind,
    },
    #[error("structural relation {0} is unsupported")]
    UnknownStructuralRelation(u16),
    #[error("temporal relation {0} is unsupported")]
    UnknownTemporalRelation(u16),
    #[error("causal relation {0} is unsupported")]
    UnknownCausalRelation(u16),
    #[error("semantic endpoint kind {0} is unsupported")]
    UnknownEndpointKind(u16),
    #[error("episode member kind {0} is unsupported")]
    UnknownEpisodeMember(u16),
    #[error("candidate status {0} is unsupported")]
    UnknownCandidateStatus(u16),
    #[error("candidate has no source evidence")]
    EmptyEvidenceBinding,
    #[error("candidate evidence range is outside the binding page")]
    EvidenceRange,
    #[error("candidate evidence rows belong to another candidate")]
    CandidateEvidenceMismatch,
    #[error("candidate references missing evidence {0}")]
    MissingEvidence(u64),
    #[error("contextual evidence references missing mention {0}")]
    MissingMention(u64),
    #[error("canonical entity binding kind {0} is unsupported")]
    UnknownCanonicalBinding(u16),
    #[error("canonical entity binding contradicts its direct or decision-derived authority")]
    InvalidCanonicalBinding,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_and_causal_codes_remain_distinct_and_extensible() {
        assert_eq!(
            TemporalRelationKind::from_raw(1),
            Some(TemporalRelationKind::Before)
        );
        assert_eq!(
            TemporalRelationKind::from_raw(8),
            Some(TemporalRelationKind::Overlaps)
        );
        assert_eq!(
            CausalRelationKind::from_raw(1),
            Some(CausalRelationKind::DirectCause)
        );
        assert_eq!(
            CausalRelationKind::from_raw(6),
            Some(CausalRelationKind::Consequence)
        );
        assert_eq!(TemporalRelationKind::from_raw(11), None);
        assert_eq!(CausalRelationKind::from_raw(7), None);
    }

    #[test]
    fn source_relation_contract_is_not_a_generic_parent_link() {
        assert_eq!(
            StructuralRelationKind::from_raw(1),
            Some(StructuralRelationKind::DocumentContainsChapter)
        );
        assert_eq!(
            StructuralRelationKind::from_raw(4),
            Some(StructuralRelationKind::DocumentContainsDynamicChunk)
        );
        assert_eq!(StructuralRelationKind::from_raw(0), None);
    }
}
