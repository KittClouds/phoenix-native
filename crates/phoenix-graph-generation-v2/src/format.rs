use crate::{
    AuthorityClass, CandidateEvidenceBindingRecord, CanonicalEntityBindingRecord, CapabilityRecord,
    CausalCandidateRecord, ChapterRecord, ChunkRecord, ContextualEvidenceRecord, DecisionRecord,
    DocumentRecord, EntityRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord,
    EvidenceRecord, IdentityCandidateRecord, MemoryStateCandidateRecord, MentionRecord,
    ModelIdentityRecord, NliAdjudicationRecord, ParagraphRecord, PublicationReceiptRecord,
    SentenceRecord, SpanRecord, StageReceiptRecord, StructuralEdgeRecord, TemporalCandidateRecord,
    TypedRelationshipCandidateRecord,
};
use bytemuck::{Pod, Zeroable};
use std::mem::{align_of, size_of};

pub const GRAPH_GENERATION_V2_CONTRACT: &str = "phoenix.graph-generation/v2";
pub const GRAPH_GENERATION_V2_MAGIC: [u8; 8] = *b"PHXGG002";
pub const GRAPH_GENERATION_V2_VERSION: u32 = 2;
pub const GRAPH_GENERATION_V2_EXTENSION: &str = "phxgg2";
pub const MAX_GENERATION_BYTES: u64 = 1 << 30;
pub const MAX_PAGE_COUNT: usize = 64;
pub const MAX_RECORDS_PER_PAGE: u64 = 16_000_000;
pub const PAGE_ALIGNMENT: u64 = 64;
pub const HEADER_FLAG_COMPLETE: u32 = 1;
pub const PAGE_FLAG_REQUIRED: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum PageKind {
    Strings = 1,
    Documents = 2,
    Chapters = 3,
    Paragraphs = 4,
    Sentences = 5,
    Chunks = 6,
    Spans = 7,
    Entities = 8,
    Mentions = 9,
    Evidence = 10,
    StructuralEdges = 11,
    TypedRelationshipCandidates = 12,
    IdentityCandidates = 13,
    Events = 14,
    Episodes = 15,
    EpisodeMemberships = 16,
    TemporalCandidates = 17,
    CausalCandidates = 18,
    MemoryStateCandidates = 19,
    ContextualEvidence = 20,
    NliAdjudications = 21,
    Decisions = 22,
    Capabilities = 23,
    ModelIdentities = 24,
    StageReceipts = 25,
    PublicationReceipts = 26,
    CandidateEvidenceBindings = 27,
    CanonicalEntityBindings = 28,
}

impl PageKind {
    pub const ALL: [Self; 28] = [
        Self::Strings,
        Self::Documents,
        Self::Chapters,
        Self::Paragraphs,
        Self::Sentences,
        Self::Chunks,
        Self::Spans,
        Self::Entities,
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
        Self::Capabilities,
        Self::ModelIdentities,
        Self::StageReceipts,
        Self::PublicationReceipts,
        Self::CandidateEvidenceBindings,
        Self::CanonicalEntityBindings,
    ];

    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Strings),
            2 => Some(Self::Documents),
            3 => Some(Self::Chapters),
            4 => Some(Self::Paragraphs),
            5 => Some(Self::Sentences),
            6 => Some(Self::Chunks),
            7 => Some(Self::Spans),
            8 => Some(Self::Entities),
            9 => Some(Self::Mentions),
            10 => Some(Self::Evidence),
            11 => Some(Self::StructuralEdges),
            12 => Some(Self::TypedRelationshipCandidates),
            13 => Some(Self::IdentityCandidates),
            14 => Some(Self::Events),
            15 => Some(Self::Episodes),
            16 => Some(Self::EpisodeMemberships),
            17 => Some(Self::TemporalCandidates),
            18 => Some(Self::CausalCandidates),
            19 => Some(Self::MemoryStateCandidates),
            20 => Some(Self::ContextualEvidence),
            21 => Some(Self::NliAdjudications),
            22 => Some(Self::Decisions),
            23 => Some(Self::Capabilities),
            24 => Some(Self::ModelIdentities),
            25 => Some(Self::StageReceipts),
            26 => Some(Self::PublicationReceipts),
            27 => Some(Self::CandidateEvidenceBindings),
            28 => Some(Self::CanonicalEntityBindings),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct GenerationHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub header_size: u32,
    pub page_count: u32,
    pub flags: u32,
    pub total_len: u64,
    pub directory_offset: u64,
    pub directory_len: u64,
    pub source_document_id_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub cohort_hash: [u8; 32],
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub published_generation: u64,
    pub reserved: [u64; 5],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct PageDescriptor {
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

pub const fn expected_authority(kind: PageKind) -> AuthorityClass {
    match kind {
        PageKind::Strings
        | PageKind::Documents
        | PageKind::Chapters
        | PageKind::Paragraphs
        | PageKind::Sentences
        | PageKind::Chunks
        | PageKind::Spans
        | PageKind::Entities
        | PageKind::Mentions
        | PageKind::Evidence
        | PageKind::CanonicalEntityBindings
        | PageKind::StructuralEdges => AuthorityClass::SourceAuthoritative,
        PageKind::TypedRelationshipCandidates
        | PageKind::IdentityCandidates
        | PageKind::Events
        | PageKind::Episodes
        | PageKind::EpisodeMemberships
        | PageKind::TemporalCandidates
        | PageKind::CausalCandidates
        | PageKind::MemoryStateCandidates
        | PageKind::CandidateEvidenceBindings
        | PageKind::NliAdjudications => AuthorityClass::SemanticCandidate,
        PageKind::ContextualEvidence => AuthorityClass::ContextualEvidenceOnly,
        PageKind::Decisions => AuthorityClass::DecisionReceipt,
        PageKind::Capabilities
        | PageKind::ModelIdentities
        | PageKind::StageReceipts
        | PageKind::PublicationReceipts => AuthorityClass::RuntimeReceipt,
    }
}

pub const fn expected_record_size(kind: PageKind) -> u32 {
    (match kind {
        PageKind::Strings => 1,
        PageKind::Documents => size::<DocumentRecord>(),
        PageKind::Chapters => size::<ChapterRecord>(),
        PageKind::Paragraphs => size::<ParagraphRecord>(),
        PageKind::Sentences => size::<SentenceRecord>(),
        PageKind::Chunks => size::<ChunkRecord>(),
        PageKind::Spans => size::<SpanRecord>(),
        PageKind::Entities => size::<EntityRecord>(),
        PageKind::Mentions => size::<MentionRecord>(),
        PageKind::Evidence => size::<EvidenceRecord>(),
        PageKind::StructuralEdges => size::<StructuralEdgeRecord>(),
        PageKind::TypedRelationshipCandidates => size::<TypedRelationshipCandidateRecord>(),
        PageKind::IdentityCandidates => size::<IdentityCandidateRecord>(),
        PageKind::Events => size::<EventRecord>(),
        PageKind::Episodes => size::<EpisodeRecord>(),
        PageKind::EpisodeMemberships => size::<EpisodeMembershipRecord>(),
        PageKind::TemporalCandidates => size::<TemporalCandidateRecord>(),
        PageKind::CausalCandidates => size::<CausalCandidateRecord>(),
        PageKind::MemoryStateCandidates => size::<MemoryStateCandidateRecord>(),
        PageKind::ContextualEvidence => size::<ContextualEvidenceRecord>(),
        PageKind::NliAdjudications => size::<NliAdjudicationRecord>(),
        PageKind::Decisions => size::<DecisionRecord>(),
        PageKind::Capabilities => size::<CapabilityRecord>(),
        PageKind::ModelIdentities => size::<ModelIdentityRecord>(),
        PageKind::StageReceipts => size::<StageReceiptRecord>(),
        PageKind::PublicationReceipts => size::<PublicationReceiptRecord>(),
        PageKind::CandidateEvidenceBindings => size::<CandidateEvidenceBindingRecord>(),
        PageKind::CanonicalEntityBindings => size::<CanonicalEntityBindingRecord>(),
    }) as u32
}

pub const fn expected_record_alignment(kind: PageKind) -> u32 {
    (match kind {
        PageKind::Strings => 1,
        PageKind::Documents => align::<DocumentRecord>(),
        PageKind::Chapters => align::<ChapterRecord>(),
        PageKind::Paragraphs => align::<ParagraphRecord>(),
        PageKind::Sentences => align::<SentenceRecord>(),
        PageKind::Chunks => align::<ChunkRecord>(),
        PageKind::Spans => align::<SpanRecord>(),
        PageKind::Entities => align::<EntityRecord>(),
        PageKind::Mentions => align::<MentionRecord>(),
        PageKind::Evidence => align::<EvidenceRecord>(),
        PageKind::StructuralEdges => align::<StructuralEdgeRecord>(),
        PageKind::TypedRelationshipCandidates => align::<TypedRelationshipCandidateRecord>(),
        PageKind::IdentityCandidates => align::<IdentityCandidateRecord>(),
        PageKind::Events => align::<EventRecord>(),
        PageKind::Episodes => align::<EpisodeRecord>(),
        PageKind::EpisodeMemberships => align::<EpisodeMembershipRecord>(),
        PageKind::TemporalCandidates => align::<TemporalCandidateRecord>(),
        PageKind::CausalCandidates => align::<CausalCandidateRecord>(),
        PageKind::MemoryStateCandidates => align::<MemoryStateCandidateRecord>(),
        PageKind::ContextualEvidence => align::<ContextualEvidenceRecord>(),
        PageKind::NliAdjudications => align::<NliAdjudicationRecord>(),
        PageKind::Decisions => align::<DecisionRecord>(),
        PageKind::Capabilities => align::<CapabilityRecord>(),
        PageKind::ModelIdentities => align::<ModelIdentityRecord>(),
        PageKind::StageReceipts => align::<StageReceiptRecord>(),
        PageKind::PublicationReceipts => align::<PublicationReceiptRecord>(),
        PageKind::CandidateEvidenceBindings => align::<CandidateEvidenceBindingRecord>(),
        PageKind::CanonicalEntityBindings => align::<CanonicalEntityBindingRecord>(),
    }) as u32
}

pub fn expected_schema_hash(kind: PageKind) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v2/page-schema\0");
    hasher.update(&(kind as u16).to_le_bytes());
    hasher.update(&expected_record_size(kind).to_le_bytes());
    hasher.update(&expected_record_alignment(kind).to_le_bytes());
    hasher.update(schema_signature(kind).as_bytes());
    *hasher.finalize().as_bytes()
}

const fn schema_signature(kind: PageKind) -> &'static str {
    match kind {
        PageKind::Strings => "bytes:u8",
        PageKind::Documents => {
            "id:u64,source_id:strref,source_len:u32,chapters:u32,paragraphs:u32,sentences:u32,chunks:u32,spans:u32,entities:u32,mentions:u32,evidence:u32,structural_edges:u32,flags:u32"
        }
        PageKind::Chapters => {
            "id:u64,document:u64,title:strref,start:u32,end:u32,paragraph_start:u32,paragraph_end:u32,ordinal:u32,flags:u32"
        }
        PageKind::Paragraphs => {
            "id:u64,document:u64,chapter:u64,start:u32,end:u32,sentence_start:u32,sentence_end:u32,ordinal:u32,flags:u32"
        }
        PageKind::Sentences => {
            "id:u64,document:u64,paragraph:u64,content_hash:u64,start:u32,end:u32,ordinal:u32,tokens:u32,quality:u16,dialogue:u16,flags:u16"
        }
        PageKind::Chunks => {
            "id:u64,document:u64,content_hash:u64,start:u32,end:u32,sentence_start:u32,sentence_end:u32,paragraph_start:u32,paragraph_end:u32,chapter:u32,tokens:u32,flags:u32"
        }
        PageKind::Spans => {
            "id:u64,document:u64,parent:u64,content_hash:u64,label:strref,start:u32,end:u32,child_start:u32,child_end:u32,tokens:u32,flags:u32,kind:u16,dialogue:u16"
        }
        PageKind::Entities => {
            "id:u64,label:strref,custom_kind:strref,mentions:u32,kind:u16,source_mask:u16,flags:u32"
        }
        PageKind::Mentions => {
            "id:u64,entity:u64,evidence:u64,chunk:u64,start:u32,end:u32,sentence:u32,confidence:f32bits,flags:u32"
        }
        PageKind::Evidence => {
            "id:u64,entity:u64,mention:u64,chunk:u64,start:u32,end:u32,role:u16,flags:u16"
        }
        PageKind::StructuralEdges => {
            "id:u64,source:u64,target:u64,evidence:u64,weight:f32bits,relation:u16,flags:u16"
        }
        PageKind::TypedRelationshipCandidates => {
            "candidate:hash32,source_entity:u64,target_entity:u64,evidence_start:u32,evidence_count:u32,premise_start:u32,premise_end:u32,relation:u16,family:u16,status:u16,flags16:u16,confidence:f32bits,flags:u32"
        }
        PageKind::IdentityCandidates => {
            "candidate:hash32,left_entity:u64,right_entity:u64,evidence_start:u32,evidence_count:u32,confidence:f32bits,flags:u32,kind:u16,status:u16"
        }
        PageKind::Events => {
            "id:u64,label:strref,evidence_start:u32,evidence_count:u32,start:u32,end:u32,kind:u16,status:u16,confidence:f32bits,flags:u32"
        }
        PageKind::Episodes => {
            "id:u64,label:strref,evidence_start:u32,evidence_count:u32,membership_start:u32,membership_count:u32,ordinal:u32,status:u16,family:u16,confidence:f32bits,flags:u32"
        }
        PageKind::EpisodeMemberships => {
            "episode:u64,member:u64,evidence_start:u32,evidence_count:u32,member_kind:u16,status:u16,confidence:f32bits,flags:u32"
        }
        PageKind::TemporalCandidates => {
            "candidate:hash32,source:u64,target:u64,evidence_start:u32,evidence_count:u32,relation:u16,status:u16,confidence:f32bits,flags:u32"
        }
        PageKind::CausalCandidates => {
            "candidate:hash32,cause:u64,effect:u64,evidence_start:u32,evidence_count:u32,relation:u16,status:u16,confidence:f32bits,flags:u32"
        }
        PageKind::MemoryStateCandidates => {
            "candidate:hash32,subject:u64,context:u64,key:strref,value:strref,evidence_start:u32,evidence_count:u32,status:u16,kind:u16,confidence:f32bits,flags:u32"
        }
        PageKind::ContextualEvidence => {
            "source_entity:u64,target_entity:u64,source_mention:u64,target_mention:u64,chunk:u64,weight:f32bits,byte_distance:u32,flags:u32"
        }
        PageKind::NliAdjudications => {
            "candidate:hash32,contradiction:f32bits,entailment:f32bits,neutral:f32bits,label:u16,status:u16,model_index:u32,flags:u32"
        }
        PageKind::Decisions => {
            "id:u64,candidate:hash32,reason:strref,evidence_hash:hash32,document_revision:u64,registry_revision:u64,producer_generation:u64,action:u16,status:u16,flags:u32"
        }
        PageKind::Capabilities => {
            "product:u16,authority:u16,state:u16,flags16:u16,producer:strref,output_count:u64,reused_generation:u64,model_index:u32,flags:u32"
        }
        PageKind::ModelIdentities => {
            "name:strref,runtime:strref,artifact_uri:strref,artifact_hash:hash32,config_hash:hash32,flags:u32"
        }
        PageKind::StageReceipts => {
            "name:strref,span:u64,parent_span:u64,elapsed_us:u64,outputs:u64,allocated_bytes:u64,copied_bytes:u64,queue_high_water:u64,cache_state:u16,status:u16,flags:u32"
        }
        PageKind::PublicationReceipts => {
            "authority_hash:hash32,previous_hash:hash32,generation:u64,previous_generation:u64,document_revision:u64,registry_revision:u64,published_ms:u64,status:u16,flags16:u16,flags:u32"
        }
        PageKind::CandidateEvidenceBindings => {
            "candidate:hash32,evidence:u64,ordinal:u32,role:u16,flags:u16"
        }
        PageKind::CanonicalEntityBindings => {
            "source_entity:u64,canonical_entity:u64,decision:u64,source_mask:u16,kind:u16,flags:u32"
        }
    }
}

const fn size<T>() -> usize {
    size_of::<T>()
}

const fn align<T>() -> usize {
    align_of::<T>()
}
