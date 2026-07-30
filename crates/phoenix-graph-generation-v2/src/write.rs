use crate::{
    align_up, compute_generation_hash, expected_authority, expected_record_alignment,
    expected_record_size, expected_schema_hash, CandidateEvidenceBindingRecord,
    CanonicalEntityBindingRecord, CapabilityRecord, CausalCandidateRecord, ChapterRecord,
    ChunkRecord, ContextualEvidenceRecord, DecisionRecord, DocumentRecord, EntityRecord,
    EpisodeMembershipRecord, EpisodeRecord, EventRecord, EvidenceRecord, GenerationHeader,
    GraphGenerationV2Error, IdentityCandidateRecord, MemoryStateCandidateRecord, MentionRecord,
    ModelIdentityRecord, NliAdjudicationRecord, PageDescriptor, PageKind, ParagraphRecord,
    PublicationReceiptRecord, SentenceRecord, SpanRecord, StageReceiptRecord, StructuralEdgeRecord,
    TemporalCandidateRecord, TypedRelationshipCandidateRecord, VerifiedGraphGenerationV2,
    GRAPH_GENERATION_V2_MAGIC, GRAPH_GENERATION_V2_VERSION, HEADER_FLAG_COMPLETE,
    MAX_GENERATION_BYTES, PAGE_ALIGNMENT, PAGE_FLAG_REQUIRED,
};
use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct GenerationWriteAuthority {
    pub source_document_id_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub cohort_hash: [u8; 32],
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub published_generation: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GenerationPages<'a> {
    pub strings: &'a [u8],
    pub documents: &'a [DocumentRecord],
    pub chapters: &'a [ChapterRecord],
    pub paragraphs: &'a [ParagraphRecord],
    pub sentences: &'a [SentenceRecord],
    pub chunks: &'a [ChunkRecord],
    pub spans: &'a [SpanRecord],
    pub entities: &'a [EntityRecord],
    pub mentions: &'a [MentionRecord],
    pub evidence: &'a [EvidenceRecord],
    pub structural_edges: &'a [StructuralEdgeRecord],
    pub typed_relationship_candidates: &'a [TypedRelationshipCandidateRecord],
    pub identity_candidates: &'a [IdentityCandidateRecord],
    pub events: &'a [EventRecord],
    pub episodes: &'a [EpisodeRecord],
    pub episode_memberships: &'a [EpisodeMembershipRecord],
    pub temporal_candidates: &'a [TemporalCandidateRecord],
    pub causal_candidates: &'a [CausalCandidateRecord],
    pub memory_state_candidates: &'a [MemoryStateCandidateRecord],
    pub contextual_evidence: &'a [ContextualEvidenceRecord],
    pub nli_adjudications: &'a [NliAdjudicationRecord],
    pub decisions: &'a [DecisionRecord],
    pub capabilities: &'a [CapabilityRecord],
    pub model_identities: &'a [ModelIdentityRecord],
    pub stage_receipts: &'a [StageReceiptRecord],
    pub publication_receipts: &'a [PublicationReceiptRecord],
    pub candidate_evidence_bindings: &'a [CandidateEvidenceBindingRecord],
    pub canonical_entity_bindings: &'a [CanonicalEntityBindingRecord],
}

impl<'a> GenerationPages<'a> {
    fn payloads(self) -> [PagePayload<'a>; 28] {
        [
            PagePayload::bytes(PageKind::Strings, self.strings),
            PagePayload::records(PageKind::Documents, self.documents),
            PagePayload::records(PageKind::Chapters, self.chapters),
            PagePayload::records(PageKind::Paragraphs, self.paragraphs),
            PagePayload::records(PageKind::Sentences, self.sentences),
            PagePayload::records(PageKind::Chunks, self.chunks),
            PagePayload::records(PageKind::Spans, self.spans),
            PagePayload::records(PageKind::Entities, self.entities),
            PagePayload::records(PageKind::Mentions, self.mentions),
            PagePayload::records(PageKind::Evidence, self.evidence),
            PagePayload::records(PageKind::StructuralEdges, self.structural_edges),
            PagePayload::records(
                PageKind::TypedRelationshipCandidates,
                self.typed_relationship_candidates,
            ),
            PagePayload::records(PageKind::IdentityCandidates, self.identity_candidates),
            PagePayload::records(PageKind::Events, self.events),
            PagePayload::records(PageKind::Episodes, self.episodes),
            PagePayload::records(PageKind::EpisodeMemberships, self.episode_memberships),
            PagePayload::records(PageKind::TemporalCandidates, self.temporal_candidates),
            PagePayload::records(PageKind::CausalCandidates, self.causal_candidates),
            PagePayload::records(
                PageKind::MemoryStateCandidates,
                self.memory_state_candidates,
            ),
            PagePayload::records(PageKind::ContextualEvidence, self.contextual_evidence),
            PagePayload::records(PageKind::NliAdjudications, self.nli_adjudications),
            PagePayload::records(PageKind::Decisions, self.decisions),
            PagePayload::records(PageKind::Capabilities, self.capabilities),
            PagePayload::records(PageKind::ModelIdentities, self.model_identities),
            PagePayload::records(PageKind::StageReceipts, self.stage_receipts),
            PagePayload::records(PageKind::PublicationReceipts, self.publication_receipts),
            PagePayload::records(
                PageKind::CandidateEvidenceBindings,
                self.candidate_evidence_bindings,
            ),
            PagePayload::records(
                PageKind::CanonicalEntityBindings,
                self.canonical_entity_bindings,
            ),
        ]
    }
}

#[derive(Clone, Copy)]
struct PagePayload<'a> {
    kind: PageKind,
    bytes: &'a [u8],
    count: u64,
}

impl<'a> PagePayload<'a> {
    fn bytes(kind: PageKind, bytes: &'a [u8]) -> Self {
        Self {
            kind,
            bytes,
            count: bytes.len() as u64,
        }
    }

    fn records<T: Pod>(kind: PageKind, records: &'a [T]) -> Self {
        Self {
            kind,
            bytes: cast_slice(records),
            count: records.len() as u64,
        }
    }
}

pub fn write_generation_new(
    path: impl AsRef<Path>,
    authority: GenerationWriteAuthority,
    pages: GenerationPages<'_>,
) -> Result<VerifiedGraphGenerationV2, GraphGenerationV2Error> {
    let path = path.as_ref();
    let result = write_generation_file(path, authority, pages);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result?;
    VerifiedGraphGenerationV2::open(path)
}

fn write_generation_file(
    path: &Path,
    authority: GenerationWriteAuthority,
    pages: GenerationPages<'_>,
) -> Result<(), GraphGenerationV2Error> {
    let payloads = pages.payloads();
    let header_size = size_of::<GenerationHeader>() as u64;
    let directory_offset = align_up(header_size, PAGE_ALIGNMENT);
    let directory_len = (size_of::<PageDescriptor>() * PageKind::ALL.len()) as u64;
    let mut cursor = align_up(directory_offset + directory_len, PAGE_ALIGNMENT);
    let mut directory = [PageDescriptor::zeroed(); 28];

    for (index, payload) in payloads.iter().enumerate() {
        if payload.kind != PageKind::ALL[index] {
            return Err(GraphGenerationV2Error::UnexpectedPageOrder {
                index,
                actual: payload.kind,
                expected: PageKind::ALL[index],
            });
        }
        cursor = align_up(cursor, PAGE_ALIGNMENT);
        directory[index] = PageDescriptor {
            kind: payload.kind as u16,
            authority: expected_authority(payload.kind) as u16,
            record_size: expected_record_size(payload.kind),
            record_alignment: expected_record_alignment(payload.kind),
            flags: PAGE_FLAG_REQUIRED,
            offset: cursor,
            length: payload.bytes.len() as u64,
            count: payload.count,
            hash: *blake3::hash(payload.bytes).as_bytes(),
            schema_hash: expected_schema_hash(payload.kind),
            reserved: [0; 3],
        };
        cursor = cursor.checked_add(payload.bytes.len() as u64).ok_or(
            GraphGenerationV2Error::OversizedGeneration {
                actual: u64::MAX,
                maximum: MAX_GENERATION_BYTES,
            },
        )?;
    }

    let total_len = align_up(cursor, PAGE_ALIGNMENT);
    if total_len > MAX_GENERATION_BYTES {
        return Err(GraphGenerationV2Error::OversizedGeneration {
            actual: total_len,
            maximum: MAX_GENERATION_BYTES,
        });
    }

    let mut header = GenerationHeader {
        magic: GRAPH_GENERATION_V2_MAGIC,
        version: GRAPH_GENERATION_V2_VERSION,
        header_size: header_size as u32,
        page_count: PageKind::ALL.len() as u32,
        flags: 0,
        total_len,
        directory_offset,
        directory_len,
        source_document_id_hash: authority.source_document_id_hash,
        content_hash: authority.content_hash,
        generation_hash: [0; 32],
        cohort_hash: authority.cohort_hash,
        native_document_id: authority.native_document_id,
        document_revision: authority.document_revision,
        registry_revision: authority.registry_revision,
        producer_generation: authority.producer_generation,
        published_generation: authority.published_generation,
        reserved: [0; 5],
    };
    header.generation_hash = compute_generation_hash(&header, &directory);

    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?;
    write_at(&mut file, 0, bytes_of(&header), path)?;
    write_at(&mut file, directory_offset, cast_slice(&directory), path)?;
    for (descriptor, payload) in directory.iter().zip(payloads) {
        write_at(&mut file, descriptor.offset, payload.bytes, path)?;
    }
    file.set_len(total_len)
        .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?;
    file.sync_all()
        .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?;

    header.flags = HEADER_FLAG_COMPLETE;
    write_at(&mut file, 0, bytes_of(&header), path)?;
    file.sync_all()
        .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))
}

fn write_at(
    file: &mut std::fs::File,
    offset: u64,
    bytes: &[u8],
    path: &Path,
) -> Result<(), GraphGenerationV2Error> {
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.write_all(bytes))
        .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))
}
