use crate::MemoryRuntimeError;
use bytemuck::bytes_of;
use phoenix_graph_generation_v2::CandidateEvidenceBindingRecord;
use phoenix_memory_contract::{
    CandidateEndpointBindingRecordV3, EvidenceRecordV3, PageKindV3, SemanticCandidateRecordV3,
    VerifiedGraphGenerationV3,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateBindingV1 {
    pub candidate_id: [u8; 32],
    pub source_generation_hash: [u8; 32],
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub source_id: u64,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub row_index: u32,
}

#[derive(Clone, Debug)]
pub struct MemoryCatalogV1 {
    source_generation_hash: [u8; 32],
    bindings: Box<[CandidateBindingV1]>,
}

impl MemoryCatalogV1 {
    pub fn from_generation(
        generation: &VerifiedGraphGenerationV3,
    ) -> Result<Self, MemoryRuntimeError> {
        let candidates =
            generation.typed_page::<SemanticCandidateRecordV3>(PageKindV3::SemanticCandidates)?;
        let endpoints = generation.typed_page::<CandidateEndpointBindingRecordV3>(
            PageKindV3::CandidateEndpointBindings,
        )?;
        let bindings = generation
            .typed_page::<CandidateEvidenceBindingRecord>(PageKindV3::CandidateEvidenceBindings)?;
        let evidence = generation.typed_page::<EvidenceRecordV3>(PageKindV3::Evidence)?;
        let source_generation_hash = generation.header().generation_hash;
        let mut output = Vec::with_capacity(candidates.len());
        for (row_index, candidate) in candidates.iter().enumerate() {
            let endpoint_range = &endpoints[candidate.endpoint_start as usize
                ..(candidate.endpoint_start + candidate.endpoint_count) as usize];
            let evidence_range = &bindings[candidate.evidence_start as usize
                ..(candidate.evidence_start + candidate.evidence_count) as usize];
            let mut candidate_hasher = blake3::Hasher::new();
            candidate_hasher.update(b"phoenix.memory-candidate-binding/v1\0");
            candidate_hasher.update(bytes_of(candidate));
            for endpoint in endpoint_range {
                candidate_hasher.update(bytes_of(endpoint));
            }
            let mut evidence_hasher = blake3::Hasher::new();
            evidence_hasher.update(b"phoenix.memory-evidence-binding/v1\0");
            for binding in evidence_range {
                evidence_hasher.update(bytes_of(binding));
                let row = evidence
                    .binary_search_by_key(&binding.evidence_id, |item| item.id)
                    .ok()
                    .and_then(|index| evidence.get(index))
                    .ok_or(MemoryRuntimeError::InvalidProjection(
                        "candidate evidence is missing from the verified generation",
                    ))?;
                evidence_hasher.update(bytes_of(row));
            }
            output.push(CandidateBindingV1 {
                candidate_id: candidate.candidate_id,
                source_generation_hash,
                candidate_hash: *candidate_hasher.finalize().as_bytes(),
                evidence_hash: *evidence_hasher.finalize().as_bytes(),
                source_id: candidate.source_id,
                valid_time_from_millis: candidate.valid_time_from_millis,
                valid_time_to_millis: candidate.valid_time_to_millis,
                row_index: u32::try_from(row_index)
                    .map_err(|_| MemoryRuntimeError::InvalidProjection("too many candidates"))?,
            });
        }
        output.sort_unstable_by_key(|binding| binding.candidate_id);
        Ok(Self {
            source_generation_hash,
            bindings: output.into_boxed_slice(),
        })
    }

    pub fn source_generation_hash(&self) -> [u8; 32] {
        self.source_generation_hash
    }

    pub fn bindings(&self) -> &[CandidateBindingV1] {
        &self.bindings
    }

    pub fn get(&self, candidate_id: [u8; 32]) -> Option<&CandidateBindingV1> {
        self.bindings
            .binary_search_by_key(&candidate_id, |binding| binding.candidate_id)
            .ok()
            .map(|index| &self.bindings[index])
    }

    pub fn exact(
        &self,
        candidate_id: [u8; 32],
        source_generation_hash: [u8; 32],
        candidate_hash: [u8; 32],
        evidence_hash: [u8; 32],
    ) -> Result<&CandidateBindingV1, MemoryRuntimeError> {
        self.get(candidate_id)
            .filter(|binding| {
                binding.source_generation_hash == source_generation_hash
                    && binding.candidate_hash == candidate_hash
                    && binding.evidence_hash == evidence_hash
            })
            .ok_or(MemoryRuntimeError::StaleCandidateBinding)
    }
}
