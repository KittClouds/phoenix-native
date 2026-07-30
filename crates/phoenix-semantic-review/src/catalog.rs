use crate::{ReviewAuthority, ReviewCandidate, ReviewPage, SemanticReviewError};
use hashbrown::HashMap;
use phoenix_graph_generation_v2::CandidateId;

#[derive(Debug)]
pub struct ReviewCatalog {
    authority: ReviewAuthority,
    candidates: Box<[ReviewCandidate]>,
    by_id: HashMap<CandidateId, usize>,
}

impl ReviewCatalog {
    pub fn new(
        authority: ReviewAuthority,
        mut candidates: Vec<ReviewCandidate>,
    ) -> Result<Self, SemanticReviewError> {
        candidates.sort_unstable_by_key(|candidate| candidate.binding.origin.candidate_id);
        let mut by_id = HashMap::with_capacity(candidates.len());
        for (index, candidate) in candidates.iter().enumerate() {
            if ReviewPage::from_raw(candidate.location.page).is_none()
                || candidate.binding.document_hash != authority.document_hash
                || candidate.binding.producer_generation != authority.producer_generation
                || candidate.binding.registry_revision != authority.registry_revision
            {
                return Err(SemanticReviewError::StaleCandidate);
            }
            phoenix_semantic_lens::validate_review_binding(&candidate.binding)
                .map_err(|_| SemanticReviewError::StaleCandidate)?;
            if let Some(previous) = by_id.insert(candidate.binding.origin.candidate_id, index) {
                if candidates[previous] != *candidate {
                    return Err(SemanticReviewError::ConflictingCandidate);
                }
            }
        }
        Ok(Self {
            authority,
            candidates: candidates.into_boxed_slice(),
            by_id,
        })
    }

    pub fn authority(&self) -> ReviewAuthority {
        self.authority
    }

    pub fn candidates(&self) -> &[ReviewCandidate] {
        &self.candidates
    }

    pub fn get(&self, candidate_id: CandidateId) -> Option<&ReviewCandidate> {
        self.by_id
            .get(&candidate_id)
            .map(|index| &self.candidates[*index])
    }

    pub fn merge_exact(
        catalogs: impl IntoIterator<Item = Self>,
    ) -> Result<Self, SemanticReviewError> {
        let mut catalogs = catalogs.into_iter();
        let first = catalogs
            .next()
            .ok_or(SemanticReviewError::MissingCandidate)?;
        let authority = first.authority;
        let mut candidates = first.candidates.into_vec();
        for catalog in catalogs {
            if catalog.authority != authority {
                return Err(SemanticReviewError::StaleCandidate);
            }
            candidates.extend(catalog.candidates);
        }
        Self::new(authority, candidates)
    }

    pub(crate) fn exact(
        &self,
        candidate_id: CandidateId,
        source_generation_hash: [u8; 32],
        candidate_hash: [u8; 32],
        evidence_hash: [u8; 32],
    ) -> Result<&ReviewCandidate, SemanticReviewError> {
        let candidate = self
            .get(candidate_id)
            .ok_or(SemanticReviewError::MissingCandidate)?;
        if self.authority.source_generation_hash != source_generation_hash
            || candidate.binding.candidate_hash != candidate_hash
            || candidate.binding.evidence_hash != evidence_hash
        {
            return Err(SemanticReviewError::StaleCandidate);
        }
        Ok(candidate)
    }
}
