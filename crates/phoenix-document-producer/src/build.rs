use crate::DocumentProducerError;
use bytemuck::cast_slice;
use hashbrown::HashSet;
use phoenix_analysis_contract::{
    AnalysisSpanRecord, PhoenixStructuralSubstrateV1, StructuralSpanKind, NO_STRUCTURAL_PARENT,
};
use phoenix_graph_generation_v2::{
    AuthorityClass, CacheState, CapabilityRecord, CapabilityState, ChapterRecord, ChunkRecord,
    DocumentRecord, GenerationPages, ModelIdentityRecord, PageKind, ParagraphRecord,
    ProducerProduct, PublicationReceiptRecord, PublicationStatus, SentenceRecord, SpanRecord,
    StageReceiptRecord, StringRef, StructuralEdgeRecord, STAGE_FLAG_ALLOCATION_NOT_MEASURED,
    STAGE_FLAG_QUEUE_NOT_OBSERVED, STAGE_FLAG_TIMING_NOT_MEASURED,
};

const STRUCTURAL_EDGE_FLAG_SOURCE: u16 = 1;
const DOCUMENT_TO_CHAPTER: u16 = 1;
const CHAPTER_TO_PARAGRAPH: u16 = 2;
const PARAGRAPH_TO_SENTENCE: u16 = 3;
const DOCUMENT_TO_CHUNK: u16 = 4;

pub(crate) struct BuiltStructuralGeneration {
    pub strings: Vec<u8>,
    pub documents: Vec<DocumentRecord>,
    pub chapters: Vec<ChapterRecord>,
    pub paragraphs: Vec<ParagraphRecord>,
    pub sentences: Vec<SentenceRecord>,
    pub chunks: Vec<ChunkRecord>,
    pub spans: Vec<SpanRecord>,
    pub structural_edges: Vec<StructuralEdgeRecord>,
    pub capabilities: Vec<CapabilityRecord>,
    pub model_identities: Vec<ModelIdentityRecord>,
    pub stage_receipts: Vec<StageReceiptRecord>,
    pub publication_receipts: Vec<PublicationReceiptRecord>,
    pub cohort_hash: [u8; 32],
}

impl BuiltStructuralGeneration {
    pub fn pages(&self) -> GenerationPages<'_> {
        GenerationPages {
            strings: &self.strings,
            documents: &self.documents,
            chapters: &self.chapters,
            paragraphs: &self.paragraphs,
            sentences: &self.sentences,
            chunks: &self.chunks,
            spans: &self.spans,
            entities: &[],
            mentions: &[],
            evidence: &[],
            structural_edges: &self.structural_edges,
            typed_relationship_candidates: &[],
            identity_candidates: &[],
            events: &[],
            episodes: &[],
            episode_memberships: &[],
            temporal_candidates: &[],
            causal_candidates: &[],
            memory_state_candidates: &[],
            contextual_evidence: &[],
            nli_adjudications: &[],
            decisions: &[],
            capabilities: &self.capabilities,
            model_identities: &self.model_identities,
            stage_receipts: &self.stage_receipts,
            publication_receipts: &self.publication_receipts,
            candidate_evidence_bindings: &[],
            canonical_entity_bindings: &[],
        }
    }
}

pub(crate) fn build_structural_pages(
    text: &str,
    structural: &PhoenixStructuralSubstrateV1,
) -> Result<BuiltStructuralGeneration, DocumentProducerError> {
    structural
        .validate()
        .map_err(DocumentProducerError::InvalidStructuralInput)?;
    let binding = &structural.binding;
    if structural.source_len as usize != text.len()
        || blake3::hash(text.as_bytes()).as_bytes() != &binding.content_hash
    {
        return Err(DocumentProducerError::SourceBindingMismatch);
    }

    let paragraph_spans = structural
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Paragraph)
        .collect::<Vec<_>>();
    let chapter_spans = structural
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Chapter)
        .collect::<Vec<_>>();
    let mut strings = StringSlab::default();
    let source_id = strings.push(&binding.source_document_id)?;
    let document_id = stable_id(
        b"document",
        &binding.content_hash,
        &[binding.native_document_id],
    );

    let chapter_ids = chapter_spans
        .iter()
        .enumerate()
        .map(|(ordinal, span)| structural_id(b"chapter", &binding.content_hash, ordinal, span))
        .collect::<Vec<_>>();
    let paragraph_ids = paragraph_spans
        .iter()
        .enumerate()
        .map(|(ordinal, span)| structural_id(b"paragraph", &binding.content_hash, ordinal, span))
        .collect::<Vec<_>>();

    let mut chapters = Vec::with_capacity(chapter_spans.len());
    for (ordinal, span) in chapter_spans.iter().enumerate() {
        chapters.push(ChapterRecord {
            id: chapter_ids[ordinal],
            document_id,
            title: strings.push(&span.label)?,
            start: span.start,
            end: span.end,
            paragraph_start: span.child_start,
            paragraph_end: span.child_end,
            ordinal: checked_len(ordinal)?,
            flags: 0,
            reserved: [0; 2],
        });
    }

    let mut paragraphs = Vec::with_capacity(paragraph_spans.len());
    for (ordinal, span) in paragraph_spans.iter().enumerate() {
        let chapter_id = *chapter_ids
            .get(span.parent_index as usize)
            .ok_or(DocumentProducerError::InvalidStructuralParent)?;
        paragraphs.push(ParagraphRecord {
            id: paragraph_ids[ordinal],
            document_id,
            chapter_id,
            start: span.start,
            end: span.end,
            sentence_start: span.child_start,
            sentence_end: span.child_end,
            ordinal: checked_len(ordinal)?,
            flags: 0,
            reserved: [0; 2],
        });
    }

    let mut sentences = Vec::with_capacity(structural.sentences.len());
    for (ordinal, sentence) in structural.sentences.iter().enumerate() {
        let paragraph_id = *paragraph_ids
            .get(sentence.paragraph_index as usize)
            .ok_or(DocumentProducerError::InvalidStructuralParent)?;
        sentences.push(SentenceRecord {
            id: stable_id(
                b"sentence",
                &binding.content_hash,
                &[
                    ordinal as u64,
                    u64::from(sentence.start),
                    u64::from(sentence.end),
                    sentence.content_hash,
                ],
            ),
            document_id,
            paragraph_id,
            content_hash: sentence.content_hash,
            start: sentence.start,
            end: sentence.end,
            ordinal: checked_len(ordinal)?,
            token_count: sentence.token_count,
            quality: sentence.quality as u16,
            dialogue_hint: sentence.dialogue_hint as u16,
            flags: 0,
            reserved_u16: 0,
            reserved: 0,
        });
    }

    let mut chunks = Vec::with_capacity(structural.chunks.len());
    for (ordinal, chunk) in structural.chunks.iter().enumerate() {
        chunks.push(ChunkRecord {
            id: stable_id(
                b"chunk",
                &binding.content_hash,
                &[
                    ordinal as u64,
                    u64::from(chunk.start),
                    u64::from(chunk.end),
                    chunk.content_hash,
                ],
            ),
            document_id,
            content_hash: chunk.content_hash,
            start: chunk.start,
            end: chunk.end,
            sentence_start: chunk.sentence_start,
            sentence_end: chunk.sentence_end,
            paragraph_start: chunk.paragraph_start,
            paragraph_end: chunk.paragraph_end,
            chapter_index: chunk.chapter_index,
            token_count: chunk.token_count,
            flags: 0,
            reserved: 0,
        });
    }

    let mut spans = Vec::with_capacity(structural.spans.len());
    let mut paragraph_ordinal = 0_usize;
    let mut chapter_ordinal = 0_usize;
    for span in &structural.spans {
        let (domain, ordinal, parent_id, label) = match span.kind {
            StructuralSpanKind::Paragraph => {
                let parent_id = *chapter_ids
                    .get(span.parent_index as usize)
                    .ok_or(DocumentProducerError::InvalidStructuralParent)?;
                let ordinal = paragraph_ordinal;
                paragraph_ordinal += 1;
                (
                    b"paragraph-span".as_slice(),
                    ordinal,
                    parent_id,
                    StringRef::default(),
                )
            }
            StructuralSpanKind::Chapter => {
                if span.parent_index != NO_STRUCTURAL_PARENT {
                    return Err(DocumentProducerError::InvalidStructuralParent);
                }
                let ordinal = chapter_ordinal;
                chapter_ordinal += 1;
                (
                    b"chapter-span".as_slice(),
                    ordinal,
                    document_id,
                    strings.push(&span.label)?,
                )
            }
        };
        spans.push(SpanRecord {
            id: structural_id(domain, &binding.content_hash, ordinal, span),
            document_id,
            parent_id,
            content_hash: span.content_hash,
            label,
            start: span.start,
            end: span.end,
            child_start: span.child_start,
            child_end: span.child_end,
            token_count: span.token_count,
            flags: 0,
            kind: span.kind as u16,
            dialogue_hint: span.dialogue_hint as u16,
            reserved: 0,
        });
    }

    let structural_edges =
        build_structural_edges(document_id, &chapters, &paragraphs, &sentences, &chunks);
    validate_unique_ids(
        document_id,
        &chapters,
        &paragraphs,
        &sentences,
        &chunks,
        &spans,
        &structural_edges,
    )?;
    let document = DocumentRecord {
        id: document_id,
        source_id,
        source_len: structural.source_len,
        chapter_count: checked_len(chapters.len())?,
        paragraph_count: checked_len(paragraphs.len())?,
        sentence_count: checked_len(sentences.len())?,
        chunk_count: checked_len(chunks.len())?,
        span_count: checked_len(spans.len())?,
        entity_count: 0,
        mention_count: 0,
        evidence_count: 0,
        structural_edge_count: checked_len(structural_edges.len())?,
        flags: 0,
        reserved: [0; 3],
    };

    let source_coordinate_hash = coordinate_hash(
        &document,
        &chapters,
        &paragraphs,
        &sentences,
        &chunks,
        &spans,
        &structural_edges,
    );
    let cohort_hash = cohort_hash(structural);
    let producer_name = strings.push("phoenix-document-producer/v1")?;
    let chunker_name = strings.push(&binding.chunker.model_id)?;
    let chunker_runtime = strings.push(&binding.chunker.runtime_id)?;
    let empty = strings.push("")?;
    let stage_name = strings.push("structural-substrate")?;
    let rust_runtime = strings.push("rust-native")?;
    let output_count = 1_u64
        + chapters.len() as u64
        + paragraphs.len() as u64
        + sentences.len() as u64
        + chunks.len() as u64
        + spans.len() as u64
        + structural_edges.len() as u64;

    Ok(BuiltStructuralGeneration {
        strings: strings.bytes,
        documents: vec![document],
        chapters,
        paragraphs,
        sentences,
        chunks,
        spans,
        structural_edges,
        capabilities: vec![CapabilityRecord {
            product: ProducerProduct::DocumentStructure as u16,
            authority: AuthorityClass::SourceAuthoritative as u16,
            state: CapabilityState::Produced as u16,
            flags_u16: 0,
            producer: producer_name,
            output_count,
            reused_generation: 0,
            model_identity_index: 0,
            flags: 0,
        }],
        model_identities: vec![
            ModelIdentityRecord {
                name: chunker_name,
                runtime: chunker_runtime,
                artifact_uri: empty,
                artifact_hash: binding.chunker.artifact_hash,
                config_hash: binding.chunker.config_hash,
                flags: 0,
                reserved: 0,
            },
            ModelIdentityRecord {
                name: producer_name,
                runtime: rust_runtime,
                artifact_uri: empty,
                artifact_hash: binding.producer_binary_hash,
                config_hash: cohort_hash,
                flags: 0,
                reserved: 0,
            },
        ],
        stage_receipts: vec![StageReceiptRecord {
            name: stage_name,
            span_id: stable_id(b"structural-span", &binding.content_hash, &[1]),
            parent_span_id: 0,
            elapsed_micros: 0,
            output_count,
            allocated_bytes: 0,
            copied_bytes: u64::from(structural.source_len),
            queue_high_water: 0,
            cache_state: CacheState::Computed as u16,
            status: CapabilityState::Produced as u16,
            flags: STAGE_FLAG_TIMING_NOT_MEASURED
                | STAGE_FLAG_ALLOCATION_NOT_MEASURED
                | STAGE_FLAG_QUEUE_NOT_OBSERVED,
        }],
        publication_receipts: vec![PublicationReceiptRecord {
            authority_hash: source_coordinate_hash,
            previous_generation_hash: [0; 32],
            generation_id: binding.analysis_generation,
            previous_generation_id: 0,
            document_revision: binding.document_revision,
            registry_revision: binding.target_registry_revision,
            published_at_unix_millis: 0,
            status: PublicationStatus::Published as u16,
            flags_u16: 0,
            flags: 0,
        }],
        cohort_hash,
    })
}

fn build_structural_edges(
    document_id: u64,
    chapters: &[ChapterRecord],
    paragraphs: &[ParagraphRecord],
    sentences: &[SentenceRecord],
    chunks: &[ChunkRecord],
) -> Vec<StructuralEdgeRecord> {
    let capacity = chapters.len() + paragraphs.len() + sentences.len() + chunks.len();
    let mut edges = Vec::with_capacity(capacity);
    for chapter in chapters {
        push_edge(&mut edges, document_id, chapter.id, DOCUMENT_TO_CHAPTER);
    }
    for paragraph in paragraphs {
        push_edge(
            &mut edges,
            paragraph.chapter_id,
            paragraph.id,
            CHAPTER_TO_PARAGRAPH,
        );
    }
    for sentence in sentences {
        push_edge(
            &mut edges,
            sentence.paragraph_id,
            sentence.id,
            PARAGRAPH_TO_SENTENCE,
        );
    }
    for chunk in chunks {
        push_edge(&mut edges, document_id, chunk.id, DOCUMENT_TO_CHUNK);
    }
    edges
}

fn push_edge(edges: &mut Vec<StructuralEdgeRecord>, source: u64, target: u64, relation: u16) {
    edges.push(StructuralEdgeRecord {
        id: stable_id(
            b"structural-edge",
            &[0; 32],
            &[source, target, relation as u64],
        ),
        source_id: source,
        target_id: target,
        evidence_id: 0,
        weight_bits: 1.0_f32.to_bits(),
        relation,
        flags: STRUCTURAL_EDGE_FLAG_SOURCE,
    });
}

fn validate_unique_ids(
    document_id: u64,
    chapters: &[ChapterRecord],
    paragraphs: &[ParagraphRecord],
    sentences: &[SentenceRecord],
    chunks: &[ChunkRecord],
    spans: &[SpanRecord],
    edges: &[StructuralEdgeRecord],
) -> Result<(), DocumentProducerError> {
    let expected =
        1 + chapters.len() + paragraphs.len() + sentences.len() + chunks.len() + spans.len();
    let mut ids = HashSet::with_capacity(expected);
    ids.insert(document_id);
    for id in chapters
        .iter()
        .map(|record| record.id)
        .chain(paragraphs.iter().map(|record| record.id))
        .chain(sentences.iter().map(|record| record.id))
        .chain(chunks.iter().map(|record| record.id))
        .chain(spans.iter().map(|record| record.id))
    {
        if id == 0 || !ids.insert(id) {
            return Err(DocumentProducerError::IdentityCollision);
        }
    }
    let mut edge_ids = HashSet::with_capacity(edges.len());
    if edges
        .iter()
        .any(|edge| edge.id == 0 || !edge_ids.insert(edge.id))
    {
        return Err(DocumentProducerError::IdentityCollision);
    }
    Ok(())
}

fn coordinate_hash(
    document: &DocumentRecord,
    chapters: &[ChapterRecord],
    paragraphs: &[ParagraphRecord],
    sentences: &[SentenceRecord],
    chunks: &[ChunkRecord],
    spans: &[SpanRecord],
    edges: &[StructuralEdgeRecord],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.document-producer/v1/source-coordinates\0");
    hasher.update(bytemuck::bytes_of(document));
    hasher.update(cast_slice(chapters));
    hasher.update(cast_slice(paragraphs));
    hasher.update(cast_slice(sentences));
    hasher.update(cast_slice(chunks));
    hasher.update(cast_slice(spans));
    hasher.update(cast_slice(edges));
    *hasher.finalize().as_bytes()
}

pub(crate) fn cohort_hash(structural: &PhoenixStructuralSubstrateV1) -> [u8; 32] {
    let binding = &structural.binding;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.document-producer/v1/cohort\0");
    hasher.update(binding.source_document_id.as_bytes());
    hasher.update(&binding.content_hash);
    hasher.update(&binding.producer_binary_hash);
    hasher.update(binding.chunker.model_id.as_bytes());
    hasher.update(&binding.chunker.artifact_hash);
    hasher.update(&binding.chunker.config_hash);
    hasher.update(binding.chunker.runtime_id.as_bytes());
    *hasher.finalize().as_bytes()
}

fn structural_id(
    domain: &[u8],
    content_hash: &[u8; 32],
    ordinal: usize,
    span: &AnalysisSpanRecord,
) -> u64 {
    stable_id(
        domain,
        content_hash,
        &[
            ordinal as u64,
            u64::from(span.start),
            u64::from(span.end),
            span.content_hash,
        ],
    )
}

fn stable_id(domain: &[u8], content_hash: &[u8; 32], values: &[u64]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.document-producer/v1/id\0");
    hasher.update(domain);
    hasher.update(content_hash);
    for value in values {
        hasher.update(&value.to_le_bytes());
    }
    let bytes = hasher.finalize();
    let mut id_bytes = [0_u8; 8];
    id_bytes.copy_from_slice(&bytes.as_bytes()[..8]);
    let mut id = u64::from_le_bytes(id_bytes);
    if id == 0 {
        id = 1;
    }
    id
}

fn checked_len(value: usize) -> Result<u32, DocumentProducerError> {
    u32::try_from(value).map_err(|_| DocumentProducerError::RecordCountOverflow)
}

#[derive(Default)]
struct StringSlab {
    bytes: Vec<u8>,
}

impl StringSlab {
    fn push(&mut self, value: &str) -> Result<StringRef, DocumentProducerError> {
        let offset = self.bytes.len() as u64;
        let length =
            u32::try_from(value.len()).map_err(|_| DocumentProducerError::RecordCountOverflow)?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(StringRef {
            offset,
            length,
            reserved: 0,
        })
    }
}

pub(crate) fn structural_page_kinds() -> [PageKind; 7] {
    [
        PageKind::Documents,
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::StructuralEdges,
    ]
}
