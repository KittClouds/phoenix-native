use super::*;
use phoenix_analysis_contract::{
    open_analysis_artifact, open_nli_artifact, write_message_new, write_nli_artifact_new,
    AnalysisEntityKind, DocumentAnalysisBinding, DocumentAnalysisRequestBinding,
    PhoenixAnalysisRequestV1, PhoenixDocumentAnalysisV1, PhoenixNliArtifactV1,
    VerifiedAnalysisArtifact, ANALYSIS_ARTIFACT_EXTENSION, ANALYSIS_CONTRACT,
};
use phoenix_scene_contract::EntityKind;
use std::fs;
use std::process::Command;

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
    pub entity_count: u32,
    pub mention_count: u32,
    pub nli_candidate_count: u32,
    pub nli_adjudication_count: u32,
    pub promotion_count: u32,
}

#[derive(Clone, Debug)]
pub struct NliPublication {
    pub analysis_artifact_hash: [u8; 32],
    pub nli_artifact_hash: [u8; 32],
    pub artifact: Arc<PhoenixNliArtifactV1>,
    pub entity_count: u32,
    pub mention_count: u32,
}

#[derive(Clone, Debug)]
pub struct LegacyAnalysisAdapterConfig {
    pub executable: PathBuf,
    pub ner_model_root: PathBuf,
    pub nli_model_root: PathBuf,
    pub source_document_id: Option<String>,
    pub max_nli_candidates: u32,
}

pub(super) struct RestoredAnalysis {
    pub nli: Arc<PhoenixNliArtifactV1>,
    pub receipt: AnalysisPublicationReceipt,
}

impl LegacyAnalysisAdapterConfig {
    pub fn from_env() -> Result<Self, KernelError> {
        Ok(Self {
            executable: required_path("PHOENIX_NATIVE_ANALYSIS_BRIDGE")?,
            ner_model_root: required_path("PHOENIX_NATIVE_NER_MODEL_ROOT")?,
            nli_model_root: required_path("PHOENIX_NATIVE_NLI_MODEL_ROOT")?,
            source_document_id: std::env::var("PHOENIX_NATIVE_SOURCE_DOCUMENT_ID").ok(),
            max_nli_candidates: std::env::var("PHOENIX_NATIVE_MAX_NLI_CANDIDATES")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_NLI_CANDIDATES),
        })
    }
}

impl PhoenixKernel {
    pub fn analyze_active_document(
        &self,
        generation: u64,
    ) -> Result<AnalysisPublicationReceipt, KernelError> {
        let config = LegacyAnalysisAdapterConfig::from_env()?;
        self.analyze_active_document_with(generation, &config)
    }

    pub fn analyze_active_document_with(
        &self,
        generation: u64,
        config: &LegacyAnalysisAdapterConfig,
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
        write_message_new(&request_path, &request)?;
        let status = Command::new(&config.executable)
            .arg("analyze")
            .arg(&request_path)
            .arg(&output_path)
            .status()
            .map_err(|error| {
                KernelError::AnalysisProducerFailed(format!(
                    "start {}: {error}",
                    config.executable.display()
                ))
            })?;
        if !status.success() {
            return Err(KernelError::AnalysisProducerFailed(format!(
                "{} exited with {status}",
                config.executable.display()
            )));
        }
        let verified = open_analysis_artifact(&output_path)?;
        self.publish_verified_analysis(verified, &output_path)
    }

    pub fn publish_verified_analysis(
        &self,
        verified: VerifiedAnalysisArtifact,
        analysis_path: &Path,
    ) -> Result<AnalysisPublicationReceipt, KernelError> {
        let analysis = Arc::clone(verified.analysis());
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
        let receipt = publication_receipt(&analysis, verified.artifact_hash(), nli_artifact_hash)?;
        self.execute(KernelCommand::PublishNliArtifact(NliPublication {
            analysis_artifact_hash: verified.artifact_hash(),
            nli_artifact_hash,
            artifact: Arc::new(analysis.nli.clone()),
            entity_count: receipt.entity_count,
            mention_count: receipt.mention_count,
        }))?;
        Ok(receipt)
    }
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
    };
    state.nli_analysis = Some(publication.artifact);
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
        let receipt = publication_receipt(
            analysis,
            verified.artifact_hash(),
            verified_nli.artifact_hash(),
        )?;
        return Ok(Some(RestoredAnalysis {
            nli: Arc::clone(verified_nli.nli()),
            receipt,
        }));
    }
    Ok(None)
}

fn publication_receipt(
    analysis: &PhoenixDocumentAnalysisV1,
    analysis_artifact_hash: [u8; 32],
    nli_artifact_hash: [u8; 32],
) -> Result<AnalysisPublicationReceipt, KernelError> {
    Ok(AnalysisPublicationReceipt {
        native_document_id: analysis.ner.binding.native_document_id,
        document_revision: analysis.ner.binding.document_revision,
        analysis_generation: analysis.ner.binding.analysis_generation,
        registry_revision: analysis.ner.binding.target_registry_revision,
        analysis_artifact_hash,
        nli_artifact_hash,
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
    })
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

fn required_path(name: &'static str) -> Result<PathBuf, KernelError> {
    let value = std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .ok_or(KernelError::AnalysisProducerUnavailable(name))?;
    Ok(PathBuf::from(value))
}
