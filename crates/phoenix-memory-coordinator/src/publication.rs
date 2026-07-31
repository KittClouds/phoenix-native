use crate::CoordinatorError;
use phoenix_memory_contract::{OpenExpectation, PreparedMixedSource, VerifiedGraphGenerationV3};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationPublication {
    pub path: PathBuf,
    pub generation_hash: [u8; 32],
    pub source_set_hash: [u8; 32],
    pub published_generation: u64,
    pub source_count: u64,
    pub document_count: u64,
    pub conversation_count: u64,
    pub turn_count: u64,
    pub reused: bool,
}

pub(crate) fn publish_or_reuse(
    artifact_dir: &Path,
    state_hash: [u8; 32],
    namespace_hash: [u8; 32],
    prepared: PreparedMixedSource,
) -> Result<GenerationPublication, CoordinatorError> {
    fs::create_dir_all(artifact_dir)
        .map_err(|source| CoordinatorError::io(artifact_dir.to_path_buf(), source))?;
    let path = artifact_dir.join(format!("generation-{}.phxgg3", hex(&state_hash)));
    if path.exists() {
        let generation = VerifiedGraphGenerationV3::open_expected(
            &path,
            OpenExpectation {
                namespace_hash: Some(namespace_hash),
                ..OpenExpectation::default()
            },
        )?;
        return Ok(receipt(path, &generation, true));
    }
    let generation = prepared.write(&path)?;
    Ok(receipt(path, &generation, false))
}

fn receipt(
    path: PathBuf,
    generation: &VerifiedGraphGenerationV3,
    reused: bool,
) -> GenerationPublication {
    let header = generation.header();
    GenerationPublication {
        path,
        generation_hash: header.generation_hash,
        source_set_hash: header.source_set_hash,
        published_generation: header.published_generation,
        source_count: header.source_count,
        document_count: header.document_revision_count,
        conversation_count: header.conversation_count,
        turn_count: header.turn_count,
        reused,
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
