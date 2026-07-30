use crate::build::build_entity_pages;
use crate::types::{EntityProducerInput, EntityPublicationReceipt};
use crate::{EditorPaintProjection, EntityProducerError};
use phoenix_graph_generation_v2::{
    write_generation_new, CandidateEvidenceBindingRecord, CanonicalEntityBindingRecord,
    CausalCandidateRecord, ChapterRecord, ChunkRecord, ContextualEvidenceRecord, DecisionRecord,
    DocumentRecord, EntityRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord,
    EvidenceRecord, GenerationPages, GenerationWriteAuthority, IdentityCandidateRecord,
    MemoryStateCandidateRecord, MentionRecord, NliAdjudicationRecord, PageKind, ParagraphRecord,
    SentenceRecord, SpanRecord, StructuralEdgeRecord, TemporalCandidateRecord,
    TypedRelationshipCandidateRecord, VerifiedGraphGenerationV2,
};
use std::fs;
use std::path::Path;

pub struct VerifiedEntityGeneration {
    generation: VerifiedGraphGenerationV2,
    paint: EditorPaintProjection,
    receipt: EntityPublicationReceipt,
}

impl VerifiedEntityGeneration {
    pub fn generation(&self) -> &VerifiedGraphGenerationV2 {
        &self.generation
    }

    pub fn paint(&self) -> &EditorPaintProjection {
        &self.paint
    }

    pub fn receipt(&self) -> &EntityPublicationReceipt {
        &self.receipt
    }

    pub fn into_generation(self) -> VerifiedGraphGenerationV2 {
        self.generation
    }
}

pub fn publish_entity_generation_new(
    path: impl AsRef<Path>,
    input: EntityProducerInput<'_>,
) -> Result<VerifiedEntityGeneration, EntityProducerError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EntityProducerError::PublicationIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let built = build_entity_pages(input)?;
    let source = input.structural;

    let chapters: &[ChapterRecord] = source.typed_page(PageKind::Chapters)?;
    let paragraphs: &[ParagraphRecord] = source.typed_page(PageKind::Paragraphs)?;
    let sentences: &[SentenceRecord] = source.typed_page(PageKind::Sentences)?;
    let chunks: &[ChunkRecord] = source.typed_page(PageKind::Chunks)?;
    let spans: &[SpanRecord] = source.typed_page(PageKind::Spans)?;
    let structural_edges: &[StructuralEdgeRecord] = source.typed_page(PageKind::StructuralEdges)?;
    let typed_relationship_candidates: &[TypedRelationshipCandidateRecord] =
        source.typed_page(PageKind::TypedRelationshipCandidates)?;
    let events: &[EventRecord] = source.typed_page(PageKind::Events)?;
    let episodes: &[EpisodeRecord] = source.typed_page(PageKind::Episodes)?;
    let episode_memberships: &[EpisodeMembershipRecord] =
        source.typed_page(PageKind::EpisodeMemberships)?;
    let temporal_candidates: &[TemporalCandidateRecord] =
        source.typed_page(PageKind::TemporalCandidates)?;
    let causal_candidates: &[CausalCandidateRecord] =
        source.typed_page(PageKind::CausalCandidates)?;
    let memory_state_candidates: &[MemoryStateCandidateRecord] =
        source.typed_page(PageKind::MemoryStateCandidates)?;
    let contextual_evidence: &[ContextualEvidenceRecord] =
        source.typed_page(PageKind::ContextualEvidence)?;
    let nli_adjudications: &[NliAdjudicationRecord] =
        source.typed_page(PageKind::NliAdjudications)?;
    let decisions: &[DecisionRecord] = source.typed_page(PageKind::Decisions)?;

    let authority = GenerationWriteAuthority {
        source_document_id_hash: source.header().source_document_id_hash,
        content_hash: source.header().content_hash,
        cohort_hash: source.header().cohort_hash,
        native_document_id: source.header().native_document_id,
        document_revision: source.header().document_revision,
        registry_revision: source.header().registry_revision,
        producer_generation: input.ner.binding.analysis_generation,
        published_generation: input.published_generation,
    };
    let generation = write_generation_new(
        path,
        authority,
        GenerationPages {
            strings: &built.strings,
            documents: std::slice::from_ref(&built.document),
            chapters,
            paragraphs,
            sentences,
            chunks,
            spans,
            entities: &built.entities,
            mentions: &built.mentions,
            evidence: &built.evidence,
            structural_edges,
            typed_relationship_candidates,
            identity_candidates: &built.identity_candidates,
            events,
            episodes,
            episode_memberships,
            temporal_candidates,
            causal_candidates,
            memory_state_candidates,
            contextual_evidence,
            nli_adjudications,
            decisions,
            capabilities: &built.capabilities,
            model_identities: &built.model_identities,
            stage_receipts: &built.stage_receipts,
            publication_receipts: &built.publication_receipts,
            candidate_evidence_bindings: &built.candidate_evidence_bindings,
            canonical_entity_bindings: &built.canonical_entity_bindings,
        },
    )?;
    verify_output(&generation, source, &built)?;
    let receipt = EntityPublicationReceipt {
        path: path.to_path_buf(),
        previous_generation_hash: source.header().generation_hash,
        generation_hash: generation.header().generation_hash,
        content_hash: generation.header().content_hash,
        entity_count: checked_count(built.entities.len())?,
        mention_count: checked_count(built.mentions.len())?,
        evidence_count: checked_count(built.evidence.len())?,
        identity_candidate_count: checked_count(built.identity_candidates.len())?,
        paint_span_count: checked_count(built.paint.spans.len())?,
        graph_evidence_hash: built.authority_hash,
        paint_projection_hash: built.paint.hash,
    };
    Ok(VerifiedEntityGeneration {
        generation,
        paint: built.paint,
        receipt,
    })
}

fn verify_output(
    generation: &VerifiedGraphGenerationV2,
    source: &VerifiedGraphGenerationV2,
    built: &crate::build::BuiltEntityGeneration,
) -> Result<(), EntityProducerError> {
    for kind in [
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::StructuralEdges,
    ] {
        if generation.descriptor(kind).hash != source.descriptor(kind).hash {
            return Err(EntityProducerError::AuthorityMismatch);
        }
    }
    let documents: &[DocumentRecord] = generation.typed_page(PageKind::Documents)?;
    let entities: &[EntityRecord] = generation.typed_page(PageKind::Entities)?;
    let mentions: &[MentionRecord] = generation.typed_page(PageKind::Mentions)?;
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence)?;
    let candidates: &[IdentityCandidateRecord] =
        generation.typed_page(PageKind::IdentityCandidates)?;
    let bindings: &[CandidateEvidenceBindingRecord] =
        generation.typed_page(PageKind::CandidateEvidenceBindings)?;
    let canonical_bindings: &[CanonicalEntityBindingRecord] =
        generation.typed_page(PageKind::CanonicalEntityBindings)?;
    if bytemuck::cast_slice::<DocumentRecord, u8>(documents) != bytemuck::bytes_of(&built.document)
        || entities.len() != built.entities.len()
        || mentions.len() != built.mentions.len()
        || evidence.len() != built.evidence.len()
        || candidates.len() != built.identity_candidates.len()
        || bindings.len() != built.candidate_evidence_bindings.len()
        || canonical_bindings.len() != built.canonical_entity_bindings.len()
    {
        return Err(EntityProducerError::AuthorityMismatch);
    }
    Ok(())
}

fn checked_count(value: usize) -> Result<u32, EntityProducerError> {
    u32::try_from(value).map_err(|_| EntityProducerError::RecordCountOverflow)
}
