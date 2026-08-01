use super::*;
use phoenix_analysis_contract::{
    NliCandidateKind, PhoenixDocumentAnalysisV1, PhoenixProducerCoordinatorV1,
};
use phoenix_document_producer::{publish_or_reuse_structural_generation, StructuralProducerInput};
use phoenix_entity_producer::{
    entity_review_catalog, publish_entity_generation_new, EntityProducerInput,
    IdentityCandidateInput, IdentityCandidateKind, UserTaggedEntityInput, UserTaggedMentionInput,
};
use phoenix_graph_generation_v2::{
    CandidateId, CapabilityRecord, CapabilityState, ContextualEvidenceRecord, EntityId, MentionId,
    MentionRecord, ModelIdentityRecord, PageKind, ProducerProduct, VerifiedGraphGenerationV2,
};
use phoenix_semantic_review::ReviewCatalog;
use phoenix_story_producer::{
    publish_deterministic_story_generation_new, story_review_catalog,
    DeterministicStoryProducerInput,
};
use std::collections::BTreeMap;
use std::fs;

#[cfg(test)]
use phoenix_graph_generation_v2::{
    write_generation_new, ChapterRecord, ChunkRecord, DocumentRecord, EntityRecord, EvidenceRecord,
    GenerationPages, GenerationWriteAuthority, ParagraphRecord, SentenceRecord, StringRef,
    StructuralEdgeRecord,
};

const AUTHORITY_DIRECTORY: &str = "graph-authority-v2";
const MAX_AUTHORITY_GENERATIONS: usize = 4_096;
const SEMANTIC_AUTHORITY_CONTRACT: &str =
    "phoenix-app-core/semantic-authority-v3/evidence-bound-full-lanes-v1";

#[derive(Debug)]
pub(super) struct ProductionSceneAuthorityV2 {
    pub generation: Arc<VerifiedGraphGenerationV2>,
    pub catalog: Arc<ReviewCatalog>,
}

pub(super) fn produce(
    workspace_path: &Path,
    lease: &DocumentLease,
    registry: &EntityRegistry,
    analysis: &PhoenixDocumentAnalysisV1,
    structural: &PhoenixStructuralSubstrateV1,
    coordinator: &PhoenixProducerCoordinatorV1,
) -> Result<ProductionSceneAuthorityV2, KernelError> {
    coordinator
        .validate_final(analysis, structural)
        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let binding = &analysis.ner.binding;
    if structural.binding != *binding
        || coordinator.binding != *binding
        || binding.native_document_id != lease.entry_id.0
        || binding.document_revision != lease.revision.0
        || binding.content_hash != lease.content_hash.0
        || binding.target_registry_revision != registry.revision()
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }

    let root = authority_root(workspace_path)?;
    let structural_generation = publish_or_reuse_structural_generation(
        root.join("structural"),
        StructuralProducerInput {
            text: &lease.content,
            structural,
        },
    )?;
    let candidates = identity_candidates(analysis, coordinator)?;
    let user_entities = registry
        .entities()
        .iter()
        .filter(|entity| entity.sources.user_tagged)
        .map(|entity| UserTaggedEntityInput {
            stable_id: EntityId(entity.id),
            label: &entity.label,
            kind: entity.kind,
            custom_kind: entity.custom_kind.as_deref(),
        })
        .collect::<Vec<_>>();
    let user_mentions = registry
        .active_mentions_for(lease)
        .map(|(mention, _)| UserTaggedMentionInput {
            source_entity_id: EntityId(mention.entity_id),
            start: mention.start,
            end: mention.end,
            surface: &mention.surface,
        })
        .collect::<Vec<_>>();
    let source = structural_generation.generation();
    let entity_path = root.join("generations").join(entity_file_name(
        lease,
        registry.revision(),
        binding.analysis_generation,
    ));
    let entity_generation = if entity_path.is_file() {
        let opened = VerifiedGraphGenerationV2::open(&entity_path)?;
        verify_existing_entity(
            &opened,
            source,
            lease,
            registry.revision(),
            binding.analysis_generation,
        )?;
        opened
    } else {
        publish_entity_generation_new(
            &entity_path,
            EntityProducerInput {
                text: &lease.content,
                structural: source,
                ner: &analysis.ner,
                user_entities: &user_entities,
                user_mentions: &user_mentions,
                merge_decisions: &[],
                identity_candidates: &candidates,
                published_generation: source
                    .header()
                    .published_generation
                    .checked_add(1)
                    .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            },
        )?
        .into_generation()
    };
    let semantic_path = root.join("generations").join(semantic_file_name(
        lease,
        registry.revision(),
        binding.analysis_generation,
    ));
    let generation = if semantic_path.is_file() {
        let opened = VerifiedGraphGenerationV2::open(&semantic_path)?;
        verify_existing_semantic(&opened, &entity_generation)?;
        opened
    } else {
        let contextual_evidence = contextual_evidence(coordinator, &entity_generation)?;
        publish_deterministic_story_generation_new(
            &semantic_path,
            DeterministicStoryProducerInput {
                text: &lease.content,
                source: &entity_generation,
                contextual_evidence: &contextual_evidence,
                producer_binary_hash: semantic_authority_hash(),
                published_generation: entity_generation
                    .header()
                    .published_generation
                    .checked_add(1)
                    .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            },
        )?
        .into_generation()
    };
    authority_from_generation(generation)
}

pub(super) fn open_exact(
    workspace_path: &Path,
    document_id: u64,
    registry_revision: u64,
    generation_hash: [u8; 32],
) -> Result<ProductionSceneAuthorityV2, KernelError> {
    let parent = workspace_path
        .parent()
        .ok_or_else(|| KernelError::AnalysisProducerFailed("workspace has no parent".into()))?;
    let directories = [
        parent.join(AUTHORITY_DIRECTORY).join("generations"),
        parent.join("atlas-decision-authority-v2"),
    ];
    let mut candidates = Vec::new();
    for directory in directories {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(KernelError::AnalysisProducerFailed(format!(
                    "open V2 authority directory {}: {error}",
                    directory.display()
                )))
            }
        };
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    KernelError::AnalysisProducerFailed(format!(
                        "read V2 authority directory {}: {error}",
                        directory.display()
                    ))
                })?
                .path();
            if path
                .extension()
                .is_some_and(|extension| extension == "pgg2")
            {
                candidates.push(path);
                if candidates.len() > MAX_AUTHORITY_GENERATIONS {
                    return Err(KernelError::AnalysisProducerFailed(
                        "V2 authority generation inventory exceeds its bound".into(),
                    ));
                }
            }
        }
    }
    candidates.sort_unstable();
    for path in candidates {
        let generation = VerifiedGraphGenerationV2::open(&path)?;
        if generation.header().generation_hash != generation_hash {
            continue;
        }
        let header = generation.header();
        if header.native_document_id != document_id || header.registry_revision != registry_revision
        {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
        verify_reopen_contract(&generation)?;
        return authority_from_generation(generation);
    }
    Err(KernelError::MissingV2CompilerAuthority)
}

fn authority_from_generation(
    generation: VerifiedGraphGenerationV2,
) -> Result<ProductionSceneAuthorityV2, KernelError> {
    let catalog = review_catalog(&generation)?;
    Ok(ProductionSceneAuthorityV2 {
        generation: Arc::new(generation),
        catalog,
    })
}

pub(super) fn review_catalog(
    generation: &VerifiedGraphGenerationV2,
) -> Result<Arc<ReviewCatalog>, KernelError> {
    Ok(Arc::new(ReviewCatalog::merge_exact([
        entity_review_catalog(generation)?,
        story_review_catalog(generation)?,
    ])?))
}

fn authority_root(workspace_path: &Path) -> Result<PathBuf, KernelError> {
    let parent = workspace_path
        .parent()
        .ok_or_else(|| KernelError::AnalysisProducerFailed("workspace has no parent".into()))?;
    let root = parent.join(AUTHORITY_DIRECTORY);
    fs::create_dir_all(root.join("generations")).map_err(|error| {
        KernelError::AnalysisProducerFailed(format!("create V2 authority directory: {error}"))
    })?;
    Ok(root)
}

fn entity_file_name(
    lease: &DocumentLease,
    registry_revision: u64,
    producer_generation: u64,
) -> String {
    format!(
        "{:016x}-r{}-rr{}-g{}-{}-entity-v2.pgg2",
        lease.entry_id.0,
        lease.revision.0,
        registry_revision,
        producer_generation,
        short_hash(lease.content_hash.0)
    )
}

fn semantic_file_name(
    lease: &DocumentLease,
    registry_revision: u64,
    producer_generation: u64,
) -> String {
    format!(
        "{:016x}-r{}-rr{}-g{}-{}-semantic-v3.pgg2",
        lease.entry_id.0,
        lease.revision.0,
        registry_revision,
        producer_generation,
        short_hash(lease.content_hash.0)
    )
}

fn verify_existing_entity(
    generation: &VerifiedGraphGenerationV2,
    structural: &VerifiedGraphGenerationV2,
    lease: &DocumentLease,
    registry_revision: u64,
    producer_generation: u64,
) -> Result<(), KernelError> {
    let header = generation.header();
    if header.native_document_id != lease.entry_id.0
        || header.document_revision != lease.revision.0
        || header.content_hash != lease.content_hash.0
        || header.registry_revision != registry_revision
        || header.producer_generation != producer_generation
        || header.published_generation
            != structural
                .header()
                .published_generation
                .checked_add(1)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    Ok(())
}

fn verify_existing_semantic(
    generation: &VerifiedGraphGenerationV2,
    entity_generation: &VerifiedGraphGenerationV2,
) -> Result<(), KernelError> {
    let header = generation.header();
    if header.native_document_id != entity_generation.header().native_document_id
        || header.document_revision != entity_generation.header().document_revision
        || header.content_hash != entity_generation.header().content_hash
        || header.registry_revision != entity_generation.header().registry_revision
        || header.producer_generation != entity_generation.header().producer_generation
        || header.published_generation
            != entity_generation
                .header()
                .published_generation
                .checked_add(1)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    verify_semantic_contract(generation)
}

fn semantic_authority_hash() -> [u8; 32] {
    *blake3::hash(SEMANTIC_AUTHORITY_CONTRACT.as_bytes()).as_bytes()
}

fn verify_semantic_contract(generation: &VerifiedGraphGenerationV2) -> Result<(), KernelError> {
    let capabilities: &[CapabilityRecord] = generation.typed_page(PageKind::Capabilities)?;
    let models: &[ModelIdentityRecord] = generation.typed_page(PageKind::ModelIdentities)?;
    let expected_hash = semantic_authority_hash();
    for (product, expected_producer) in [
        (
            ProducerProduct::Relationships,
            phoenix_story_producer::RELATIONSHIP_PRODUCER,
        ),
        (
            ProducerProduct::Events,
            phoenix_story_producer::EVENT_PRODUCER,
        ),
        (
            ProducerProduct::Episodes,
            phoenix_story_producer::EPISODE_PRODUCER,
        ),
        (
            ProducerProduct::Temporal,
            phoenix_story_producer::TEMPORAL_PRODUCER,
        ),
        (
            ProducerProduct::Causal,
            phoenix_story_producer::CAUSAL_PRODUCER,
        ),
        (
            ProducerProduct::MemoryState,
            phoenix_story_producer::MEMORY_PRODUCER,
        ),
        (
            ProducerProduct::ContextualEvidence,
            "phoenix-contextual-evidence/v1",
        ),
    ] {
        let row = capabilities
            .iter()
            .rev()
            .find(|row| row.product == product as u16)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let model = models
            .get(row.model_identity_index as usize)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        if row.state != CapabilityState::Produced as u16
            || generation.resolve_string(row.producer)? != expected_producer
            || model.artifact_hash != expected_hash
        {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
    }
    Ok(())
}

fn verify_reopen_contract(generation: &VerifiedGraphGenerationV2) -> Result<(), KernelError> {
    #[cfg(test)]
    if generation.descriptor(PageKind::Capabilities).count == 0 {
        return Ok(());
    }
    verify_semantic_contract(generation)
}

fn contextual_evidence(
    coordinator: &PhoenixProducerCoordinatorV1,
    generation: &VerifiedGraphGenerationV2,
) -> Result<Vec<ContextualEvidenceRecord>, KernelError> {
    let mentions: &[MentionRecord] = generation.typed_page(PageKind::Mentions)?;
    let chunks: &[phoenix_graph_generation_v2::ChunkRecord] =
        generation.typed_page(PageKind::Chunks)?;
    let mention_index = mentions
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    let mut output = Vec::with_capacity(coordinator.contextual_evidence_bindings.len());
    for binding in &coordinator.contextual_evidence_bindings {
        let source = mention_index
            .get(&binding.source_mention_id)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let target = mention_index
            .get(&binding.target_mention_id)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let chunk = chunks
            .get(binding.chunk_index as usize)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        if source.entity_id != binding.source_entity_id
            || target.entity_id != binding.target_entity_id
            || source.chunk_id != chunk.id
            || target.chunk_id != chunk.id
        {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
        let distance = if source.end <= target.start {
            target.start - source.end
        } else {
            source.start.saturating_sub(target.end)
        };
        let weight = 1.0_f32 / (1.0 + distance as f32 / 64.0);
        output.push(ContextualEvidenceRecord {
            source_entity_id: source.entity_id,
            target_entity_id: target.entity_id,
            source_mention_id: source.id,
            target_mention_id: target.id,
            chunk_id: chunk.id,
            weight_bits: weight.to_bits(),
            byte_distance: distance,
            flags: 0,
            reserved: 0,
        });
    }
    output.sort_unstable_by_key(|row| (row.chunk_id, row.source_mention_id, row.target_mention_id));
    output.dedup_by_key(|row| (row.chunk_id, row.source_mention_id, row.target_mention_id));
    Ok(output)
}

fn identity_candidates(
    analysis: &PhoenixDocumentAnalysisV1,
    coordinator: &PhoenixProducerCoordinatorV1,
) -> Result<Vec<IdentityCandidateInput>, KernelError> {
    let mentions = analysis
        .ner
        .mentions
        .iter()
        .map(|mention| (mention.mention_id, mention))
        .collect::<BTreeMap<_, _>>();
    let evidence = coordinator
        .evidence_bindings
        .iter()
        .map(|binding| (binding.candidate_id, binding))
        .collect::<BTreeMap<_, _>>();
    let adjudications = analysis
        .nli
        .nli_adjudications
        .iter()
        .map(|row| (row.candidate_id, row))
        .collect::<BTreeMap<_, _>>();
    let mut output = Vec::new();
    for candidate in &analysis.nli.nli_candidates {
        let kind = match candidate.kind {
            NliCandidateKind::SameSurface => IdentityCandidateKind::SameSurface,
            NliCandidateKind::Alias => IdentityCandidateKind::Alias,
            NliCandidateKind::Coreference => IdentityCandidateKind::Coreference,
            NliCandidateKind::Related => continue,
        };
        if candidate.left_entity_id == candidate.right_entity_id {
            continue;
        }
        let binding = evidence
            .get(&candidate.candidate_id)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let left = mentions
            .get(&binding.left_mention_id)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let right = mentions
            .get(&binding.right_mention_id)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        if left.entity_id != candidate.left_entity_id
            || right.entity_id != candidate.right_entity_id
        {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
        let confidence = adjudications
            .get(&candidate.candidate_id)
            .map(|row| row.confidence_millis as f32 / 1_000.0)
            .unwrap_or_default();
        output.push(IdentityCandidateInput {
            candidate_id: CandidateId(candidate.candidate_id),
            left_entity_id: EntityId(candidate.left_entity_id),
            right_entity_id: EntityId(candidate.right_entity_id),
            left_mention_id: MentionId(binding.left_mention_id),
            right_mention_id: MentionId(binding.right_mention_id),
            kind,
            confidence,
        });
    }
    output.sort_unstable_by_key(|candidate| candidate.candidate_id);
    Ok(output)
}

fn short_hash(hash: [u8; 32]) -> String {
    hash[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(super) fn produce_test_fixture(
    workspace_path: &Path,
    lease: &DocumentLease,
    registry: &EntityRegistry,
) -> Result<ProductionSceneAuthorityV2, KernelError> {
    let root = authority_root(workspace_path)?;
    let path = root.join("generations").join(format!(
        "test-{:016x}-r{}-rr{}-{}.pgg2",
        lease.entry_id.0,
        lease.revision.0,
        registry.revision(),
        short_hash(lease.content_hash.0)
    ));
    let generation = if path.is_file() {
        VerifiedGraphGenerationV2::open(&path)?
    } else {
        write_fixture_generation(&path, lease, registry)?
    };
    let catalog = ReviewCatalog::new(
        phoenix_semantic_review::ReviewAuthority {
            source_generation_hash: generation.header().generation_hash,
            document_hash: generation.header().content_hash,
            native_document_id: generation.header().native_document_id,
            document_revision: generation.header().document_revision,
            registry_revision: generation.header().registry_revision,
            producer_generation: generation.header().producer_generation,
        },
        Vec::new(),
    )?;
    Ok(ProductionSceneAuthorityV2 {
        generation: Arc::new(generation),
        catalog: Arc::new(catalog),
    })
}

#[cfg(test)]
fn write_fixture_generation(
    path: &Path,
    lease: &DocumentLease,
    registry: &EntityRegistry,
) -> Result<VerifiedGraphGenerationV2, KernelError> {
    let source_len =
        u32::try_from(lease.content.len()).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let mut strings = Vec::new();
    let source = push_string(&mut strings, &format!("native:{:016x}", lease.entry_id.0))?;
    let chapter_title = push_string(&mut strings, "Document")?;
    let document_id = fixture_id(b"document", lease, 0);
    let chapter_id = fixture_id(b"chapter", lease, 0);
    let paragraph_id = fixture_id(b"paragraph", lease, 0);
    let sentence_id = fixture_id(b"sentence", lease, 0);
    let chunk_id = fixture_id(b"chunk", lease, 0);
    let mentions = registry.active_mentions_for(lease).collect::<Vec<_>>();
    let entities = registry
        .entities()
        .iter()
        .map(|entity| {
            Ok(EntityRecord {
                id: entity.id,
                label: push_string(&mut strings, &entity.label)?,
                custom_kind: match entity.custom_kind.as_deref() {
                    Some(kind) => push_string(&mut strings, kind)?,
                    None => StringRef::default(),
                },
                mention_count: u32::try_from(
                    mentions
                        .iter()
                        .filter(|(mention, _)| mention.entity_id == entity.id)
                        .count(),
                )
                .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
                kind: entity.kind as u16,
                source_mask: u16::from(entity.sources.ner)
                    | (u16::from(entity.sources.user_tagged) << 1),
                flags: 0,
                reserved: 0,
            })
        })
        .collect::<Result<Vec<_>, KernelError>>()?;
    let mut mention_rows = Vec::with_capacity(mentions.len());
    let mut evidence_rows = Vec::with_capacity(mentions.len());
    for (ordinal, (mention, _)) in mentions.iter().enumerate() {
        let mention_id = fixture_id(b"mention", lease, ordinal as u64);
        let evidence_id = fixture_id(b"evidence", lease, ordinal as u64);
        mention_rows.push(MentionRecord {
            id: mention_id,
            entity_id: mention.entity_id,
            evidence_id,
            chunk_id,
            start: mention.start,
            end: mention.end,
            sentence_index: 0,
            confidence_bits: 1.0_f32.to_bits(),
            flags: 0,
            reserved: 0,
        });
        evidence_rows.push(EvidenceRecord {
            id: evidence_id,
            entity_id: mention.entity_id,
            mention_id,
            chunk_id,
            start: mention.start,
            end: mention.end,
            role: 1,
            flags: 0,
            reserved: 0,
        });
    }
    let document = DocumentRecord {
        id: document_id,
        source_id: source,
        source_len,
        chapter_count: 1,
        paragraph_count: 1,
        sentence_count: 1,
        chunk_count: 1,
        span_count: 0,
        entity_count: entities.len() as u32,
        mention_count: mention_rows.len() as u32,
        evidence_count: evidence_rows.len() as u32,
        structural_edge_count: 4,
        flags: 0,
        reserved: [0; 3],
    };
    let chapters = [ChapterRecord {
        id: chapter_id,
        document_id,
        title: chapter_title,
        start: 0,
        end: source_len,
        paragraph_start: 0,
        paragraph_end: 1,
        ordinal: 0,
        flags: 0,
        reserved: [0; 2],
    }];
    let paragraphs = [ParagraphRecord {
        id: paragraph_id,
        document_id,
        chapter_id,
        start: 0,
        end: source_len,
        sentence_start: 0,
        sentence_end: 1,
        ordinal: 0,
        flags: 0,
        reserved: [0; 2],
    }];
    let sentences = [SentenceRecord {
        id: sentence_id,
        document_id,
        paragraph_id,
        content_hash: fixture_id(b"sentence-content", lease, 0),
        start: 0,
        end: source_len,
        ordinal: 0,
        token_count: lease.content.split_whitespace().count() as u32,
        quality: 1,
        dialogue_hint: 0,
        flags: 0,
        reserved_u16: 0,
        reserved: 0,
    }];
    let chunks = [ChunkRecord {
        id: chunk_id,
        document_id,
        content_hash: fixture_id(b"chunk-content", lease, 0),
        start: 0,
        end: source_len,
        sentence_start: 0,
        sentence_end: 1,
        paragraph_start: 0,
        paragraph_end: 1,
        chapter_index: 0,
        token_count: lease.content.split_whitespace().count() as u32,
        flags: 0,
        reserved: 0,
    }];
    let structural_edges = [
        fixture_edge(lease, 0, document_id, chapter_id),
        fixture_edge(lease, 1, chapter_id, paragraph_id),
        fixture_edge(lease, 2, paragraph_id, sentence_id),
        fixture_edge(lease, 3, sentence_id, chunk_id),
    ];
    Ok(write_generation_new(
        path,
        GenerationWriteAuthority {
            source_document_id_hash: *blake3::hash(
                format!("native:{:016x}", lease.entry_id.0).as_bytes(),
            )
            .as_bytes(),
            content_hash: lease.content_hash.0,
            cohort_hash: *blake3::hash(b"phoenix-app-core-v2-test-fixture").as_bytes(),
            native_document_id: lease.entry_id.0,
            document_revision: lease.revision.0,
            registry_revision: registry.revision(),
            producer_generation: 1,
            published_generation: 1,
        },
        GenerationPages {
            strings: &strings,
            documents: &[document],
            chapters: &chapters,
            paragraphs: &paragraphs,
            sentences: &sentences,
            chunks: &chunks,
            entities: &entities,
            mentions: &mention_rows,
            evidence: &evidence_rows,
            structural_edges: &structural_edges,
            ..GenerationPages::default()
        },
    )?)
}

#[cfg(test)]
fn push_string(strings: &mut Vec<u8>, value: &str) -> Result<StringRef, KernelError> {
    let offset =
        u64::try_from(strings.len()).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let length = u32::try_from(value.len()).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    strings.extend_from_slice(value.as_bytes());
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}

#[cfg(test)]
fn fixture_id(domain: &[u8], lease: &DocumentLease, ordinal: u64) -> u64 {
    let mut hash = blake3::Hasher::new();
    hash.update(b"phoenix-app-core-v2-test-fixture-id");
    hash.update(domain);
    hash.update(&lease.content_hash.0);
    hash.update(&ordinal.to_le_bytes());
    u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap_or([1; 8])).max(1)
}

#[cfg(test)]
fn fixture_edge(
    lease: &DocumentLease,
    ordinal: u64,
    source_id: u64,
    target_id: u64,
) -> StructuralEdgeRecord {
    StructuralEdgeRecord {
        id: fixture_id(b"edge", lease, ordinal),
        source_id,
        target_id,
        evidence_id: 0,
        weight_bits: 1.0_f32.to_bits(),
        relation: ordinal as u16 + 1,
        flags: 1,
    }
}
