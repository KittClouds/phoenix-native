use super::*;
use phoenix_analysis_contract::{NliCandidateKind, NliDecision};
use phoenix_scene_publisher::ScenePublicationKind;

pub const ATLAS_CONTROL_CONTRACT: &str = "phoenix.native.atlas-control/v2";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasStage {
    Source,
    Entities,
    Connections,
    Review,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasStageState {
    Complete,
    Ready,
    Waiting,
    NeedsAttention,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasBuildState {
    WaitingForDocument,
    RuntimeUnavailable,
    Ready,
    Building,
    Published,
    VerificationRequired,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasPrimaryAction {
    OpenDocument,
    ConfigurePipeline,
    RunPipeline,
    Wait,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasAnalysisSummary {
    pub analysis_generation: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub registry_revision: u64,
    pub analysis_artifact_hash: [u8; 32],
    pub nli_artifact_hash: [u8; 32],
    pub producer_coordinator_hash: [u8; 32],
    pub entity_count: u32,
    pub mention_count: u32,
    pub nli_candidate_count: u32,
    pub nli_adjudication_count: u32,
    pub promotion_count: u32,
    pub dynamic_ner_model: Arc<str>,
    pub dynamic_ner_runtime: Arc<str>,
    pub nli_model: Arc<str>,
    pub nli_runtime: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasStageSummary {
    pub stage: AtlasStage,
    pub state: AtlasStageState,
    pub value: u64,
    pub detail: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasControlSnapshot {
    pub contract: &'static str,
    pub kernel_revision: u64,
    pub build_state: AtlasBuildState,
    pub primary_action: AtlasPrimaryAction,
    pub headline: Arc<str>,
    pub guidance: Arc<str>,
    pub document_id: Option<u64>,
    pub document_revision: Option<u64>,
    pub content_hash: Option<[u8; 32]>,
    pub content_bytes: u64,
    pub registry_revision: u64,
    pub canonical_entities: u64,
    pub resident_anchor_count: u64,
    pub user_entities: u64,
    pub ner_entities: u64,
    pub analysis_runtime: AnalysisRuntimeInfo,
    pub analysis: Option<AtlasAnalysisSummary>,
    pub generation_id: Option<u64>,
    pub node_count: u64,
    pub edge_count: u64,
    pub graph_reviews: AtlasGraphReviewCounts,
    pub decisions: AtlasDecisionCounts,
    pub stages: [AtlasStageSummary; 5],
    pub last_run: Option<AtlasRunReceiptV1>,
    pub last_run_hash: Option<[u8; 32]>,
    pub last_run_restored: bool,
    pub memory: AtlasMemorySummary,
    pub last_error: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AtlasMemorySummary {
    pub generation_hash: Option<[u8; 32]>,
    pub source_count: u64,
    pub document_count: u64,
    pub conversation_count: u64,
    pub turn_count: u64,
    pub proposed_candidates: u64,
    pub accepted_candidates: u64,
    pub rejected_candidates: u64,
    pub deferred_candidates: u64,
    pub supported_producers: u64,
    pub unsupported_producers: u64,
    pub indexed_items: u64,
    pub pending_document_count: u64,
    pub active_scope_hash: [u8; 32],
    pub queue_high_water: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasReviewCandidateState {
    Open,
    Accepted,
    Rejected,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasDecisionApplicability {
    None,
    Current,
    Preserved,
    Superseded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasReviewCandidateSummary {
    pub candidate_id: AtlasCandidateId,
    pub left_entity_id: u64,
    pub right_entity_id: u64,
    pub left_label: Arc<str>,
    pub right_label: Arc<str>,
    pub kind: Arc<str>,
    pub premise: Arc<str>,
    pub hypothesis: Arc<str>,
    pub adjudication: Arc<str>,
    pub confidence_millis: u32,
    pub state: AtlasReviewCandidateState,
    pub applicability: AtlasDecisionApplicability,
    pub evidence_start: u32,
    pub evidence_end: u32,
    pub expected_receipt_id: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasReviewSnapshot {
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub registry_revision: u64,
    pub analysis_generation: u64,
    pub candidates: Arc<[AtlasReviewCandidateSummary]>,
}

#[derive(Debug, Default)]
pub(super) struct GraphBuildRuntime {
    next_run_id: u64,
    active_run: Option<u64>,
    last_run: Option<AtlasRunReceiptV1>,
    last_run_hash: Option<[u8; 32]>,
    last_run_restored: bool,
    cancelled: bool,
    last_error: Option<Arc<str>>,
}

impl GraphBuildRuntime {
    pub(super) fn restored(restored: Option<([u8; 32], AtlasRunReceiptV1)>) -> Self {
        match restored {
            Some((hash, receipt)) => Self {
                next_run_id: receipt.run_id,
                active_run: None,
                last_run: Some(receipt),
                last_run_hash: Some(hash),
                last_run_restored: true,
                cancelled: false,
                last_error: None,
            },
            None => Self::default(),
        }
    }

    fn begin(&mut self) -> Result<u64, KernelError> {
        if self.active_run.is_some() {
            return Err(KernelError::GraphBuildAlreadyRunning);
        }
        self.next_run_id = self
            .next_run_id
            .checked_add(1)
            .ok_or(KernelError::CoordinatorUnavailable)?;
        self.active_run = Some(self.next_run_id);
        self.cancelled = false;
        self.last_error = None;
        Ok(self.next_run_id)
    }

    fn finish(
        &mut self,
        run_id: u64,
        result: &Result<CommandReceipt, KernelError>,
        durable_run: Option<([u8; 32], AtlasRunReceiptV1)>,
    ) -> Result<(), KernelError> {
        if self.active_run != Some(run_id) {
            return Err(KernelError::GraphBuildRunMismatch);
        }
        self.active_run = None;
        match result {
            Ok(CommandReceipt {
                outcome: KernelOutcome::GraphRebuilt(receipt),
                ..
            }) => match durable_run {
                Some((hash, run))
                    if run.run_id == run_id
                        && receipt.run_id == run_id
                        && run.authority.published_generation
                            == receipt.publication.generation_id =>
                {
                    self.last_run = Some(run);
                    self.last_run_hash = Some(hash);
                    self.last_run_restored = false;
                    self.last_error = None;
                }
                _ => {
                    self.last_error = Some(Arc::from(
                        "durable Atlas run receipt is missing or mismatched",
                    ));
                }
            },
            Ok(_) => {
                self.last_error = Some(Arc::from("graph rebuild receipt contract mismatch"));
            }
            Err(error) => {
                if matches!(error, KernelError::AnalysisProducerCancelled) {
                    self.cancelled = true;
                    self.last_error = None;
                } else {
                    self.last_error = Some(Arc::from(error.to_string()));
                }
            }
        }
        Ok(())
    }
}

impl PhoenixKernel {
    pub fn atlas_control_snapshot(&self) -> Result<AtlasControlSnapshot, KernelError> {
        let state = read_state(&self.shared)?;
        let runtime = self
            .shared
            .graph_build
            .lock()
            .map_err(|_| KernelError::Poisoned("graph build runtime"))?;
        let lease = state.active_document_lease.as_deref();
        let document_id = lease.map(|document| document.entry_id.0);
        let document_revision = lease.map(|document| document.revision.0);
        let content_hash = lease.map(|document| document.content_hash.0);
        let content_bytes = lease
            .and_then(|document| u64::try_from(document.content.len()).ok())
            .unwrap_or_default();
        let resident_anchor_count = state
            .document_anchors
            .as_ref()
            .map(|anchors| anchors.anchors().len() as u64)
            .unwrap_or_default();
        let analysis_runtime = NativeProducerRuntimeConfig::runtime_info();
        let pipeline_ready = analysis_runtime.ready || cfg!(test);
        let analysis = analysis_summary(&state);
        let publication = state.scene_publication;
        let full_publication =
            publication.filter(|receipt| receipt.kind == ScenePublicationKind::Full);
        let full_for_document = full_publication.is_some_and(|receipt| {
            receipt.document_id == document_id
                && receipt.registry_revision == state.atlas_registry.registry_revision
        });
        let last_run_matches = runtime.last_run.as_ref().is_some_and(|receipt| {
            Some(receipt.authority.document_id) == document_id
                && Some(receipt.authority.document_revision) == document_revision
                && Some(receipt.authority.content_hash) == content_hash
                && receipt.authority.registry_revision == state.atlas_registry.registry_revision
                && publication.is_some_and(|published| {
                    published.generation_id == receipt.authority.published_generation
                        && published.archive_cohort_hash == receipt.authority.archive_cohort_hash
                        && published.product_index_hash == receipt.authority.product_index_hash
                })
        });
        let graph_reviews = state
            .scene_product_index
            .as_ref()
            .map(|index| atlas_run::graph_review_counts(index))
            .unwrap_or_default();
        let decisions = runtime
            .last_run
            .as_ref()
            .filter(|_| last_run_matches)
            .map(|receipt| receipt.decisions)
            .unwrap_or_default();
        let build_in_progress = runtime.active_run.is_some();
        let failed = runtime.last_error.is_some();
        let cancelled = runtime.cancelled;
        let (build_state, primary_action, headline, guidance) = decide_control_state(
            lease.is_some(),
            pipeline_ready,
            full_for_document,
            last_run_matches,
            build_in_progress,
            cancelled,
            failed,
        );
        let generation_id = publication.map(|receipt| receipt.generation_id);
        let node_count = publication.map_or(0, |receipt| receipt.node_count);
        let edge_count = publication.map_or(0, |receipt| receipt.edge_count);
        let stages = build_stages(
            lease.is_some(),
            analysis.is_some(),
            pipeline_ready,
            content_bytes,
            resident_anchor_count,
            state.atlas_registry.entities.len() as u64,
            analysis
                .as_ref()
                .map(|summary| u64::from(summary.mention_count)),
            full_for_document,
            edge_count,
            graph_reviews,
            generation_id,
            failed,
        );
        let resident_memory = self.shared.resident_memory.snapshot()?;
        Ok(AtlasControlSnapshot {
            contract: ATLAS_CONTROL_CONTRACT,
            kernel_revision: state.revision,
            build_state,
            primary_action,
            headline: Arc::from(headline),
            guidance: Arc::from(guidance),
            document_id,
            document_revision,
            content_hash,
            content_bytes,
            registry_revision: state.atlas_registry.registry_revision,
            canonical_entities: state.atlas_registry.entities.len() as u64,
            resident_anchor_count,
            user_entities: state.atlas_registry.user_tagged_source_count as u64,
            ner_entities: state.atlas_registry.ner_source_count as u64,
            analysis_runtime,
            analysis,
            generation_id,
            node_count,
            edge_count,
            graph_reviews,
            decisions,
            stages,
            last_run: runtime.last_run.clone(),
            last_run_hash: runtime.last_run_hash,
            last_run_restored: runtime.last_run_restored,
            memory: memory_summary(&resident_memory)?,
            last_error: runtime.last_error.clone(),
        })
    }

    pub fn atlas_review_snapshot(&self) -> Result<Option<AtlasReviewSnapshot>, KernelError> {
        let state = read_state(&self.shared)?;
        let Some(lease) = state.active_document_lease.as_deref() else {
            return Ok(None);
        };
        let Some(publication) = state.analysis_publication else {
            return Ok(None);
        };
        let Some(analysis) = state.nli_analysis.as_deref() else {
            return Ok(None);
        };
        if analysis.binding.native_document_id != lease.entry_id.0
            || analysis.binding.document_revision != lease.revision.0
            || analysis.binding.content_hash != lease.content_hash.0
            || analysis.binding.analysis_generation != publication.analysis_generation
            || analysis.binding.target_registry_revision != publication.registry_revision
        {
            return Ok(None);
        }

        let labels = state
            .atlas_registry
            .entities
            .iter()
            .map(|entity| (entity.stable_id, Arc::clone(&entity.label)))
            .collect::<hashbrown::HashMap<_, _>>();
        let adjudications = analysis
            .nli_adjudications
            .iter()
            .map(|item| (item.candidate_id, item))
            .collect::<hashbrown::HashMap<_, _>>();
        let ledger = self
            .shared
            .atlas_review
            .lock()
            .map_err(|_| KernelError::Poisoned("Atlas review ledger"))?;
        let mut candidates = Vec::with_capacity(analysis.nli_candidates.len());
        for candidate in &analysis.nli_candidates {
            let candidate_id = AtlasCandidateId(candidate.candidate_id);
            if state
                .review_catalog_v2
                .as_deref()
                .and_then(|catalog| {
                    catalog.get(phoenix_graph_generation_v2::CandidateId(
                        candidate.candidate_id,
                    ))
                })
                .is_none()
            {
                continue;
            }
            let binding = atlas_review::current_candidate_binding(&state, candidate_id)?;
            let head = ledger.head_for_binding(
                candidate_id,
                binding.candidate_hash,
                binding.evidence_hash,
            );
            let applicable = head.filter(|receipt| {
                receipt.authority.document_id == binding.authority.document_id
                    && receipt.authority.document_hash == binding.authority.document_hash
            });
            let applicability = if let Some(receipt) = applicable {
                if receipt.authority == binding.authority {
                    AtlasDecisionApplicability::Current
                } else {
                    AtlasDecisionApplicability::Preserved
                }
            } else if ledger.has_other_binding(candidate_id) || head.is_some() {
                AtlasDecisionApplicability::Superseded
            } else {
                AtlasDecisionApplicability::None
            };
            let adjudication = adjudications.get(&candidate.candidate_id).copied();
            candidates.push(AtlasReviewCandidateSummary {
                candidate_id,
                left_entity_id: candidate.left_entity_id,
                right_entity_id: candidate.right_entity_id,
                left_label: labels
                    .get(&candidate.left_entity_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::from(format!("#{}", candidate.left_entity_id))),
                right_label: labels
                    .get(&candidate.right_entity_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::from(format!("#{}", candidate.right_entity_id))),
                kind: Arc::from(candidate_kind_label(candidate.kind)),
                premise: Arc::from(candidate.premise.as_str()),
                hypothesis: Arc::from(candidate.hypothesis.as_str()),
                adjudication: Arc::from(
                    adjudication
                        .map(|item| adjudication_label(item.decision))
                        .unwrap_or("NOT ADJUDICATED"),
                ),
                confidence_millis: adjudication
                    .map(|item| item.confidence_millis)
                    .unwrap_or_default(),
                state: applicable
                    .and_then(|receipt| receipt.resulting_status)
                    .map(review_candidate_state)
                    .unwrap_or(AtlasReviewCandidateState::Open),
                applicability,
                evidence_start: binding.premise_start,
                evidence_end: binding.premise_end,
                expected_receipt_id: applicable.map(|receipt| receipt.receipt_id),
            });
        }
        Ok(Some(AtlasReviewSnapshot {
            document_id: lease.entry_id.0,
            document_revision: lease.revision.0,
            content_hash: lease.content_hash.0,
            registry_revision: publication.registry_revision,
            analysis_generation: publication.analysis_generation,
            candidates: candidates.into(),
        }))
    }
}

fn memory_summary(
    resident: &crate::ResidentMemorySnapshot,
) -> Result<AtlasMemorySummary, crate::ResidentMemoryError> {
    let mut summary = AtlasMemorySummary {
        indexed_items: u64::from(resident.recall_indexes.indexed_items),
        pending_document_count: u64::from(resident.pending_document_count),
        active_scope_hash: resident.active_scope.fingerprint(),
        queue_high_water: resident.commands.queue_high_water,
        ..AtlasMemorySummary::default()
    };
    let Some(publication) = resident.publication.as_ref() else {
        return Ok(summary);
    };
    summary.generation_hash = Some(publication.receipt.generation_hash);
    summary.source_count = publication.receipt.source_count;
    summary.document_count = publication.receipt.document_count;
    summary.conversation_count = publication.receipt.conversation_count;
    summary.turn_count = publication.receipt.turn_count;
    let candidates = publication
        .graph
        .typed_page::<phoenix_memory_contract::SemanticCandidateRecordV3>(
            phoenix_memory_contract::PageKindV3::SemanticCandidates,
        )
        .map_err(phoenix_memory_coordinator::CoordinatorError::from)?;
    for candidate in candidates {
        match phoenix_memory_contract::CandidateStatus::from_raw(candidate.status) {
            Some(phoenix_memory_contract::CandidateStatus::Proposed) => {
                summary.proposed_candidates += 1
            }
            Some(phoenix_memory_contract::CandidateStatus::Accepted) => {
                summary.accepted_candidates += 1
            }
            Some(phoenix_memory_contract::CandidateStatus::Rejected) => {
                summary.rejected_candidates += 1
            }
            Some(phoenix_memory_contract::CandidateStatus::Deferred) => {
                summary.deferred_candidates += 1
            }
            Some(phoenix_memory_contract::CandidateStatus::Superseded) | None => {}
        }
    }
    let capabilities = publication
        .graph
        .typed_page::<phoenix_memory_contract::ProducerCapabilityRecordV3>(
            phoenix_memory_contract::PageKindV3::ProducerCapabilitiesV3,
        )
        .map_err(phoenix_memory_coordinator::CoordinatorError::from)?;
    for capability in capabilities {
        match phoenix_memory_contract::ProducerStateV3::from_raw(capability.state) {
            Some(phoenix_memory_contract::ProducerStateV3::Unsupported) => {
                summary.unsupported_producers += 1
            }
            Some(
                phoenix_memory_contract::ProducerStateV3::Produced
                | phoenix_memory_contract::ProducerStateV3::DurableVerified,
            ) => summary.supported_producers += 1,
            Some(
                phoenix_memory_contract::ProducerStateV3::Cancelled
                | phoenix_memory_contract::ProducerStateV3::Failed,
            )
            | None => {}
        }
    }
    Ok(summary)
}

const fn candidate_kind_label(kind: NliCandidateKind) -> &'static str {
    match kind {
        NliCandidateKind::SameSurface => "SAME SURFACE",
        NliCandidateKind::Alias => "ALIAS",
        NliCandidateKind::Coreference => "COREFERENCE",
        NliCandidateKind::Related => "RELATED EVIDENCE",
    }
}

const fn adjudication_label(decision: NliDecision) -> &'static str {
    match decision {
        NliDecision::Supported => "SUPPORTED",
        NliDecision::Contradicted => "CONTRADICTED",
        NliDecision::Unknown => "UNKNOWN",
    }
}

const fn review_candidate_state(status: AtlasDecisionStatus) -> AtlasReviewCandidateState {
    match status {
        AtlasDecisionStatus::Accepted => AtlasReviewCandidateState::Accepted,
        AtlasDecisionStatus::Rejected => AtlasReviewCandidateState::Rejected,
        AtlasDecisionStatus::Deferred => AtlasReviewCandidateState::Deferred,
    }
}

pub(super) fn begin_graph_build(shared: &KernelShared) -> Result<u64, KernelError> {
    shared
        .graph_build
        .lock()
        .map_err(|_| KernelError::Poisoned("graph build runtime"))?
        .begin()
}

pub(super) fn finish_graph_build(
    shared: &KernelShared,
    run_id: u64,
    result: &Result<CommandReceipt, KernelError>,
    durable_run: Option<([u8; 32], AtlasRunReceiptV1)>,
) -> Result<(), KernelError> {
    shared
        .graph_build
        .lock()
        .map_err(|_| KernelError::Poisoned("graph build runtime"))?
        .finish(run_id, result, durable_run)
}

pub(super) fn cancel_graph_build(
    shared: &KernelShared,
    sequence: u64,
) -> Result<CommandReceipt, KernelError> {
    let active = shared
        .graph_build
        .lock()
        .map_err(|_| KernelError::Poisoned("graph build runtime"))?
        .active_run
        .is_some();
    if !active {
        return Err(KernelError::AtlasRunNotActive);
    }
    shared.producer_cancel.store(true, Ordering::Release);
    let revision = read_state(shared)?.revision;
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::AtlasRunCancellationRequested,
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

fn analysis_summary(state: &KernelState) -> Option<AtlasAnalysisSummary> {
    let receipt = state.analysis_publication?;
    let artifact = state.nli_analysis.as_deref()?;
    let lease = state.active_document_lease.as_deref()?;
    let binding = &artifact.binding;
    if binding.native_document_id != lease.entry_id.0
        || binding.document_revision != lease.revision.0
        || binding.content_hash != lease.content_hash.0
        || binding.analysis_generation != receipt.analysis_generation
        || binding.target_registry_revision != receipt.registry_revision
    {
        return None;
    }
    Some(AtlasAnalysisSummary {
        analysis_generation: receipt.analysis_generation,
        document_revision: receipt.document_revision,
        content_hash: binding.content_hash,
        registry_revision: receipt.registry_revision,
        analysis_artifact_hash: receipt.analysis_artifact_hash,
        nli_artifact_hash: receipt.nli_artifact_hash,
        producer_coordinator_hash: receipt.producer_coordinator_hash,
        entity_count: receipt.entity_count,
        mention_count: receipt.mention_count,
        nli_candidate_count: receipt.nli_candidate_count,
        nli_adjudication_count: receipt.nli_adjudication_count,
        promotion_count: receipt.promotion_count,
        dynamic_ner_model: Arc::from(binding.dynamic_ner.model_id.as_str()),
        dynamic_ner_runtime: Arc::from(binding.dynamic_ner.runtime_id.as_str()),
        nli_model: Arc::from(binding.nli.model_id.as_str()),
        nli_runtime: Arc::from(binding.nli.runtime_id.as_str()),
    })
}

fn decide_control_state(
    has_document: bool,
    analysis_ready: bool,
    full_for_document: bool,
    last_run_matches: bool,
    build_in_progress: bool,
    cancelled: bool,
    failed: bool,
) -> (
    AtlasBuildState,
    AtlasPrimaryAction,
    &'static str,
    &'static str,
) {
    if build_in_progress {
        return (
            AtlasBuildState::Building,
            AtlasPrimaryAction::Wait,
            "Building the graph",
            "Phoenix is compiling and publishing one verified native generation.",
        );
    }
    if cancelled {
        return (
            AtlasBuildState::Cancelled,
            if analysis_ready {
                AtlasPrimaryAction::RunPipeline
            } else {
                AtlasPrimaryAction::ConfigurePipeline
            },
            "The run was cancelled",
            "The previous generation is intact. Run again whenever you are ready.",
        );
    }
    if failed {
        return (
            AtlasBuildState::Failed,
            if analysis_ready {
                AtlasPrimaryAction::RunPipeline
            } else {
                AtlasPrimaryAction::ConfigurePipeline
            },
            "The pipeline stopped safely",
            "The previous generation is intact. Fix the stated issue, then run the same pipeline again.",
        );
    }
    if !has_document {
        return (
            AtlasBuildState::WaitingForDocument,
            AtlasPrimaryAction::OpenDocument,
            "Choose a note",
            "Atlas needs one active document lease before it can build.",
        );
    }
    if !analysis_ready {
        return (
            AtlasBuildState::RuntimeUnavailable,
            AtlasPrimaryAction::ConfigurePipeline,
            "Connect the analysis runtime",
            "Phoenix needs the verified native producer and both model roots before this document can run.",
        );
    }
    if full_for_document && last_run_matches {
        return (
            AtlasBuildState::Published,
            AtlasPrimaryAction::RunPipeline,
            "Your graph is live",
            "The note, analysis, archive, and product index agree. Run again whenever the source changes.",
        );
    }
    if full_for_document {
        return (
            AtlasBuildState::VerificationRequired,
            AtlasPrimaryAction::RunPipeline,
            "Verify the current note",
            "A graph is live, but this process has no matching source receipt. Rebuild once to prove it.",
        );
    }
    (
        AtlasBuildState::Ready,
        AtlasPrimaryAction::RunPipeline,
        "Ready to run",
        "One action runs Dynamic NER, candidate-only NLI, native graph compilation, and atomic publication.",
    )
}

#[allow(clippy::too_many_arguments)]
fn build_stages(
    has_document: bool,
    has_analysis: bool,
    analysis_ready: bool,
    content_bytes: u64,
    resident_anchor_count: u64,
    canonical_entities: u64,
    analysis_mentions: Option<u64>,
    full_for_document: bool,
    edge_count: u64,
    graph_reviews: AtlasGraphReviewCounts,
    generation_id: Option<u64>,
    failed: bool,
) -> [AtlasStageSummary; 5] {
    let source_state = if has_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::NeedsAttention
    };
    let entity_state = if has_analysis {
        AtlasStageState::Complete
    } else if has_document && analysis_ready {
        AtlasStageState::Ready
    } else if has_document {
        AtlasStageState::Blocked
    } else {
        AtlasStageState::Waiting
    };
    let connection_state = if failed {
        AtlasStageState::Blocked
    } else if full_for_document {
        AtlasStageState::Complete
    } else if has_analysis || (has_document && analysis_ready) {
        AtlasStageState::Ready
    } else {
        AtlasStageState::Waiting
    };
    let review_state = if graph_reviews.proposed_edges > 0 {
        AtlasStageState::NeedsAttention
    } else if full_for_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::Waiting
    };
    let live_state = if full_for_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::Waiting
    };
    [
        AtlasStageSummary {
            stage: AtlasStage::Source,
            state: source_state,
            value: content_bytes,
            detail: Arc::from(if has_document {
                "Active note leased"
            } else {
                "No note selected"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Entities,
            state: entity_state,
            value: canonical_entities,
            detail: if let Some(mentions) = analysis_mentions {
                Arc::from(format!(
                    "{mentions} analysis mentions / {resident_anchor_count} resident anchors"
                ))
            } else if analysis_ready {
                Arc::from("Models ready")
            } else if canonical_entities > 0 {
                Arc::from("Runtime unavailable")
            } else {
                Arc::from("Tag text to begin")
            },
        },
        AtlasStageSummary {
            stage: AtlasStage::Connections,
            state: connection_state,
            value: edge_count,
            detail: Arc::from(if full_for_document {
                "Native topology published"
            } else {
                "Built after anchors"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Review,
            state: review_state,
            value: graph_reviews.proposed_edges,
            detail: Arc::from(if graph_reviews.proposed_edges > 0 {
                "Needs your decision"
            } else if full_for_document {
                "No proposed graph edges"
            } else {
                "Available after build"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Live,
            state: live_state,
            value: generation_id.unwrap_or_default(),
            detail: Arc::from(if full_for_document {
                "Verified generation"
            } else {
                "Previous truth stays live"
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_state_exposes_exactly_one_next_action() {
        assert_eq!(
            decide_control_state(true, false, false, false, false, false, false).1,
            AtlasPrimaryAction::ConfigurePipeline
        );
        assert_eq!(
            decide_control_state(true, true, false, false, false, false, false).1,
            AtlasPrimaryAction::RunPipeline
        );
        assert_eq!(
            decide_control_state(true, true, true, true, false, false, false).1,
            AtlasPrimaryAction::RunPipeline
        );
    }

    #[test]
    fn failed_build_never_claims_publication() {
        let (state, action, _, _) =
            decide_control_state(true, true, true, true, false, false, true);
        assert_eq!(state, AtlasBuildState::Failed);
        assert_eq!(action, AtlasPrimaryAction::RunPipeline);
    }

    #[test]
    fn cancelled_build_is_not_reported_as_failed() {
        let (state, action, _, _) =
            decide_control_state(true, true, true, true, false, true, false);
        assert_eq!(state, AtlasBuildState::Cancelled);
        assert_eq!(action, AtlasPrimaryAction::RunPipeline);
    }

    #[test]
    fn runtime_records_cancellation_without_a_failure_message() {
        let mut runtime = GraphBuildRuntime::default();
        let run_id = runtime.begin().expect("run begins");
        runtime
            .finish(run_id, &Err(KernelError::AnalysisProducerCancelled), None)
            .expect("matching run finishes");
        assert!(runtime.cancelled);
        assert!(runtime.last_error.is_none());
        assert!(runtime.active_run.is_none());
    }
}
