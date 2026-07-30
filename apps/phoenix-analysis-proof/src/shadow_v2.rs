use anyhow::{bail, Context, Result};
use phoenix_analysis_contract::{
    open_analysis_artifact, open_producer_coordinator, open_structural_artifact,
    CandidateEvidenceBinding, NliCandidateKind, PhoenixDocumentAnalysisV1,
    PhoenixProducerCoordinatorV1, PhoenixStructuralSubstrateV1,
};
use phoenix_document_producer::{
    publish_or_reuse_structural_generation, StructuralProducerInput, StructuralReuseState,
    VerifiedStructuralGeneration,
};
use phoenix_entity_producer::{
    publish_entity_generation_new, EntityProducerInput, IdentityCandidateInput,
    IdentityCandidateKind,
};
use phoenix_graph_generation_v2::{
    CandidateId, CanonicalBindingKind, CanonicalEntityBindingRecord, ChunkRecord, EntityId,
    EntityRecord, EvidenceRecord, IdentityCandidateRecord, MentionId, MentionRecord, PageKind,
    VerifiedGraphGenerationV2,
};
use phoenix_workspace::{open_document, EntryId, WorkspaceDocument};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

struct ShadowAuthority {
    text: std::sync::Arc<str>,
    analysis: std::sync::Arc<PhoenixDocumentAnalysisV1>,
    structural: std::sync::Arc<PhoenixStructuralSubstrateV1>,
    coordinator: std::sync::Arc<PhoenixProducerCoordinatorV1>,
}

pub(crate) fn produce(
    workspace_path: &Path,
    entry_id: u64,
    analysis_path: &Path,
    structural_path: &Path,
    coordinator_path: &Path,
    output_root: &Path,
    run_label: &str,
) -> Result<()> {
    validate_run_label(run_label)?;
    let authority = open_authority(
        workspace_path,
        entry_id,
        analysis_path,
        structural_path,
        coordinator_path,
    )?;
    let structural = publish_or_reuse_structural_generation(
        output_root.join("structural"),
        StructuralProducerInput {
            text: &authority.text,
            structural: &authority.structural,
        },
    )
    .context("publish the structural V2 shadow generation")?;
    preflight_bindings(&authority)?;
    let identity_candidates = identity_candidates(&authority.analysis, &authority.coordinator)?;
    let entity_path = output_root
        .join("entities")
        .join(format!("{run_label}.pgg2"));
    if entity_path.exists() {
        bail!(
            "shadow entity output already exists; refusing overwrite: {}",
            entity_path.display()
        );
    }
    let published_generation = structural
        .generation()
        .header()
        .published_generation
        .checked_add(1)
        .context("published generation exhausted")?;
    let entity = publish_entity_generation_new(
        &entity_path,
        EntityProducerInput {
            text: &authority.text,
            structural: structural.generation(),
            ner: &authority.analysis.ner,
            user_entities: &[],
            user_mentions: &[],
            merge_decisions: &[],
            identity_candidates: &identity_candidates,
            published_generation,
        },
    )
    .context("publish the entity/evidence V2 shadow generation")?;

    print_produce_receipt(&authority, &structural, &entity_path, entity.receipt());
    Ok(())
}

pub(crate) fn verify(
    workspace_path: &Path,
    entry_id: u64,
    analysis_path: &Path,
    structural_path: &Path,
    coordinator_path: &Path,
    entity_path: &Path,
) -> Result<()> {
    let authority = open_authority(
        workspace_path,
        entry_id,
        analysis_path,
        structural_path,
        coordinator_path,
    )?;
    let generation =
        VerifiedGraphGenerationV2::open(entity_path).context("open shadow V2 generation")?;
    verify_generation(&authority, &generation)?;
    print_verify_receipt(&authority, &generation, entity_path)?;
    Ok(())
}

fn open_authority(
    workspace_path: &Path,
    entry_id: u64,
    analysis_path: &Path,
    structural_path: &Path,
    coordinator_path: &Path,
) -> Result<ShadowAuthority> {
    let workspace = WorkspaceDocument::load(workspace_path)
        .with_context(|| format!("open workspace {}", workspace_path.display()))?;
    let lease = open_document(workspace_path, &workspace, EntryId(entry_id))
        .with_context(|| format!("open document {entry_id}"))?;
    let analysis = open_analysis_artifact(analysis_path)
        .with_context(|| format!("open analysis {}", analysis_path.display()))?;
    let structural = open_structural_artifact(structural_path)
        .with_context(|| format!("open structural {}", structural_path.display()))?;
    let coordinator = open_producer_coordinator(coordinator_path)
        .with_context(|| format!("open coordinator {}", coordinator_path.display()))?;
    let analysis = analysis.analysis().clone();
    let structural = structural.structural().clone();
    let coordinator = coordinator.coordinator().clone();

    coordinator
        .validate_final(&analysis, &structural)
        .map_err(anyhow::Error::msg)
        .context("coordinator is not final and evidence-bound")?;
    let binding = &analysis.ner.binding;
    if structural.binding != *binding
        || coordinator.binding != *binding
        || binding.native_document_id != entry_id
        || binding.document_revision != lease.revision.0
        || binding.content_hash != lease.content_hash.0
        || structural.source_len as usize != lease.content.len()
        || *blake3::hash(lease.content.as_bytes()).as_bytes() != binding.content_hash
    {
        bail!("workspace, analysis, structural, and coordinator authorities do not match");
    }
    Ok(ShadowAuthority {
        text: lease.content,
        analysis,
        structural,
        coordinator,
    })
}

fn identity_candidates(
    analysis: &PhoenixDocumentAnalysisV1,
    coordinator: &PhoenixProducerCoordinatorV1,
) -> Result<Vec<IdentityCandidateInput>> {
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
        .map(|adjudication| (adjudication.candidate_id, adjudication))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut candidates = Vec::new();
    for candidate in &analysis.nli.nli_candidates {
        let kind = match candidate.kind {
            NliCandidateKind::SameSurface => IdentityCandidateKind::SameSurface,
            NliCandidateKind::Alias => IdentityCandidateKind::Alias,
            NliCandidateKind::Coreference => IdentityCandidateKind::Coreference,
            NliCandidateKind::Related => continue,
        };
        let binding = evidence
            .get(&candidate.candidate_id)
            .copied()
            .context("identity candidate has no coordinator evidence binding")?;
        let adjudication = adjudications
            .get(&candidate.candidate_id)
            .copied()
            .context("identity candidate has no NLI adjudication")?;
        let left_mention = mentions
            .get(&binding.left_mention_id)
            .copied()
            .context("identity candidate left mention is absent")?;
        let right_mention = mentions
            .get(&binding.right_mention_id)
            .copied()
            .context("identity candidate right mention is absent")?;
        if candidate.left_entity_id == candidate.right_entity_id {
            continue;
        }
        if left_mention.entity_id != candidate.left_entity_id
            || right_mention.entity_id != candidate.right_entity_id
        {
            bail!(
                "identity candidate {} evidence mentions do not match its stable entities",
                hex(&candidate.candidate_id)
            );
        }
        let candidate_id = stable_candidate_id(&candidate.candidate_id);
        if !ids.insert(candidate_id) {
            bail!("identity candidate ID collision");
        }
        candidates.push(to_identity_candidate(
            candidate_id,
            candidate,
            binding,
            kind,
            adjudication.confidence_millis,
        ));
    }
    candidates.sort_unstable_by_key(|candidate| candidate.candidate_id);
    Ok(candidates)
}

fn preflight_bindings(authority: &ShadowAuthority) -> Result<()> {
    for mention in &authority.analysis.ner.mentions {
        let chunks = authority
            .structural
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.start <= mention.start && mention.end <= chunk.end)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let sentences = authority
            .structural
            .sentences
            .iter()
            .enumerate()
            .filter(|(_, sentence)| sentence.start <= mention.start && mention.end <= sentence.end)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if chunks.is_empty()
            || sentences.len() != 1
            || sentences[0] != mention.sentence_index as usize
        {
            bail!(
                "mention {} entity {} range {}..{} resolves to chunks {:?}, sentences {:?}, declared sentence {}",
                mention.mention_id,
                mention.entity_id,
                mention.start,
                mention.end,
                chunks,
                sentences,
                mention.sentence_index
            );
        }
    }
    Ok(())
}

fn to_identity_candidate(
    candidate_id: CandidateId,
    candidate: &phoenix_analysis_contract::NliCandidate,
    binding: &CandidateEvidenceBinding,
    kind: IdentityCandidateKind,
    confidence_millis: u32,
) -> IdentityCandidateInput {
    IdentityCandidateInput {
        candidate_id,
        left_entity_id: EntityId(candidate.left_entity_id),
        right_entity_id: EntityId(candidate.right_entity_id),
        left_mention_id: MentionId(binding.left_mention_id),
        right_mention_id: MentionId(binding.right_mention_id),
        kind,
        confidence: confidence_millis as f32 / 1_000.0,
    }
}

fn stable_candidate_id(source: &[u8; 32]) -> CandidateId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.entity-producer/v1/nli-candidate\0");
    hasher.update(source);
    let mut value = *hasher.finalize().as_bytes();
    if value == [0; 32] {
        value[0] = 1;
    }
    CandidateId(value)
}

fn verify_generation(
    authority: &ShadowAuthority,
    generation: &VerifiedGraphGenerationV2,
) -> Result<()> {
    let binding = &authority.analysis.ner.binding;
    let header = generation.header();
    if header.native_document_id != binding.native_document_id
        || header.document_revision != binding.document_revision
        || header.registry_revision != binding.target_registry_revision
        || header.content_hash != binding.content_hash
        || header.producer_generation != binding.analysis_generation
    {
        bail!("fresh-process V2 header authority mismatch");
    }

    let entities: &[EntityRecord] = generation.typed_page(PageKind::Entities)?;
    let mentions: &[MentionRecord] = generation.typed_page(PageKind::Mentions)?;
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence)?;
    let chunks: &[ChunkRecord] = generation.typed_page(PageKind::Chunks)?;
    let candidates: &[IdentityCandidateRecord] =
        generation.typed_page(PageKind::IdentityCandidates)?;
    let canonical: &[CanonicalEntityBindingRecord] =
        generation.typed_page(PageKind::CanonicalEntityBindings)?;
    let expected_candidates = identity_candidates(&authority.analysis, &authority.coordinator)?;

    if entities.len() != authority.analysis.ner.entities.len()
        || mentions.len() != authority.analysis.ner.mentions.len()
        || evidence.len() != mentions.len()
        || canonical.len() != entities.len()
        || candidates.len() != expected_candidates.len()
    {
        bail!("fresh-process V2 resource counts mismatch");
    }
    verify_entities(authority, generation, entities, canonical)?;
    verify_mentions(authority, mentions, evidence, chunks)?;
    verify_identity_candidates(candidates, &expected_candidates)?;
    Ok(())
}

fn verify_entities(
    authority: &ShadowAuthority,
    generation: &VerifiedGraphGenerationV2,
    entities: &[EntityRecord],
    canonical: &[CanonicalEntityBindingRecord],
) -> Result<()> {
    let expected = authority
        .analysis
        .ner
        .entities
        .iter()
        .map(|entity| (entity.stable_id, entity))
        .collect::<BTreeMap<_, _>>();
    for entity in entities {
        let source = expected
            .get(&entity.id)
            .copied()
            .context("V2 entity has no stable-ID source")?;
        if generation.resolve_string(entity.label)? != source.label
            || entity.mention_count != source.mention_count
        {
            bail!("V2 entity payload differs for stable ID {}", entity.id);
        }
    }
    for binding in canonical {
        if binding.source_entity_id != binding.canonical_entity_id
            || binding.decision_id != 0
            || binding.kind != CanonicalBindingKind::Direct as u16
        {
            bail!("shadow run invented an implicit canonical merge");
        }
    }
    Ok(())
}

fn verify_mentions(
    authority: &ShadowAuthority,
    mentions: &[MentionRecord],
    evidence: &[EvidenceRecord],
    chunks: &[ChunkRecord],
) -> Result<()> {
    let expected = authority
        .analysis
        .ner
        .mentions
        .iter()
        .map(|mention| (mention.mention_id, mention))
        .collect::<BTreeMap<_, _>>();
    let evidence_by_mention = evidence
        .iter()
        .map(|row| (row.mention_id, row))
        .collect::<BTreeMap<_, _>>();
    for mention in mentions {
        let source = expected
            .get(&mention.id)
            .copied()
            .context("V2 mention has no analysis source")?;
        let chunk = chunks
            .iter()
            .find(|chunk| chunk.id == mention.chunk_id)
            .context("V2 mention has no exact chunk")?;
        let anchor = evidence_by_mention
            .get(&mention.id)
            .copied()
            .context("V2 mention has no graph evidence")?;
        if mention.entity_id != source.entity_id
            || mention.start != source.start
            || mention.end != source.end
            || mention.sentence_index != source.sentence_index
            || chunk.start > mention.start
            || chunk.end < mention.end
            || anchor.entity_id != mention.entity_id
            || anchor.chunk_id != mention.chunk_id
            || anchor.start != mention.start
            || anchor.end != mention.end
        {
            bail!("mention {} lost its exact source/chunk binding", mention.id);
        }
    }
    Ok(())
}

fn verify_identity_candidates(
    candidates: &[IdentityCandidateRecord],
    expected: &[IdentityCandidateInput],
) -> Result<()> {
    for (actual, source) in candidates.iter().zip(expected) {
        if actual.candidate_id != source.candidate_id
            || actual.left_entity_id != source.left_entity_id.0
            || actual.right_entity_id != source.right_entity_id.0
            || actual.evidence_count != 2
        {
            bail!("identity candidate drifted from coordinator evidence");
        }
    }
    Ok(())
}

fn print_produce_receipt(
    authority: &ShadowAuthority,
    structural: &VerifiedStructuralGeneration,
    entity_path: &Path,
    receipt: &phoenix_entity_producer::EntityPublicationReceipt,
) {
    println!("shadow_v2_phase=produced");
    print_authority(authority);
    println!(
        "structural_reuse={}",
        match structural.receipt().reuse_state {
            StructuralReuseState::Produced => "computed",
            StructuralReuseState::DurableVerified => "durable_verified",
        }
    );
    println!("chunks={}", authority.structural.chunks.len());
    println!("sentences={}", authority.structural.sentences.len());
    println!("entities={}", receipt.entity_count);
    println!("mentions={}", receipt.mention_count);
    println!("evidence={}", receipt.evidence_count);
    println!("identity_candidates={}", receipt.identity_candidate_count);
    println!(
        "identity_already_canonical={}",
        authority
            .analysis
            .nli
            .nli_candidates
            .iter()
            .filter(|candidate| {
                candidate.kind != NliCandidateKind::Related
                    && candidate.left_entity_id == candidate.right_entity_id
            })
            .count()
    );
    println!(
        "generic_related_candidates={}",
        authority
            .analysis
            .nli
            .nli_candidates
            .iter()
            .filter(|candidate| candidate.kind == NliCandidateKind::Related)
            .count()
    );
    println!("paint_spans={}", receipt.paint_span_count);
    println!("generation_hash={}", hex(&receipt.generation_hash));
    println!("graph_evidence_hash={}", hex(&receipt.graph_evidence_hash));
    println!(
        "paint_projection_hash={}",
        hex(&receipt.paint_projection_hash)
    );
    println!("entity_generation={}", entity_path.display());
}

fn print_verify_receipt(
    authority: &ShadowAuthority,
    generation: &VerifiedGraphGenerationV2,
    entity_path: &Path,
) -> Result<()> {
    println!("shadow_v2_phase=fresh_process_verified");
    print_authority(authority);
    println!(
        "generation_hash={}",
        hex(&generation.header().generation_hash)
    );
    println!(
        "entities={}",
        generation.descriptor(PageKind::Entities).count
    );
    println!(
        "mentions={}",
        generation.descriptor(PageKind::Mentions).count
    );
    println!(
        "evidence={}",
        generation.descriptor(PageKind::Evidence).count
    );
    println!(
        "identity_candidates={}",
        generation.descriptor(PageKind::IdentityCandidates).count
    );
    println!(
        "canonical_bindings={}",
        generation
            .descriptor(PageKind::CanonicalEntityBindings)
            .count
    );
    println!("entity_generation={}", entity_path.display());
    println!("authority_match=true");
    Ok(())
}

fn print_authority(authority: &ShadowAuthority) {
    let binding = &authority.analysis.ner.binding;
    println!("source_document_id={}", binding.source_document_id);
    println!("document_id={}", binding.native_document_id);
    println!("document_revision={}", binding.document_revision);
    println!("document_blake3={}", hex(&binding.content_hash));
    println!("source_bytes={}", authority.text.len());
    println!("analysis_generation={}", binding.analysis_generation);
    println!(
        "source_registry_revision={}",
        binding.source_registry_revision
    );
    println!(
        "target_registry_revision={}",
        binding.target_registry_revision
    );
    println!(
        "producer_binary_hash={}",
        hex(&binding.producer_binary_hash)
    );
    for (lane, model) in [
        ("chunker", &binding.chunker),
        ("dynamic_ner", &binding.dynamic_ner),
        ("nli", &binding.nli),
    ] {
        println!("{lane}_model_id={}", model.model_id);
        println!("{lane}_runtime={}", model.runtime_id);
        println!("{lane}_artifact_hash={}", hex(&model.artifact_hash));
        println!("{lane}_config_hash={}", hex(&model.config_hash));
    }
    println!(
        "coordinator_queue_high_water={}/{}",
        authority.coordinator.queue_high_water, authority.coordinator.queue_capacity
    );
}

fn validate_run_label(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("run label must contain only ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}
