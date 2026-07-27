use serde::{Deserialize, Serialize};

pub const MAX_ANALYSIS_ENTITIES: usize = 250_000;
pub const MAX_ANALYSIS_MENTIONS: usize = 2_000_000;
pub const MAX_NLI_CANDIDATES: usize = 65_536;
pub const MAX_ID_BYTES: usize = 512;
pub const MAX_LABEL_BYTES: usize = 512;
pub const MAX_TEXT_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisModelIdentity {
    pub model_id: String,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub runtime_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentAnalysisBinding {
    pub source_document_id: String,
    pub native_document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub analysis_generation: u64,
    pub source_registry_revision: u64,
    pub target_registry_revision: u64,
    pub producer_binary_hash: [u8; 32],
    pub chunker: AnalysisModelIdentity,
    pub dynamic_ner: AnalysisModelIdentity,
    pub nli: AnalysisModelIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentAnalysisRequestBinding {
    pub source_document_id: String,
    pub native_document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub analysis_generation: u64,
    pub source_registry_revision: u64,
    pub target_registry_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u16)]
pub enum AnalysisEntityKind {
    Character = 1,
    Location = 2,
    Npc = 3,
    Faction = 4,
    Event = 5,
    Concept = 6,
    Custom = 255,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisEntity {
    pub stable_id: u64,
    pub label: String,
    pub kind: AnalysisEntityKind,
    pub custom_kind: Option<String>,
    pub mention_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisMention {
    pub mention_id: u64,
    pub entity_id: u64,
    pub start: u32,
    pub end: u32,
    pub sentence_index: u32,
    pub confidence: f32,
    pub accepted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NliCandidateKind {
    SameSurface,
    Alias,
    Coreference,
    Related,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NliCandidate {
    pub candidate_id: [u8; 32],
    pub kind: NliCandidateKind,
    pub left_entity_id: u64,
    pub right_entity_id: u64,
    pub premise_start: u32,
    pub premise_end: u32,
    pub premise: String,
    pub hypothesis: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NliDecision {
    Supported,
    Contradicted,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NliAdjudication {
    pub candidate_id: [u8; 32],
    pub decision: NliDecision,
    pub entailment_millis: u32,
    pub contradiction_millis: u32,
    pub neutral_millis: u32,
    pub confidence_millis: u32,
    pub needs_human_review: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisStageReceipt {
    pub chunk_count: u32,
    pub sentence_count: u32,
    pub mention_count: u32,
    pub entity_count: u32,
    pub nli_candidate_count: u32,
    pub nli_adjudication_count: u32,
    pub chunker_micros: u64,
    pub dynamic_ner_micros: u64,
    pub nli_load_micros: u64,
    pub nli_adjudication_micros: u64,
    pub promotion_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoenixNerArtifactV1 {
    pub binding: DocumentAnalysisBinding,
    pub ner_revision: u64,
    pub entities: Vec<AnalysisEntity>,
    pub mentions: Vec<AnalysisMention>,
    pub receipt: AnalysisStageReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoenixNliArtifactV1 {
    pub binding: DocumentAnalysisBinding,
    pub nli_candidates: Vec<NliCandidate>,
    pub nli_adjudications: Vec<NliAdjudication>,
    pub promotion_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoenixDocumentAnalysisV1 {
    pub schema: String,
    pub ner: PhoenixNerArtifactV1,
    pub nli: PhoenixNliArtifactV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhoenixAnalysisRequestV1 {
    pub schema: String,
    pub binding: DocumentAnalysisRequestBinding,
    pub text: String,
    pub ner_model_root: String,
    pub nli_model_root: String,
    pub max_nli_candidates: u32,
}

impl PhoenixAnalysisRequestV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_request_binding(&self.binding)?;
        if self.schema != crate::ANALYSIS_CONTRACT {
            return Err("unsupported request schema");
        }
        if self.text.is_empty() || self.text.len() > MAX_TEXT_BYTES {
            return Err("request text is empty or oversized");
        }
        if *blake3::hash(self.text.as_bytes()).as_bytes() != self.binding.content_hash {
            return Err("request text hash does not match its binding");
        }
        if self.ner_model_root.is_empty() || self.nli_model_root.is_empty() {
            return Err("model roots are required");
        }
        if self.max_nli_candidates == 0 || self.max_nli_candidates as usize > MAX_NLI_CANDIDATES {
            return Err("NLI candidate budget is invalid");
        }
        Ok(())
    }
}

impl PhoenixDocumentAnalysisV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != crate::ANALYSIS_CONTRACT {
            return Err("unsupported analysis schema");
        }
        self.ner.validate()?;
        self.nli.validate()?;
        if self.ner.binding != self.nli.binding
            || self.ner.receipt.nli_candidate_count as usize != self.nli.nli_candidates.len()
            || self.ner.receipt.nli_adjudication_count as usize != self.nli.nli_adjudications.len()
        {
            return Err("NER and NLI artifacts do not share one authority binding");
        }
        Ok(())
    }
}

impl PhoenixNerArtifactV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_binding(&self.binding)?;
        if self.ner_revision == 0
            || self.entities.len() > MAX_ANALYSIS_ENTITIES
            || self.mentions.len() > MAX_ANALYSIS_MENTIONS
        {
            return Err("NER counts are invalid");
        }
        validate_entities(&self.entities)?;
        validate_mentions(&self.entities, &self.mentions)?;
        if usize::try_from(self.receipt.entity_count).ok() != Some(self.entities.len())
            || usize::try_from(self.receipt.mention_count).ok() != Some(self.mentions.len())
        {
            return Err("NER receipt counts do not match analysis rows");
        }
        Ok(())
    }
}

impl PhoenixNliArtifactV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_binding(&self.binding)?;
        if self.nli_candidates.len() > MAX_NLI_CANDIDATES
            || self.nli_adjudications.len() != self.nli_candidates.len()
            || self.promotion_count != 0
        {
            return Err("NLI counts or candidate-only invariant are invalid");
        }
        validate_nli(&self.nli_candidates, &self.nli_adjudications)
    }
}

fn validate_binding(binding: &DocumentAnalysisBinding) -> Result<(), &'static str> {
    if binding.source_document_id.is_empty()
        || binding.source_document_id.len() > MAX_ID_BYTES
        || binding.native_document_id == 0
        || binding.document_revision == 0
        || binding.analysis_generation == 0
        || binding.target_registry_revision != binding.source_registry_revision.saturating_add(1)
        || binding.producer_binary_hash == [0; 32]
    {
        return Err("analysis authority binding is incomplete");
    }
    for model in [&binding.chunker, &binding.dynamic_ner, &binding.nli] {
        if model.model_id.is_empty()
            || model.model_id.len() > MAX_ID_BYTES
            || model.runtime_id.is_empty()
            || model.runtime_id.len() > MAX_ID_BYTES
            || model.artifact_hash == [0; 32]
            || model.config_hash == [0; 32]
        {
            return Err("analysis model identity is incomplete");
        }
    }
    Ok(())
}

fn validate_request_binding(binding: &DocumentAnalysisRequestBinding) -> Result<(), &'static str> {
    if binding.source_document_id.is_empty()
        || binding.source_document_id.len() > MAX_ID_BYTES
        || binding.native_document_id == 0
        || binding.document_revision == 0
        || binding.analysis_generation == 0
        || binding.target_registry_revision != binding.source_registry_revision.saturating_add(1)
    {
        return Err("analysis request binding is incomplete");
    }
    Ok(())
}

fn validate_entities(entities: &[AnalysisEntity]) -> Result<(), &'static str> {
    let mut ids = std::collections::BTreeSet::new();
    for entity in entities {
        if entity.stable_id == 0
            || !ids.insert(entity.stable_id)
            || entity.label.trim().is_empty()
            || entity.label.len() > MAX_LABEL_BYTES
            || entity.mention_count == 0
        {
            return Err("invalid or duplicate analysis entity");
        }
        if entity.kind == AnalysisEntityKind::Custom
            && entity.custom_kind.as_deref().is_none_or(str::is_empty)
        {
            return Err("custom entity is missing its kind");
        }
    }
    Ok(())
}

fn validate_mentions(
    entities: &[AnalysisEntity],
    mentions: &[AnalysisMention],
) -> Result<(), &'static str> {
    let entity_ids = entities
        .iter()
        .map(|entity| entity.stable_id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut mention_ids = std::collections::BTreeSet::new();
    for mention in mentions {
        if mention.mention_id == 0
            || !mention_ids.insert(mention.mention_id)
            || !entity_ids.contains(&mention.entity_id)
            || mention.start >= mention.end
            || !mention.confidence.is_finite()
            || !(0.0..=1.0).contains(&mention.confidence)
        {
            return Err("invalid or duplicate analysis mention");
        }
    }
    Ok(())
}

fn validate_nli(
    candidates: &[NliCandidate],
    adjudications: &[NliAdjudication],
) -> Result<(), &'static str> {
    let mut ids = std::collections::BTreeSet::new();
    for candidate in candidates {
        if candidate.candidate_id == [0; 32]
            || !ids.insert(candidate.candidate_id)
            || candidate.left_entity_id == 0
            || candidate.right_entity_id == 0
            || candidate.premise_start >= candidate.premise_end
            || candidate.premise.is_empty()
            || candidate.premise.len() > MAX_TEXT_BYTES
            || candidate.hypothesis.is_empty()
            || candidate.hypothesis.len() > MAX_LABEL_BYTES
        {
            return Err("invalid or duplicate NLI candidate");
        }
    }
    if adjudications
        .iter()
        .zip(candidates)
        .any(|(adjudication, candidate)| {
            adjudication.candidate_id != candidate.candidate_id
                || adjudication.entailment_millis > 1_000
                || adjudication.contradiction_millis > 1_000
                || adjudication.neutral_millis > 1_000
                || adjudication.confidence_millis > 1_000
        })
    {
        return Err("NLI adjudication is invalid or out of order");
    }
    Ok(())
}
