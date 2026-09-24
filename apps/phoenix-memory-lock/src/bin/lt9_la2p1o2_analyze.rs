//! Frozen P1O2 candidate-specific compatibility probe.
//! Reads feature rows only after the external label and graph-sufficiency seals.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "lt9_la2p1o2_analyze_core.rs"]
mod analysis_core;
use analysis_core::{
    analyze_view, contains_candidate_token, label_counts, make_features, triangles, Example, Label,
    LabelCounts, Triangle, View, ViewReceipt,
};

#[derive(Deserialize)]
struct Judgment {
    packet_id: String,
    judgment: Label,
}

#[derive(Deserialize)]
struct LedgerRow {
    packet_id: String,
    candidate_id: String,
    split: String,
    overlap_band: String,
    lexical_pair: [String; 2],
    left_occurrence: Occurrence,
    right_occurrence: Occurrence,
    pair_features: PairFeatures,
}

#[derive(Deserialize)]
struct Occurrence {
    node_id: String,
}

#[derive(Deserialize)]
struct PairFeatures {
    shared_tokens: Vec<String>,
    shared_role_tokens: Vec<String>,
    shared_bigrams: Vec<String>,
    shared_trigrams: Vec<String>,
    token_jaccard: f64,
    bigram_jaccard: f64,
    trigram_jaccard: f64,
    role_count_abs_delta: [u16; 3],
    token_count_abs_delta: u16,
    near_count_abs_delta: u16,
    support_cue_equal: bool,
    contradiction_cue_equal: bool,
    same_field_kind: bool,
}

#[derive(Serialize)]
struct AnalysisReceipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    review_mode: String,
    pre_review_root_sha256: String,
    validation_sha256: String,
    sufficiency_sha256: String,
    judgments_sha256: String,
    private_ledger_sha256: String,
    analyzer_source_sha256: String,
    analyzer_manifest_sha256: String,
    analyzer_lockfile_sha256: String,
    analyzer_binary_sha256: String,
    candidate_id: String,
    lexical_pair: [String; 2],
    fit_graph_rows: usize,
    holdout_graph_rows: usize,
    fit_label_counts: LabelCounts,
    holdout_label_counts: LabelCounts,
    views: Vec<ViewReceipt>,
    triangles: Vec<Triangle>,
    authority_updated: bool,
    retrieval_run: bool,
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(args.len() == 7, "usage: lt9_la2p1o2_analyze <private-ledger.json> <review-pass1.json> <validation.json> <sufficiency.json> <pre-review-root.json> <new-output-dir>");
    let ledger_path = PathBuf::from(&args[1]);
    let judgments_path = PathBuf::from(&args[2]);
    let validation_path = PathBuf::from(&args[3]);
    let sufficiency_path = PathBuf::from(&args[4]);
    let root_path = PathBuf::from(&args[5]);
    let output_dir = PathBuf::from(&args[6]);
    ensure!(
        !output_dir.exists(),
        "refusing to overwrite output directory {}",
        output_dir.display()
    );

    let validation: Value = read_json(&validation_path)?;
    let sufficiency: Value = read_json(&sufficiency_path)?;
    let root: Value = read_json(&root_path)?;
    ensure!(
        validation["status"] == "LABELS_VALIDATED_AND_SEALED",
        "label validation seal missing"
    );
    ensure!(
        sufficiency["status"] == "ONE_CANDIDATE_ELIGIBLE_FOR_FROZEN_DIAGNOSTIC",
        "sufficiency seal missing"
    );
    ensure!(
        sufficiency["feature_fit_authorized"] == true,
        "feature fit not authorized by sufficiency seal"
    );
    ensure!(
        root["status"] == "BLIND_PACKETS_READY_WITH_CANDIDATE_SHORTFALL",
        "unexpected pre-review root status"
    );
    ensure!(
        validation["pre_review_root_sha256"] == sha256(&root_path)?,
        "validation root hash mismatch"
    );
    ensure!(
        sufficiency["judgments_sha256"] == sha256(&judgments_path)?,
        "judgment hash differs from sufficiency seal"
    );
    ensure!(
        sufficiency["private_ledger_sha256"] == sha256(&ledger_path)?,
        "private ledger hash differs from sufficiency seal"
    );
    ensure!(
        sufficiency["validation_receipt_sha256"] == sha256(&validation_path)?,
        "validation receipt hash differs from sufficiency seal"
    );
    ensure!(
        sufficiency["pre_review_root_sha256"] == sha256(&root_path)?,
        "pre-review root hash differs from sufficiency seal"
    );

    let judgments: Vec<Judgment> = read_json(&judgments_path)?;
    let ledger: Vec<LedgerRow> = read_json(&ledger_path)?;
    let eligible: HashSet<String> = sufficiency["eligible_candidates"]
        .as_array()
        .context("eligible_candidates missing")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    ensure!(
        eligible.len() == 1,
        "expected exactly one eligible candidate"
    );
    let labels: HashMap<String, Label> = judgments
        .into_iter()
        .map(|row| (row.packet_id, row.judgment))
        .collect();
    ensure!(labels.len() == 30, "expected 30 unique sealed labels");
    ensure!(ledger.len() == 30, "expected 30 private edges");
    let mut full_rows = Vec::with_capacity(ledger.len());
    let mut reduced_rows = Vec::with_capacity(ledger.len());
    for row in ledger {
        if !eligible.contains(&row.candidate_id) {
            continue;
        }
        let label = *labels
            .get(&row.packet_id)
            .with_context(|| format!("no judgment for packet {}", row.packet_id))?;
        ensure!(
            row.split == "fit" || row.split == "holdout",
            "unknown graph split"
        );
        let a = row.lexical_pair[0].as_str();
        let b = row.lexical_pair[1].as_str();
        for token in row
            .pair_features
            .shared_tokens
            .iter()
            .chain(row.pair_features.shared_role_tokens.iter())
            .chain(row.pair_features.shared_bigrams.iter())
            .chain(row.pair_features.shared_trigrams.iter())
        {
            ensure!(
                !contains_candidate_token(token, a, b),
                "candidate token leaked into pair features"
            );
        }
        let common = Example {
            packet_id: row.packet_id,
            candidate_id: row.candidate_id,
            split: row.split,
            overlap_band: row.overlap_band,
            left_node: row.left_occurrence.node_id,
            right_node: row.right_occurrence.node_id,
            label,
            features: Default::default(),
        };
        let mut full = common.clone();
        full.features = make_features(&row.pair_features, View::FullLocal);
        let mut reduced = common;
        reduced.features = make_features(&row.pair_features, View::NoExactOverlap);
        full_rows.push(full);
        reduced_rows.push(reduced);
    }

    let fit: Vec<&Example> = full_rows.iter().filter(|row| row.split == "fit").collect();
    let holdout: Vec<&Example> = full_rows
        .iter()
        .filter(|row| row.split == "holdout")
        .collect();
    let fit_counts = label_counts(&fit);
    let holdout_counts = label_counts(&holdout);
    let candidate_id = full_rows[0].candidate_id.clone();
    let lexical_pair = ["save".to_owned(), "spare".to_owned()];
    let views = vec![
        analyze_view(&full_rows, View::FullLocal),
        analyze_view(&reduced_rows, View::NoExactOverlap),
    ];
    let triangles = triangles(&full_rows);
    ensure!(
        triangles.len() == 40,
        "expected all 40 triangles from two complete six-node graphs, found {}",
        triangles.len()
    );

    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_analyze.rs");
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let lockfile_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let binary_path = env::current_exe()?;
    let receipt = AnalysisReceipt {
        schema: "phoenix.lexical.lt9-la2-p1o2-analysis/v1",
        date: "2026-09-23",
        status: "DISCOVERY_ONLY_HOLDOUT_DIAGNOSTIC",
        review_mode: validation["review_mode"]
            .as_str()
            .unwrap_or("UNKNOWN")
            .to_owned(),
        pre_review_root_sha256: sha256(&root_path)?,
        validation_sha256: sha256(&validation_path)?,
        sufficiency_sha256: sha256(&sufficiency_path)?,
        judgments_sha256: sha256(&judgments_path)?,
        private_ledger_sha256: sha256(&ledger_path)?,
        analyzer_source_sha256: sha256(&source_path)?,
        analyzer_manifest_sha256: sha256(&manifest_path)?,
        analyzer_lockfile_sha256: sha256(&lockfile_path)?,
        analyzer_binary_sha256: sha256(&binary_path)?,
        candidate_id,
        lexical_pair,
        fit_graph_rows: fit.len(),
        holdout_graph_rows: holdout.len(),
        fit_label_counts: fit_counts,
        holdout_label_counts: holdout_counts,
        views,
        triangles,
        authority_updated: false,
        retrieval_run: false,
    };
    fs::create_dir_all(&output_dir)?;
    let receipt_path = output_dir.join("analysis-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    fs::write(&receipt_path, bytes)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    println!("analysis_receipt_sha256={}", sha256(&receipt_path)?);
    Ok(())
}
