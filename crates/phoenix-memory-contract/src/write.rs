use crate::{
    align_up, compute_generation_hash, compute_source_set_hash, expected_authority,
    expected_record_alignment, expected_record_size, expected_schema_hash,
    CandidateEndpointBindingRecordV3, CandidateEvidenceBindingRecord, CanonicalEntityBindingRecord,
    CapabilityRecord, CausalCandidateRecord, ChapterRecord, ChunkRecord, ContentUnitRecord,
    ContextualEvidenceRecord, ConversationRecord, DecisionRecord, DocumentRevisionRecord,
    EntityRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord, EvidenceRecordV3,
    GenerationHeaderV3, IdentityCandidateRecord, MemoryContractError, MemoryStateCandidateRecord,
    MentionRecordV3, ModelIdentityRecord, NliAdjudicationRecord, PageDescriptorV3, PageKindV3,
    ParagraphRecord, ProducerCapabilityRecordV3, PublicationReceiptRecord,
    SemanticCandidateRecordV3, SentenceRecord, SpanRecord, StageReceiptRecord,
    StructuralEdgeRecord, SupersessionRecord, TemporalCandidateRecord, TurnRecord,
    TypedRelationshipCandidateRecord, ValidityIntervalRecord, VerifiedGraphGenerationV3,
    VocabularyPackRecordV3, GRAPH_GENERATION_V3_MAGIC, GRAPH_GENERATION_V3_VERSION,
    HEADER_FLAG_COMPLETE, MAX_GENERATION_BYTES, PAGE_ALIGNMENT, PAGE_FLAG_REQUIRED,
};
use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct GenerationWriteAuthorityV3 {
    pub namespace_hash: [u8; 32],
    pub cohort_hash: [u8; 32],
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub published_generation: u64,
}

#[derive(Debug, Default)]
pub struct GenerationPagesV3 {
    pub strings: Vec<u8>,
    pub source_text: Vec<u8>,
    pub sources: Vec<crate::SourceRecord>,
    pub document_revisions: Vec<DocumentRevisionRecord>,
    pub conversations: Vec<ConversationRecord>,
    pub turns: Vec<TurnRecord>,
    pub content_units: Vec<ContentUnitRecord>,
    pub chapters: Vec<ChapterRecord>,
    pub paragraphs: Vec<ParagraphRecord>,
    pub sentences: Vec<SentenceRecord>,
    pub chunks: Vec<ChunkRecord>,
    pub spans: Vec<SpanRecord>,
    pub entities: Vec<EntityRecord>,
    pub canonical_entity_bindings: Vec<CanonicalEntityBindingRecord>,
    pub mentions: Vec<MentionRecordV3>,
    pub evidence: Vec<EvidenceRecordV3>,
    pub structural_edges: Vec<StructuralEdgeRecord>,
    pub typed_relationship_candidates: Vec<TypedRelationshipCandidateRecord>,
    pub identity_candidates: Vec<IdentityCandidateRecord>,
    pub events: Vec<EventRecord>,
    pub episodes: Vec<EpisodeRecord>,
    pub episode_memberships: Vec<EpisodeMembershipRecord>,
    pub temporal_candidates: Vec<TemporalCandidateRecord>,
    pub causal_candidates: Vec<CausalCandidateRecord>,
    pub memory_state_candidates: Vec<MemoryStateCandidateRecord>,
    pub contextual_evidence: Vec<ContextualEvidenceRecord>,
    pub nli_adjudications: Vec<NliAdjudicationRecord>,
    pub decisions: Vec<DecisionRecord>,
    pub validity_intervals: Vec<ValidityIntervalRecord>,
    pub supersessions: Vec<SupersessionRecord>,
    pub capabilities: Vec<CapabilityRecord>,
    pub model_identities: Vec<ModelIdentityRecord>,
    pub stage_receipts: Vec<StageReceiptRecord>,
    pub publication_receipts: Vec<PublicationReceiptRecord>,
    pub candidate_evidence_bindings: Vec<CandidateEvidenceBindingRecord>,
    pub semantic_candidates: Vec<SemanticCandidateRecordV3>,
    pub producer_capabilities_v3: Vec<ProducerCapabilityRecordV3>,
    pub vocabulary_packs: Vec<VocabularyPackRecordV3>,
    pub candidate_endpoint_bindings: Vec<CandidateEndpointBindingRecordV3>,
}

impl GenerationPagesV3 {
    pub fn canonicalize(&mut self) {
        self.sources.sort_unstable_by_key(|record| record.id);
        self.document_revisions
            .sort_unstable_by_key(|record| (record.source_id, record.revision));
        self.conversations
            .sort_unstable_by_key(|record| record.conversation_id);
        self.content_units.sort_unstable_by_key(|record| {
            (record.source_id, record.kind, record.ordinal, record.id)
        });
        self.chapters.sort_unstable_by_key(|record| record.id);
        self.paragraphs.sort_unstable_by_key(|record| record.id);
        self.sentences.sort_unstable_by_key(|record| record.id);
        self.spans.sort_unstable_by_key(|record| record.id);
        self.entities.sort_unstable_by_key(|record| record.id);
        self.canonical_entity_bindings
            .sort_unstable_by_key(|record| (record.source_entity_id, record.canonical_entity_id));
        self.mentions.sort_unstable_by_key(|record| record.id);
        self.evidence.sort_unstable_by_key(|record| record.id);
        self.structural_edges
            .sort_unstable_by_key(|record| record.id);
        self.typed_relationship_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.identity_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.events.sort_unstable_by_key(|record| record.id);
        self.episodes.sort_unstable_by_key(|record| record.id);
        self.temporal_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.causal_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.memory_state_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.contextual_evidence.sort_unstable_by_key(|record| {
            (
                record.source_entity_id,
                record.target_entity_id,
                record.source_mention_id,
                record.target_mention_id,
            )
        });
        self.nli_adjudications
            .sort_unstable_by_key(|record| record.candidate_id);
        self.decisions.sort_unstable_by_key(|record| record.id);
        self.validity_intervals.sort_unstable_by_key(|record| {
            (
                record.subject_id,
                record.subject_kind,
                record.system_generation_from,
            )
        });
        self.supersessions
            .sort_unstable_by_key(|record| (record.subject_id, record.replacement_id));
        self.capabilities
            .sort_unstable_by_key(|record| (record.product, record.producer.offset));
        self.stage_receipts
            .sort_unstable_by_key(|record| (record.span_id, record.parent_span_id));
        self.publication_receipts
            .sort_unstable_by_key(|record| record.generation_id);
        self.semantic_candidates
            .sort_unstable_by_key(|record| record.candidate_id);
        self.vocabulary_packs
            .sort_unstable_by_key(|record| record.id);
        self.candidate_endpoint_bindings
            .sort_unstable_by_key(|record| (record.candidate_id, record.ordinal));
        self.candidate_evidence_bindings
            .sort_unstable_by_key(|record| (record.candidate_id, record.ordinal));
        let mut endpoint_cursor = 0_usize;
        for candidate in &mut self.semantic_candidates {
            candidate.endpoint_start = endpoint_cursor as u32;
            let start = endpoint_cursor;
            while self
                .candidate_endpoint_bindings
                .get(endpoint_cursor)
                .is_some_and(|binding| binding.candidate_id == candidate.candidate_id)
            {
                endpoint_cursor += 1;
            }
            candidate.endpoint_count = (endpoint_cursor - start) as u32;
        }
        let mut binding_cursor = 0_usize;
        for candidate in &mut self.semantic_candidates {
            candidate.evidence_start = binding_cursor as u32;
            let start = binding_cursor;
            while self
                .candidate_evidence_bindings
                .get(binding_cursor)
                .is_some_and(|binding| binding.candidate_id.0 == candidate.candidate_id)
            {
                binding_cursor += 1;
            }
            candidate.evidence_count = (binding_cursor - start) as u32;
        }
        self.producer_capabilities_v3
            .sort_unstable_by_key(|record| record.product);
        // Do not reorder range-addressed pages. Conversation turn ranges,
        // document chunk ranges, episode membership ranges, model indexes, and
        // candidate evidence ranges are part of the frozen contract. Their
        // producers must emit canonical grouped order before this writer.
    }

    fn payloads(&self) -> [PagePayload<'_>; 39] {
        [
            PagePayload::bytes(PageKindV3::Strings, &self.strings),
            PagePayload::bytes(PageKindV3::SourceText, &self.source_text),
            PagePayload::records(PageKindV3::Sources, &self.sources),
            PagePayload::records(PageKindV3::DocumentRevisions, &self.document_revisions),
            PagePayload::records(PageKindV3::Conversations, &self.conversations),
            PagePayload::records(PageKindV3::Turns, &self.turns),
            PagePayload::records(PageKindV3::ContentUnits, &self.content_units),
            PagePayload::records(PageKindV3::Chapters, &self.chapters),
            PagePayload::records(PageKindV3::Paragraphs, &self.paragraphs),
            PagePayload::records(PageKindV3::Sentences, &self.sentences),
            PagePayload::records(PageKindV3::Chunks, &self.chunks),
            PagePayload::records(PageKindV3::Spans, &self.spans),
            PagePayload::records(PageKindV3::Entities, &self.entities),
            PagePayload::records(
                PageKindV3::CanonicalEntityBindings,
                &self.canonical_entity_bindings,
            ),
            PagePayload::records(PageKindV3::Mentions, &self.mentions),
            PagePayload::records(PageKindV3::Evidence, &self.evidence),
            PagePayload::records(PageKindV3::StructuralEdges, &self.structural_edges),
            PagePayload::records(
                PageKindV3::TypedRelationshipCandidates,
                &self.typed_relationship_candidates,
            ),
            PagePayload::records(PageKindV3::IdentityCandidates, &self.identity_candidates),
            PagePayload::records(PageKindV3::Events, &self.events),
            PagePayload::records(PageKindV3::Episodes, &self.episodes),
            PagePayload::records(PageKindV3::EpisodeMemberships, &self.episode_memberships),
            PagePayload::records(PageKindV3::TemporalCandidates, &self.temporal_candidates),
            PagePayload::records(PageKindV3::CausalCandidates, &self.causal_candidates),
            PagePayload::records(
                PageKindV3::MemoryStateCandidates,
                &self.memory_state_candidates,
            ),
            PagePayload::records(PageKindV3::ContextualEvidence, &self.contextual_evidence),
            PagePayload::records(PageKindV3::NliAdjudications, &self.nli_adjudications),
            PagePayload::records(PageKindV3::Decisions, &self.decisions),
            PagePayload::records(PageKindV3::ValidityIntervals, &self.validity_intervals),
            PagePayload::records(PageKindV3::Supersessions, &self.supersessions),
            PagePayload::records(PageKindV3::Capabilities, &self.capabilities),
            PagePayload::records(PageKindV3::ModelIdentities, &self.model_identities),
            PagePayload::records(PageKindV3::StageReceipts, &self.stage_receipts),
            PagePayload::records(PageKindV3::PublicationReceipts, &self.publication_receipts),
            PagePayload::records(
                PageKindV3::CandidateEvidenceBindings,
                &self.candidate_evidence_bindings,
            ),
            PagePayload::records(PageKindV3::SemanticCandidates, &self.semantic_candidates),
            PagePayload::records(
                PageKindV3::ProducerCapabilitiesV3,
                &self.producer_capabilities_v3,
            ),
            PagePayload::records(PageKindV3::VocabularyPacks, &self.vocabulary_packs),
            PagePayload::records(
                PageKindV3::CandidateEndpointBindings,
                &self.candidate_endpoint_bindings,
            ),
        ]
    }
}

#[derive(Clone, Copy)]
struct PagePayload<'a> {
    kind: PageKindV3,
    bytes: &'a [u8],
    count: u64,
}

impl<'a> PagePayload<'a> {
    fn bytes(kind: PageKindV3, bytes: &'a [u8]) -> Self {
        Self {
            kind,
            bytes,
            count: bytes.len() as u64,
        }
    }

    fn records<T: Pod>(kind: PageKindV3, records: &'a [T]) -> Self {
        Self {
            kind,
            bytes: cast_slice(records),
            count: records.len() as u64,
        }
    }
}

pub fn write_generation_new(
    path: impl AsRef<Path>,
    authority: GenerationWriteAuthorityV3,
    mut pages: GenerationPagesV3,
) -> Result<VerifiedGraphGenerationV3, MemoryContractError> {
    pages.canonicalize();
    let path = path.as_ref();
    let result = write_generation_file(path, authority, &pages);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result?;
    match VerifiedGraphGenerationV3::open(path) {
        Ok(generation) => Ok(generation),
        Err(error) => {
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

fn write_generation_file(
    path: &Path,
    authority: GenerationWriteAuthorityV3,
    pages: &GenerationPagesV3,
) -> Result<(), MemoryContractError> {
    let payloads = pages.payloads();
    let header_size = size_of::<GenerationHeaderV3>() as u64;
    let directory_offset = align_up(header_size, PAGE_ALIGNMENT);
    let directory_len = (size_of::<PageDescriptorV3>() * PageKindV3::ALL.len()) as u64;
    let mut cursor = align_up(directory_offset + directory_len, PAGE_ALIGNMENT);
    let mut directory = [PageDescriptorV3::zeroed(); 39];

    for (index, payload) in payloads.iter().enumerate() {
        if payload.kind != PageKindV3::ALL[index] {
            return Err(MemoryContractError::UnexpectedPageOrder {
                index,
                actual: payload.kind,
                expected: PageKindV3::ALL[index],
            });
        }
        cursor = align_up(cursor, PAGE_ALIGNMENT);
        directory[index] = PageDescriptorV3 {
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
            MemoryContractError::OversizedGeneration {
                actual: u64::MAX,
                maximum: MAX_GENERATION_BYTES,
            },
        )?;
    }
    let total_len = align_up(cursor, PAGE_ALIGNMENT);
    if total_len > MAX_GENERATION_BYTES {
        return Err(MemoryContractError::OversizedGeneration {
            actual: total_len,
            maximum: MAX_GENERATION_BYTES,
        });
    }

    let source_text_hash = directory[(PageKindV3::SourceText as usize) - 1].hash;
    let source_hashes = PageKindV3::ALL
        [(PageKindV3::Sources as usize) - 1..=(PageKindV3::Spans as usize) - 1]
        .iter()
        .map(|kind| directory[(*kind as usize) - 1].hash)
        .collect::<Vec<_>>();
    let source_set_hash = compute_source_set_hash(&source_text_hash, &source_hashes);
    let mut header = GenerationHeaderV3 {
        magic: GRAPH_GENERATION_V3_MAGIC,
        version: GRAPH_GENERATION_V3_VERSION,
        header_size: header_size as u32,
        page_count: PageKindV3::ALL.len() as u32,
        flags: 0,
        total_len,
        directory_offset,
        directory_len,
        namespace_hash: authority.namespace_hash,
        source_set_hash,
        generation_hash: [0; 32],
        cohort_hash: authority.cohort_hash,
        registry_revision: authority.registry_revision,
        producer_generation: authority.producer_generation,
        published_generation: authority.published_generation,
        source_count: pages.sources.len() as u64,
        document_revision_count: pages.document_revisions.len() as u64,
        conversation_count: pages.conversations.len() as u64,
        turn_count: pages.turns.len() as u64,
        reserved: [0; 5],
    };
    header.generation_hash = compute_generation_hash(&header, &directory);

    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?;
    write_at(&mut file, 0, bytes_of(&header), path)?;
    write_at(&mut file, directory_offset, cast_slice(&directory), path)?;
    for (descriptor, payload) in directory.iter().zip(payloads) {
        write_at(&mut file, descriptor.offset, payload.bytes, path)?;
    }
    file.set_len(total_len)
        .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?;
    file.sync_all()
        .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?;
    header.flags = HEADER_FLAG_COMPLETE;
    write_at(&mut file, 0, bytes_of(&header), path)?;
    file.sync_all()
        .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))
}

fn write_at(
    file: &mut std::fs::File,
    offset: u64,
    bytes: &[u8],
    path: &Path,
) -> Result<(), MemoryContractError> {
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.write_all(bytes))
        .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))
}
