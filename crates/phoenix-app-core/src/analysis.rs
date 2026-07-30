use super::*;
use hashbrown::HashMap;
use phoenix_analysis_contract::{
    open_analysis_artifact, open_nli_artifact, open_producer_coordinator, open_structural_artifact,
    write_message_new, write_nli_artifact_new, write_producer_coordinator_new, AnalysisEntityKind,
    AnalysisStageReceipt, ContextualEvidenceBinding, DocumentAnalysisBinding,
    DocumentAnalysisRequestBinding, PhoenixAnalysisRequestV1, PhoenixDocumentAnalysisV1,
    PhoenixNerArtifactV1, PhoenixNliArtifactV1, PhoenixProducerCoordinatorV1,
    VerifiedAnalysisArtifact, VerifiedProducerCoordinator, VerifiedStructuralArtifact,
    ANALYSIS_ARTIFACT_EXTENSION, ANALYSIS_CONTRACT, MAX_CONTEXTUAL_EVIDENCE_BINDINGS,
    PRODUCER_COORDINATOR_EXTENSION, STRUCTURAL_ARTIFACT_EXTENSION,
};
use phoenix_scene_contract::{AnchorCandidate, AnchorSource, EntityKind};
use std::collections::BTreeMap;
use std::fs;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::Duration;

const ANALYSIS_DIRECTORY: &str = "analysis-authority-v1";
const MAX_RESTORE_FILES: usize = 256;
const DEFAULT_NLI_CANDIDATES: u32 = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisPublicationReceipt {
    pub native_document_id: u64,
    pub document_revision: u64,
    pub analysis_generation: u64,
    pub registry_revision: u64,
    pub analysis_artifact_hash: [u8; 32],
    pub nli_artifact_hash: [u8; 32],
    pub producer_coordinator_hash: [u8; 32],
    pub entity_count: u32,
    pub mention_count: u32,
    pub nli_candidate_count: u32,
    pub nli_adjudication_count: u32,
    pub promotion_count: u32,
    pub stages: AnalysisStageReceipt,
}

#[derive(Clone, Debug)]
pub struct NliPublication {
    pub analysis_artifact_hash: [u8; 32],
    pub nli_artifact_hash: [u8; 32],
    pub producer_coordinator_hash: [u8; 32],
    pub analysis: Arc<PhoenixDocumentAnalysisV1>,
    pub structural: Arc<PhoenixStructuralSubstrateV1>,
    pub artifact: Arc<PhoenixNliArtifactV1>,
    pub coordinator: Arc<PhoenixProducerCoordinatorV1>,
    pub entity_count: u32,
    pub mention_count: u32,
    pub stages: AnalysisStageReceipt,
}

#[derive(Clone, Debug)]
pub struct NativeProducerRuntimeConfig {
    pub producer_executable: PathBuf,
    pub ner_model_root: PathBuf,
    pub nli_model_root: PathBuf,
    pub source_document_id: Option<String>,
    pub max_nli_candidates: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisRuntimeInfo {
    pub ready: bool,
    pub producer: Arc<str>,
    pub dynamic_ner: Arc<str>,
    pub nli: Arc<str>,
    pub detail: Arc<str>,
}

pub(super) struct RestoredAnalysis {
    pub analysis: Arc<PhoenixDocumentAnalysisV1>,
    pub structural: Arc<PhoenixStructuralSubstrateV1>,
    pub nli: Arc<PhoenixNliArtifactV1>,
    pub coordinator: Arc<PhoenixProducerCoordinatorV1>,
    pub receipt: AnalysisPublicationReceipt,
}

impl NativeProducerRuntimeConfig {
    pub fn from_env() -> Result<Self, KernelError> {
        Ok(Self {
            producer_executable: required_path("PHOENIX_NATIVE_PRODUCER")?,
            ner_model_root: required_path("PHOENIX_NATIVE_NER_MODEL_ROOT")?,
            nli_model_root: required_path("PHOENIX_NATIVE_NLI_MODEL_ROOT")?,
            source_document_id: std::env::var("PHOENIX_NATIVE_SOURCE_DOCUMENT_ID").ok(),
            max_nli_candidates: std::env::var("PHOENIX_NATIVE_MAX_NLI_CANDIDATES")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_NLI_CANDIDATES),
        })
    }

    pub fn runtime_info() -> AnalysisRuntimeInfo {
        let config = match Self::from_env() {
            Ok(config) => config,
            Err(error) => {
                return AnalysisRuntimeInfo {
                    ready: false,
                    producer: Arc::from("not configured"),
                    dynamic_ner: Arc::from("not configured"),
                    nli: Arc::from("not configured"),
                    detail: Arc::from(error.to_string()),
                };
            }
        };
        let producer_ready = config.producer_executable.is_file();
        let ner_ready = config.ner_model_root.is_dir();
        let nli_ready = config.nli_model_root.is_dir();
        let ready = producer_ready && ner_ready && nli_ready;
        AnalysisRuntimeInfo {
            ready,
            producer: display_name(&config.producer_executable),
            dynamic_ner: display_name(&config.ner_model_root),
            nli: display_name(&config.nli_model_root),
            detail: Arc::from(if ready {
                "Verified native producer and model roots are available".to_owned()
            } else {
                format!(
                    "missing runtime input: producer={} dynamic_ner={} nli={}",
                    !producer_ready, !ner_ready, !nli_ready
                )
            }),
        }
    }
}

impl PhoenixKernel {
    pub fn analyze_active_document(
        &self,
        generation: u64,
    ) -> Result<AnalysisPublicationReceipt, KernelError> {
        let config = NativeProducerRuntimeConfig::from_env()?;
        self.analyze_active_document_with(generation, &config)
    }

    pub fn analyze_active_document_with(
        &self,
        generation: u64,
        config: &NativeProducerRuntimeConfig,
    ) -> Result<AnalysisPublicationReceipt, KernelError> {
        let (lease, source_registry_revision) = {
            let state = read_state(&self.shared)?;
            (
                state
                    .active_document_lease
                    .as_ref()
                    .map(Arc::clone)
                    .ok_or(KernelError::ActiveSceneDocumentUnavailable)?,
                state.entity_registry.revision(),
            )
        };
        let target_registry_revision = source_registry_revision
            .checked_add(1)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let source_document_id = config
            .source_document_id
            .clone()
            .unwrap_or_else(|| format!("native:{:016x}", lease.entry_id.0));
        let request = PhoenixAnalysisRequestV1 {
            schema: ANALYSIS_CONTRACT.to_owned(),
            binding: DocumentAnalysisRequestBinding {
                source_document_id,
                native_document_id: lease.entry_id.0,
                document_revision: lease.revision.0,
                content_hash: lease.content_hash.0,
                analysis_generation: generation,
                source_registry_revision,
                target_registry_revision,
            },
            text: lease.content.to_string(),
            ner_model_root: path_text(&config.ner_model_root),
            nli_model_root: path_text(&config.nli_model_root),
            max_nli_candidates: config.max_nli_candidates,
        };
        request
            .validate()
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let directory = analysis_directory(&self.shared.workspace_path)?;
        fs::create_dir_all(&directory).map_err(|error| {
            KernelError::AnalysisProducerFailed(format!("create {}: {error}", directory.display()))
        })?;
        let stem = artifact_stem(&request.binding);
        let request_path = directory.join(format!("{stem}.request.{ANALYSIS_ARTIFACT_EXTENSION}"));
        let output_path = directory.join(format!("{stem}.analysis.{ANALYSIS_ARTIFACT_EXTENSION}"));
        let structural_path =
            directory.join(format!("{stem}.structural.{STRUCTURAL_ARTIFACT_EXTENSION}"));
        let preliminary_coordinator_path =
            directory.join(format!("{stem}.producer.{PRODUCER_COORDINATOR_EXTENSION}"));
        write_message_new(&request_path, &request)?;
        let mut child = Command::new(&config.producer_executable)
            .arg("analyze")
            .arg(&request_path)
            .arg(&output_path)
            .arg(&structural_path)
            .arg(&preliminary_coordinator_path)
            .spawn()
            .map_err(|error| {
                KernelError::AnalysisProducerFailed(format!(
                    "start {}: {error}",
                    config.producer_executable.display()
                ))
            })?;
        let status = wait_for_producer(&mut child, &self.shared.producer_cancel)?;
        if !status.success() {
            return Err(KernelError::AnalysisProducerFailed(format!(
                "{} exited with {status}",
                config.producer_executable.display()
            )));
        }
        let verified = open_analysis_artifact(&output_path)?;
        let structural = open_structural_artifact(&structural_path)?;
        let coordinator = open_producer_coordinator(&preliminary_coordinator_path)?;
        self.publish_verified_analysis(verified, structural, coordinator, &output_path)
    }

    pub fn publish_verified_analysis(
        &self,
        verified: VerifiedAnalysisArtifact,
        structural: VerifiedStructuralArtifact,
        preliminary_coordinator: VerifiedProducerCoordinator,
        analysis_path: &Path,
    ) -> Result<AnalysisPublicationReceipt, KernelError> {
        let analysis = Arc::clone(verified.analysis());
        preliminary_coordinator
            .coordinator()
            .validate_preliminary(&analysis, structural.structural())
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        {
            let state = read_state(&self.shared)?;
            validate_binding(
                &state,
                &analysis.ner.binding,
                analysis.ner.binding.source_registry_revision,
            )?;
        }
        let nli_path = nli_path_for(analysis_path);
        let nli_artifact_hash = if nli_path.exists() {
            let opened = open_nli_artifact(&nli_path)?;
            if opened.nli().as_ref() != &analysis.nli {
                return Err(KernelError::AnalysisAuthorityMismatch);
            }
            opened.artifact_hash()
        } else {
            write_nli_artifact_new(&nli_path, &analysis.nli)?
        };
        self.execute(KernelCommand::PublishNerEntities(
            NerEntityBatch::from_verified(&verified),
        ))?;
        let anchors = {
            let state = read_state(&self.shared)?;
            let lease = state
                .active_document_lease
                .as_deref()
                .ok_or(KernelError::ActiveSceneDocumentUnavailable)?;
            Arc::new(verified_analysis_anchors(
                &analysis.ner,
                lease,
                &state.entity_registry,
            )?)
        };
        self.execute(KernelCommand::PublishDocumentAnchors(anchors))?;
        let mut coordinator = preliminary_coordinator.coordinator().as_ref().clone();
        let (canonical_entity_count, contextual_evidence_bindings) = {
            let state = read_state(&self.shared)?;
            (
                u32::try_from(state.entity_registry.entities().len())
                    .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
                contextual_evidence_bindings(&analysis.ner, structural.structural())?,
            )
        };
        coordinator
            .finalize(canonical_entity_count, contextual_evidence_bindings)
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        coordinator
            .validate_final(&analysis, structural.structural())
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let coordinator_path = producer_coordinator_path_for(analysis_path);
        let producer_coordinator_hash =
            write_producer_coordinator_new(&coordinator_path, &coordinator)?;
        let coordinator = Arc::new(coordinator);
        let receipt = publication_receipt(
            &analysis,
            verified.artifact_hash(),
            nli_artifact_hash,
            producer_coordinator_hash,
        )?;
        self.execute(KernelCommand::PublishNliArtifact(Box::new(
            NliPublication {
                analysis_artifact_hash: verified.artifact_hash(),
                nli_artifact_hash,
                producer_coordinator_hash,
                analysis: Arc::clone(&analysis),
                structural: Arc::clone(structural.structural()),
                artifact: Arc::new(analysis.nli.clone()),
                coordinator,
                entity_count: receipt.entity_count,
                mention_count: receipt.mention_count,
                stages: analysis.ner.receipt,
            },
        )))?;
        Ok(receipt)
    }
}

pub(super) fn verified_analysis_anchors(
    artifact: &PhoenixNerArtifactV1,
    lease: &DocumentLease,
    registry: &EntityRegistry,
) -> Result<VerifiedDocumentAnchors, KernelError> {
    if artifact.binding.native_document_id != lease.entry_id.0
        || artifact.binding.document_revision != lease.revision.0
        || artifact.binding.content_hash != lease.content_hash.0
        || artifact.binding.target_registry_revision != registry.revision()
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let mut entity_slots = HashMap::with_capacity(registry.entities().len());
    for (slot, entity) in registry.entities().iter().enumerate() {
        let slot = u32::try_from(slot).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        if entity_slots
            .insert(entity.id, (slot, entity.kind.family()))
            .is_some()
        {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
    }
    let manual_mentions = registry.active_mentions_for(lease).collect::<Vec<_>>();
    let capacity = artifact
        .mentions
        .len()
        .checked_add(manual_mentions.len())
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let mut analysis_candidates = Vec::with_capacity(artifact.mentions.len());
    for mention in &artifact.mentions {
        let (slot, family) = entity_slots
            .get(&mention.entity_id)
            .copied()
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let start =
            usize::try_from(mention.start).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let end =
            usize::try_from(mention.end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let surface = lease
            .content
            .get(start..end)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        // The native producer exports only accepted or alias-candidate mention
        // packets into this artifact. Canonical registry membership therefore
        // controls paint visibility; `accepted` remains semantic evidence and
        // must not silently promote a candidate into graph topology.
        analysis_candidates.push(AnalysisPaintCandidate {
            anchor: AnchorCandidate {
                start: mention.start,
                end: mention.end,
                node_id: mention.entity_id,
                entity_slot: slot,
                family,
                surface: surface.to_owned(),
            },
            accepted: mention.accepted,
            confidence: mention.confidence,
            mention_id: mention.mention_id,
        });
    }
    let mut candidates = resolve_analysis_anchor_overlaps(analysis_candidates, capacity);
    if !manual_mentions.is_empty() {
        candidates.retain(|candidate| {
            manual_mentions.iter().all(|(mention, _)| {
                !ranges_overlap(candidate.start, candidate.end, mention.start, mention.end)
            })
        });
        for (mention, entity) in &manual_mentions {
            let (slot, family) = entity_slots
                .get(&entity.id)
                .copied()
                .ok_or(KernelError::AnalysisAuthorityMismatch)?;
            candidates.push(AnchorCandidate {
                start: mention.start,
                end: mention.end,
                node_id: entity.id,
                entity_slot: slot,
                family,
                surface: mention.surface.clone(),
            });
        }
    }
    Ok(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        None,
        if manual_mentions.is_empty() {
            AnchorSource::VerifiedAnalysis
        } else {
            AnchorSource::CanonicalRegistry
        },
        &lease.content,
        candidates,
    )?)
}

struct AnalysisPaintCandidate {
    anchor: AnchorCandidate,
    accepted: bool,
    confidence: f32,
    mention_id: u64,
}

fn resolve_analysis_anchor_overlaps(
    mut candidates: Vec<AnalysisPaintCandidate>,
    total_capacity: usize,
) -> Vec<AnchorCandidate> {
    candidates.sort_unstable_by(|left, right| {
        let left_len = left.anchor.end.saturating_sub(left.anchor.start);
        let right_len = right.anchor.end.saturating_sub(right.anchor.start);
        left.anchor
            .start
            .cmp(&right.anchor.start)
            // Match Angular's useful user-facing behavior for nested surfaces:
            // at the same left edge, paint the longest exact registry surface.
            .then_with(|| right_len.cmp(&left_len))
            .then_with(|| right.accepted.cmp(&left.accepted))
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| left.anchor.node_id.cmp(&right.anchor.node_id))
            .then_with(|| left.mention_id.cmp(&right.mention_id))
    });

    let mut resolved = Vec::with_capacity(total_capacity);
    let mut previous_end = None;
    for candidate in candidates {
        if previous_end.is_some_and(|end| candidate.anchor.start < end) {
            continue;
        }
        previous_end = Some(candidate.anchor.end);
        resolved.push(candidate.anchor);
    }
    resolved
}

const fn ranges_overlap(left_start: u32, left_end: u32, right_start: u32, right_end: u32) -> bool {
    left_start < right_end && right_start < left_end
}

pub(super) fn publish_nli_artifact(
    shared: &KernelShared,
    sequence: u64,
    publication: NliPublication,
) -> Result<CommandReceipt, KernelError> {
    publication
        .artifact
        .validate()
        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let mut state = write_state(shared)?;
    validate_binding(
        &state,
        &publication.artifact.binding,
        publication.artifact.binding.target_registry_revision,
    )?;
    let publication_receipt_value = AnalysisPublicationReceipt {
        native_document_id: publication.artifact.binding.native_document_id,
        document_revision: publication.artifact.binding.document_revision,
        analysis_generation: publication.artifact.binding.analysis_generation,
        registry_revision: publication.artifact.binding.target_registry_revision,
        analysis_artifact_hash: publication.analysis_artifact_hash,
        nli_artifact_hash: publication.nli_artifact_hash,
        producer_coordinator_hash: publication.producer_coordinator_hash,
        entity_count: publication.entity_count,
        mention_count: publication.mention_count,
        nli_candidate_count: publication
            .artifact
            .nli_candidates
            .len()
            .try_into()
            .unwrap_or(u32::MAX),
        nli_adjudication_count: publication
            .artifact
            .nli_adjudications
            .len()
            .try_into()
            .unwrap_or(u32::MAX),
        promotion_count: publication.artifact.promotion_count,
        stages: publication.stages,
    };
    state.nli_analysis = Some(publication.artifact);
    state.document_analysis = Some(publication.analysis);
    state.structural_analysis = Some(publication.structural);
    state.producer_coordinator = Some(publication.coordinator);
    state.graph_generation_v2 = None;
    state.review_catalog_v2 = None;
    state.analysis_publication = Some(publication_receipt_value);
    state.revision = checked_revision(state.revision)?;
    let kernel_revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision,
            kind: KernelEventKind::NliCandidatesCommitted {
                receipt: publication_receipt_value,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        kernel_revision,
        KernelOutcome::NliCandidatesPublished(publication_receipt_value),
    ))
}

pub(super) fn validate_binding(
    state: &KernelState,
    binding: &DocumentAnalysisBinding,
    expected_registry_revision: u64,
) -> Result<(), KernelError> {
    let lease = state
        .active_document_lease
        .as_ref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    if binding.native_document_id != lease.entry_id.0
        || binding.document_revision != lease.revision.0
        || binding.content_hash != lease.content_hash.0
        || binding.analysis_generation == 0
        || expected_registry_revision != state.entity_registry.revision()
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    Ok(())
}

pub(super) fn entity_kind(kind: AnalysisEntityKind) -> EntityKind {
    match kind {
        AnalysisEntityKind::Character => EntityKind::Character,
        AnalysisEntityKind::Location => EntityKind::Location,
        AnalysisEntityKind::Npc => EntityKind::Npc,
        AnalysisEntityKind::Faction => EntityKind::Faction,
        AnalysisEntityKind::Event => EntityKind::Event,
        AnalysisEntityKind::Concept => EntityKind::Concept,
        AnalysisEntityKind::Custom => EntityKind::Custom,
    }
}

pub(super) fn restore_active_analysis(
    workspace_path: &Path,
    lease: Option<&DocumentLease>,
    registry_revision: u64,
) -> Result<Option<RestoredAnalysis>, KernelError> {
    let Some(lease) = lease else {
        return Ok(None);
    };
    let directory = analysis_directory(workspace_path)?;
    let Ok(entries) = fs::read_dir(&directory) else {
        return Ok(None);
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".analysis.pnaa"))
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    if paths.len() > MAX_RESTORE_FILES {
        paths.drain(..paths.len() - MAX_RESTORE_FILES);
    }
    for path in paths.into_iter().rev() {
        let Ok(verified) = open_analysis_artifact(&path) else {
            continue;
        };
        let analysis = verified.analysis();
        let binding = &analysis.nli.binding;
        if binding.native_document_id != lease.entry_id.0
            || binding.document_revision != lease.revision.0
            || binding.content_hash != lease.content_hash.0
            || binding.target_registry_revision != registry_revision
        {
            continue;
        }
        let nli_path = nli_path_for(&path);
        let verified_nli = open_nli_artifact(&nli_path)?;
        if verified_nli.nli().as_ref() != &analysis.nli {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
        let Ok(structural) = open_structural_artifact(&structural_path_for(&path)) else {
            continue;
        };
        let Ok(coordinator) = open_producer_coordinator(&producer_coordinator_path_for(&path))
        else {
            continue;
        };
        coordinator
            .coordinator()
            .validate_final(analysis, structural.structural())
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let receipt = publication_receipt(
            analysis,
            verified.artifact_hash(),
            verified_nli.artifact_hash(),
            coordinator.artifact_hash(),
        )?;
        return Ok(Some(RestoredAnalysis {
            analysis: Arc::clone(verified.analysis()),
            structural: Arc::clone(structural.structural()),
            nli: Arc::clone(verified_nli.nli()),
            coordinator: Arc::clone(coordinator.coordinator()),
            receipt,
        }));
    }
    Ok(None)
}

fn publication_receipt(
    analysis: &PhoenixDocumentAnalysisV1,
    analysis_artifact_hash: [u8; 32],
    nli_artifact_hash: [u8; 32],
    producer_coordinator_hash: [u8; 32],
) -> Result<AnalysisPublicationReceipt, KernelError> {
    Ok(AnalysisPublicationReceipt {
        native_document_id: analysis.ner.binding.native_document_id,
        document_revision: analysis.ner.binding.document_revision,
        analysis_generation: analysis.ner.binding.analysis_generation,
        registry_revision: analysis.ner.binding.target_registry_revision,
        analysis_artifact_hash,
        nli_artifact_hash,
        producer_coordinator_hash,
        entity_count: analysis
            .ner
            .entities
            .len()
            .try_into()
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
        mention_count: analysis
            .ner
            .mentions
            .len()
            .try_into()
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
        nli_candidate_count: analysis
            .nli
            .nli_candidates
            .len()
            .try_into()
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
        nli_adjudication_count: analysis
            .nli
            .nli_adjudications
            .len()
            .try_into()
            .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
        promotion_count: analysis.nli.promotion_count,
        stages: analysis.ner.receipt,
    })
}

fn wait_for_producer(
    child: &mut Child,
    cancellation: &AtomicBool,
) -> Result<ExitStatus, KernelError> {
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            KernelError::AnalysisProducerFailed(format!("wait for semantic producer: {error}"))
        })? {
            return Ok(status);
        }
        if cancellation.load(Ordering::Acquire) {
            if let Err(error) = child.kill() {
                if let Some(status) = child.try_wait().map_err(|wait_error| {
                    KernelError::AnalysisProducerFailed(format!(
                        "wait after cancellation race: {wait_error}"
                    ))
                })? {
                    return Ok(status);
                }
                return Err(KernelError::AnalysisProducerFailed(format!(
                    "terminate cancelled semantic producer: {error}"
                )));
            }
            let _ = child.wait();
            return Err(KernelError::AnalysisProducerCancelled);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn contextual_evidence_bindings(
    ner: &PhoenixNerArtifactV1,
    structural: &phoenix_analysis_contract::PhoenixStructuralSubstrateV1,
) -> Result<Vec<ContextualEvidenceBinding>, KernelError> {
    let mut per_chunk = vec![BTreeMap::<u64, u64>::new(); structural.chunks.len()];
    for mention in ner.mentions.iter().filter(|mention| mention.accepted) {
        let chunk_index = structural
            .chunks
            .partition_point(|chunk| chunk.end <= mention.start);
        let chunk = structural
            .chunks
            .get(chunk_index)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        if chunk.start > mention.start || chunk.end < mention.end {
            return Err(KernelError::AnalysisAuthorityMismatch);
        }
        per_chunk[chunk_index]
            .entry(mention.entity_id)
            .and_modify(|mention_id| *mention_id = (*mention_id).min(mention.mention_id))
            .or_insert(mention.mention_id);
    }
    let mut bindings = Vec::new();
    for (chunk_index, entities) in per_chunk.iter().enumerate() {
        let entities = entities.iter().collect::<Vec<_>>();
        for source in 0..entities.len() {
            for target in (source + 1)..entities.len() {
                if bindings.len() >= MAX_CONTEXTUAL_EVIDENCE_BINDINGS {
                    return Err(KernelError::AnalysisAuthorityMismatch);
                }
                bindings.push(ContextualEvidenceBinding {
                    source_entity_id: *entities[source].0,
                    target_entity_id: *entities[target].0,
                    source_mention_id: *entities[source].1,
                    target_mention_id: *entities[target].1,
                    chunk_index: u32::try_from(chunk_index)
                        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
                });
            }
        }
    }
    Ok(bindings)
}

fn analysis_directory(workspace_path: &Path) -> Result<PathBuf, KernelError> {
    workspace_path
        .parent()
        .map(|parent| parent.join(ANALYSIS_DIRECTORY))
        .ok_or_else(|| KernelError::AnalysisProducerFailed("workspace has no parent".into()))
}

fn nli_path_for(analysis_path: &Path) -> PathBuf {
    let file = analysis_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("analysis.pnaa");
    analysis_path.with_file_name(file.replace(".analysis.pnaa", ".nli.pnaa"))
}

fn structural_path_for(analysis_path: &Path) -> PathBuf {
    let file = analysis_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("analysis.pnaa");
    analysis_path.with_file_name(file.replace(
        ".analysis.pnaa",
        &format!(".structural.{STRUCTURAL_ARTIFACT_EXTENSION}"),
    ))
}

fn producer_coordinator_path_for(analysis_path: &Path) -> PathBuf {
    let file = analysis_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("analysis.pnaa");
    analysis_path.with_file_name(file.replace(
        ".analysis.pnaa",
        &format!(".coordinator.{PRODUCER_COORDINATOR_EXTENSION}"),
    ))
}

fn artifact_stem(binding: &DocumentAnalysisRequestBinding) -> String {
    let hash = binding
        .content_hash
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{:016x}-r{}-g{}-{hash}",
        binding.native_document_id, binding.document_revision, binding.analysis_generation
    )
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn display_name(path: &Path) -> Arc<str> {
    Arc::from(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("configured"),
    )
}

fn required_path(name: &'static str) -> Result<PathBuf, KernelError> {
    let value = std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .ok_or(KernelError::AnalysisProducerUnavailable(name))?;
    Ok(PathBuf::from(value))
}
