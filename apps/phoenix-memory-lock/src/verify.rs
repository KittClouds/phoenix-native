use std::path::Path;

use anyhow::{bail, Context, Result};
use hashbrown::HashSet;
use serde::Serialize;

use crate::artifact::read_json;
use crate::model::{FreezeManifest, FREEZE_CONTRACT};

pub fn load_and_verify(path: &Path) -> Result<(FreezeManifest, VerificationReceipt)> {
    let manifest: FreezeManifest = read_json(path)?;
    verify(&manifest)?;
    let receipt = VerificationReceipt {
        contract: manifest.contract.clone(),
        freeze_id: manifest.freeze_id.clone(),
        mentedb_commit: manifest.behavioral_reference.commit.clone(),
        longmemeval_commit: manifest.benchmark.commit.clone(),
        dataset_revision: manifest.benchmark.dataset_revision.clone(),
        dataset_count: manifest.benchmark.datasets.len(),
        source_lock_count: manifest.behavioral_reference.source_files.len()
            + manifest.benchmark.source_files.len(),
        prompt_profile_count: manifest.benchmark.prompt_profiles.len(),
        model_profile_count: manifest.benchmark.model_profiles.len(),
        runtime_dependency: false,
        gold_firewall: "typed-artifacts-and-distinct-magic",
    };
    Ok((manifest, receipt))
}

fn verify(manifest: &FreezeManifest) -> Result<()> {
    if manifest.contract != FREEZE_CONTRACT {
        bail!("unsupported freeze contract {}", manifest.contract);
    }
    if manifest.freeze_id.trim().is_empty() {
        bail!("freeze ID is empty");
    }
    let reference = &manifest.behavioral_reference;
    if reference.name != "MenteDB" || reference.runtime_dependency {
        bail!("MenteDB must be a behavioral reference with no runtime dependency");
    }
    if reference.workspace_version != "0.35.0" {
        bail!("MenteDB workspace-version drift");
    }
    verify_commit("MenteDB", &reference.commit)?;
    verify_commit("LongMemEval", &manifest.benchmark.commit)?;
    verify_commit("dataset revision", &manifest.benchmark.dataset_revision)?;
    if reference.repository != "https://github.com/nambok/mentedb"
        || manifest.benchmark.repository != "https://github.com/xiaowu0162/LongMemEval"
    {
        bail!("upstream repository identity drift");
    }
    if manifest.benchmark.name != "LongMemEval"
        || manifest.benchmark.dataset_repository
            != "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned"
    {
        bail!("benchmark or dataset repository identity drift");
    }
    if reference.license != "Apache-2.0" || manifest.benchmark.license != "MIT" {
        bail!("upstream license identity drift");
    }
    verify_source_locks(&reference.source_files)?;
    verify_source_locks(&manifest.benchmark.source_files)?;
    let variants = manifest
        .benchmark
        .datasets
        .iter()
        .map(|dataset| dataset.variant.as_str())
        .collect::<HashSet<_>>();
    if variants != HashSet::from(["small", "medium", "oracle"]) {
        bail!("dataset lock must contain exactly small, medium, and oracle");
    }
    for dataset in &manifest.benchmark.datasets {
        verify_sha256(&dataset.sha256)
            .with_context(|| format!("invalid dataset hash for {}", dataset.variant))?;
        if dataset.bytes == 0
            || !dataset.url.contains(&manifest.benchmark.dataset_revision)
            || !dataset.url.ends_with(&dataset.filename)
        {
            bail!("invalid dataset lock for {}", dataset.variant);
        }
    }
    if manifest.benchmark.baseline_variant != "small" {
        bail!("Cut 0 baseline must remain bound to cleaned small");
    }
    let firewall = &manifest.benchmark.gold_firewall;
    if firewall.workload_magic != "PHXLMW01"
        || firewall.gold_magic != "PHXLMG01"
        || firewall.retrieval_magic != "PHXLMR01"
    {
        bail!("gold firewall magic drift");
    }
    let forbidden = firewall
        .forbidden_workload_fields
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if forbidden != HashSet::from(["answer", "answer_session_ids", "has_answer"]) {
        bail!("gold firewall field set drift");
    }
    for prompt in &manifest.benchmark.prompt_profiles {
        verify_sha256(&prompt.source_sha256)?;
        let source = manifest
            .benchmark
            .source_files
            .iter()
            .find(|source| source.path == prompt.source_path)
            .with_context(|| format!("prompt {} has no source lock", prompt.id))?;
        if source.sha256 != prompt.source_sha256
            || prompt.symbol.is_empty()
            || prompt.mode.is_empty()
        {
            bail!("prompt {} is not bound to its source", prompt.id);
        }
    }
    let model_roles = manifest
        .benchmark
        .model_profiles
        .iter()
        .map(|profile| profile.role.as_str())
        .collect::<HashSet<_>>();
    for required in [
        "phoenix-cut0-retriever",
        "official-reader",
        "official-judge",
        "mentedb-reference-embedding",
    ] {
        if !model_roles.contains(required) {
            bail!("missing model profile {required}");
        }
    }
    for model in &manifest.benchmark.model_profiles {
        if model.provider.is_empty() || model.model.is_empty() || model.weights_sha256.is_empty() {
            bail!("incomplete model profile {}", model.role);
        }
    }
    Ok(())
}

fn verify_source_locks(files: &[crate::model::SourceFileLock]) -> Result<()> {
    let mut paths = HashSet::new();
    for file in files {
        if !paths.insert(file.path.as_str()) || file.bytes == 0 {
            bail!("invalid or duplicate source lock {}", file.path);
        }
        verify_sha256(&file.sha256)?;
    }
    Ok(())
}

fn verify_commit(label: &str, value: &str) -> Result<()> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{label} is not a lowercase 40-character Git commit");
    }
    Ok(())
}

fn verify_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("not a lowercase SHA-256: {value}");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct VerificationReceipt {
    pub contract: String,
    pub freeze_id: String,
    pub mentedb_commit: String,
    pub longmemeval_commit: String,
    pub dataset_revision: String,
    pub dataset_count: usize,
    pub source_lock_count: usize,
    pub prompt_profile_count: usize,
    pub model_profile_count: usize,
    pub runtime_dependency: bool,
    pub gold_firewall: &'static str,
}
