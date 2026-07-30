use crate::SemanticLensError;
use phoenix_graph_generation_v2::CandidateId;

pub struct CandidateKeyBuilder {
    hasher: blake3::Hasher,
}

impl CandidateKeyBuilder {
    /// Creates the exact producer-scoped key shape used by a versioned lens.
    ///
    /// The namespace is part of the hash domain, so two lenses cannot produce
    /// the same candidate key from identical endpoints and evidence.
    pub fn producer_scoped(
        candidate_namespace: &str,
        semantic_domain: &[u8],
        content_hash: &[u8; 32],
        producer_id: &str,
    ) -> Result<Self, SemanticLensError> {
        validate_name(candidate_namespace)?;
        if semantic_domain.is_empty()
            || semantic_domain.len() > 128
            || semantic_domain.contains(&0)
            || producer_id.is_empty()
            || producer_id.len() > 256
            || producer_id.as_bytes().contains(&0)
        {
            return Err(SemanticLensError::InvalidCandidateNamespace);
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(candidate_namespace.as_bytes());
        hasher.update(&[0]);
        update_bytes(&mut hasher, semantic_domain);
        hasher.update(content_hash);
        update_bytes(&mut hasher, producer_id.as_bytes());
        Ok(Self { hasher })
    }

    /// Creates a key for a semantic object whose stable ID already contains
    /// producer, vocabulary, source, and evidence identity.
    pub fn object_scoped(
        candidate_namespace: &str,
        semantic_domain: &str,
        content_hash: &[u8; 32],
    ) -> Result<Self, SemanticLensError> {
        validate_name(candidate_namespace)?;
        validate_name(semantic_domain)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(candidate_namespace.as_bytes());
        hasher.update(b"/");
        hasher.update(semantic_domain.as_bytes());
        hasher.update(&[0]);
        hasher.update(content_hash);
        Ok(Self { hasher })
    }

    pub fn update_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        update_bytes(&mut self.hasher, bytes);
        self
    }

    pub fn update_u16(&mut self, value: u16) -> &mut Self {
        self.hasher.update(&value.to_le_bytes());
        self
    }

    pub fn update_u32(&mut self, value: u32) -> &mut Self {
        self.hasher.update(&value.to_le_bytes());
        self
    }

    pub fn update_u64(&mut self, value: u64) -> &mut Self {
        self.hasher.update(&value.to_le_bytes());
        self
    }

    pub fn finish(self) -> CandidateId {
        CandidateId(*self.hasher.finalize().as_bytes())
    }

    pub fn finish_bytes(self) -> [u8; 32] {
        *self.hasher.finalize().as_bytes()
    }
}

fn validate_name(value: &str) -> Result<(), SemanticLensError> {
    if value.is_empty() || value.len() > 256 || value.as_bytes().contains(&0) {
        return Err(SemanticLensError::InvalidCandidateNamespace);
    }
    Ok(())
}

fn update_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
