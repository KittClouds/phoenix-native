use crate::{
    DecisionLedger, ReviewCandidateLocation, ReviewCatalog, ReviewPage, SemanticReviewError,
    VerifiedDecisionReceipt,
};
use phoenix_graph_generation_v2::{
    expected_authority, write_generation_new, AuthorityClass, CandidateStatus,
    CausalCandidateRecord, DecisionRecord, EpisodeMembershipRecord, EpisodeRecord, EventRecord,
    GenerationPages, GenerationWriteAuthority, IdentityCandidateRecord, MemoryStateCandidateRecord,
    PageKind, PublicationReceiptRecord, PublicationStatus, StringRef, TemporalCandidateRecord,
    TypedRelationshipCandidateRecord, VerifiedGraphGenerationV2,
};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewPublicationReceipt {
    pub previous_generation_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub accepted_count: u32,
    pub rejected_count: u32,
    pub deferred_count: u32,
    pub superseded_decision_count: u32,
    pub decision_count: u32,
}

pub fn publish_reviewed_generation_new(
    path: impl AsRef<Path>,
    source: &VerifiedGraphGenerationV2,
    catalog: &ReviewCatalog,
    ledger: &DecisionLedger,
    published_at_unix_millis: u64,
) -> Result<(VerifiedGraphGenerationV2, ReviewPublicationReceipt), SemanticReviewError> {
    validate_catalog_authority(source, catalog)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| SemanticReviewError::io(parent, source))?;
    }

    let strings = source.page_bytes(PageKind::Strings);
    let documents = copy(source, PageKind::Documents)?;
    let chapters = copy(source, PageKind::Chapters)?;
    let paragraphs = copy(source, PageKind::Paragraphs)?;
    let sentences = copy(source, PageKind::Sentences)?;
    let chunks = copy(source, PageKind::Chunks)?;
    let spans = copy(source, PageKind::Spans)?;
    let entities = copy(source, PageKind::Entities)?;
    let mentions = copy(source, PageKind::Mentions)?;
    let evidence = copy(source, PageKind::Evidence)?;
    let structural_edges = copy(source, PageKind::StructuralEdges)?;
    let mut relationships = copy(source, PageKind::TypedRelationshipCandidates)?;
    let mut identities = copy(source, PageKind::IdentityCandidates)?;
    let mut events = copy(source, PageKind::Events)?;
    let mut episodes = copy(source, PageKind::Episodes)?;
    let mut memberships = copy(source, PageKind::EpisodeMemberships)?;
    let mut temporal = copy(source, PageKind::TemporalCandidates)?;
    let mut causal = copy(source, PageKind::CausalCandidates)?;
    let mut memory = copy(source, PageKind::MemoryStateCandidates)?;
    let contextual = copy(source, PageKind::ContextualEvidence)?;
    let nli = copy(source, PageKind::NliAdjudications)?;
    let mut decisions: Vec<DecisionRecord> = copy(source, PageKind::Decisions)?;
    let capabilities = copy(source, PageKind::Capabilities)?;
    let models = copy(source, PageKind::ModelIdentities)?;
    let stages = copy(source, PageKind::StageReceipts)?;
    let mut publications: Vec<PublicationReceiptRecord> =
        copy(source, PageKind::PublicationReceipts)?;
    let candidate_bindings = copy(source, PageKind::CandidateEvidenceBindings)?;
    let canonical_bindings = copy(source, PageKind::CanonicalEntityBindings)?;

    reset_reviewable_statuses(
        &mut relationships,
        &mut identities,
        &mut events,
        &mut episodes,
        &mut memberships,
        &mut temporal,
        &mut causal,
        &mut memory,
    );
    let mut accepted = 0_u32;
    let mut rejected = 0_u32;
    let mut deferred = 0_u32;
    for candidate in catalog.candidates() {
        let Some(receipt) = ledger.head(candidate.binding.origin.candidate_id) else {
            continue;
        };
        if !receipt_matches(receipt, catalog, candidate.binding.origin.candidate_id) {
            continue;
        }
        let status = CandidateStatus::from_raw(receipt.header().status)
            .ok_or(SemanticReviewError::InvalidDecision)?;
        set_status(
            candidate.location,
            status,
            &mut relationships,
            &mut identities,
            &mut events,
            &mut episodes,
            &mut memberships,
            &mut temporal,
            &mut causal,
            &mut memory,
        )?;
        match status {
            CandidateStatus::Accepted => accepted += 1,
            CandidateStatus::Rejected => rejected += 1,
            CandidateStatus::Deferred => deferred += 1,
            CandidateStatus::Proposed | CandidateStatus::Superseded => {}
        }
    }

    let mut superseded = 0_u32;
    for receipt in ledger.head_receipts() {
        let exact = receipt_matches(receipt, catalog, receipt.header().candidate_id);
        let status = if exact {
            receipt.header().status
        } else {
            superseded += 1;
            CandidateStatus::Superseded as u16
        };
        upsert_decision(&mut decisions, receipt, status);
    }

    let next_generation = source
        .header()
        .published_generation
        .checked_add(1)
        .ok_or(SemanticReviewError::RecordCountOverflow)?;
    let authority_hash = review_authority_hash(
        &relationships,
        &identities,
        &events,
        &episodes,
        &memberships,
        &temporal,
        &causal,
        &memory,
        &decisions,
    );
    publications.push(PublicationReceiptRecord {
        authority_hash,
        previous_generation_hash: source.header().generation_hash,
        generation_id: next_generation,
        previous_generation_id: source.header().published_generation,
        document_revision: source.header().document_revision,
        registry_revision: source.header().registry_revision,
        published_at_unix_millis,
        status: PublicationStatus::Published as u16,
        flags_u16: 0,
        flags: 0,
    });

    let output = write_generation_new(
        path,
        GenerationWriteAuthority {
            source_document_id_hash: source.header().source_document_id_hash,
            content_hash: source.header().content_hash,
            cohort_hash: source.header().cohort_hash,
            native_document_id: source.header().native_document_id,
            document_revision: source.header().document_revision,
            registry_revision: source.header().registry_revision,
            producer_generation: source.header().producer_generation,
            published_generation: next_generation,
        },
        GenerationPages {
            strings,
            documents: &documents,
            chapters: &chapters,
            paragraphs: &paragraphs,
            sentences: &sentences,
            chunks: &chunks,
            spans: &spans,
            entities: &entities,
            mentions: &mentions,
            evidence: &evidence,
            structural_edges: &structural_edges,
            typed_relationship_candidates: &relationships,
            identity_candidates: &identities,
            events: &events,
            episodes: &episodes,
            episode_memberships: &memberships,
            temporal_candidates: &temporal,
            causal_candidates: &causal,
            memory_state_candidates: &memory,
            contextual_evidence: &contextual,
            nli_adjudications: &nli,
            decisions: &decisions,
            capabilities: &capabilities,
            model_identities: &models,
            stage_receipts: &stages,
            publication_receipts: &publications,
            candidate_evidence_bindings: &candidate_bindings,
            canonical_entity_bindings: &canonical_bindings,
        },
    )?;
    verify_source_pages(source, &output)?;
    verify_accepted_receipts(&output, catalog)?;
    let receipt = ReviewPublicationReceipt {
        previous_generation_hash: source.header().generation_hash,
        generation_hash: output.header().generation_hash,
        accepted_count: accepted,
        rejected_count: rejected,
        deferred_count: deferred,
        superseded_decision_count: superseded,
        decision_count: u32::try_from(decisions.len())
            .map_err(|_| SemanticReviewError::RecordCountOverflow)?,
    };
    Ok((output, receipt))
}

#[allow(clippy::too_many_arguments)]
fn reset_reviewable_statuses(
    relationships: &mut [TypedRelationshipCandidateRecord],
    identities: &mut [IdentityCandidateRecord],
    events: &mut [EventRecord],
    episodes: &mut [EpisodeRecord],
    memberships: &mut [EpisodeMembershipRecord],
    temporal: &mut [TemporalCandidateRecord],
    causal: &mut [CausalCandidateRecord],
    memory: &mut [MemoryStateCandidateRecord],
) {
    let proposed = CandidateStatus::Proposed as u16;
    relationships
        .iter_mut()
        .for_each(|row| row.status = proposed);
    identities.iter_mut().for_each(|row| row.status = proposed);
    events.iter_mut().for_each(|row| row.status = proposed);
    episodes.iter_mut().for_each(|row| row.status = proposed);
    memberships.iter_mut().for_each(|row| row.status = proposed);
    temporal.iter_mut().for_each(|row| row.status = proposed);
    causal.iter_mut().for_each(|row| row.status = proposed);
    memory.iter_mut().for_each(|row| row.status = proposed);
}

#[allow(clippy::too_many_arguments)]
fn set_status(
    location: ReviewCandidateLocation,
    status: CandidateStatus,
    relationships: &mut [TypedRelationshipCandidateRecord],
    identities: &mut [IdentityCandidateRecord],
    events: &mut [EventRecord],
    episodes: &mut [EpisodeRecord],
    memberships: &mut [EpisodeMembershipRecord],
    temporal: &mut [TemporalCandidateRecord],
    causal: &mut [CausalCandidateRecord],
    memory: &mut [MemoryStateCandidateRecord],
) -> Result<(), SemanticReviewError> {
    let index = location.row_index as usize;
    let status = status as u16;
    match ReviewPage::from_raw(location.page) {
        Some(ReviewPage::TypedRelationship) => {
            relationships.get_mut(index).map(|r| r.status = status)
        }
        Some(ReviewPage::Identity) => identities.get_mut(index).map(|r| r.status = status),
        Some(ReviewPage::Event) => events.get_mut(index).map(|r| r.status = status),
        Some(ReviewPage::Episode) => episodes.get_mut(index).map(|r| r.status = status),
        Some(ReviewPage::EpisodeMembership) => {
            memberships.get_mut(index).map(|r| r.status = status)
        }
        Some(ReviewPage::Temporal) => temporal.get_mut(index).map(|r| r.status = status),
        Some(ReviewPage::Causal) => causal.get_mut(index).map(|r| r.status = status),
        Some(ReviewPage::MemoryState) => memory.get_mut(index).map(|r| r.status = status),
        None => None,
    }
    .ok_or(SemanticReviewError::InvalidCandidateLocation)
}

fn receipt_matches(
    receipt: &VerifiedDecisionReceipt,
    catalog: &ReviewCatalog,
    candidate_id: phoenix_graph_generation_v2::CandidateId,
) -> bool {
    let header = receipt.header();
    catalog.get(candidate_id).is_some_and(|candidate| {
        let authority = catalog.authority();
        header.document_hash == authority.document_hash
            && header.document_revision == authority.document_revision
            && header.registry_revision == authority.registry_revision
            && header.producer_generation == authority.producer_generation
            && header.candidate_hash == candidate.binding.candidate_hash
            && header.evidence_hash == candidate.binding.evidence_hash
            && header.lens_id == candidate.binding.origin.lens_id
            && header.vocabulary_hash == candidate.binding.origin.vocabulary_hash
    })
}

fn upsert_decision(
    decisions: &mut Vec<DecisionRecord>,
    receipt: &VerifiedDecisionReceipt,
    status: u16,
) {
    let header = receipt.header();
    let id = u64::from_le_bytes(header.receipt_id[..8].try_into().unwrap_or([0; 8]));
    let record = DecisionRecord {
        id,
        candidate_id: header.candidate_id,
        reason: StringRef::default(),
        evidence_hash: header.evidence_hash,
        decided_at_revision: header.document_revision,
        registry_revision: header.registry_revision,
        producer_generation: header.producer_generation,
        action: header.action,
        status,
        flags: 0,
    };
    if let Some(existing) = decisions.iter_mut().find(|existing| existing.id == id) {
        *existing = record;
    } else {
        decisions.push(record);
    }
}

fn validate_catalog_authority(
    source: &VerifiedGraphGenerationV2,
    catalog: &ReviewCatalog,
) -> Result<(), SemanticReviewError> {
    let authority = catalog.authority();
    if authority.source_generation_hash != source.header().generation_hash
        || authority.document_hash != source.header().content_hash
        || authority.native_document_id != source.header().native_document_id
        || authority.document_revision != source.header().document_revision
        || authority.registry_revision != source.header().registry_revision
        || authority.producer_generation != source.header().producer_generation
    {
        return Err(SemanticReviewError::GenerationAuthorityMismatch);
    }
    Ok(())
}

fn verify_source_pages(
    source: &VerifiedGraphGenerationV2,
    output: &VerifiedGraphGenerationV2,
) -> Result<(), SemanticReviewError> {
    for kind in PageKind::ALL {
        if expected_authority(kind) == AuthorityClass::SourceAuthoritative
            && source.descriptor(kind).hash != output.descriptor(kind).hash
        {
            return Err(SemanticReviewError::GenerationAuthorityMismatch);
        }
    }
    Ok(())
}

fn verify_accepted_receipts(
    output: &VerifiedGraphGenerationV2,
    catalog: &ReviewCatalog,
) -> Result<(), SemanticReviewError> {
    let decisions: &[DecisionRecord] = output.typed_page(PageKind::Decisions)?;
    for candidate in catalog.candidates() {
        if candidate_status(output, candidate.location)? == CandidateStatus::Accepted as u16
            && !decisions.iter().any(|decision| {
                decision.candidate_id == candidate.binding.origin.candidate_id
                    && decision.status == CandidateStatus::Accepted as u16
                    && decision.evidence_hash == candidate.binding.evidence_hash
            })
        {
            return Err(SemanticReviewError::GenerationAuthorityMismatch);
        }
    }
    Ok(())
}

fn candidate_status(
    source: &VerifiedGraphGenerationV2,
    location: ReviewCandidateLocation,
) -> Result<u16, SemanticReviewError> {
    let index = location.row_index as usize;
    match ReviewPage::from_raw(location.page) {
        Some(ReviewPage::TypedRelationship) => source
            .typed_page::<TypedRelationshipCandidateRecord>(PageKind::TypedRelationshipCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Identity) => source
            .typed_page::<IdentityCandidateRecord>(PageKind::IdentityCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Event) => source
            .typed_page::<EventRecord>(PageKind::Events)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Episode) => source
            .typed_page::<EpisodeRecord>(PageKind::Episodes)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::EpisodeMembership) => source
            .typed_page::<EpisodeMembershipRecord>(PageKind::EpisodeMemberships)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Temporal) => source
            .typed_page::<TemporalCandidateRecord>(PageKind::TemporalCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Causal) => source
            .typed_page::<CausalCandidateRecord>(PageKind::CausalCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::MemoryState) => source
            .typed_page::<MemoryStateCandidateRecord>(PageKind::MemoryStateCandidates)?
            .get(index)
            .map(|row| row.status),
        None => None,
    }
    .ok_or(SemanticReviewError::InvalidCandidateLocation)
}

#[allow(clippy::too_many_arguments)]
fn review_authority_hash(
    relationships: &[TypedRelationshipCandidateRecord],
    identities: &[IdentityCandidateRecord],
    events: &[EventRecord],
    episodes: &[EpisodeRecord],
    memberships: &[EpisodeMembershipRecord],
    temporal: &[TemporalCandidateRecord],
    causal: &[CausalCandidateRecord],
    memory: &[MemoryStateCandidateRecord],
    decisions: &[DecisionRecord],
) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"phoenix-reviewed-generation/v1");
    for bytes in [
        bytemuck::cast_slice(relationships),
        bytemuck::cast_slice(identities),
        bytemuck::cast_slice(events),
        bytemuck::cast_slice(episodes),
        bytemuck::cast_slice(memberships),
        bytemuck::cast_slice(temporal),
        bytemuck::cast_slice(causal),
        bytemuck::cast_slice(memory),
        bytemuck::cast_slice(decisions),
    ] {
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    *hash.finalize().as_bytes()
}

fn copy<T: bytemuck::Pod + Copy>(
    source: &VerifiedGraphGenerationV2,
    kind: PageKind,
) -> Result<Vec<T>, SemanticReviewError> {
    Ok(source.typed_page::<T>(kind)?.to_vec())
}
