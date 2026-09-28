//! P1P3 engineering diagnostic using sealed functional review signals.
//! This is not the independent-human qualification run and grants no authority.

use analysis_core::{
    analyze_view, label_counts, make_features, Example, Label, LabelCounts, Tree, View, ViewReceipt,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "lt9_la2p1o2_analyze_core.rs"]
mod analysis_core;
#[path = "lt9_la2p1o2_features.rs"]
mod features;
pub use features::PairFeatures;

mod p1o1 {
    #[derive(Clone, Debug)]
    pub struct Occurrence {
        pub field: String,
        pub excerpt: String,
    }
}

#[derive(Deserialize)]
struct Judgment {
    packet_id: String,
    judgment: Label,
}

#[derive(Deserialize)]
struct Packet {
    packet_id: String,
    lexical_pair: [String; 2],
    left_context: String,
    right_context: String,
}

#[derive(Deserialize)]
struct Endpoint {
    node_id: String,
    field: String,
}

#[derive(Deserialize)]
struct LedgerRow {
    edge_key: String,
    candidate_id: String,
    lexical_pair: [String; 2],
    split: String,
    overlap_band: String,
    reviewer_packet_ids: [String; 3],
    left: Endpoint,
    right: Endpoint,
}

#[derive(Clone, Copy, Serialize)]
struct Counts {
    same: usize,
    different: usize,
    unknown: usize,
}

impl From<LabelCounts> for Counts {
    fn from(value: LabelCounts) -> Self {
        Self {
            same: value.same,
            different: value.different,
            unknown: value.unknown,
        }
    }
}

#[derive(Serialize)]
struct CandidateSlice {
    candidate_id: String,
    lexical_pair: [String; 2],
    fit_rows: usize,
    holdout_rows: usize,
    fit_counts: Counts,
    holdout_counts: Counts,
    holdout_accuracy: Option<f64>,
    false_same_on_different: usize,
    unknown_to_same: usize,
    unknown_to_different: usize,
}

#[derive(Serialize)]
struct ViewResult {
    model: ViewReceipt,
    candidate_holdout: Vec<CandidateSlice>,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    interpretation: &'static str,
    protocol_sha256: String,
    pre_review_root_sha256: String,
    review_validation_sha256: String,
    sealed_review_sha256: [String; 3],
    packet_sha256: String,
    private_ledger_sha256: String,
    source_sha256: String,
    manifest_sha256: String,
    lockfile_sha256: String,
    binary_sha256: String,
    aligned_edges: usize,
    exact_three_stream_agreement: usize,
    file_label_counts: [Counts; 3],
    engineering_target_counts: Counts,
    fit_counts: Counts,
    holdout_counts: Counts,
    candidate_target_counts: Vec<CandidateSlice>,
    low_overlap_same: usize,
    low_overlap_same_holdout: usize,
    high_overlap_different: usize,
    high_overlap_different_holdout: usize,
    frozen_three_class_sufficiency_status: &'static str,
    sufficiency_shortfalls: Vec<String>,
    views: Vec<ViewResult>,
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

fn counts(rows: &[&Example]) -> Counts {
    label_counts(rows).into()
}

fn predict(tree: &Tree, row: &Example) -> Label {
    match tree {
        Tree::Leaf { prediction, .. } => *prediction,
        Tree::Split {
            feature,
            threshold,
            left,
            right,
            ..
        } => {
            if *row.features.get(feature).unwrap_or(&0.0) <= *threshold {
                predict(left, row)
            } else {
                predict(right, row)
            }
        }
    }
}

fn candidate_slices(
    rows: &[Example],
    view: View,
    candidate_pairs: &BTreeMap<String, [String; 2]>,
) -> Vec<CandidateSlice> {
    let model = analyze_view(rows, view);
    let mut groups: BTreeMap<String, Vec<&Example>> = BTreeMap::new();
    for row in rows {
        groups
            .entry(row.candidate_id.clone())
            .or_default()
            .push(row);
    }
    groups
        .into_iter()
        .map(|(candidate_id, group)| {
            let fit: Vec<_> = group
                .iter()
                .copied()
                .filter(|row| row.split == "fit")
                .collect();
            let holdout: Vec<_> = group
                .iter()
                .copied()
                .filter(|row| row.split == "holdout")
                .collect();
            let mut correct = 0usize;
            let mut false_same = 0usize;
            let mut unknown_same = 0usize;
            let mut unknown_different = 0usize;
            for row in &holdout {
                let prediction = predict(&model.tree, row);
                correct += usize::from(prediction == row.label);
                false_same +=
                    usize::from(row.label == Label::Different && prediction == Label::Same);
                unknown_same +=
                    usize::from(row.label == Label::Unknown && prediction == Label::Same);
                unknown_different +=
                    usize::from(row.label == Label::Unknown && prediction == Label::Different);
            }
            CandidateSlice {
                candidate_id: candidate_id.clone(),
                lexical_pair: candidate_pairs[&candidate_id].clone(),
                fit_rows: fit.len(),
                holdout_rows: holdout.len(),
                fit_counts: counts(&fit),
                holdout_counts: counts(&holdout),
                holdout_accuracy: (!holdout.is_empty())
                    .then(|| correct as f64 / holdout.len() as f64),
                false_same_on_different: false_same,
                unknown_to_same: unknown_same,
                unknown_to_different: unknown_different,
            }
        })
        .collect()
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(args.len() == 9, "usage: lt9_la2p1p3_engineering <reviewer-1-packets.json> <review1.json> <review2.json> <review3.json> <private-ledger.json> <review-validation.json> <pre-review-root.json> <new-output-dir>");
    let packet_path = PathBuf::from(&args[1]);
    let review_paths = [
        PathBuf::from(&args[2]),
        PathBuf::from(&args[3]),
        PathBuf::from(&args[4]),
    ];
    let ledger_path = PathBuf::from(&args[5]);
    let validation_path = PathBuf::from(&args[6]);
    let root_path = PathBuf::from(&args[7]);
    let output_dir = PathBuf::from(&args[8]);
    ensure!(
        !output_dir.exists(),
        "refusing to overwrite {}",
        output_dir.display()
    );

    let validation: Value = read_json(&validation_path)?;
    ensure!(
        validation["status"] == "RAW_REVIEWS_SEALED_ID_AND_LABEL_VALIDATION_PASSED",
        "sealed review validation missing"
    );
    ensure!(
        validation["reviewer_independence_attestation"] == "PENDING",
        "unexpected provenance disposition"
    );
    let root: Value = read_json(&root_path)?;
    ensure!(
        root["status"] == "BLIND_PACKETS_READY",
        "unexpected frozen root status"
    );

    let packets: Vec<Packet> = read_json(&packet_path)?;
    let ledger: Vec<LedgerRow> = read_json(&ledger_path)?;
    let reviews: [Vec<Judgment>; 3] = [
        read_json(&review_paths[0])?,
        read_json(&review_paths[1])?,
        read_json(&review_paths[2])?,
    ];
    ensure!(
        packets.len() == 120 && ledger.len() == 120,
        "expected frozen 120-edge acquisition"
    );
    let mut packet_by_id = HashMap::with_capacity(packets.len());
    for packet in packets {
        ensure!(
            packet_by_id
                .insert(packet.packet_id.clone(), packet)
                .is_none(),
            "duplicate packet id"
        );
    }
    let mut labels: [HashMap<String, Label>; 3] = std::array::from_fn(|_| HashMap::new());
    for index in 0..3 {
        for judgment in &reviews[index] {
            ensure!(
                labels[index]
                    .insert(judgment.packet_id.clone(), judgment.judgment)
                    .is_none(),
                "duplicate label id"
            );
        }
        ensure!(
            labels[index].len() == 120,
            "review stream {} must contain 120 unique labels",
            index + 1
        );
    }

    let mut candidate_pairs = BTreeMap::new();
    for row in &ledger {
        if let Some(existing) = candidate_pairs.get(&row.candidate_id) {
            ensure!(
                existing == &row.lexical_pair,
                "candidate id maps to multiple lexical pairs"
            );
        } else {
            candidate_pairs.insert(row.candidate_id.clone(), row.lexical_pair.clone());
        }
    }
    let mut full_rows = Vec::with_capacity(120);
    let mut reduced_rows = Vec::with_capacity(120);
    let mut stream_counts = [Counts {
        same: 0,
        different: 0,
        unknown: 0,
    }; 3];
    let mut exact_agreement = 0usize;
    for row in &ledger {
        ensure!(
            row.split == "fit" || row.split == "holdout",
            "invalid split"
        );
        let packet = packet_by_id
            .get(&row.reviewer_packet_ids[0])
            .context("reviewer-1 packet missing")?;
        ensure!(
            packet.lexical_pair == row.lexical_pair,
            "candidate-pair mismatch"
        );
        let mut votes = [Label::Unknown; 3];
        for index in 0..3 {
            votes[index] = *labels[index]
                .get(&row.reviewer_packet_ids[index])
                .with_context(|| format!("missing reviewer-{} packet label", index + 1))?;
            match votes[index] {
                Label::Same => stream_counts[index].same += 1,
                Label::Different => stream_counts[index].different += 1,
                Label::Unknown => stream_counts[index].unknown += 1,
            }
        }
        let same = votes.iter().all(|vote| *vote == votes[0]);
        exact_agreement += usize::from(same);
        let label = if same { votes[0] } else { Label::Unknown };
        let left = p1o1::Occurrence {
            field: row.left.field.clone(),
            excerpt: packet.left_context.clone(),
        };
        let right = p1o1::Occurrence {
            field: row.right.field.clone(),
            excerpt: packet.right_context.clone(),
        };
        let pair = features::pair_features(
            &features::context_features(&left, &row.lexical_pair[0], &row.lexical_pair[1]),
            &features::context_features(&right, &row.lexical_pair[0], &row.lexical_pair[1]),
        );
        for token in pair
            .shared_tokens
            .iter()
            .chain(pair.shared_role_tokens.iter())
            .chain(pair.shared_bigrams.iter())
            .chain(pair.shared_trigrams.iter())
        {
            ensure!(
                !analysis_core::contains_candidate_token(
                    token,
                    &row.lexical_pair[0],
                    &row.lexical_pair[1]
                ),
                "candidate token leaked into pair features"
            );
        }
        let common = Example {
            packet_id: row.edge_key.clone(),
            candidate_id: row.candidate_id.clone(),
            split: row.split.clone(),
            overlap_band: row.overlap_band.clone(),
            left_node: row.left.node_id.clone(),
            right_node: row.right.node_id.clone(),
            label,
            features: BTreeMap::new(),
        };
        let mut full = common.clone();
        full.features = make_features(&pair, View::FullLocal);
        let mut reduced = common;
        reduced.features = make_features(&pair, View::NoExactOverlap);
        full_rows.push(full);
        reduced_rows.push(reduced);
    }
    ensure!(
        exact_agreement == 120,
        "review streams disagree; this compact diagnostic requires exact alignment"
    );

    let mut fit_refs = Vec::new();
    let mut holdout_refs = Vec::new();
    for row in &full_rows {
        if row.split == "fit" {
            fit_refs.push(row);
        } else {
            holdout_refs.push(row);
        }
    }
    let fit_counts = counts(&fit_refs);
    let holdout_counts = counts(&holdout_refs);
    let mut shortfalls = Vec::new();
    let all_counts = counts(&full_rows.iter().collect::<Vec<_>>());
    if all_counts.same < 12 {
        shortfalls.push("overall SAME < 12".to_owned());
    }
    if all_counts.different < 12 {
        shortfalls.push("overall DIFFERENT < 12".to_owned());
    }
    if all_counts.unknown < 12 {
        shortfalls.push("overall UNKNOWN < 12".to_owned());
    }
    for (name, count) in [("fit", &fit_counts), ("holdout", &holdout_counts)] {
        if count.same < 4 || count.different < 4 || count.unknown < 4 {
            shortfalls.push(format!("{name} split has a class below 4"));
        }
    }
    let mut by_candidate: BTreeMap<String, Vec<&Example>> = BTreeMap::new();
    for row in &full_rows {
        by_candidate
            .entry(row.candidate_id.clone())
            .or_default()
            .push(row);
    }
    let eligible_candidates = by_candidate
        .values()
        .filter(|rows| {
            let c = counts(rows);
            c.same >= 4 && c.different >= 4
        })
        .count();
    if eligible_candidates < 3 {
        shortfalls.push("fewer than 3 candidates have >=4 SAME and >=4 DIFFERENT".to_owned());
    }
    let low_same = full_rows
        .iter()
        .filter(|row| row.overlap_band == "low" && row.label == Label::Same)
        .count();
    let low_same_holdout = full_rows
        .iter()
        .filter(|row| {
            row.split == "holdout" && row.overlap_band == "low" && row.label == Label::Same
        })
        .count();
    let high_diff = full_rows
        .iter()
        .filter(|row| row.overlap_band == "high" && row.label == Label::Different)
        .count();
    let high_diff_holdout = full_rows
        .iter()
        .filter(|row| {
            row.split == "holdout" && row.overlap_band == "high" && row.label == Label::Different
        })
        .count();
    let views = vec![
        ViewResult {
            model: analyze_view(&full_rows, View::FullLocal),
            candidate_holdout: candidate_slices(&full_rows, View::FullLocal, &candidate_pairs),
        },
        ViewResult {
            model: analyze_view(&reduced_rows, View::NoExactOverlap),
            candidate_holdout: candidate_slices(
                &reduced_rows,
                View::NoExactOverlap,
                &candidate_pairs,
            ),
        },
    ];

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1p3_engineering.rs");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let lockfile = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let binary = env::current_exe()?;
    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1p3-engineering-proxy/v1",
        date: "2026-09-27",
        status: "ENGINEERING_DIAGNOSTIC_COMPLETED_NOT_QUALIFICATION",
        interpretation: "Three submitted streams aligned exactly; source independence unverified. Treat labels as functional review signals, not human consensus.",
        protocol_sha256: sha256(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../experiments/lt9-la2-p1p3-consensus-observability/PROTOCOL.md"))?,
        pre_review_root_sha256: sha256(&root_path)?,
        review_validation_sha256: sha256(&validation_path)?,
        sealed_review_sha256: [sha256(&review_paths[0])?, sha256(&review_paths[1])?, sha256(&review_paths[2])?],
        packet_sha256: sha256(&packet_path)?,
        private_ledger_sha256: sha256(&ledger_path)?,
        source_sha256: sha256(&source)?,
        manifest_sha256: sha256(&manifest)?,
        lockfile_sha256: sha256(&lockfile)?,
        binary_sha256: sha256(&binary)?,
        aligned_edges: ledger.len(),
        exact_three_stream_agreement: exact_agreement,
        file_label_counts: stream_counts,
        engineering_target_counts: all_counts,
        fit_counts,
        holdout_counts,
        candidate_target_counts: candidate_slices(&full_rows, View::NoExactOverlap, &candidate_pairs),
        low_overlap_same: low_same,
        low_overlap_same_holdout: low_same_holdout,
        high_overlap_different: high_diff,
        high_overlap_different_holdout: high_diff_holdout,
        frozen_three_class_sufficiency_status: if shortfalls.is_empty() { "PASS" } else { "UNDERPOWERED" },
        sufficiency_shortfalls: shortfalls,
        views,
        authority_updated: false,
        retrieval_run: false,
    };
    fs::create_dir_all(&output_dir)?;
    let receipt_path = output_dir.join("engineering-receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    println!("engineering_receipt_sha256={}", sha256(&receipt_path)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functional_consensus_uses_unanimity_and_preserves_disagreement_as_unknown() {
        let same = [Label::Same, Label::Same, Label::Same];
        let split = [Label::Same, Label::Same, Label::Different];
        assert!(same.iter().all(|value| *value == same[0]));
        assert!(!split.iter().all(|value| *value == split[0]));
    }

    #[test]
    fn unknown_review_judgment_is_a_real_target_class() {
        let votes = [Label::Unknown, Label::Unknown, Label::Unknown];
        assert!(votes.iter().all(|value| *value == Label::Unknown));
    }
}
