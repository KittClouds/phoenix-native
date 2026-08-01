use crate::{
    AnalysisMention, DocumentAnalysisBinding, NliCandidateKind, PhoenixDocumentAnalysisV1,
    PhoenixStructuralSubstrateV1, MAX_ID_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PRODUCER_COORDINATOR_CONTRACT: &str = "phoenix.native.semantic-producer-coordinator/v1";
pub const PRODUCER_COORDINATOR_EXTENSION: &str = "pnpc";
pub const PRODUCER_QUEUE_CAPACITY: u32 = 1;
pub const MAX_CONTEXTUAL_EVIDENCE_BINDINGS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(u16)]
pub enum SemanticProduct {
    DocumentStructure = 1,
    DynamicChunksAndSpans = 2,
    MentionsAndEvidence = 3,
    CanonicalEntityBindings = 4,
    IdentityAliasCoreference = 5,
    GenericRelatedEvidence = 6,
    TypedRelationships = 7,
    EventsTimeline = 8,
    Causality = 9,
    MemoryState = 10,
    ContextualCoOccurrence = 11,
}

impl SemanticProduct {
    pub const ALL: [Self; 11] = [
        Self::DocumentStructure,
        Self::DynamicChunksAndSpans,
        Self::MentionsAndEvidence,
        Self::CanonicalEntityBindings,
        Self::IdentityAliasCoreference,
        Self::GenericRelatedEvidence,
        Self::TypedRelationships,
        Self::EventsTimeline,
        Self::Causality,
        Self::MemoryState,
        Self::ContextualCoOccurrence,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProductAuthority {
    Authoritative,
    CandidateOnly,
    ContextualEvidenceOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProducerRunState {
    Produced,
    NotRun,
    Unsupported,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerCapabilityReceipt {
    pub product: SemanticProduct,
    pub producer_id: String,
    pub authority: ProductAuthority,
    pub state: ProducerRunState,
    pub output_count: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateEvidenceBinding {
    pub candidate_id: [u8; 32],
    pub left_mention_id: u64,
    pub right_mention_id: u64,
    pub premise_start: u32,
    pub premise_end: u32,
    pub first_chunk: u32,
    pub last_chunk: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextualEvidenceBinding {
    pub source_entity_id: u64,
    pub target_entity_id: u64,
    pub source_mention_id: u64,
    pub target_mention_id: u64,
    pub chunk_index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhoenixProducerCoordinatorV1 {
    pub schema: String,
    pub binding: DocumentAnalysisBinding,
    pub queue_capacity: u32,
    pub queue_high_water: u32,
    pub cancellation_observed: bool,
    pub promotion_count: u32,
    pub capabilities: Vec<ProducerCapabilityReceipt>,
    pub evidence_bindings: Vec<CandidateEvidenceBinding>,
    pub contextual_evidence_bindings: Vec<ContextualEvidenceBinding>,
}

impl PhoenixProducerCoordinatorV1 {
    pub fn validate_preliminary(
        &self,
        analysis: &PhoenixDocumentAnalysisV1,
        structural: &PhoenixStructuralSubstrateV1,
    ) -> Result<(), &'static str> {
        self.validate_common(analysis, structural)?;
        require_state(
            &self.capabilities,
            SemanticProduct::CanonicalEntityBindings,
            ProducerRunState::NotRun,
        )?;
        require_state(
            &self.capabilities,
            SemanticProduct::ContextualCoOccurrence,
            ProducerRunState::NotRun,
        )?;
        if !self.contextual_evidence_bindings.is_empty() {
            return Err("preliminary coordinator contains contextual evidence");
        }
        Ok(())
    }

    pub fn finalize(
        &mut self,
        canonical_entity_count: u32,
        contextual_evidence_bindings: Vec<ContextualEvidenceBinding>,
    ) -> Result<(), &'static str> {
        if contextual_evidence_bindings.len() > MAX_CONTEXTUAL_EVIDENCE_BINDINGS {
            return Err("contextual evidence binding count is oversized");
        }
        let contextual_candidate_count = contextual_evidence_bindings
            .iter()
            .map(|binding| (binding.source_entity_id, binding.target_entity_id))
            .collect::<BTreeSet<_>>()
            .len()
            .try_into()
            .map_err(|_| "contextual candidate count overflows u32")?;
        self.contextual_evidence_bindings = contextual_evidence_bindings;
        set_produced(
            &mut self.capabilities,
            SemanticProduct::CanonicalEntityBindings,
            canonical_entity_count,
        )?;
        set_produced(
            &mut self.capabilities,
            SemanticProduct::ContextualCoOccurrence,
            contextual_candidate_count,
        )
    }

    pub fn validate_final(
        &self,
        analysis: &PhoenixDocumentAnalysisV1,
        structural: &PhoenixStructuralSubstrateV1,
    ) -> Result<(), &'static str> {
        self.validate_common(analysis, structural)?;
        require_state(
            &self.capabilities,
            SemanticProduct::CanonicalEntityBindings,
            ProducerRunState::Produced,
        )?;
        require_state(
            &self.capabilities,
            SemanticProduct::ContextualCoOccurrence,
            ProducerRunState::Produced,
        )?;
        validate_contextual_evidence_bindings(self, analysis, structural)
    }

    pub fn capability(&self, product: SemanticProduct) -> Option<&ProducerCapabilityReceipt> {
        self.capabilities
            .iter()
            .find(|capability| capability.product == product)
    }

    fn validate_common(
        &self,
        analysis: &PhoenixDocumentAnalysisV1,
        structural: &PhoenixStructuralSubstrateV1,
    ) -> Result<(), &'static str> {
        if self.schema != PRODUCER_COORDINATOR_CONTRACT
            || self.binding != analysis.ner.binding
            || self.binding != structural.binding
            || self.queue_capacity != PRODUCER_QUEUE_CAPACITY
            || self.queue_high_water > self.queue_capacity
            || self.cancellation_observed
            || self.promotion_count != 0
        {
            return Err("producer coordinator authority or bounded-runtime contract is invalid");
        }
        let mut products = BTreeSet::new();
        for capability in &self.capabilities {
            if !products.insert(capability.product)
                || capability.producer_id.is_empty()
                || capability.producer_id.len() > MAX_ID_BYTES
                || capability.authority != expected_authority(capability.product)
                || matches!(capability.state, ProducerRunState::Produced)
                    != capability.output_count.is_some()
            {
                return Err("producer capability matrix is invalid");
            }
        }
        if products.into_iter().ne(SemanticProduct::ALL) {
            return Err("producer capability matrix is incomplete");
        }
        for product in [
            SemanticProduct::DocumentStructure,
            SemanticProduct::DynamicChunksAndSpans,
            SemanticProduct::MentionsAndEvidence,
        ] {
            require_state(&self.capabilities, product, ProducerRunState::Produced)?;
        }
        for product in [
            SemanticProduct::TypedRelationships,
            SemanticProduct::EventsTimeline,
            SemanticProduct::Causality,
            SemanticProduct::MemoryState,
        ] {
            require_state(&self.capabilities, product, ProducerRunState::Unsupported)?;
        }
        validate_evidence_bindings(self, analysis, structural)
    }
}

pub fn expected_authority(product: SemanticProduct) -> ProductAuthority {
    match product {
        SemanticProduct::DocumentStructure
        | SemanticProduct::DynamicChunksAndSpans
        | SemanticProduct::MentionsAndEvidence
        | SemanticProduct::CanonicalEntityBindings => ProductAuthority::Authoritative,
        SemanticProduct::ContextualCoOccurrence => ProductAuthority::ContextualEvidenceOnly,
        _ => ProductAuthority::CandidateOnly,
    }
}

pub fn capability(
    product: SemanticProduct,
    producer_id: impl Into<String>,
    state: ProducerRunState,
    output_count: Option<u32>,
) -> ProducerCapabilityReceipt {
    ProducerCapabilityReceipt {
        product,
        producer_id: producer_id.into(),
        authority: expected_authority(product),
        state,
        output_count,
    }
}

fn validate_evidence_bindings(
    coordinator: &PhoenixProducerCoordinatorV1,
    analysis: &PhoenixDocumentAnalysisV1,
    structural: &PhoenixStructuralSubstrateV1,
) -> Result<(), &'static str> {
    if coordinator.evidence_bindings.len() != analysis.nli.nli_candidates.len() {
        return Err("not every semantic candidate has one evidence binding");
    }
    let mentions = analysis
        .ner
        .mentions
        .iter()
        .map(|mention| (mention.mention_id, mention))
        .collect::<BTreeMap<_, _>>();
    let candidates = analysis
        .nli
        .nli_candidates
        .iter()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for evidence in &coordinator.evidence_bindings {
        let candidate = candidates
            .get(&evidence.candidate_id)
            .ok_or("candidate evidence references an unknown candidate")?;
        let left = mention(&mentions, evidence.left_mention_id)?;
        let right = mention(&mentions, evidence.right_mention_id)?;
        let first_chunk = structural
            .chunks
            .get(evidence.first_chunk as usize)
            .ok_or("candidate evidence references an unknown chunk")?;
        let last_chunk = structural
            .chunks
            .get(evidence.last_chunk as usize)
            .ok_or("candidate evidence references an unknown chunk")?;
        if !seen.insert(evidence.candidate_id)
            || evidence.first_chunk > evidence.last_chunk
            || evidence.premise_start != candidate.premise_start
            || evidence.premise_end != candidate.premise_end
            || left.entity_id != candidate.left_entity_id
            || right.entity_id != candidate.right_entity_id
            || evidence.premise_start > left.start.min(right.start)
            || evidence.premise_end < left.end.max(right.end)
            || first_chunk.start > left.start.min(right.start)
            || last_chunk.end < left.end.max(right.end)
        {
            return Err("candidate evidence does not bind its mentions, chunks, and premise");
        }
    }
    let identity = analysis
        .nli
        .nli_candidates
        .iter()
        .filter(|candidate| candidate.kind != NliCandidateKind::Related)
        .count() as u32;
    let generic = analysis
        .nli
        .nli_candidates
        .iter()
        .filter(|candidate| candidate.kind == NliCandidateKind::Related)
        .count() as u32;
    require_count(
        &coordinator.capabilities,
        SemanticProduct::IdentityAliasCoreference,
        identity,
    )?;
    require_count(
        &coordinator.capabilities,
        SemanticProduct::GenericRelatedEvidence,
        generic,
    )
}

fn validate_contextual_evidence_bindings(
    coordinator: &PhoenixProducerCoordinatorV1,
    analysis: &PhoenixDocumentAnalysisV1,
    structural: &PhoenixStructuralSubstrateV1,
) -> Result<(), &'static str> {
    if coordinator.contextual_evidence_bindings.len() > MAX_CONTEXTUAL_EVIDENCE_BINDINGS {
        return Err("contextual evidence binding count is oversized");
    }
    let mentions = analysis
        .ner
        .mentions
        .iter()
        .map(|mention| (mention.mention_id, mention))
        .collect::<BTreeMap<_, _>>();
    let mut expected_per_chunk = vec![BTreeMap::<u64, u64>::new(); structural.chunks.len()];
    // Analysis artifacts contain only exportable mentions.  `accepted` is a
    // semantic-promotion verdict, while this page is contextual evidence only.
    // Requiring promotion here would erase valid alias-candidate evidence.
    for exported in &analysis.ner.mentions {
        let chunk_index = structural
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.start <= exported.start && exported.end <= chunk.end)
            .min_by_key(|(ordinal, chunk)| (chunk.end - chunk.start, *ordinal))
            .map(|(ordinal, _)| ordinal)
            .ok_or("exported mention has no contextual chunk")?;
        expected_per_chunk[chunk_index]
            .entry(exported.entity_id)
            .and_modify(|mention_id| *mention_id = (*mention_id).min(exported.mention_id))
            .or_insert(exported.mention_id);
    }
    let mut expected_bindings = BTreeSet::new();
    for (chunk_index, entities) in expected_per_chunk.iter().enumerate() {
        let entities = entities.iter().collect::<Vec<_>>();
        for source in 0..entities.len() {
            for target in (source + 1)..entities.len() {
                expected_bindings.insert((
                    u32::try_from(chunk_index).map_err(|_| "contextual chunk index overflows")?,
                    *entities[source].0,
                    *entities[target].0,
                    *entities[source].1,
                    *entities[target].1,
                ));
                if expected_bindings.len() > MAX_CONTEXTUAL_EVIDENCE_BINDINGS {
                    return Err("contextual evidence binding count is oversized");
                }
            }
        }
    }
    let mut bindings = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    for evidence in &coordinator.contextual_evidence_bindings {
        let source = mention(&mentions, evidence.source_mention_id)?;
        let target = mention(&mentions, evidence.target_mention_id)?;
        let chunk = structural
            .chunks
            .get(evidence.chunk_index as usize)
            .ok_or("contextual evidence references an unknown chunk")?;
        if evidence.source_entity_id >= evidence.target_entity_id
            || source.entity_id != evidence.source_entity_id
            || target.entity_id != evidence.target_entity_id
            || chunk.start > source.start.min(target.start)
            || chunk.end < source.end.max(target.end)
            || !bindings.insert((
                evidence.chunk_index,
                evidence.source_entity_id,
                evidence.target_entity_id,
                evidence.source_mention_id,
                evidence.target_mention_id,
            ))
        {
            return Err("contextual candidate is not bound to exact exported evidence");
        }
        pairs.insert((evidence.source_entity_id, evidence.target_entity_id));
    }
    if bindings != expected_bindings {
        return Err("contextual evidence bindings are incomplete or nondeterministic");
    }
    let expected =
        u32::try_from(pairs.len()).map_err(|_| "contextual candidate count overflows")?;
    require_count(
        &coordinator.capabilities,
        SemanticProduct::ContextualCoOccurrence,
        expected,
    )
}

fn mention<'a>(
    mentions: &'a BTreeMap<u64, &'a AnalysisMention>,
    id: u64,
) -> Result<&'a AnalysisMention, &'static str> {
    mentions
        .get(&id)
        .copied()
        .ok_or("candidate evidence references an unknown mention")
}

fn require_count(
    capabilities: &[ProducerCapabilityReceipt],
    product: SemanticProduct,
    expected: u32,
) -> Result<(), &'static str> {
    let capability = capabilities
        .iter()
        .find(|capability| capability.product == product)
        .ok_or("producer capability is missing")?;
    if capability.state != ProducerRunState::Produced || capability.output_count != Some(expected) {
        return Err("producer capability count is incorrect");
    }
    Ok(())
}

fn require_state(
    capabilities: &[ProducerCapabilityReceipt],
    product: SemanticProduct,
    expected: ProducerRunState,
) -> Result<(), &'static str> {
    let capability = capabilities
        .iter()
        .find(|capability| capability.product == product)
        .ok_or("producer capability is missing")?;
    if capability.state != expected {
        return Err("producer capability state is incorrect");
    }
    Ok(())
}

fn set_produced(
    capabilities: &mut [ProducerCapabilityReceipt],
    product: SemanticProduct,
    count: u32,
) -> Result<(), &'static str> {
    let capability = capabilities
        .iter_mut()
        .find(|capability| capability.product == product)
        .ok_or("producer capability is missing")?;
    capability.state = ProducerRunState::Produced;
    capability.output_count = Some(count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AnalysisChunkRecord, AnalysisEntity, AnalysisEntityKind, AnalysisModelIdentity,
        AnalysisSentenceRecord, AnalysisSpanRecord, AnalysisStageReceipt, NliAdjudication,
        NliCandidate, NliDecision, PhoenixNerArtifactV1, PhoenixNliArtifactV1,
        StructuralDialogueHint, StructuralSentenceQuality, StructuralSpanKind,
        STRUCTURAL_SUBSTRATE_CONTRACT,
    };

    #[test]
    fn coordinator_is_evidence_bound_and_never_promotes_generic_related() {
        let (analysis, structural) = fixture();
        assert!(analysis
            .ner
            .mentions
            .iter()
            .all(|mention| !mention.accepted));
        let mut coordinator = preliminary(&analysis, &structural);
        coordinator
            .validate_preliminary(&analysis, &structural)
            .expect("preliminary coordinator");
        coordinator
            .finalize(
                2,
                vec![ContextualEvidenceBinding {
                    source_entity_id: 11,
                    target_entity_id: 12,
                    source_mention_id: 101,
                    target_mention_id: 102,
                    chunk_index: 0,
                }],
            )
            .expect("finalize");
        coordinator
            .validate_final(&analysis, &structural)
            .expect("final coordinator");
        assert_eq!(
            coordinator
                .capability(SemanticProduct::GenericRelatedEvidence)
                .expect("generic evidence")
                .authority,
            ProductAuthority::CandidateOnly
        );
        assert_eq!(
            coordinator
                .capability(SemanticProduct::TypedRelationships)
                .expect("typed relationship")
                .state,
            ProducerRunState::Unsupported
        );
        assert_eq!(coordinator.promotion_count, 0);
    }

    #[test]
    fn coordinator_fails_closed_without_exact_evidence_or_capability_truth() {
        let (analysis, structural) = fixture();
        let mut missing_evidence = preliminary(&analysis, &structural);
        missing_evidence.evidence_bindings.clear();
        assert!(missing_evidence
            .validate_preliminary(&analysis, &structural)
            .is_err());

        let mut false_typed_lane = preliminary(&analysis, &structural);
        let typed = false_typed_lane
            .capabilities
            .iter_mut()
            .find(|capability| capability.product == SemanticProduct::TypedRelationships)
            .expect("typed relationship capability");
        typed.state = ProducerRunState::Produced;
        typed.output_count = Some(1);
        assert!(false_typed_lane
            .validate_preliminary(&analysis, &structural)
            .is_err());

        let mut missing_context = preliminary(&analysis, &structural);
        missing_context
            .finalize(
                2,
                vec![ContextualEvidenceBinding {
                    source_entity_id: 11,
                    target_entity_id: 12,
                    source_mention_id: 101,
                    target_mention_id: 102,
                    chunk_index: 0,
                }],
            )
            .expect("finalize contextual evidence");
        missing_context.contextual_evidence_bindings.clear();
        assert!(missing_context
            .validate_final(&analysis, &structural)
            .is_err());
    }

    #[test]
    fn contextual_evidence_uses_the_smallest_overlapping_chunk() {
        let (mut analysis, mut structural) = fixture();
        analysis.ner.mentions[0].start = 8;
        analysis.ner.mentions[0].end = 9;
        analysis.ner.mentions[1].start = 10;
        analysis.ner.mentions[1].end = 11;
        structural.chunks.push(AnalysisChunkRecord {
            start: 8,
            end: 12,
            sentence_start: 0,
            sentence_end: 1,
            paragraph_start: 0,
            paragraph_end: 1,
            chapter_index: crate::NO_STRUCTURAL_PARENT,
            token_count: 2,
            content_hash: 6,
            dialogue_hint: StructuralDialogueHint::None,
        });
        let mut coordinator = preliminary(&analysis, &structural);
        coordinator
            .finalize(
                2,
                vec![ContextualEvidenceBinding {
                    source_entity_id: 11,
                    target_entity_id: 12,
                    source_mention_id: 101,
                    target_mention_id: 102,
                    chunk_index: 1,
                }],
            )
            .expect("finalize contextual evidence");
        coordinator
            .validate_final(&analysis, &structural)
            .expect("shortest containing chunk is canonical");
    }

    fn preliminary(
        analysis: &PhoenixDocumentAnalysisV1,
        structural: &PhoenixStructuralSubstrateV1,
    ) -> PhoenixProducerCoordinatorV1 {
        PhoenixProducerCoordinatorV1 {
            schema: PRODUCER_COORDINATOR_CONTRACT.into(),
            binding: analysis.ner.binding.clone(),
            queue_capacity: PRODUCER_QUEUE_CAPACITY,
            queue_high_water: 1,
            cancellation_observed: false,
            promotion_count: 0,
            capabilities: vec![
                capability(
                    SemanticProduct::DocumentStructure,
                    "chunker",
                    ProducerRunState::Produced,
                    Some(1),
                ),
                capability(
                    SemanticProduct::DynamicChunksAndSpans,
                    "chunker",
                    ProducerRunState::Produced,
                    Some(structural.chunks.len() as u32),
                ),
                capability(
                    SemanticProduct::MentionsAndEvidence,
                    "ner",
                    ProducerRunState::Produced,
                    Some(2),
                ),
                capability(
                    SemanticProduct::CanonicalEntityBindings,
                    "registry",
                    ProducerRunState::NotRun,
                    None,
                ),
                capability(
                    SemanticProduct::IdentityAliasCoreference,
                    "nli",
                    ProducerRunState::Produced,
                    Some(0),
                ),
                capability(
                    SemanticProduct::GenericRelatedEvidence,
                    "nli",
                    ProducerRunState::Produced,
                    Some(1),
                ),
                capability(
                    SemanticProduct::TypedRelationships,
                    "none",
                    ProducerRunState::Unsupported,
                    None,
                ),
                capability(
                    SemanticProduct::EventsTimeline,
                    "none",
                    ProducerRunState::Unsupported,
                    None,
                ),
                capability(
                    SemanticProduct::Causality,
                    "none",
                    ProducerRunState::Unsupported,
                    None,
                ),
                capability(
                    SemanticProduct::MemoryState,
                    "none",
                    ProducerRunState::Unsupported,
                    None,
                ),
                capability(
                    SemanticProduct::ContextualCoOccurrence,
                    "compiler",
                    ProducerRunState::NotRun,
                    None,
                ),
            ],
            evidence_bindings: vec![CandidateEvidenceBinding {
                candidate_id: [9; 32],
                left_mention_id: 101,
                right_mention_id: 102,
                premise_start: 0,
                premise_end: 16,
                first_chunk: 0,
                last_chunk: 0,
            }],
            contextual_evidence_bindings: Vec::new(),
        }
    }

    fn fixture() -> (PhoenixDocumentAnalysisV1, PhoenixStructuralSubstrateV1) {
        let identity = AnalysisModelIdentity {
            model_id: "fixture".into(),
            artifact_hash: [2; 32],
            config_hash: [3; 32],
            runtime_id: "test".into(),
        };
        let binding = DocumentAnalysisBinding {
            source_document_id: "fixture".into(),
            native_document_id: 1,
            document_revision: 1,
            content_hash: [4; 32],
            analysis_generation: 1,
            source_registry_revision: 0,
            target_registry_revision: 1,
            producer_binary_hash: [1; 32],
            chunker: identity.clone(),
            dynamic_ner: identity.clone(),
            nli: identity,
        };
        let receipt = AnalysisStageReceipt {
            chunk_count: 1,
            sentence_count: 1,
            mention_count: 2,
            entity_count: 2,
            nli_candidate_count: 1,
            nli_adjudication_count: 1,
            chunker_micros: 1,
            dynamic_ner_micros: 1,
            nli_load_micros: 1,
            nli_adjudication_micros: 1,
            promotion_count: 0,
        };
        let analysis = PhoenixDocumentAnalysisV1 {
            schema: crate::ANALYSIS_CONTRACT.into(),
            ner: PhoenixNerArtifactV1 {
                binding: binding.clone(),
                ner_revision: 1,
                entities: vec![
                    AnalysisEntity {
                        stable_id: 11,
                        label: "Alpha".into(),
                        kind: AnalysisEntityKind::Character,
                        custom_kind: None,
                        mention_count: 1,
                    },
                    AnalysisEntity {
                        stable_id: 12,
                        label: "Beta".into(),
                        kind: AnalysisEntityKind::Character,
                        custom_kind: None,
                        mention_count: 1,
                    },
                ],
                mentions: vec![
                    AnalysisMention {
                        mention_id: 101,
                        entity_id: 11,
                        start: 0,
                        end: 5,
                        sentence_index: 0,
                        confidence: 1.0,
                        accepted: false,
                    },
                    AnalysisMention {
                        mention_id: 102,
                        entity_id: 12,
                        start: 6,
                        end: 10,
                        sentence_index: 0,
                        confidence: 1.0,
                        accepted: false,
                    },
                ],
                receipt,
            },
            nli: PhoenixNliArtifactV1 {
                binding: binding.clone(),
                nli_candidates: vec![NliCandidate {
                    candidate_id: [9; 32],
                    kind: NliCandidateKind::Related,
                    left_entity_id: 11,
                    right_entity_id: 12,
                    premise_start: 0,
                    premise_end: 16,
                    premise: "Alpha met Beta.".into(),
                    hypothesis: "Alpha is related to Beta in this passage.".into(),
                }],
                nli_adjudications: vec![NliAdjudication {
                    candidate_id: [9; 32],
                    decision: NliDecision::Supported,
                    entailment_millis: 800,
                    contradiction_millis: 50,
                    neutral_millis: 150,
                    confidence_millis: 800,
                    needs_human_review: true,
                }],
                promotion_count: 0,
            },
        };
        let structural = PhoenixStructuralSubstrateV1 {
            schema: STRUCTURAL_SUBSTRATE_CONTRACT.into(),
            binding,
            source_len: 16,
            chunks: vec![AnalysisChunkRecord {
                start: 0,
                end: 16,
                sentence_start: 0,
                sentence_end: 1,
                paragraph_start: 0,
                paragraph_end: 1,
                chapter_index: crate::NO_STRUCTURAL_PARENT,
                token_count: 3,
                content_hash: 5,
                dialogue_hint: StructuralDialogueHint::None,
            }],
            sentences: vec![AnalysisSentenceRecord {
                start: 0,
                end: 16,
                paragraph_index: 0,
                chapter_index: 0,
                token_count: 3,
                content_hash: 5,
                quality: StructuralSentenceQuality::Complete,
                dialogue_hint: StructuralDialogueHint::None,
            }],
            spans: vec![
                AnalysisSpanRecord {
                    kind: StructuralSpanKind::Paragraph,
                    start: 0,
                    end: 16,
                    parent_index: 0,
                    child_start: 0,
                    child_end: 1,
                    token_count: 3,
                    content_hash: 5,
                    label: String::new(),
                    dialogue_hint: StructuralDialogueHint::None,
                },
                AnalysisSpanRecord {
                    kind: StructuralSpanKind::Chapter,
                    start: 0,
                    end: 16,
                    parent_index: crate::NO_STRUCTURAL_PARENT,
                    child_start: 0,
                    child_end: 1,
                    token_count: 3,
                    content_hash: 5,
                    label: "Fixture".into(),
                    dialogue_hint: StructuralDialogueHint::None,
                },
            ],
        };
        (analysis, structural)
    }
}
