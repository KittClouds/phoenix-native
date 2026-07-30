use crate::EntityProducerError;
use bytemuck::bytes_of;
use hashbrown::HashMap;
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, EvidenceRecord, IdentityCandidateRecord, PageKind,
    VerifiedGraphGenerationV2,
};
use phoenix_semantic_lens::{
    validate_definition, CandidateOrigin, CoreSemanticClass, EndpointKind, EndpointMask,
    LensCodeDefinition, LensNeutralReviewBinding, SemanticEndpointRef, SemanticLensDefinition,
};
use phoenix_semantic_review::{
    ReviewAuthority, ReviewCandidate, ReviewCandidateLocation, ReviewCatalog, ReviewPage,
};

const IDENTITY_LENS: SemanticLensDefinition<'static> = SemanticLensDefinition {
    namespace: "phoenix.identity/v1",
    version: 1,
    configuration_hash: [0x49; 32],
    codes: &[
        code(1, "identity.same_surface"),
        code(2, "identity.alias"),
        code(3, "identity.coreference"),
    ],
};

const fn code(local: u16, stable_name: &'static str) -> LensCodeDefinition<'static> {
    LensCodeDefinition {
        code: ((CoreSemanticClass::Identity as u32) << 16) | local as u32,
        stable_name,
        class: CoreSemanticClass::Identity,
        source_endpoints: EndpointMask::ENTITY,
        target_endpoints: EndpointMask::ENTITY,
        flags: 0,
    }
}

pub fn entity_review_catalog(
    generation: &VerifiedGraphGenerationV2,
) -> Result<ReviewCatalog, EntityProducerError> {
    let identity = validate_definition(&IDENTITY_LENS)
        .map_err(|_| EntityProducerError::InvalidIdentityCandidate)?;
    let evidence: &[EvidenceRecord] = generation.typed_page(PageKind::Evidence)?;
    let evidence_by_id = evidence
        .iter()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();
    let bindings: &[CandidateEvidenceBindingRecord] =
        generation.typed_page(PageKind::CandidateEvidenceBindings)?;
    let rows: &[IdentityCandidateRecord] = generation.typed_page(PageKind::IdentityCandidates)?;
    let authority = ReviewAuthority {
        source_generation_hash: generation.header().generation_hash,
        document_hash: generation.header().content_hash,
        native_document_id: generation.header().native_document_id,
        document_revision: generation.header().document_revision,
        registry_revision: generation.header().registry_revision,
        producer_generation: generation.header().producer_generation,
    };
    let mut candidates = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let start = row.evidence_start as usize;
        let end = start
            .checked_add(row.evidence_count as usize)
            .ok_or(EntityProducerError::InvalidIdentityCandidate)?;
        let candidate_bindings = bindings
            .get(start..end)
            .ok_or(EntityProducerError::InvalidIdentityCandidate)?;
        if candidate_bindings.is_empty() {
            return Err(EntityProducerError::InvalidIdentityCandidate);
        }
        let mut evidence_hash = blake3::Hasher::new();
        evidence_hash.update(b"phoenix-review-evidence/v1");
        for binding in candidate_bindings {
            if binding.candidate_id != row.candidate_id {
                return Err(EntityProducerError::InvalidIdentityCandidate);
            }
            evidence_hash.update(bytes_of(binding));
            let record = evidence_by_id
                .get(&binding.evidence_id)
                .ok_or(EntityProducerError::InvalidIdentityCandidate)?;
            evidence_hash.update(bytes_of(*record));
        }
        candidates.push(ReviewCandidate {
            binding: LensNeutralReviewBinding {
                origin: CandidateOrigin::from_identity(
                    identity,
                    ((CoreSemanticClass::Identity as u32) << 16) | u32::from(row.kind),
                    CoreSemanticClass::Identity,
                    row.candidate_id,
                )
                .map_err(|_| EntityProducerError::InvalidIdentityCandidate)?,
                origin_alignment_padding: 0,
                source: SemanticEndpointRef::new(EndpointKind::Entity, row.left_entity_id),
                target: SemanticEndpointRef::new(EndpointKind::Entity, row.right_entity_id),
                document_hash: generation.header().content_hash,
                candidate_hash: *blake3::hash(bytes_of(row)).as_bytes(),
                evidence_hash: *evidence_hash.finalize().as_bytes(),
                producer_generation: generation.header().producer_generation,
                registry_revision: generation.header().registry_revision,
                flags: 0,
                reserved: 0,
            },
            location: ReviewCandidateLocation::new(
                ReviewPage::Identity,
                u32::try_from(index).map_err(|_| EntityProducerError::RecordCountOverflow)?,
            ),
        });
    }
    ReviewCatalog::new(authority, candidates)
        .map_err(|_| EntityProducerError::InvalidIdentityCandidate)
}
