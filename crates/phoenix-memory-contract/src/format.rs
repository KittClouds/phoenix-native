use crate::{
    CandidateEndpointBindingRecordV3, CandidateEvidenceBindingRecord, CanonicalEntityBindingRecord,
    CapabilityRecord, CausalCandidateRecord, ChapterRecord, ContentUnitRecord,
    ContextualEvidenceRecord, ConversationRecord, DecisionRecord, DocumentRevisionRecord,
    EntityRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord, EvidenceRecordV3,
    IdentityCandidateRecord, MemoryStateCandidateRecord, MentionRecordV3, ModelIdentityRecord,
    NliAdjudicationRecord, ParagraphRecord, ProducerCapabilityRecordV3, PublicationReceiptRecord,
    SemanticCandidateRecordV3, SentenceRecord, SourceRecord, SpanRecord, StageReceiptRecord,
    StructuralEdgeRecord, SupersessionRecord, TemporalCandidateRecord, TurnRecord,
    TypedRelationshipCandidateRecord, ValidityIntervalRecord, VocabularyPackRecordV3,
};
use bytemuck::{Pod, Zeroable};
use phoenix_graph_generation_v2::{AuthorityClass, ChunkRecord};
use std::mem::{align_of, size_of};

pub const GRAPH_GENERATION_V3_CONTRACT: &str = "phoenix.graph-generation/v3";
pub const GRAPH_GENERATION_V3_MAGIC: [u8; 8] = *b"PHXGG003";
pub const GRAPH_GENERATION_V3_VERSION: u32 = 3;
pub const GRAPH_GENERATION_V3_EXTENSION: &str = "phxgg3";
pub const MAX_GENERATION_BYTES: u64 = 16 << 30;
pub const MAX_PAGE_COUNT: usize = 64;
pub const MAX_RECORDS_PER_PAGE: u64 = 64_000_000;
pub const PAGE_ALIGNMENT: u64 = 64;
pub const HEADER_FLAG_COMPLETE: u32 = 1;
pub const PAGE_FLAG_REQUIRED: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum PageKindV3 {
    Strings = 1,
    SourceText = 2,
    Sources = 3,
    DocumentRevisions = 4,
    Conversations = 5,
    Turns = 6,
    ContentUnits = 7,
    Chapters = 8,
    Paragraphs = 9,
    Sentences = 10,
    Chunks = 11,
    Spans = 12,
    Entities = 13,
    CanonicalEntityBindings = 14,
    Mentions = 15,
    Evidence = 16,
    StructuralEdges = 17,
    TypedRelationshipCandidates = 18,
    IdentityCandidates = 19,
    Events = 20,
    Episodes = 21,
    EpisodeMemberships = 22,
    TemporalCandidates = 23,
    CausalCandidates = 24,
    MemoryStateCandidates = 25,
    ContextualEvidence = 26,
    NliAdjudications = 27,
    Decisions = 28,
    ValidityIntervals = 29,
    Supersessions = 30,
    Capabilities = 31,
    ModelIdentities = 32,
    StageReceipts = 33,
    PublicationReceipts = 34,
    CandidateEvidenceBindings = 35,
    SemanticCandidates = 36,
    ProducerCapabilitiesV3 = 37,
    VocabularyPacks = 38,
    CandidateEndpointBindings = 39,
}

impl PageKindV3 {
    pub const ALL: [Self; 39] = [
        Self::Strings,
        Self::SourceText,
        Self::Sources,
        Self::DocumentRevisions,
        Self::Conversations,
        Self::Turns,
        Self::ContentUnits,
        Self::Chapters,
        Self::Paragraphs,
        Self::Sentences,
        Self::Chunks,
        Self::Spans,
        Self::Entities,
        Self::CanonicalEntityBindings,
        Self::Mentions,
        Self::Evidence,
        Self::StructuralEdges,
        Self::TypedRelationshipCandidates,
        Self::IdentityCandidates,
        Self::Events,
        Self::Episodes,
        Self::EpisodeMemberships,
        Self::TemporalCandidates,
        Self::CausalCandidates,
        Self::MemoryStateCandidates,
        Self::ContextualEvidence,
        Self::NliAdjudications,
        Self::Decisions,
        Self::ValidityIntervals,
        Self::Supersessions,
        Self::Capabilities,
        Self::ModelIdentities,
        Self::StageReceipts,
        Self::PublicationReceipts,
        Self::CandidateEvidenceBindings,
        Self::SemanticCandidates,
        Self::ProducerCapabilitiesV3,
        Self::VocabularyPacks,
        Self::CandidateEndpointBindings,
    ];

    pub const fn from_raw(raw: u16) -> Option<Self> {
        if raw == 0 || raw > Self::ALL.len() as u16 {
            return None;
        }
        Some(Self::ALL[(raw - 1) as usize])
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct GenerationHeaderV3 {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub page_count: u32,
    pub flags: u32,
    pub total_len: u64,
    pub directory_offset: u64,
    pub directory_len: u64,
    pub namespace_hash: [u8; 32],
    pub source_set_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub cohort_hash: [u8; 32],
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub published_generation: u64,
    pub source_count: u64,
    pub document_revision_count: u64,
    pub conversation_count: u64,
    pub turn_count: u64,
    pub reserved: [u64; 5],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct PageDescriptorV3 {
    pub kind: u16,
    pub authority: u16,
    pub record_size: u32,
    pub record_alignment: u32,
    pub flags: u32,
    pub offset: u64,
    pub length: u64,
    pub count: u64,
    pub hash: [u8; 32],
    pub schema_hash: [u8; 32],
    pub reserved: [u64; 3],
}

pub const fn expected_authority(kind: PageKindV3) -> AuthorityClass {
    match kind {
        PageKindV3::Strings
        | PageKindV3::SourceText
        | PageKindV3::Sources
        | PageKindV3::DocumentRevisions
        | PageKindV3::Conversations
        | PageKindV3::Turns
        | PageKindV3::ContentUnits
        | PageKindV3::Chapters
        | PageKindV3::Paragraphs
        | PageKindV3::Sentences
        | PageKindV3::Chunks
        | PageKindV3::Spans
        | PageKindV3::Entities
        | PageKindV3::CanonicalEntityBindings
        | PageKindV3::Mentions
        | PageKindV3::Evidence
        | PageKindV3::StructuralEdges => AuthorityClass::SourceAuthoritative,
        PageKindV3::TypedRelationshipCandidates
        | PageKindV3::IdentityCandidates
        | PageKindV3::Events
        | PageKindV3::Episodes
        | PageKindV3::EpisodeMemberships
        | PageKindV3::TemporalCandidates
        | PageKindV3::CausalCandidates
        | PageKindV3::MemoryStateCandidates
        | PageKindV3::NliAdjudications
        | PageKindV3::CandidateEvidenceBindings
        | PageKindV3::SemanticCandidates
        | PageKindV3::CandidateEndpointBindings => AuthorityClass::SemanticCandidate,
        PageKindV3::ContextualEvidence => AuthorityClass::ContextualEvidenceOnly,
        PageKindV3::Decisions | PageKindV3::ValidityIntervals | PageKindV3::Supersessions => {
            AuthorityClass::DecisionReceipt
        }
        PageKindV3::Capabilities
        | PageKindV3::ModelIdentities
        | PageKindV3::StageReceipts
        | PageKindV3::PublicationReceipts
        | PageKindV3::ProducerCapabilitiesV3
        | PageKindV3::VocabularyPacks => AuthorityClass::RuntimeReceipt,
    }
}

macro_rules! record_layout {
    ($kind:expr, $($variant:ident => $record:ty),+ $(,)?) => {
        match $kind {
            PageKindV3::Strings | PageKindV3::SourceText => 1,
            $(PageKindV3::$variant => size_of::<$record>(),)+
        }
    };
}

pub const fn expected_record_size(kind: PageKindV3) -> u32 {
    record_layout!(
        kind,
        Sources => SourceRecord,
        DocumentRevisions => DocumentRevisionRecord,
        Conversations => ConversationRecord,
        Turns => TurnRecord,
        ContentUnits => ContentUnitRecord,
        Chapters => ChapterRecord,
        Paragraphs => ParagraphRecord,
        Sentences => SentenceRecord,
        Chunks => ChunkRecord,
        Spans => SpanRecord,
        Entities => EntityRecord,
        CanonicalEntityBindings => CanonicalEntityBindingRecord,
        Mentions => MentionRecordV3,
        Evidence => EvidenceRecordV3,
        StructuralEdges => StructuralEdgeRecord,
        TypedRelationshipCandidates => TypedRelationshipCandidateRecord,
        IdentityCandidates => IdentityCandidateRecord,
        Events => EventRecord,
        Episodes => EpisodeRecord,
        EpisodeMemberships => EpisodeMembershipRecord,
        TemporalCandidates => TemporalCandidateRecord,
        CausalCandidates => CausalCandidateRecord,
        MemoryStateCandidates => MemoryStateCandidateRecord,
        ContextualEvidence => ContextualEvidenceRecord,
        NliAdjudications => NliAdjudicationRecord,
        Decisions => DecisionRecord,
        ValidityIntervals => ValidityIntervalRecord,
        Supersessions => SupersessionRecord,
        Capabilities => CapabilityRecord,
        ModelIdentities => ModelIdentityRecord,
        StageReceipts => StageReceiptRecord,
        PublicationReceipts => PublicationReceiptRecord,
        CandidateEvidenceBindings => CandidateEvidenceBindingRecord,
        SemanticCandidates => SemanticCandidateRecordV3,
        ProducerCapabilitiesV3 => ProducerCapabilityRecordV3,
        VocabularyPacks => VocabularyPackRecordV3,
        CandidateEndpointBindings => CandidateEndpointBindingRecordV3,
    ) as u32
}

macro_rules! record_alignment {
    ($kind:expr, $($variant:ident => $record:ty),+ $(,)?) => {
        match $kind {
            PageKindV3::Strings | PageKindV3::SourceText => 1,
            $(PageKindV3::$variant => align_of::<$record>(),)+
        }
    };
}

pub const fn expected_record_alignment(kind: PageKindV3) -> u32 {
    record_alignment!(
        kind,
        Sources => SourceRecord,
        DocumentRevisions => DocumentRevisionRecord,
        Conversations => ConversationRecord,
        Turns => TurnRecord,
        ContentUnits => ContentUnitRecord,
        Chapters => ChapterRecord,
        Paragraphs => ParagraphRecord,
        Sentences => SentenceRecord,
        Chunks => ChunkRecord,
        Spans => SpanRecord,
        Entities => EntityRecord,
        CanonicalEntityBindings => CanonicalEntityBindingRecord,
        Mentions => MentionRecordV3,
        Evidence => EvidenceRecordV3,
        StructuralEdges => StructuralEdgeRecord,
        TypedRelationshipCandidates => TypedRelationshipCandidateRecord,
        IdentityCandidates => IdentityCandidateRecord,
        Events => EventRecord,
        Episodes => EpisodeRecord,
        EpisodeMemberships => EpisodeMembershipRecord,
        TemporalCandidates => TemporalCandidateRecord,
        CausalCandidates => CausalCandidateRecord,
        MemoryStateCandidates => MemoryStateCandidateRecord,
        ContextualEvidence => ContextualEvidenceRecord,
        NliAdjudications => NliAdjudicationRecord,
        Decisions => DecisionRecord,
        ValidityIntervals => ValidityIntervalRecord,
        Supersessions => SupersessionRecord,
        Capabilities => CapabilityRecord,
        ModelIdentities => ModelIdentityRecord,
        StageReceipts => StageReceiptRecord,
        PublicationReceipts => PublicationReceiptRecord,
        CandidateEvidenceBindings => CandidateEvidenceBindingRecord,
        SemanticCandidates => SemanticCandidateRecordV3,
        ProducerCapabilitiesV3 => ProducerCapabilityRecordV3,
        VocabularyPacks => VocabularyPackRecordV3,
        CandidateEndpointBindings => CandidateEndpointBindingRecordV3,
    ) as u32
}

pub fn expected_schema_hash(kind: PageKindV3) -> [u8; 32] {
    *blake3::hash(schema_signature(kind).as_bytes()).as_bytes()
}

fn schema_signature(kind: PageKindV3) -> &'static str {
    match kind {
        PageKindV3::Strings => "v3/strings:u8",
        PageKindV3::SourceText => "v3/source_text:utf8",
        PageKindV3::Sources => "v3/source:id,namespace,external_hash,content_hash,kind,flags",
        PageKindV3::DocumentRevisions => {
            "v3/document_revision:source,document,revision,path,content,hash,valid,system,chunks"
        }
        PageKindV3::Conversations => {
            "v3/conversation:source,conversation,start,end,turn_range,flags,hash"
        }
        PageKindV3::Turns => {
            "v3/turn:id,source,conversation,reply,actor,content,hash,time,ordinal,model,role,flags"
        }
        PageKindV3::ContentUnits => {
            "v3/content_unit:id,source,owner,parent,hash,range,ordinal,tokens,kind,flags"
        }
        PageKindV3::Chapters => "v2/chapter",
        PageKindV3::Paragraphs => "v2/paragraph",
        PageKindV3::Sentences => "v2/sentence",
        PageKindV3::Chunks => "v2/chunk",
        PageKindV3::Spans => "v2/span",
        PageKindV3::Entities => "v2/entity",
        PageKindV3::CanonicalEntityBindings => "v2/canonical_entity_binding",
        PageKindV3::Mentions => "v3/mention:source,entity,evidence,content_unit,range,confidence",
        PageKindV3::Evidence => "v3/evidence:source,entity,mention,content_unit,range,role",
        PageKindV3::StructuralEdges => "v2/structural_edge",
        PageKindV3::TypedRelationshipCandidates => "v2/typed_relationship_candidate",
        PageKindV3::IdentityCandidates => "v2/identity_candidate",
        PageKindV3::Events => "v2/event",
        PageKindV3::Episodes => "v2/episode",
        PageKindV3::EpisodeMemberships => "v2/episode_membership",
        PageKindV3::TemporalCandidates => "v2/temporal_candidate",
        PageKindV3::CausalCandidates => "v2/causal_candidate",
        PageKindV3::MemoryStateCandidates => "v2/memory_state_candidate",
        PageKindV3::ContextualEvidence => "v2/contextual_evidence",
        PageKindV3::NliAdjudications => "v2/nli_adjudication",
        PageKindV3::Decisions => "v2/decision",
        PageKindV3::ValidityIntervals => {
            "v3/validity:subject,valid_from,valid_to,system_from,system_to,kind,flags"
        }
        PageKindV3::Supersessions => {
            "v3/supersession:subject,replacement,evidence,decision,generation,status,flags"
        }
        PageKindV3::Capabilities => "v2/capability",
        PageKindV3::ModelIdentities => "v2/model_identity",
        PageKindV3::StageReceipts => "v2/stage_receipt",
        PageKindV3::PublicationReceipts => "v2/publication_receipt",
        PageKindV3::CandidateEvidenceBindings => "v2/candidate_evidence_binding",
        PageKindV3::SemanticCandidates => {
            "v3/semantic_candidate:id,source,pack,relation,value,endpoints,evidence,valid,system,confidence,model,producer,family,status,flags"
        }
        PageKindV3::ProducerCapabilitiesV3 => {
            "v3/producer_capability:producer,binary,config,count,reuse,model,product,state,flags"
        }
        PageKindV3::VocabularyPacks => {
            "v3/vocabulary_pack:id,name,version,schema,producer,kind,flags"
        }
        PageKindV3::CandidateEndpointBindings => {
            "v3/candidate_endpoint:candidate,endpoint,ordinal,role,flags"
        }
    }
}
