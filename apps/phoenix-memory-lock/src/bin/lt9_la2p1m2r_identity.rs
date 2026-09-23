//! Prepare and validate P1M2R outcome-replay identities without opening outcomes.

use super::{hash_file, validate_screen, DISCOVERY_IDS, EXPECTED_PROTOCOL_SHA256, SCREEN_SHA256};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

const MANIFEST_SCHEMA: &str = "phoenix.lexical.lt9-la2-p1m2r-prepared-replay/v1";

fn hash_path(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn path_text(path: &Path) -> Result<String> {
    Ok(path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))?
        .to_string_lossy()
        .into_owned())
}

fn protocol_hash(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read protocol {}", path.display()))?;
    let hash = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        hash == EXPECTED_PROTOCOL_SHA256,
        "P1M2R outcome protocol hash mismatch"
    );
    Ok(hash)
}

fn corpus_inputs(screen: &Value) -> Result<Vec<Value>> {
    let rows = screen["corpora"]
        .as_array()
        .context("screen missing corpus rows")?;
    ensure!(
        rows.len() == DISCOVERY_IDS.len(),
        "P1M2R corpus count mismatch"
    );
    let mut archive_cache = BTreeMap::<PathBuf, (String, u64)>::new();
    let mut inputs = Vec::with_capacity(rows.len());
    for (row, id) in rows.iter().zip(DISCOVERY_IDS) {
        ensure!(row["corpus_id"].as_str() == Some(id), "corpus order drift");
        let corpus_path = PathBuf::from(
            row["corpus_path"]
                .as_str()
                .context("screen corpus path missing")?,
        );
        let archive_path = PathBuf::from(
            row["archive_path"]
                .as_str()
                .context("screen archive path missing")?,
        );
        let corpus_hash = hash_file(&corpus_path)?;
        ensure!(
            row["corpus_sha256"].as_str() == Some(corpus_hash.as_str()),
            "normalized corpus identity changed for {id}"
        );
        let (archive_hash, archive_bytes) = if let Some(cached) = archive_cache.get(&archive_path) {
            cached.clone()
        } else {
            let digest = hash_file(&archive_path)?;
            let bytes = archive_path.metadata()?.len();
            let value = (digest, bytes);
            archive_cache.insert(archive_path.clone(), value.clone());
            value
        };
        ensure!(
            row["archive_sha256"].as_str() == Some(archive_hash.as_str()),
            "source archive identity changed for {id}"
        );
        ensure!(
            row["archive_bytes"].as_u64() == Some(archive_bytes),
            "source archive length changed for {id}"
        );
        inputs.push(json!({
            "corpus_id": id,
            "corpus_path": path_text(&corpus_path)?,
            "corpus_sha256": corpus_hash,
            "archive_path": path_text(&archive_path)?,
            "archive_sha256": archive_hash,
            "archive_bytes": archive_bytes,
        }));
    }
    Ok(inputs)
}

pub(super) fn prepare(
    protocol_path: &Path,
    screen_path: &Path,
    source_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let protocol_sha256 = protocol_hash(protocol_path)?;
    let (screen, screen_sha256) = validate_screen(screen_path)?;
    ensure!(
        screen_sha256 == SCREEN_SHA256,
        "unexpected frozen screen receipt"
    );
    ensure!(
        screen["validity_labels_opened"] == false
            && screen["qrels_or_queries_opened"] == false
            && screen["reserved_qualification_labels_opened"] == false,
        "screen firewall flags are not closed"
    );
    let source_sha256 = hash_path(source_path)?;
    let executable_path = std::env::current_exe()?;
    let executable_sha256 = hash_path(&executable_path)?;
    let inputs = corpus_inputs(&screen)?;
    let manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "date": "2026-09-23",
        "protocol_path": path_text(protocol_path)?,
        "protocol_sha256": protocol_sha256,
        "screen_path": path_text(screen_path)?,
        "screen_sha256": screen_sha256,
        "source_path": path_text(source_path)?,
        "source_sha256": source_sha256,
        "executable_path": executable_path.to_string_lossy(),
        "executable_sha256": executable_sha256,
        "corpora": inputs,
        "validity_labels_opened": false,
        "outcome_replay_started": false,
        "qrels_or_queries_opened": false,
        "reserved_qualification_labels_opened": false,
    });
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

pub(super) fn validate_before_replay(
    protocol_path: &Path,
    manifest_path: &Path,
    screen_path: &Path,
    screen: &Value,
    screen_sha256: &str,
) -> Result<String> {
    let protocol_sha256 = protocol_hash(protocol_path)?;
    let bytes = std::fs::read(manifest_path)
        .with_context(|| format!("read prepared manifest {}", manifest_path.display()))?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let manifest: Value = serde_json::from_slice(&bytes)?;
    ensure!(manifest["schema"].as_str() == Some(MANIFEST_SCHEMA));
    ensure!(manifest["protocol_sha256"].as_str() == Some(protocol_sha256.as_str()));
    ensure!(manifest["screen_sha256"].as_str() == Some(screen_sha256));
    ensure!(manifest["screen_sha256"].as_str() == Some(SCREEN_SHA256));
    ensure!(manifest["validity_labels_opened"] == false);
    ensure!(manifest["outcome_replay_started"] == false);
    ensure!(manifest["qrels_or_queries_opened"] == false);
    ensure!(manifest["reserved_qualification_labels_opened"] == false);
    ensure!(manifest["protocol_path"].as_str() == Some(path_text(protocol_path)?.as_str()));
    ensure!(manifest["screen_path"].as_str() == Some(path_text(screen_path)?.as_str()));

    let source_path = PathBuf::from(
        manifest["source_path"]
            .as_str()
            .context("prepared source path missing")?,
    );
    ensure!(
        manifest["source_sha256"].as_str() == Some(hash_path(&source_path)?.as_str()),
        "replay source changed after preparation"
    );
    let executable_path = std::env::current_exe()?;
    ensure!(
        manifest["executable_sha256"].as_str() == Some(hash_path(&executable_path)?.as_str()),
        "replay executable changed after preparation"
    );

    let expected_inputs = corpus_inputs(screen)?;
    ensure!(
        manifest["corpora"] == Value::Array(expected_inputs),
        "normalized corpus or source archive changed after preparation"
    );
    Ok(manifest_sha256)
}
