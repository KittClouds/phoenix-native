use crate::build::{build_story_pages, BuiltStoryGeneration};
use crate::{StoryProducerError, StoryProducerInput, StoryPublicationReceipt};
use phoenix_graph_generation_v2::{
    write_generation_new, CandidateEvidenceBindingRecord, GenerationPages,
    GenerationWriteAuthority, PageKind, VerifiedGraphGenerationV2,
};
use std::fs;
use std::path::Path;

pub struct VerifiedStoryGeneration {
    generation: VerifiedGraphGenerationV2,
    receipt: StoryPublicationReceipt,
}

impl VerifiedStoryGeneration {
    pub fn generation(&self) -> &VerifiedGraphGenerationV2 {
        &self.generation
    }

    pub fn receipt(&self) -> &StoryPublicationReceipt {
        &self.receipt
    }
}

pub fn publish_story_generation_new(
    path: impl AsRef<Path>,
    input: StoryProducerInput<'_>,
) -> Result<VerifiedStoryGeneration, StoryProducerError> {
    let path = path.as_ref();
    crate::story_lens_identity()
        .map_err(|error| StoryProducerError::InvalidLensContract(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StoryProducerError::PublicationIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let built = build_story_pages(&input)?;
    let source = input.source;
    let generation = write_generation_new(
        path,
        GenerationWriteAuthority {
            source_document_id_hash: source.header().source_document_id_hash,
            content_hash: source.header().content_hash,
            cohort_hash: source.header().cohort_hash,
            native_document_id: source.header().native_document_id,
            document_revision: source.header().document_revision,
            registry_revision: source.header().registry_revision,
            producer_generation: source.header().producer_generation,
            published_generation: input.published_generation,
        },
        GenerationPages {
            strings: &built.strings,
            documents: typed(source, PageKind::Documents)?,
            chapters: typed(source, PageKind::Chapters)?,
            paragraphs: typed(source, PageKind::Paragraphs)?,
            sentences: typed(source, PageKind::Sentences)?,
            chunks: typed(source, PageKind::Chunks)?,
            spans: typed(source, PageKind::Spans)?,
            entities: typed(source, PageKind::Entities)?,
            mentions: typed(source, PageKind::Mentions)?,
            evidence: typed(source, PageKind::Evidence)?,
            structural_edges: typed(source, PageKind::StructuralEdges)?,
            typed_relationship_candidates: &built.typed_relationships,
            identity_candidates: typed(source, PageKind::IdentityCandidates)?,
            events: &built.events,
            episodes: &built.episodes,
            episode_memberships: &built.episode_memberships,
            temporal_candidates: &built.temporal,
            causal_candidates: &built.causal,
            memory_state_candidates: &built.memory_state,
            contextual_evidence: typed(source, PageKind::ContextualEvidence)?,
            nli_adjudications: typed(source, PageKind::NliAdjudications)?,
            decisions: typed(source, PageKind::Decisions)?,
            capabilities: &built.capabilities,
            model_identities: &built.model_identities,
            stage_receipts: &built.stage_receipts,
            publication_receipts: &built.publication_receipts,
            candidate_evidence_bindings: &built.candidate_evidence_bindings,
            canonical_entity_bindings: typed(source, PageKind::CanonicalEntityBindings)?,
        },
    )?;
    verify_output(&generation, source, &built)?;
    let receipt = StoryPublicationReceipt {
        path: path.to_path_buf(),
        previous_generation_hash: source.header().generation_hash,
        generation_hash: generation.header().generation_hash,
        candidate_authority_hash: built.candidate_authority_hash,
        relationship_count: count(built.typed_relationships.len())?,
        event_count: count(built.events.len())?,
        episode_count: count(built.episodes.len())?,
        membership_count: count(built.episode_memberships.len())?,
        temporal_count: count(built.temporal.len())?,
        causal_count: count(built.causal.len())?,
        memory_state_count: count(built.memory_state.len())?,
        evidence_binding_count: count(
            built.candidate_evidence_bindings.len() - built.story_binding_start,
        )?,
        model_ranked_count: count(built.model_ranked_count)?,
        unsupported_mask: built.unsupported_mask,
    };
    Ok(VerifiedStoryGeneration {
        generation,
        receipt,
    })
}

fn verify_output(
    generation: &VerifiedGraphGenerationV2,
    source: &VerifiedGraphGenerationV2,
    built: &BuiltStoryGeneration,
) -> Result<(), StoryProducerError> {
    for kind in [
        PageKind::Documents,
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::Entities,
        PageKind::Mentions,
        PageKind::Evidence,
        PageKind::StructuralEdges,
        PageKind::IdentityCandidates,
        PageKind::ContextualEvidence,
        PageKind::NliAdjudications,
        PageKind::Decisions,
        PageKind::CanonicalEntityBindings,
    ] {
        if generation.descriptor(kind).hash != source.descriptor(kind).hash {
            return Err(StoryProducerError::AuthorityMismatch);
        }
    }
    let source_bindings: &[CandidateEvidenceBindingRecord] =
        source.typed_page(PageKind::CandidateEvidenceBindings)?;
    let output_bindings: &[CandidateEvidenceBindingRecord] =
        generation.typed_page(PageKind::CandidateEvidenceBindings)?;
    let output_strings = generation.page_bytes(PageKind::Strings);
    let source_strings = source.page_bytes(PageKind::Strings);
    let preserved_bindings = output_bindings
        .get(..source_bindings.len())
        .is_some_and(|prefix| {
            bytemuck::cast_slice::<_, u8>(prefix) == bytemuck::cast_slice::<_, u8>(source_bindings)
        });
    if output_strings != built.strings
        || !output_strings.starts_with(source_strings)
        || !preserved_bindings
        || output_bindings.len() != built.candidate_evidence_bindings.len()
        || !records_match(
            generation,
            PageKind::TypedRelationshipCandidates,
            &built.typed_relationships,
        )
        || !records_match(generation, PageKind::Events, &built.events)
        || !records_match(generation, PageKind::Episodes, &built.episodes)
        || !records_match(
            generation,
            PageKind::EpisodeMemberships,
            &built.episode_memberships,
        )
        || !records_match(generation, PageKind::TemporalCandidates, &built.temporal)
        || !records_match(generation, PageKind::CausalCandidates, &built.causal)
        || !records_match(
            generation,
            PageKind::MemoryStateCandidates,
            &built.memory_state,
        )
        || !records_match(
            generation,
            PageKind::CandidateEvidenceBindings,
            &built.candidate_evidence_bindings,
        )
    {
        return Err(StoryProducerError::AuthorityMismatch);
    }
    Ok(())
}

fn records_match<T: bytemuck::Pod>(
    generation: &VerifiedGraphGenerationV2,
    kind: PageKind,
    expected: &[T],
) -> bool {
    generation.page_bytes(kind) == bytemuck::cast_slice(expected)
}

fn typed<T: bytemuck::Pod>(
    source: &VerifiedGraphGenerationV2,
    kind: PageKind,
) -> Result<&[T], StoryProducerError> {
    Ok(source.typed_page(kind)?)
}

fn count(value: usize) -> Result<u32, StoryProducerError> {
    u32::try_from(value).map_err(|_| StoryProducerError::RecordCountOverflow)
}
