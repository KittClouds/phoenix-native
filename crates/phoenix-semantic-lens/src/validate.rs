use crate::{
    compute_lens_identity, CandidateOrigin, CoreSemanticClass, EndpointKind,
    LensNeutralReviewBinding, SemanticLensDefinition, SemanticLensError,
    VerifiedSemanticLensPackV1, MAX_CODE_COUNT, MAX_CODE_NAME_BYTES, MAX_NAMESPACE_BYTES,
};
use hashbrown::HashSet;

pub fn validate_definition(
    definition: &SemanticLensDefinition<'_>,
) -> Result<crate::LensIdentity, SemanticLensError> {
    if definition.namespace.is_empty()
        || definition.namespace.len() > MAX_NAMESPACE_BYTES
        || definition.namespace.as_bytes().contains(&0)
    {
        return Err(SemanticLensError::InvalidNamespace);
    }
    if definition.version == 0
        || definition.codes.is_empty()
        || definition.codes.len() > MAX_CODE_COUNT
    {
        return Err(SemanticLensError::InvalidCodeCount);
    }

    let mut previous = 0_u32;
    let mut names = HashSet::with_capacity(definition.codes.len());
    for code in definition.codes {
        if code.code == 0 || code.code <= previous {
            return Err(SemanticLensError::DuplicateCode(code.code));
        }
        previous = code.code;
        if code.stable_name.is_empty()
            || code.stable_name.len() > MAX_CODE_NAME_BYTES
            || code.stable_name.as_bytes().contains(&0)
            || !names.insert(code.stable_name)
        {
            return Err(SemanticLensError::InvalidCodeName);
        }
        if CoreSemanticClass::from_raw(code.class as u16).is_none() {
            return Err(SemanticLensError::InvalidSemanticClass(code.code));
        }
        if !code.source_endpoints.is_valid() || !code.target_endpoints.is_valid() {
            return Err(SemanticLensError::InvalidEndpointMask(code.code));
        }
    }
    Ok(compute_lens_identity(definition))
}

pub fn validate_review_binding(
    binding: &LensNeutralReviewBinding,
) -> Result<(), SemanticLensError> {
    let CandidateOrigin {
        candidate_id,
        lens_id,
        vocabulary_hash,
        semantic_code,
        semantic_class,
        ..
    } = binding.origin;
    if candidate_id.is_zero()
        || lens_id == [0; 32]
        || vocabulary_hash == [0; 32]
        || semantic_code == 0
        || CoreSemanticClass::from_raw(semantic_class).is_none()
        || binding.document_hash == [0; 32]
        || binding.candidate_hash == [0; 32]
        || binding.evidence_hash == [0; 32]
        || binding.producer_generation == 0
        || binding.registry_revision == 0
        || binding.source.id == 0
        || EndpointKind::from_raw(binding.source.kind).is_none()
        || (binding.target.id != 0 && EndpointKind::from_raw(binding.target.kind).is_none())
    {
        return Err(SemanticLensError::InvalidReviewBinding);
    }
    Ok(())
}

pub fn validate_review_binding_against_pack(
    binding: &LensNeutralReviewBinding,
    pack: &VerifiedSemanticLensPackV1,
) -> Result<(), SemanticLensError> {
    validate_review_binding(binding)?;
    let identity = pack.identity();
    if binding.origin.lens_id != identity.lens_id
        || binding.origin.vocabulary_hash != identity.vocabulary_hash
    {
        return Err(SemanticLensError::InvalidReviewBinding);
    }
    let code = pack
        .code(binding.origin.semantic_code)
        .ok_or(SemanticLensError::UnknownCode(binding.origin.semantic_code))?;
    if code.class != binding.origin.semantic_class {
        return Err(SemanticLensError::InvalidReviewBinding);
    }
    let source_kind = binding
        .source
        .kind()
        .ok_or(SemanticLensError::InvalidReviewBinding)?;
    if !crate::EndpointMask(code.source_mask).contains(source_kind) {
        return Err(SemanticLensError::InvalidReviewBinding);
    }
    if binding.target.id == 0 {
        if code.target_mask != 0 {
            return Err(SemanticLensError::InvalidReviewBinding);
        }
    } else {
        let target_kind = binding
            .target
            .kind()
            .ok_or(SemanticLensError::InvalidReviewBinding)?;
        if !crate::EndpointMask(code.target_mask).contains(target_kind) {
            return Err(SemanticLensError::InvalidReviewBinding);
        }
    }
    Ok(())
}
