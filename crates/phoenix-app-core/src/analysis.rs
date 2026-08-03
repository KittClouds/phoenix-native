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
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self as control_mpsc, Receiver as ControlReceiver, RecvTimeoutError};
use std::sync::TryLockError;
use std::thread;
use std::time::Duration;

const ANALYSIS_DIRECTORY: &str = "analysis-authority-v1";
const MAX_RESTORE_FILES: usize = 256;
const DEFAULT_NLI_CANDIDATES: u32 = 65_536;
const ANALYSIS_REUSE_MARKER: &[u8] = b"PHOENIX_ANALYSIS_REUSE_V1\0";

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
    pub configured: bool,
    pub ready: bool,
    pub resident: bool,
    pub producer: Arc<str>,
    pub dynamic_ner: Arc<str>,
    pub nli: Arc<str>,
    pub detail: Arc<str>,
    pub warm_receipt: Option<AnalysisModelWarmReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisModelWarmReceipt {
    pub config_hash: [u8; 32],
    pub producer_pid: u32,
    pub ner_load_micros: u64,
    pub nli_load_micros: u64,
    pub total_micros: u64,
    pub ner_cache_hit: bool,
    pub nli_cache_hit: bool,
    pub reused: bool,
}

pub(super) struct ResidentAnalysisProducer {
    child: Child,
    input: ChildStdin,
    output: ControlReceiver<Result<String, String>>,
    receipt: AnalysisModelWarmReceipt,
}

struct AnalysisWarmGuard<'a>(&'a AtomicBool);

impl Drop for AnalysisWarmGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
                    configured: false,
                    ready: false,
                    resident: false,
                    producer: Arc::from("not configured"),
                    dynamic_ner: Arc::from("not configured"),
                    nli: Arc::from("not configured"),
                    detail: Arc::from(error.to_string()),
                    warm_receipt: None,
                };
            }
        };
        let producer_ready = config.producer_executable.is_file();
        let ner_ready = config.ner_model_root.is_dir();
        let nli_ready = config.nli_model_root.is_dir();
        let configured = producer_ready && ner_ready && nli_ready;
        AnalysisRuntimeInfo {
            configured,
            ready: false,
            resident: false,
            producer: display_name(&config.producer_executable),
            dynamic_ner: display_name(&config.ner_model_root),
            nli: display_name(&config.nli_model_root),
            detail: Arc::from(if configured {
                "Verified native producer and model roots are configured; warm them before running"
                    .to_owned()
            } else {
                format!(
                    "missing runtime input: producer={} dynamic_ner={} nli={}",
                    !producer_ready, !ner_ready, !nli_ready
                )
            }),
            warm_receipt: None,
        }
    }

    fn resident_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix.analysis.resident-models/v1\0");
        hash_path_identity(&mut hasher, &self.producer_executable);
        hash_path_identity(&mut hasher, &self.ner_model_root);
        hash_path_identity(&mut hasher, &self.nli_model_root);
        *hasher.finalize().as_bytes()
    }
}

impl PhoenixKernel {
    pub fn analysis_runtime_info(&self) -> AnalysisRuntimeInfo {
        let mut info = NativeProducerRuntimeConfig::runtime_info();
        let Ok(config) = NativeProducerRuntimeConfig::from_env() else {
            return info;
        };
        let mut producer = match self.shared.analysis_producer.try_lock() {
            Ok(producer) => producer,
            Err(TryLockError::WouldBlock) => {
                let warming = self.shared.analysis_warming.load(Ordering::Acquire);
                info.ready = info.configured && !warming;
                info.resident = info.configured && !warming;
                info.detail = Arc::from(if warming {
                    "analysis models are warming"
                } else {
                    "resident models are executing the active pipeline"
                });
                return info;
            }
            Err(TryLockError::Poisoned(_)) => {
                info.ready = false;
                info.detail = Arc::from("resident analysis runtime lock is poisoned");
                return info;
            }
        };
        if let Some(runtime) = producer.as_mut() {
            let healthy = runtime.receipt.config_hash == config.resident_hash()
                && runtime.child.try_wait().ok().flatten().is_none();
            if healthy {
                info.ready = true;
                info.resident = true;
                info.warm_receipt = Some(runtime.receipt);
                info.detail = Arc::from("GLiNER and ModernBERT are resident and ready");
            } else {
                *producer = None;
                info.ready = false;
                info.detail =
                    Arc::from("configured models are not resident; warm them before running");
            }
        }
        info
    }

    pub fn warm_analysis_models(&self) -> Result<AnalysisModelWarmReceipt, KernelError> {
        let config = NativeProducerRuntimeConfig::from_env()?;
        self.warm_analysis_models_with(&config)
    }

    pub fn warm_analysis_models_with(
        &self,
        config: &NativeProducerRuntimeConfig,
    ) -> Result<AnalysisModelWarmReceipt, KernelError> {
        validate_runtime_paths(config)?;
        self.shared
            .analysis_warming
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                KernelError::AnalysisProducerFailed(
                    "analysis model warm is already in progress".into(),
                )
            })?;
        let _warming = AnalysisWarmGuard(&self.shared.analysis_warming);
        let config_hash = config.resident_hash();
        let mut slot = match self.shared.analysis_producer.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => {
                return Err(if self.shared.analysis_warming.load(Ordering::Acquire) {
                    KernelError::AnalysisModelsNotWarm
                } else {
                    KernelError::AnalysisProducerFailed("resident analysis producer is busy".into())
                });
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(KernelError::Poisoned("resident analysis producer"));
            }
        };
        if let Some(runtime) = slot.as_mut() {
            if runtime.receipt.config_hash == config_hash
                && runtime.child.try_wait().ok().flatten().is_none()
            {
                let mut receipt = runtime.receipt;
                receipt.reused = true;
                return Ok(receipt);
            }
            runtime.shutdown();
            *slot = None;
        }
        let mut child = Command::new(&config.producer_executable)
            .arg("serve")
            .arg(&config.ner_model_root)
            .arg(&config.nli_model_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| producer_start_error(config, error))?;
        let input = child.stdin.take().ok_or_else(|| {
            KernelError::AnalysisProducerFailed("resident producer stdin unavailable".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            KernelError::AnalysisProducerFailed("resident producer stdout unavailable".into())
        })?;
        let (control_sender, control_receiver) = control_mpsc::sync_channel(8);
        thread::Builder::new()
            .name("phoenix-analysis-control".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::with_capacity(512);
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if control_sender.send(Ok(line.clone())).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = control_sender.send(Err(error.to_string()));
                            break;
                        }
                    }
                }
            })
            .map_err(|error| {
                KernelError::AnalysisProducerFailed(format!(
                    "start resident producer control reader: {error}"
                ))
            })?;
        let mut runtime = ResidentAnalysisProducer {
            child,
            input,
            output: control_receiver,
            receipt: AnalysisModelWarmReceipt {
                config_hash,
                producer_pid: 0,
                ner_load_micros: 0,
                nli_load_micros: 0,
                total_micros: 0,
                ner_cache_hit: false,
                nli_cache_hit: false,
                reused: false,
            },
        };
        let fields = match runtime.read_control("READY", None) {
            Ok(fields) => fields,
            Err(error) => {
                runtime.shutdown();
                return Err(error);
            }
        };
        if fields.len() != 7 {
            runtime.shutdown();
            return Err(KernelError::AnalysisProducerFailed(
                "malformed resident producer READY receipt".into(),
            ));
        }
        let parsed = (|| {
            Ok::<_, KernelError>((
                parse_control(&fields[1], "producer pid")?,
                parse_control(&fields[2], "GLiNER load micros")?,
                parse_control(&fields[3], "NLI load micros")?,
                parse_control(&fields[4], "total warm micros")?,
                parse_control::<u8>(&fields[5], "GLiNER optimized cache hit")? != 0,
                parse_control::<u8>(&fields[6], "NLI optimized cache hit")? != 0,
            ))
        })();
        let (
            producer_pid,
            ner_load_micros,
            nli_load_micros,
            total_micros,
            ner_cache_hit,
            nli_cache_hit,
        ) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                runtime.shutdown();
                return Err(error);
            }
        };
        runtime.receipt.producer_pid = producer_pid;
        runtime.receipt.ner_load_micros = ner_load_micros;
        runtime.receipt.nli_load_micros = nli_load_micros;
        runtime.receipt.total_micros = total_micros;
        runtime.receipt.ner_cache_hit = ner_cache_hit;
        runtime.receipt.nli_cache_hit = nli_cache_hit;
        let receipt = runtime.receipt;
        *slot = Some(runtime);
        Ok(receipt)
    }

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
        let config_hash = config.resident_hash();
        let (lease, source_registry_revision, reusable) = {
            let state = read_state(&self.shared)?;
            let lease = state
                .active_document_lease
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::ActiveSceneDocumentUnavailable)?;
            let reusable = reusable_analysis_receipt(
                &self.shared.workspace_path,
                &state,
                &lease,
                config_hash,
            )?;
            (lease, state.entity_registry.revision(), reusable)
        };
        if let Some(receipt) = reusable {
            return Ok(receipt);
        }
        let target_registry_revision = source_registry_revision
            .checked_add(1)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let source_document_id = config
            .source_document_id
            .clone()
            .unwrap_or_else(|| format!("native:{:016x}", lease.entry_id.0));
        let directory = analysis_directory(&self.shared.workspace_path)?;
        fs::create_dir_all(&directory).map_err(|error| {
            KernelError::AnalysisProducerFailed(format!("create {}: {error}", directory.display()))
        })?;
        let generation = first_free_analysis_generation(
            &directory,
            lease.entry_id.0,
            lease.revision.0,
            lease.content_hash.0,
            generation,
        )?;
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
        let stem = artifact_stem(&request.binding);
        let request_path = directory.join(format!("{stem}.request.{ANALYSIS_ARTIFACT_EXTENSION}"));
        let output_path = directory.join(format!("{stem}.analysis.{ANALYSIS_ARTIFACT_EXTENSION}"));
        let structural_path =
            directory.join(format!("{stem}.structural.{STRUCTURAL_ARTIFACT_EXTENSION}"));
        let preliminary_coordinator_path =
            directory.join(format!("{stem}.producer.{PRODUCER_COORDINATOR_EXTENSION}"));
        write_message_new(&request_path, &request)?;
        self.run_resident_analysis(
            config,
            &request_path,
            &output_path,
            &structural_path,
            &preliminary_coordinator_path,
        )?;
        // The marker is only an optimization hint.  The immutable artifacts
        // remain authoritative; a missing marker simply causes the next run
        // to execute normally rather than risking reuse under a new model or
        // producer configuration.
        let _ = write_analysis_reuse_marker(&output_path, config_hash);
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

    fn run_resident_analysis(
        &self,
        config: &NativeProducerRuntimeConfig,
        request: &Path,
        output: &Path,
        structural: &Path,
        coordinator: &Path,
    ) -> Result<(), KernelError> {
        let expected = config.resident_hash();
        let mut slot = match self.shared.analysis_producer.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => {
                return Err(if self.shared.analysis_warming.load(Ordering::Acquire) {
                    KernelError::AnalysisModelsNotWarm
                } else {
                    KernelError::AnalysisProducerFailed("resident analysis producer is busy".into())
                });
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(KernelError::Poisoned("resident analysis producer"));
            }
        };
        let runtime = slot
            .as_mut()
            .filter(|runtime| runtime.receipt.config_hash == expected)
            .ok_or(KernelError::AnalysisModelsNotWarm)?;
        if runtime.child.try_wait().ok().flatten().is_some() {
            *slot = None;
            return Err(KernelError::AnalysisModelsNotWarm);
        }
        for path in [request, output, structural, coordinator] {
            if path.to_string_lossy().contains(['\r', '\n', '\t']) {
                return Err(KernelError::AnalysisProducerFailed(
                    "analysis artifact path contains a control delimiter".into(),
                ));
            }
        }
        writeln!(
            runtime.input,
            "ANALYZE\t{}\t{}\t{}\t{}",
            request.display(),
            output.display(),
            structural.display(),
            coordinator.display()
        )
        .and_then(|_| runtime.input.flush())
        .map_err(|error| KernelError::AnalysisProducerFailed(format!("send ANALYZE: {error}")))?;
        match runtime.read_control("DONE", Some(&self.shared.producer_cancel)) {
            Ok(_) => Ok(()),
            Err(error) => {
                runtime.shutdown();
                *slot = None;
                Err(error)
            }
        }
    }
}

impl ResidentAnalysisProducer {
    fn read_control(
        &mut self,
        expected: &str,
        cancellation: Option<&AtomicBool>,
    ) -> Result<Vec<String>, KernelError> {
        loop {
            if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(KernelError::AnalysisProducerCancelled);
            }
            let line = match self.output.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    return Err(KernelError::AnalysisProducerFailed(format!(
                        "read producer control: {error}"
                    )))
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.child.try_wait().ok().flatten();
                    return Err(KernelError::AnalysisProducerFailed(format!(
                        "resident producer closed its control stream ({status:?})"
                    )));
                }
            };
            let Some(payload) = line
                .trim_end_matches(['\r', '\n'])
                .strip_prefix("PHOENIX_CONTROL\t")
            else {
                continue;
            };
            let fields = payload.split('\t').map(str::to_owned).collect::<Vec<_>>();
            if fields.first().is_some_and(|field| field == "ERROR") {
                return Err(KernelError::AnalysisProducerFailed(
                    fields
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "unknown producer error".into()),
                ));
            }
            if fields.first().is_none_or(|field| field != expected) {
                return Err(KernelError::AnalysisProducerFailed(format!(
                    "expected producer control {expected}, received {payload}"
                )));
            }
            return Ok(fields);
        }
    }

    pub(super) fn shutdown(&mut self) {
        let _ = writeln!(self.input, "SHUTDOWN");
        let _ = self.input.flush();
        for _ in 0..20 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn validate_runtime_paths(config: &NativeProducerRuntimeConfig) -> Result<(), KernelError> {
    if !config.producer_executable.is_file()
        || !config.ner_model_root.is_dir()
        || !config.nli_model_root.is_dir()
    {
        return Err(KernelError::AnalysisProducerFailed(
            "producer executable or model root is unavailable".into(),
        ));
    }
    Ok(())
}

fn producer_start_error(
    config: &NativeProducerRuntimeConfig,
    error: std::io::Error,
) -> KernelError {
    KernelError::AnalysisProducerFailed(format!(
        "start resident {}: {error}",
        config.producer_executable.display()
    ))
}

fn parse_control<T: std::str::FromStr>(raw: &str, name: &str) -> Result<T, KernelError> {
    raw.parse()
        .map_err(|_| KernelError::AnalysisProducerFailed(format!("invalid {name} in warm receipt")))
}

fn hash_path_identity(hasher: &mut blake3::Hasher, path: &Path) {
    hasher.update(path.as_os_str().to_string_lossy().as_bytes());
    if let Ok(metadata) = fs::metadata(path) {
        hasher.update(&metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(&duration.as_nanos().to_le_bytes());
            }
        }
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
    let lease = state
        .active_document_lease
        .as_ref()
        .map(Arc::clone)
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let memory_analysis = Arc::clone(&publication.analysis);
    let memory_structural = Arc::clone(&publication.structural);
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
    shared
        .resident_memory
        .publish_document(lease, &memory_structural, &memory_analysis)?;
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
        AnalysisEntityKind::Network => EntityKind::Network,
        AnalysisEntityKind::Creature => EntityKind::Creature,
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
        // A producer-contract upgrade may make an otherwise intact historical
        // coordinator artifact incomplete (for example, a generation created
        // before contextual evidence became a required product).  Such an
        // artifact is not current authority, but it is not a corrupt file
        // either.  Leave it historical and continue looking for a generation
        // that satisfies the current contract; if none does, startup exposes
        // no matching analysis and the user can run the production pipeline.
        if coordinator
            .coordinator()
            .validate_final(analysis, structural.structural())
            .is_err()
        {
            continue;
        }
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

fn contextual_evidence_bindings(
    ner: &PhoenixNerArtifactV1,
    structural: &phoenix_analysis_contract::PhoenixStructuralSubstrateV1,
) -> Result<Vec<ContextualEvidenceBinding>, KernelError> {
    let mut per_chunk = vec![BTreeMap::<u64, u64>::new(); structural.chunks.len()];
    // PhoenixNerArtifactV1 contains only exportable dynamic-NER mentions.  The
    // `accepted` bit records semantic promotion (AcceptedKnown/AcceptedNew),
    // not whether the exact source span may serve as contextual evidence.
    // Contextual co-occurrence is evidence-only and must therefore preserve
    // alias-candidate mentions without silently promoting them to topology.
    for mention in &ner.mentions {
        // Match the entity producer's structural binding rule exactly.  In an
        // overlapping chunk window the last chunk can be shorter than its
        // predecessor; selecting merely the first containing chunk would give
        // one source span two different chunk identities downstream.
        let chunk_index = structural
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.start <= mention.start && mention.end <= chunk.end)
            .min_by_key(|(ordinal, chunk)| (chunk.end - chunk.start, *ordinal))
            .map(|(ordinal, _)| ordinal)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
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

fn analysis_reuse_marker_path(analysis_path: &Path) -> PathBuf {
    let mut marker = analysis_path.as_os_str().to_os_string();
    marker.push(".reuse");
    PathBuf::from(marker)
}

fn analysis_reuse_marker(config_hash: [u8; 32]) -> Vec<u8> {
    let mut marker = Vec::with_capacity(ANALYSIS_REUSE_MARKER.len() + config_hash.len());
    marker.extend_from_slice(ANALYSIS_REUSE_MARKER);
    marker.extend_from_slice(&config_hash);
    marker
}

fn write_analysis_reuse_marker(
    analysis_path: &Path,
    config_hash: [u8; 32],
) -> Result<(), KernelError> {
    let marker_path = analysis_reuse_marker_path(analysis_path);
    if marker_path.exists() {
        return Ok(());
    }
    let temporary_path = marker_path.with_extension("reuse.tmp");
    fs::write(&temporary_path, analysis_reuse_marker(config_hash)).map_err(|error| {
        KernelError::AnalysisProducerFailed(format!(
            "write analysis reuse marker {}: {error}",
            temporary_path.display()
        ))
    })?;
    fs::rename(&temporary_path, &marker_path).map_err(|error| {
        let _ = fs::remove_file(&temporary_path);
        KernelError::AnalysisProducerFailed(format!(
            "publish analysis reuse marker {}: {error}",
            marker_path.display()
        ))
    })
}

fn reusable_analysis_receipt(
    workspace_path: &Path,
    state: &KernelState,
    lease: &DocumentLease,
    config_hash: [u8; 32],
) -> Result<Option<AnalysisPublicationReceipt>, KernelError> {
    let Some(publication) = state.analysis_publication else {
        return Ok(None);
    };
    let Some(analysis) = state.document_analysis.as_deref() else {
        return Ok(None);
    };
    let Some(structural) = state.structural_analysis.as_deref() else {
        return Ok(None);
    };
    let Some(nli) = state.nli_analysis.as_deref() else {
        return Ok(None);
    };
    let Some(coordinator) = state.producer_coordinator.as_deref() else {
        return Ok(None);
    };
    let binding = &analysis.ner.binding;
    if binding.native_document_id != lease.entry_id.0
        || binding.document_revision != lease.revision.0
        || binding.content_hash != lease.content_hash.0
        || binding.target_registry_revision != state.entity_registry.revision()
        || publication.native_document_id != binding.native_document_id
        || publication.document_revision != binding.document_revision
        || publication.analysis_generation != binding.analysis_generation
        || publication.registry_revision != binding.target_registry_revision
    {
        return Ok(None);
    }
    let directory = analysis_directory(workspace_path)?;
    let stem = artifact_stem(&DocumentAnalysisRequestBinding {
        source_document_id: String::new(),
        native_document_id: binding.native_document_id,
        document_revision: binding.document_revision,
        content_hash: binding.content_hash,
        analysis_generation: binding.analysis_generation,
        source_registry_revision: binding.source_registry_revision,
        target_registry_revision: binding.target_registry_revision,
    });
    let analysis_path = directory.join(format!("{stem}.analysis.{ANALYSIS_ARTIFACT_EXTENSION}"));
    let marker_path = analysis_reuse_marker_path(&analysis_path);
    let Ok(marker) = fs::read(&marker_path) else {
        return Ok(None);
    };
    if marker != analysis_reuse_marker(config_hash) {
        return Ok(None);
    }

    // A matching marker must still pass every immutable artifact check.  A
    // truncated or replaced file therefore fails closed instead of silently
    // falling back to a slower analysis under the same apparent key.
    let verified = open_analysis_artifact(&analysis_path)?;
    if verified.artifact_hash() != publication.analysis_artifact_hash
        || verified.analysis().as_ref() != analysis
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let nli_path = nli_path_for(&analysis_path);
    let verified_nli = open_nli_artifact(&nli_path)?;
    if verified_nli.artifact_hash() != publication.nli_artifact_hash
        || verified_nli.nli().as_ref() != nli
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let verified_structural = open_structural_artifact(&structural_path_for(&analysis_path))?;
    if verified_structural.structural().as_ref() != structural {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let verified_coordinator =
        open_producer_coordinator(&producer_coordinator_path_for(&analysis_path))?;
    if verified_coordinator.coordinator().as_ref() != coordinator
        || verified_coordinator
            .coordinator()
            .validate_final(verified.analysis(), verified_structural.structural())
            .is_err()
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let receipt = publication_receipt(
        verified.analysis(),
        verified.artifact_hash(),
        verified_nli.artifact_hash(),
        verified_coordinator.artifact_hash(),
    )?;
    if receipt != publication {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    Ok(Some(receipt))
}

fn artifact_stem(binding: &DocumentAnalysisRequestBinding) -> String {
    artifact_stem_parts(
        binding.native_document_id,
        binding.document_revision,
        binding.analysis_generation,
        binding.content_hash,
    )
}

fn artifact_stem_parts(
    native_document_id: u64,
    document_revision: u64,
    analysis_generation: u64,
    content_hash: [u8; 32],
) -> String {
    let hash = content_hash
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{:016x}-r{}-g{}-{hash}",
        native_document_id, document_revision, analysis_generation
    )
}

fn first_free_analysis_generation(
    directory: &Path,
    native_document_id: u64,
    document_revision: u64,
    content_hash: [u8; 32],
    mut generation: u64,
) -> Result<u64, KernelError> {
    for _ in 0..MAX_RESTORE_FILES {
        let stem = artifact_stem_parts(
            native_document_id,
            document_revision,
            generation,
            content_hash,
        );
        let request = directory.join(format!("{stem}.request.{ANALYSIS_ARTIFACT_EXTENSION}"));
        if !request.exists() {
            return Ok(generation);
        }
        generation = generation
            .checked_add(1)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    }
    Err(KernelError::AnalysisProducerFailed(format!(
        "no free analysis generation within {} reserved artifacts",
        MAX_RESTORE_FILES
    )))
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

#[cfg(test)]
mod generation_tests {
    use super::*;

    #[test]
    fn partial_immutable_run_reserves_its_generation() {
        let directory = std::env::temp_dir().join(format!(
            "phoenix-analysis-generation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let hash = [0x5a; 32];
        let stem = artifact_stem_parts(7, 3, 11, hash);
        fs::write(
            directory.join(format!("{stem}.request.{ANALYSIS_ARTIFACT_EXTENSION}")),
            b"reserved",
        )
        .unwrap();
        assert_eq!(
            first_free_analysis_generation(&directory, 7, 3, hash, 11).unwrap(),
            12
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
