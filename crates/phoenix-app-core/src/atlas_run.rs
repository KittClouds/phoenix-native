use super::*;
use memmap2::Mmap;
use phoenix_analysis_contract::{AnalysisModelIdentity, NliCandidateKind, PhoenixNliArtifactV1};
use phoenix_scene_product_index::{PhoenixSceneProductIndexV1, ReviewState};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod error;
pub use error::AtlasRunReceiptError;

pub const ATLAS_RUN_RECEIPT_CONTRACT: &str = "phoenix.native.atlas-run-receipt/v1";
const AUTHORITY_DIRECTORY: &str = "atlas-run-authority-v1";
const RECEIPT_EXTENSION: &str = "phxar";
const MAGIC: [u8; 8] = *b"PHXATR01";
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AtlasCapabilityState {
    Produced,
    Unsupported,
    NotRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasCapabilityCount {
    pub state: AtlasCapabilityState,
    pub count: Option<u64>,
}

impl AtlasCapabilityCount {
    pub const fn produced(count: u64) -> Self {
        Self {
            state: AtlasCapabilityState::Produced,
            count: Some(count),
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            state: AtlasCapabilityState::Unsupported,
            count: None,
        }
    }

    pub const fn not_run() -> Self {
        Self {
            state: AtlasCapabilityState::NotRun,
            count: None,
        }
    }

    fn validate(self) -> Result<(), AtlasRunReceiptError> {
        match (self.state, self.count) {
            (AtlasCapabilityState::Produced, Some(_))
            | (AtlasCapabilityState::Unsupported | AtlasCapabilityState::NotRun, None) => Ok(()),
            _ => Err(AtlasRunReceiptError::Invalid(
                "capability state and count disagree",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AtlasWorkDisposition {
    Computed,
    ReusedResident,
    ReusedDurable,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AtlasSpanKind {
    Pipeline,
    Analysis,
    Chunker,
    DynamicNer,
    NliLoad,
    NliAdjudication,
    Compiler,
    Publisher,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasSpanReceipt {
    pub trace_id: u64,
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub kind: AtlasSpanKind,
    pub elapsed_micros: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasModelIdentity {
    pub model_id: String,
    pub artifact_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub runtime_id: String,
}

impl From<&AnalysisModelIdentity> for AtlasModelIdentity {
    fn from(identity: &AnalysisModelIdentity) -> Self {
        Self {
            model_id: identity.model_id.clone(),
            artifact_hash: identity.artifact_hash,
            config_hash: identity.config_hash,
            runtime_id: identity.runtime_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasProducerIdentities {
    pub producer_binary_hash: Option<[u8; 32]>,
    pub chunker: Option<AtlasModelIdentity>,
    pub dynamic_ner: Option<AtlasModelIdentity>,
    pub nli: Option<AtlasModelIdentity>,
    pub compiler_contract: String,
    pub publisher_contract: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasAuthoritySnapshotV1 {
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub registry_revision: u64,
    pub analysis_generation: Option<u64>,
    pub previous_generation: Option<u64>,
    pub published_generation: u64,
    pub archive_cohort_hash: [u8; 32],
    pub product_index_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasResourceCounts {
    pub documents: u64,
    pub analysis_chunks: AtlasCapabilityCount,
    pub scene_chunks: u64,
    pub sentences: AtlasCapabilityCount,
    pub analysis_entities: AtlasCapabilityCount,
    pub canonical_entities: u64,
    pub analysis_mentions: AtlasCapabilityCount,
    pub resident_verified_anchors: u64,
    pub graph_nodes: u64,
    pub graph_edges: u64,
    pub nli_candidates: AtlasCapabilityCount,
    pub nli_adjudications: AtlasCapabilityCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasSemanticCounts {
    pub identity_candidates: AtlasCapabilityCount,
    pub generic_related_candidates: AtlasCapabilityCount,
    pub temporal_candidates: AtlasCapabilityCount,
    pub causal_candidates: AtlasCapabilityCount,
    pub memory_state_candidates: AtlasCapabilityCount,
    pub event_candidates: AtlasCapabilityCount,
    pub contextual_cooccurrence_candidates: AtlasCapabilityCount,
    pub promotions: AtlasCapabilityCount,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasGraphReviewCounts {
    pub accepted_edges: u64,
    pub proposed_edges: u64,
    pub rejected_edges: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasDecisionCounts {
    pub accepted: AtlasCapabilityCount,
    pub rejected: AtlasCapabilityCount,
    pub deferred: AtlasCapabilityCount,
}

impl Default for AtlasDecisionCounts {
    fn default() -> Self {
        Self {
            accepted: AtlasCapabilityCount::unsupported(),
            rejected: AtlasCapabilityCount::unsupported(),
            deferred: AtlasCapabilityCount::unsupported(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasTimingSnapshot {
    pub total_micros: u64,
    pub analysis_total_micros: AtlasCapabilityCount,
    pub chunker_micros: AtlasCapabilityCount,
    pub dynamic_ner_micros: AtlasCapabilityCount,
    pub nli_load_micros: AtlasCapabilityCount,
    pub nli_adjudication_micros: AtlasCapabilityCount,
    pub compiler_micros: u64,
    pub publisher_micros: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasReuseSnapshot {
    pub source: AtlasWorkDisposition,
    pub analysis: AtlasWorkDisposition,
    pub compiler: AtlasWorkDisposition,
    pub publisher: AtlasWorkDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasQueueSnapshot {
    pub command_capacity: u64,
    pub event_capacity: u64,
    pub command_high_water_before: u64,
    pub command_high_water_after: u64,
    pub event_high_water_before: u64,
    pub event_high_water_after: u64,
    pub commands_pending_before: u64,
    pub commands_pending_after: u64,
    pub events_pending_before: u64,
    pub events_pending_after: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasRunReceiptV1 {
    pub contract: String,
    pub run_id: u64,
    pub authority: AtlasAuthoritySnapshotV1,
    pub producers: AtlasProducerIdentities,
    pub spans: Vec<AtlasSpanReceipt>,
    pub resources: AtlasResourceCounts,
    pub semantics: AtlasSemanticCounts,
    pub graph_reviews: AtlasGraphReviewCounts,
    pub decisions: AtlasDecisionCounts,
    pub timings: AtlasTimingSnapshot,
    pub reuse: AtlasReuseSnapshot,
    pub queues: AtlasQueueSnapshot,
}

impl AtlasRunReceiptV1 {
    pub fn validate(&self) -> Result<(), AtlasRunReceiptError> {
        if self.contract != ATLAS_RUN_RECEIPT_CONTRACT
            || self.run_id == 0
            || self.authority.document_id == 0
            || self.authority.document_revision == 0
            || self.authority.content_hash == [0; 32]
            || self.authority.published_generation == 0
            || self.authority.archive_cohort_hash == [0; 32]
            || self.authority.product_index_hash == [0; 32]
            || self.resources.documents != 1
            || self.resources.graph_nodes == 0
        {
            return Err(AtlasRunReceiptError::Invalid(
                "run authority or required resource counts are incomplete",
            ));
        }
        if self.producers.compiler_contract.is_empty()
            || self.producers.publisher_contract.is_empty()
        {
            return Err(AtlasRunReceiptError::Invalid(
                "compiler or publisher identity is missing",
            ));
        }
        self.validate_analysis_authority()?;
        if self
            .authority
            .previous_generation
            .is_some_and(|generation| generation >= self.authority.published_generation)
        {
            return Err(AtlasRunReceiptError::Invalid(
                "generation lineage is not strictly increasing",
            ));
        }
        for capability in [
            self.resources.analysis_chunks,
            self.resources.sentences,
            self.resources.analysis_mentions,
            self.resources.analysis_entities,
            self.resources.nli_candidates,
            self.resources.nli_adjudications,
            self.semantics.identity_candidates,
            self.semantics.generic_related_candidates,
            self.semantics.temporal_candidates,
            self.semantics.causal_candidates,
            self.semantics.memory_state_candidates,
            self.semantics.event_candidates,
            self.semantics.contextual_cooccurrence_candidates,
            self.semantics.promotions,
            self.decisions.accepted,
            self.decisions.rejected,
            self.decisions.deferred,
            self.timings.analysis_total_micros,
            self.timings.chunker_micros,
            self.timings.dynamic_ner_micros,
            self.timings.nli_load_micros,
            self.timings.nli_adjudication_micros,
        ] {
            capability.validate()?;
        }
        if self.graph_reviews.accepted_edges
            + self.graph_reviews.proposed_edges
            + self.graph_reviews.rejected_edges
            > self.resources.graph_edges
        {
            return Err(AtlasRunReceiptError::Invalid(
                "graph edge review counts exceed graph edges",
            ));
        }
        if let (Some(total), Some(identity), Some(generic_related)) = (
            self.resources.nli_candidates.count,
            self.semantics.identity_candidates.count,
            self.semantics.generic_related_candidates.count,
        ) {
            if identity + generic_related != total {
                return Err(AtlasRunReceiptError::Invalid(
                    "semantic candidate families do not explain the NLI total",
                ));
            }
        }
        if self.resources.nli_candidates != self.resources.nli_adjudications {
            return Err(AtlasRunReceiptError::Invalid(
                "NLI candidates and adjudications do not describe one complete candidate-only run",
            ));
        }
        if self.queues.command_high_water_after < self.queues.command_high_water_before
            || self.queues.event_high_water_after < self.queues.event_high_water_before
            || self.queues.command_high_water_after > self.queues.command_capacity
            || self.queues.event_high_water_after > self.queues.event_capacity
            || self.queues.commands_pending_before > self.queues.command_capacity
            || self.queues.commands_pending_after > self.queues.command_capacity
            || self.queues.events_pending_before > self.queues.event_capacity
            || self.queues.events_pending_after > self.queues.event_capacity
        {
            return Err(AtlasRunReceiptError::Invalid(
                "queue counters exceed their bounded capacities or regress",
            ));
        }
        self.validate_spans()
    }

    fn validate_analysis_authority(&self) -> Result<(), AtlasRunReceiptError> {
        let identities = [
            self.producers.chunker.as_ref(),
            self.producers.dynamic_ner.as_ref(),
            self.producers.nli.as_ref(),
        ];
        let all_present = identities.iter().all(|identity| identity.is_some());
        let all_missing = identities.iter().all(|identity| identity.is_none());
        match (
            self.authority.analysis_generation,
            self.producers.producer_binary_hash,
            all_present,
            all_missing,
        ) {
            (Some(generation), Some(hash), true, false) if generation != 0 && hash != [0; 32] => {}
            (None, None, false, true) => {}
            _ => {
                return Err(AtlasRunReceiptError::Invalid(
                    "analysis generation and producer identities are incomplete",
                ));
            }
        }
        for identity in identities.into_iter().flatten() {
            if identity.model_id.is_empty()
                || identity.runtime_id.is_empty()
                || identity.artifact_hash == [0; 32]
                || identity.config_hash == [0; 32]
            {
                return Err(AtlasRunReceiptError::Invalid(
                    "analysis model identity is incomplete",
                ));
            }
        }
        Ok(())
    }

    fn validate_spans(&self) -> Result<(), AtlasRunReceiptError> {
        if self.spans.is_empty() || self.spans[0].kind != AtlasSpanKind::Pipeline {
            return Err(AtlasRunReceiptError::Invalid(
                "pipeline root span is missing",
            ));
        }
        let mut ids = hashbrown::HashSet::with_capacity(self.spans.len());
        for span in &self.spans {
            if span.trace_id != self.run_id || span.span_id == 0 || !ids.insert(span.span_id) {
                return Err(AtlasRunReceiptError::Invalid(
                    "span trace or identity is invalid",
                ));
            }
        }
        if self.spans[0].parent_span_id.is_some()
            || self.spans.iter().skip(1).any(|span| {
                span.parent_span_id
                    .is_none_or(|parent| !ids.contains(&parent))
            })
        {
            return Err(AtlasRunReceiptError::Invalid(
                "span parent lineage is invalid",
            ));
        }
        Ok(())
    }

    fn matches(&self, publication: ScenePublicationReceipt, lease: Option<&DocumentLease>) -> bool {
        let authority = self.authority;
        authority.published_generation == publication.generation_id
            && authority.registry_revision == publication.registry_revision
            && Some(authority.document_id) == publication.document_id
            && authority.archive_cohort_hash == publication.archive_cohort_hash
            && authority.product_index_hash == publication.product_index_hash
            && self.resources.canonical_entities == publication.entity_count
            && self.resources.graph_nodes == publication.node_count
            && self.resources.graph_edges == publication.edge_count
            && lease.is_none_or(|lease| {
                authority.document_id == lease.entry_id.0
                    && authority.document_revision == lease.revision.0
                    && authority.content_hash == lease.content_hash.0
            })
    }
}

pub(super) struct CompletedRunInput<'a> {
    pub run_id: u64,
    pub previous_generation: Option<u64>,
    pub analysis: Option<AnalysisPublicationReceipt>,
    pub analysis_total_micros: u64,
    pub graph: GraphRebuildReceipt,
    pub publisher_micros: u64,
    pub total_micros: u64,
    pub metrics_before: KernelMetrics,
    pub metrics_after: KernelMetrics,
    pub nli: Option<&'a PhoenixNliArtifactV1>,
    pub product_index: &'a PhoenixSceneProductIndexV1,
}

pub(super) fn completed_receipt(
    input: CompletedRunInput<'_>,
) -> Result<AtlasRunReceiptV1, AtlasRunReceiptError> {
    let compile = input.graph.compile;
    let publication = input.graph.publication;
    let analysis_binding = input.nli.map(|artifact| &artifact.binding);
    let stages = input.analysis.map(|receipt| receipt.stages);
    let (nli_identity_candidates, generic_related_candidates) =
        input.nli.map(candidate_family_counts).unwrap_or_default();
    let analyzed = input.analysis.is_some();
    let capability = |value| {
        if analyzed {
            AtlasCapabilityCount::produced(value)
        } else {
            AtlasCapabilityCount::unsupported()
        }
    };
    let spans = build_spans(
        input.run_id,
        input.total_micros,
        input.analysis_total_micros,
        stages,
        compile.compile_micros,
        input.publisher_micros,
    );
    let receipt = AtlasRunReceiptV1 {
        contract: ATLAS_RUN_RECEIPT_CONTRACT.to_owned(),
        run_id: input.run_id,
        authority: AtlasAuthoritySnapshotV1 {
            document_id: compile.document_id,
            document_revision: compile.document_revision,
            content_hash: compile.content_hash,
            registry_revision: compile.registry_revision,
            analysis_generation: input.analysis.map(|receipt| receipt.analysis_generation),
            previous_generation: input.previous_generation,
            published_generation: publication.generation_id,
            archive_cohort_hash: publication.archive_cohort_hash,
            product_index_hash: publication.product_index_hash,
        },
        producers: AtlasProducerIdentities {
            producer_binary_hash: analysis_binding.map(|binding| binding.producer_binary_hash),
            chunker: analysis_binding.map(|binding| (&binding.chunker).into()),
            dynamic_ner: analysis_binding.map(|binding| (&binding.dynamic_ner).into()),
            nli: analysis_binding.map(|binding| (&binding.nli).into()),
            compiler_contract: NATIVE_SCENE_COMPILER_CONTRACT.to_owned(),
            publisher_contract: SCENE_PUBLISHER_CONTRACT.to_owned(),
        },
        spans,
        resources: AtlasResourceCounts {
            documents: 1,
            analysis_chunks: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.chunk_count.into()))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            scene_chunks: compile.chunk_count,
            sentences: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.sentence_count.into()))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            analysis_entities: input
                .analysis
                .map(|receipt| AtlasCapabilityCount::produced(receipt.entity_count.into()))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            canonical_entities: publication.entity_count,
            analysis_mentions: input
                .analysis
                .map(|receipt| AtlasCapabilityCount::produced(receipt.mention_count.into()))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            resident_verified_anchors: compile.verified_mentions,
            graph_nodes: publication.node_count,
            graph_edges: publication.edge_count,
            nli_candidates: input
                .analysis
                .map(|receipt| AtlasCapabilityCount::produced(receipt.nli_candidate_count.into()))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            nli_adjudications: input
                .analysis
                .map(|receipt| {
                    AtlasCapabilityCount::produced(receipt.nli_adjudication_count.into())
                })
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
        },
        semantics: AtlasSemanticCounts {
            // This V1 receipt field is the identity-family partition of the
            // NLI candidate run. Exact packed identity-page cardinality lives
            // in `NativeSceneCompileReceiptV2`; mixing the two authorities
            // makes the NLI family invariant unsatisfiable as soon as the
            // native producer emits additional identity proposals.
            identity_candidates: capability(nli_identity_candidates),
            generic_related_candidates: capability(generic_related_candidates),
            temporal_candidates: AtlasCapabilityCount::produced(compile.temporal_candidate_count),
            causal_candidates: AtlasCapabilityCount::produced(compile.causal_candidate_count),
            memory_state_candidates: AtlasCapabilityCount::produced(
                compile.memory_state_candidate_count,
            ),
            event_candidates: AtlasCapabilityCount::produced(compile.event_candidate_count),
            contextual_cooccurrence_candidates: AtlasCapabilityCount::produced(
                compile.contextual_evidence_count,
            ),
            promotions: AtlasCapabilityCount::unsupported(),
        },
        graph_reviews: graph_review_counts(input.product_index),
        decisions: AtlasDecisionCounts::default(),
        timings: AtlasTimingSnapshot {
            total_micros: input.total_micros,
            analysis_total_micros: capability(input.analysis_total_micros),
            chunker_micros: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.chunker_micros))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            dynamic_ner_micros: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.dynamic_ner_micros))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            nli_load_micros: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.nli_load_micros))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            nli_adjudication_micros: stages
                .map(|receipt| AtlasCapabilityCount::produced(receipt.nli_adjudication_micros))
                .unwrap_or_else(AtlasCapabilityCount::unsupported),
            compiler_micros: compile.compile_micros,
            publisher_micros: input.publisher_micros,
        },
        reuse: AtlasReuseSnapshot {
            source: AtlasWorkDisposition::ReusedResident,
            analysis: if analyzed {
                AtlasWorkDisposition::Computed
            } else {
                AtlasWorkDisposition::Unsupported
            },
            compiler: AtlasWorkDisposition::Computed,
            publisher: AtlasWorkDisposition::Computed,
        },
        queues: AtlasQueueSnapshot {
            command_capacity: COMMAND_CAPACITY as u64,
            event_capacity: EVENT_CAPACITY as u64,
            command_high_water_before: input.metrics_before.command_queue_high_water,
            command_high_water_after: input.metrics_after.command_queue_high_water,
            event_high_water_before: input.metrics_before.event_queue_high_water,
            event_high_water_after: input.metrics_after.event_queue_high_water,
            commands_pending_before: input.metrics_before.commands_pending,
            commands_pending_after: input.metrics_after.commands_pending,
            events_pending_before: input.metrics_before.events_pending,
            events_pending_after: input.metrics_after.events_pending,
        },
    };
    receipt.validate()?;
    Ok(receipt)
}

fn candidate_family_counts(artifact: &PhoenixNliArtifactV1) -> (u64, u64) {
    artifact
        .nli_candidates
        .iter()
        .fold(
            (0, 0),
            |(identity, relationship), candidate| match candidate.kind {
                NliCandidateKind::SameSurface
                | NliCandidateKind::Alias
                | NliCandidateKind::Coreference => (identity + 1, relationship),
                NliCandidateKind::Related => (identity, relationship + 1),
            },
        )
}

pub(super) fn graph_review_counts(index: &PhoenixSceneProductIndexV1) -> AtlasGraphReviewCounts {
    let mut counts = AtlasGraphReviewCounts::default();
    for edge in index.edges() {
        counts.accepted_edges += u64::from(edge.review_mask & ReviewState::Accepted as u32 != 0);
        counts.proposed_edges += u64::from(edge.review_mask & ReviewState::Proposed as u32 != 0);
        counts.rejected_edges += u64::from(edge.review_mask & ReviewState::Rejected as u32 != 0);
    }
    counts
}

fn build_spans(
    run_id: u64,
    total_micros: u64,
    analysis_total_micros: u64,
    stages: Option<phoenix_analysis_contract::AnalysisStageReceipt>,
    compiler_micros: u64,
    publisher_micros: u64,
) -> Vec<AtlasSpanReceipt> {
    let mut spans = Vec::with_capacity(if stages.is_some() { 8 } else { 3 });
    spans.push(span(run_id, 1, None, AtlasSpanKind::Pipeline, total_micros));
    if let Some(stages) = stages {
        spans.push(span(
            run_id,
            2,
            Some(1),
            AtlasSpanKind::Analysis,
            analysis_total_micros,
        ));
        spans.push(span(
            run_id,
            3,
            Some(2),
            AtlasSpanKind::Chunker,
            stages.chunker_micros,
        ));
        spans.push(span(
            run_id,
            4,
            Some(2),
            AtlasSpanKind::DynamicNer,
            stages.dynamic_ner_micros,
        ));
        spans.push(span(
            run_id,
            5,
            Some(2),
            AtlasSpanKind::NliLoad,
            stages.nli_load_micros,
        ));
        spans.push(span(
            run_id,
            6,
            Some(2),
            AtlasSpanKind::NliAdjudication,
            stages.nli_adjudication_micros,
        ));
    }
    spans.push(span(
        run_id,
        7,
        Some(1),
        AtlasSpanKind::Compiler,
        compiler_micros,
    ));
    spans.push(span(
        run_id,
        8,
        Some(1),
        AtlasSpanKind::Publisher,
        publisher_micros,
    ));
    spans
}

const fn span(
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    kind: AtlasSpanKind,
    elapsed_micros: u64,
) -> AtlasSpanReceipt {
    AtlasSpanReceipt {
        trace_id,
        span_id,
        parent_span_id,
        kind,
        elapsed_micros,
    }
}

pub(super) fn persist(
    workspace_path: &Path,
    receipt: &AtlasRunReceiptV1,
) -> Result<[u8; 32], AtlasRunReceiptError> {
    receipt.validate()?;
    let path = authority_receipt_path(workspace_path, &receipt.authority)?;
    if path.exists() {
        let (hash, existing) = open(&path)?;
        if existing == *receipt {
            return Ok(hash);
        }
        return Err(AtlasRunReceiptError::AuthorityMismatch);
    }
    let payload = postcard::to_allocvec(receipt)
        .map_err(|error| AtlasRunReceiptError::Codec(error.to_string()))?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(AtlasRunReceiptError::Oversized(payload.len()));
    }
    let payload_hash = *blake3::hash(&payload).as_bytes();
    let mut header = [0_u8; HEADER_LEN];
    header[..8].copy_from_slice(&MAGIC);
    header[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    header[24..56].copy_from_slice(&payload_hash);
    let parent = path.parent().ok_or_else(|| AtlasRunReceiptError::Io {
        path: path.clone(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "receipt path has no parent"),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| AtlasRunReceiptError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let pending = path.with_extension(format!(
        "{RECEIPT_EXTENSION}.pending-{}",
        std::process::id()
    ));
    if pending.exists() {
        std::fs::remove_file(&pending).map_err(|source| AtlasRunReceiptError::Io {
            path: pending.clone(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|source| AtlasRunReceiptError::Io {
            path: pending.clone(),
            source,
        })?;
    file.write_all(&header)
        .and_then(|_| file.write_all(&payload))
        .and_then(|_| file.sync_all())
        .map_err(|source| AtlasRunReceiptError::Io {
            path: pending.clone(),
            source,
        })?;
    std::fs::rename(&pending, &path).map_err(|source| AtlasRunReceiptError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(payload_hash)
}

pub(super) fn restore_matching(
    workspace_path: &Path,
    publication: Option<ScenePublicationReceipt>,
    lease: Option<&DocumentLease>,
) -> Result<Option<([u8; 32], AtlasRunReceiptV1)>, AtlasRunReceiptError> {
    let Some(publication) = publication else {
        return Ok(None);
    };
    let keyed_path = publication_receipt_path(workspace_path, publication)?;
    if keyed_path.exists() {
        let (hash, receipt) = open(&keyed_path)?;
        return if receipt.matches(publication, lease) {
            Ok(Some((hash, receipt)))
        } else {
            Ok(None)
        };
    }
    let legacy_path = legacy_receipt_path(workspace_path, publication.generation_id)?;
    if !legacy_path.exists() {
        return Ok(None);
    }
    let (hash, receipt) = open(&legacy_path)?;
    if !receipt.matches(publication, lease) {
        return Ok(None);
    }
    Ok(Some((hash, receipt)))
}

fn authority_receipt_path(
    workspace_path: &Path,
    authority: &AtlasAuthoritySnapshotV1,
) -> Result<PathBuf, AtlasRunReceiptError> {
    receipt_directory(workspace_path).map(|directory| {
        directory.join(format!(
            "generation-{:020}-{}-{}.{}",
            authority.published_generation,
            hash_prefix(authority.archive_cohort_hash),
            hash_prefix(authority.product_index_hash),
            RECEIPT_EXTENSION,
        ))
    })
}

fn publication_receipt_path(
    workspace_path: &Path,
    publication: ScenePublicationReceipt,
) -> Result<PathBuf, AtlasRunReceiptError> {
    receipt_directory(workspace_path).map(|directory| {
        directory.join(format!(
            "generation-{:020}-{}-{}.{}",
            publication.generation_id,
            hash_prefix(publication.archive_cohort_hash),
            hash_prefix(publication.product_index_hash),
            RECEIPT_EXTENSION,
        ))
    })
}

fn legacy_receipt_path(
    workspace_path: &Path,
    generation: u64,
) -> Result<PathBuf, AtlasRunReceiptError> {
    receipt_directory(workspace_path)
        .map(|directory| directory.join(format!("generation-{generation:020}.{RECEIPT_EXTENSION}")))
}

fn receipt_directory(workspace_path: &Path) -> Result<PathBuf, AtlasRunReceiptError> {
    let parent = workspace_path
        .parent()
        .ok_or(AtlasRunReceiptError::Invalid(
            "workspace path has no parent",
        ))?;
    Ok(parent.join(AUTHORITY_DIRECTORY))
}

fn hash_prefix(hash: [u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(16);
    for byte in hash.iter().take(8) {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod receipt_path_tests {
    use super::*;

    fn authority(archive: u8, index: u8) -> AtlasAuthoritySnapshotV1 {
        AtlasAuthoritySnapshotV1 {
            document_id: 7,
            document_revision: 1,
            content_hash: [3; 32],
            registry_revision: 4,
            analysis_generation: Some(4),
            previous_generation: Some(3),
            published_generation: 4,
            archive_cohort_hash: [archive; 32],
            product_index_hash: [index; 32],
        }
    }

    #[test]
    fn receipt_path_is_bound_to_both_published_artifacts() {
        let workspace = Path::new(r"C:\Phoenix\workspace-v1.json");
        let first = authority_receipt_path(workspace, &authority(1, 2)).unwrap();
        let changed_archive = authority_receipt_path(workspace, &authority(3, 2)).unwrap();
        let changed_index = authority_receipt_path(workspace, &authority(1, 4)).unwrap();
        let legacy = legacy_receipt_path(workspace, 4).unwrap();

        assert_ne!(first, changed_archive);
        assert_ne!(first, changed_index);
        assert_ne!(first, legacy);
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("generation-00000000000000000004-0101010101010101-0202020202020202.phxar")
        );
    }
}

fn open(path: &Path) -> Result<([u8; 32], AtlasRunReceiptV1), AtlasRunReceiptError> {
    let file = File::open(path).map_err(|source| AtlasRunReceiptError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // SAFETY: generation receipts are immutable after their atomic rename.
    let mmap = unsafe { Mmap::map(&file) }.map_err(|source| AtlasRunReceiptError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if mmap.len() < HEADER_LEN || mmap[..8] != MAGIC {
        return Err(AtlasRunReceiptError::InvalidHeader);
    }
    if read_u32(&mmap[8..12]) != FORMAT_VERSION {
        return Err(AtlasRunReceiptError::UnsupportedVersion(read_u32(
            &mmap[8..12],
        )));
    }
    if read_u32(&mmap[12..16]) as usize != HEADER_LEN {
        return Err(AtlasRunReceiptError::InvalidHeader);
    }
    let payload_len = usize::try_from(read_u64(&mmap[16..24]))
        .map_err(|_| AtlasRunReceiptError::InvalidHeader)?;
    if payload_len > MAX_PAYLOAD_BYTES || mmap.len() != HEADER_LEN + payload_len {
        return Err(AtlasRunReceiptError::Oversized(payload_len));
    }
    let payload = &mmap[HEADER_LEN..];
    let payload_hash = *blake3::hash(payload).as_bytes();
    if mmap[24..56] != payload_hash {
        return Err(AtlasRunReceiptError::HashMismatch);
    }
    if mmap[56..HEADER_LEN].iter().any(|byte| *byte != 0) {
        return Err(AtlasRunReceiptError::InvalidHeader);
    }
    let receipt: AtlasRunReceiptV1 = postcard::from_bytes(payload)
        .map_err(|error| AtlasRunReceiptError::Codec(error.to_string()))?;
    receipt.validate()?;
    Ok((payload_hash, receipt))
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(bytes);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[derive(Serialize)]
    struct LegacyAtlasSemanticCounts {
        identity_candidates: AtlasCapabilityCount,
        relationship_candidates: AtlasCapabilityCount,
        temporal_candidates: AtlasCapabilityCount,
        causal_candidates: AtlasCapabilityCount,
        memory_state_candidates: AtlasCapabilityCount,
        event_candidates: AtlasCapabilityCount,
        story_signal_candidates: AtlasCapabilityCount,
        promotions: AtlasCapabilityCount,
    }

    #[test]
    fn corrected_semantic_names_preserve_the_v1_postcard_layout() {
        let legacy = LegacyAtlasSemanticCounts {
            identity_candidates: AtlasCapabilityCount::produced(1),
            relationship_candidates: AtlasCapabilityCount::produced(2),
            temporal_candidates: AtlasCapabilityCount::unsupported(),
            causal_candidates: AtlasCapabilityCount::unsupported(),
            memory_state_candidates: AtlasCapabilityCount::unsupported(),
            event_candidates: AtlasCapabilityCount::unsupported(),
            story_signal_candidates: AtlasCapabilityCount::produced(3),
            promotions: AtlasCapabilityCount::produced(0),
        };
        let payload = postcard::to_allocvec(&legacy).expect("serialize legacy semantic counts");
        let corrected: AtlasSemanticCounts =
            postcard::from_bytes(&payload).expect("decode corrected semantic counts");

        assert_eq!(
            corrected.generic_related_candidates,
            legacy.relationship_candidates
        );
        assert_eq!(
            corrected.contextual_cooccurrence_candidates,
            legacy.story_signal_candidates
        );
    }
}
