use crate::{
    PhoenixDocumentAnalysisV1, PhoenixNliArtifactV1, PhoenixProducerCoordinatorV1,
    PhoenixStructuralSubstrateV1, ANALYSIS_FORMAT_VERSION,
};
use memmap2::Mmap;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

const MAGIC: [u8; 8] = *b"PHXANL01";
const HEADER_LEN: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
pub const ANALYSIS_ARTIFACT_EXTENSION: &str = "pnaa";

#[derive(Debug, Error)]
pub enum AnalysisContractError {
    #[error("analysis artifact I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("analysis artifact header is invalid")]
    InvalidHeader,
    #[error("analysis artifact version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("analysis artifact payload is oversized: {0} bytes")]
    Oversized(usize),
    #[error("analysis artifact payload hash mismatch")]
    HashMismatch,
    #[error("analysis artifact codec failed: {0}")]
    Codec(String),
    #[error("analysis artifact contract failed: {0}")]
    Contract(&'static str),
}

pub struct VerifiedAnalysisArtifact {
    mmap: Arc<Mmap>,
    artifact_hash: [u8; 32],
    analysis: Arc<PhoenixDocumentAnalysisV1>,
}

pub struct VerifiedNliArtifact {
    mmap: Arc<Mmap>,
    artifact_hash: [u8; 32],
    nli: Arc<PhoenixNliArtifactV1>,
}

pub struct VerifiedStructuralArtifact {
    mmap: Arc<Mmap>,
    artifact_hash: [u8; 32],
    structural: Arc<PhoenixStructuralSubstrateV1>,
}

pub struct VerifiedProducerCoordinator {
    mmap: Arc<Mmap>,
    artifact_hash: [u8; 32],
    coordinator: Arc<PhoenixProducerCoordinatorV1>,
}

impl VerifiedAnalysisArtifact {
    pub fn artifact_hash(&self) -> [u8; 32] {
        self.artifact_hash
    }

    pub fn analysis(&self) -> &Arc<PhoenixDocumentAnalysisV1> {
        &self.analysis
    }

    pub fn mapped_bytes(&self) -> usize {
        self.mmap.len()
    }
}

impl VerifiedNliArtifact {
    pub fn artifact_hash(&self) -> [u8; 32] {
        self.artifact_hash
    }

    pub fn nli(&self) -> &Arc<PhoenixNliArtifactV1> {
        &self.nli
    }

    pub fn mapped_bytes(&self) -> usize {
        self.mmap.len()
    }
}

impl VerifiedStructuralArtifact {
    pub fn artifact_hash(&self) -> [u8; 32] {
        self.artifact_hash
    }

    pub fn structural(&self) -> &Arc<PhoenixStructuralSubstrateV1> {
        &self.structural
    }

    pub fn mapped_bytes(&self) -> usize {
        self.mmap.len()
    }
}

impl VerifiedProducerCoordinator {
    pub fn artifact_hash(&self) -> [u8; 32] {
        self.artifact_hash
    }

    pub fn coordinator(&self) -> &Arc<PhoenixProducerCoordinatorV1> {
        &self.coordinator
    }

    pub fn mapped_bytes(&self) -> usize {
        self.mmap.len()
    }
}

pub fn write_analysis_artifact_new(
    path: &Path,
    analysis: &PhoenixDocumentAnalysisV1,
) -> Result<[u8; 32], AnalysisContractError> {
    analysis
        .validate()
        .map_err(AnalysisContractError::Contract)?;
    write_message_new(path, analysis)
}

pub fn open_analysis_artifact(
    path: &Path,
) -> Result<VerifiedAnalysisArtifact, AnalysisContractError> {
    let (mmap, artifact_hash, analysis) = open_message::<PhoenixDocumentAnalysisV1>(path)?;
    analysis
        .validate()
        .map_err(AnalysisContractError::Contract)?;
    Ok(VerifiedAnalysisArtifact {
        mmap,
        artifact_hash,
        analysis: Arc::new(analysis),
    })
}

pub fn write_nli_artifact_new(
    path: &Path,
    nli: &PhoenixNliArtifactV1,
) -> Result<[u8; 32], AnalysisContractError> {
    nli.validate().map_err(AnalysisContractError::Contract)?;
    write_message_new(path, nli)
}

pub fn open_nli_artifact(path: &Path) -> Result<VerifiedNliArtifact, AnalysisContractError> {
    let (mmap, artifact_hash, nli) = open_message::<PhoenixNliArtifactV1>(path)?;
    nli.validate().map_err(AnalysisContractError::Contract)?;
    Ok(VerifiedNliArtifact {
        mmap,
        artifact_hash,
        nli: Arc::new(nli),
    })
}

pub fn write_structural_artifact_new(
    path: &Path,
    structural: &PhoenixStructuralSubstrateV1,
) -> Result<[u8; 32], AnalysisContractError> {
    structural
        .validate()
        .map_err(AnalysisContractError::Contract)?;
    write_message_new(path, structural)
}

pub fn open_structural_artifact(
    path: &Path,
) -> Result<VerifiedStructuralArtifact, AnalysisContractError> {
    let (mmap, artifact_hash, structural) = open_message::<PhoenixStructuralSubstrateV1>(path)?;
    structural
        .validate()
        .map_err(AnalysisContractError::Contract)?;
    Ok(VerifiedStructuralArtifact {
        mmap,
        artifact_hash,
        structural: Arc::new(structural),
    })
}

pub fn write_producer_coordinator_new(
    path: &Path,
    coordinator: &PhoenixProducerCoordinatorV1,
) -> Result<[u8; 32], AnalysisContractError> {
    write_message_new(path, coordinator)
}

pub fn open_producer_coordinator(
    path: &Path,
) -> Result<VerifiedProducerCoordinator, AnalysisContractError> {
    let (mmap, artifact_hash, coordinator) = open_message::<PhoenixProducerCoordinatorV1>(path)?;
    Ok(VerifiedProducerCoordinator {
        mmap,
        artifact_hash,
        coordinator: Arc::new(coordinator),
    })
}

pub fn write_message_new<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<[u8; 32], AnalysisContractError> {
    let payload = postcard::to_allocvec(value)
        .map_err(|error| AnalysisContractError::Codec(error.to_string()))?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(AnalysisContractError::Oversized(payload.len()));
    }
    let payload_hash = *blake3::hash(&payload).as_bytes();
    let mut header = [0_u8; HEADER_LEN];
    header[..8].copy_from_slice(&MAGIC);
    header[8..12].copy_from_slice(&ANALYSIS_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    header[24..56].copy_from_slice(&payload_hash);
    let parent = path.parent().ok_or_else(|| AnalysisContractError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "artifact path has no parent"),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| AnalysisContractError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| AnalysisContractError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&header)
        .and_then(|_| file.write_all(&payload))
        .and_then(|_| file.sync_all())
        .map_err(|source| AnalysisContractError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(payload_hash)
}

pub fn open_message<T: DeserializeOwned>(
    path: &Path,
) -> Result<(Arc<Mmap>, [u8; 32], T), AnalysisContractError> {
    let file = File::open(path).map_err(|source| AnalysisContractError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // SAFETY: The file is immutable for the lifetime of this mapping by contract.
    let mmap = unsafe { Mmap::map(&file) }.map_err(|source| AnalysisContractError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if mmap.len() < HEADER_LEN || mmap[..8] != MAGIC {
        return Err(AnalysisContractError::InvalidHeader);
    }
    let version = read_u32(&mmap[8..12]);
    if version != ANALYSIS_FORMAT_VERSION {
        return Err(AnalysisContractError::UnsupportedVersion(version));
    }
    if read_u32(&mmap[12..16]) as usize != HEADER_LEN {
        return Err(AnalysisContractError::InvalidHeader);
    }
    let payload_len = usize::try_from(read_u64(&mmap[16..24]))
        .map_err(|_| AnalysisContractError::InvalidHeader)?;
    if payload_len > MAX_PAYLOAD_BYTES {
        return Err(AnalysisContractError::Oversized(payload_len));
    }
    let end = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(AnalysisContractError::InvalidHeader)?;
    if end != mmap.len() {
        return Err(AnalysisContractError::InvalidHeader);
    }
    let payload = &mmap[HEADER_LEN..end];
    let artifact_hash = *blake3::hash(payload).as_bytes();
    if mmap[24..56] != artifact_hash {
        return Err(AnalysisContractError::HashMismatch);
    }
    let value = postcard::from_bytes(payload)
        .map_err(|error| AnalysisContractError::Codec(error.to_string()))?;
    Ok((Arc::new(mmap), artifact_hash, value))
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut raw = [0_u8; 4];
    raw.copy_from_slice(bytes);
    u32::from_le_bytes(raw)
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(bytes);
    u64::from_le_bytes(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn identity(id: &str, seed: u8) -> AnalysisModelIdentity {
        AnalysisModelIdentity {
            model_id: id.into(),
            artifact_hash: [seed; 32],
            config_hash: [seed.wrapping_add(1); 32],
            runtime_id: "test".into(),
        }
    }

    fn fixture() -> PhoenixDocumentAnalysisV1 {
        let binding = DocumentAnalysisBinding {
            source_document_id: "note".into(),
            native_document_id: 3,
            document_revision: 1,
            content_hash: [4; 32],
            analysis_generation: 1,
            source_registry_revision: 1,
            target_registry_revision: 2,
            producer_binary_hash: [5; 32],
            chunker: identity("chunker", 6),
            dynamic_ner: identity("ner", 8),
            nli: identity("nli", 10),
        };
        PhoenixDocumentAnalysisV1 {
            schema: ANALYSIS_CONTRACT.into(),
            ner: PhoenixNerArtifactV1 {
                binding: binding.clone(),
                ner_revision: 1,
                entities: vec![AnalysisEntity {
                    stable_id: 7,
                    label: "Ryan".into(),
                    kind: AnalysisEntityKind::Character,
                    custom_kind: None,
                    mention_count: 1,
                }],
                mentions: vec![AnalysisMention {
                    mention_id: 1,
                    entity_id: 7,
                    start: 0,
                    end: 4,
                    sentence_index: 0,
                    confidence: 0.9,
                    accepted: true,
                }],
                receipt: AnalysisStageReceipt {
                    chunk_count: 1,
                    sentence_count: 1,
                    mention_count: 1,
                    entity_count: 1,
                    nli_candidate_count: 0,
                    nli_adjudication_count: 0,
                    chunker_micros: 1,
                    dynamic_ner_micros: 1,
                    nli_load_micros: 1,
                    nli_adjudication_micros: 1,
                    promotion_count: 0,
                },
            },
            nli: PhoenixNliArtifactV1 {
                binding,
                nli_candidates: Vec::new(),
                nli_adjudications: Vec::new(),
                promotion_count: 0,
            },
        }
    }

    #[test]
    fn sealed_artifact_roundtrips_and_detects_corruption() {
        let root =
            std::env::temp_dir().join(format!("phoenix-analysis-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("analysis.pnaa");
        let expected = fixture();
        write_analysis_artifact_new(&path, &expected).expect("write");
        let opened = open_analysis_artifact(&path).expect("open");
        assert_eq!(opened.analysis().as_ref(), &expected);
        drop(opened);
        let mut bytes = std::fs::read(&path).expect("read");
        *bytes.last_mut().expect("payload") ^= 1;
        std::fs::write(&path, bytes).expect("corrupt");
        assert!(matches!(
            open_analysis_artifact(&path),
            Err(AnalysisContractError::HashMismatch)
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn nli_artifact_is_candidate_only_and_bound_to_the_ner_authority() {
        let mut artifact = fixture();
        artifact.nli.promotion_count = 1;
        assert_eq!(
            artifact.validate(),
            Err("NLI counts or candidate-only invariant are invalid")
        );
        let mut artifact = fixture();
        artifact.nli.binding.analysis_generation += 1;
        assert_eq!(
            artifact.validate(),
            Err("NER and NLI artifacts do not share one authority binding")
        );
    }
}
