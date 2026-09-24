//! P1P2 label-side reliability census for the three sealed P1O2 reviews.
//! This program never reads context features and performs no model fitting.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Label {
    Same,
    Different,
    Unknown,
}

impl Label {
    fn code(self) -> char {
        match self {
            Self::Same => 'S',
            Self::Different => 'D',
            Self::Unknown => 'U',
        }
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
}

#[derive(Deserialize)]
struct Metadata {
    packet_id: String,
    candidate_id: String,
    split: String,
    overlap_band: String,
    left_occurrence: Occurrence,
    right_occurrence: Occurrence,
}

#[derive(Deserialize)]
struct Occurrence {
    node_id: String,
}

#[derive(Clone, Serialize)]
struct EdgeVote {
    packet_id: String,
    candidate_id: String,
    graph: String,
    overlap_band: String,
    nodes: [String; 2],
    votes_r0_r1_r2: [Label; 3],
    vote_pattern: String,
    vote_entropy_bits: f64,
    target_class: String,
}

#[derive(Clone, Copy, Default, Serialize)]
struct Counts {
    same: usize,
    different: usize,
    unknown: usize,
}

impl Counts {
    fn add(&mut self, label: Label) {
        match label {
            Label::Same => self.same += 1,
            Label::Different => self.different += 1,
            Label::Unknown => self.unknown += 1,
        }
    }

    fn total(self) -> usize {
        self.same + self.different + self.unknown
    }
}

#[derive(Serialize)]
struct PairAgreement {
    exact: usize,
    n: usize,
    exact_rate: f64,
    same_different_reversals: usize,
    confusion_label_order: [&'static str; 3],
    /// Rows are the first rater's S/D/U labels; columns are the second's.
    confusion: [[usize; 3]; 3],
}

#[derive(Serialize)]
struct PairwiseMatrix {
    r0_vs_r1: PairAgreement,
    r0_vs_r2: PairAgreement,
    r1_vs_r2: PairAgreement,
}

#[derive(Serialize)]
struct StratumSummary {
    group: String,
    rows: usize,
    unanimous_same: usize,
    unanimous_different: usize,
    disputed: usize,
    r0_r1_exact: usize,
    r0_r2_exact: usize,
    r1_r2_exact: usize,
    rater_pair_comparisons: usize,
    exact_pair_agreement_rate: f64,
    mean_vote_entropy_bits: f64,
}

#[derive(Serialize)]
struct Triangle {
    graph: String,
    nodes: [String; 3],
    packet_ids: [String; 3],
    unanimous_labels: [Label; 3],
    pattern: String,
    two_same_one_different: bool,
}

#[derive(Serialize)]
struct ReliabilityReceipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    rater_order: [&'static str; 3],
    interpretation_limit: &'static str,
    packets_sha256: String,
    author_review_sha256: String,
    luna_a_review_sha256: String,
    luna_b_review_sha256: String,
    comparison_receipt_sha256: String,
    analysis_receipt_sha256: String,
    rubric_sha256: String,
    pre_review_root_sha256: String,
    validation_receipt_sha256: String,
    sufficiency_receipt_sha256: String,
    metadata_ledger_sha256: String,
    analyzer_source_sha256: String,
    analyzer_manifest_sha256: String,
    analyzer_lockfile_sha256: String,
    analyzer_binary_sha256: String,
    packet_count: usize,
    label_counts_by_reviewer: [Counts; 3],
    consensus_counts: Counts,
    vote_pattern_counts: BTreeMap<String, usize>,
    pairwise_agreement: PairwiseMatrix,
    fleiss_kappa_nominal: Option<f64>,
    mean_edge_vote_entropy_bits: f64,
    unanimous_edge_vote_entropy_bits: f64,
    disputed_edge_vote_entropy_bits: f64,
    by_candidate: Vec<StratumSummary>,
    by_overlap_band: Vec<StratumSummary>,
    by_graph: Vec<StratumSummary>,
    edge_votes: Vec<EdgeVote>,
    unanimous_edge_triangle_count: usize,
    unanimous_triangle_pattern_counts: BTreeMap<String, usize>,
    unanimous_two_same_one_different_triangle_count: usize,
    unanimous_triangles: Vec<Triangle>,
    private_join_scope: &'static str,
    feature_values_read: bool,
    feature_fit: bool,
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

fn label_map(path: &Path) -> Result<HashMap<String, Label>> {
    let rows: Vec<Judgment> = read_json(path)?;
    let mut result = HashMap::with_capacity(rows.len());
    for row in rows {
        ensure!(
            result.insert(row.packet_id, row.judgment).is_none(),
            "duplicate packet ID in {}",
            path.display()
        );
    }
    Ok(result)
}

fn entropy(votes: &[Label; 3]) -> f64 {
    if votes[0] == votes[1] && votes[1] == votes[2] {
        return 0.0;
    }
    let mut counts = Counts::default();
    for label in votes {
        counts.add(*label);
    }
    [counts.same, counts.different, counts.unknown]
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let p = count as f64 / 3.0;
            -p * p.log2()
        })
        .sum()
}

fn target_class(votes: &[Label; 3]) -> &'static str {
    if votes.iter().all(|label| *label == Label::Same) {
        "UNANIMOUS_SAME"
    } else if votes.iter().all(|label| *label == Label::Different) {
        "UNANIMOUS_DIFFERENT"
    } else {
        "DISPUTED"
    }
}

fn pair_agreement(rows: &[EdgeVote], a: usize, b: usize) -> PairAgreement {
    let exact = rows
        .iter()
        .filter(|row| row.votes_r0_r1_r2[a] == row.votes_r0_r1_r2[b])
        .count();
    let mut confusion = [[0_usize; 3]; 3];
    let label_index = |label: Label| match label {
        Label::Same => 0,
        Label::Different => 1,
        Label::Unknown => 2,
    };
    for row in rows {
        confusion[label_index(row.votes_r0_r1_r2[a])][label_index(row.votes_r0_r1_r2[b])] += 1;
    }
    let reversals = rows
        .iter()
        .filter(|row| {
            matches!(
                (row.votes_r0_r1_r2[a], row.votes_r0_r1_r2[b]),
                (Label::Same, Label::Different) | (Label::Different, Label::Same)
            )
        })
        .count();
    PairAgreement {
        exact,
        n: rows.len(),
        exact_rate: exact as f64 / rows.len() as f64,
        same_different_reversals: reversals,
        confusion_label_order: ["SAME", "DIFFERENT", "UNKNOWN"],
        confusion,
    }
}

fn fleiss_kappa(rows: &[EdgeVote]) -> Option<f64> {
    if rows.is_empty() {
        return None;
    }
    let mut totals = Counts::default();
    let mut per_item_agreement = 0.0;
    for row in rows {
        let mut counts = Counts::default();
        for label in row.votes_r0_r1_r2 {
            counts.add(label);
            totals.add(label);
        }
        let n = counts.total() as f64;
        per_item_agreement += (counts.same * counts.same.saturating_sub(1)
            + counts.different * counts.different.saturating_sub(1)
            + counts.unknown * counts.unknown.saturating_sub(1))
            as f64
            / (n * (n - 1.0));
    }
    let p_bar = per_item_agreement / rows.len() as f64;
    let n_ratings = (rows.len() * 3) as f64;
    let p_same = totals.same as f64 / n_ratings;
    let p_different = totals.different as f64 / n_ratings;
    let p_unknown = totals.unknown as f64 / n_ratings;
    let p_expected = p_same * p_same + p_different * p_different + p_unknown * p_unknown;
    (p_expected < 1.0).then_some((p_bar - p_expected) / (1.0 - p_expected))
}

fn summarize(group: &str, rows: &[&EdgeVote]) -> StratumSummary {
    let mut unanimous_same = 0;
    let mut unanimous_different = 0;
    let mut disputed = 0;
    for row in rows {
        match row.target_class.as_str() {
            "UNANIMOUS_SAME" => unanimous_same += 1,
            "UNANIMOUS_DIFFERENT" => unanimous_different += 1,
            _ => disputed += 1,
        }
    }
    let agreements = [
        pair_agreement_refs(rows, 0, 1),
        pair_agreement_refs(rows, 0, 2),
        pair_agreement_refs(rows, 1, 2),
    ];
    let r0_r1_exact = agreements[0];
    let r0_r2_exact = agreements[1];
    let r1_r2_exact = agreements[2];
    let comparisons = rows.len() * 3;
    let mean_entropy =
        rows.iter().map(|row| row.vote_entropy_bits).sum::<f64>() / rows.len().max(1) as f64;
    StratumSummary {
        group: group.to_owned(),
        rows: rows.len(),
        unanimous_same,
        unanimous_different,
        disputed,
        r0_r1_exact,
        r0_r2_exact,
        r1_r2_exact,
        rater_pair_comparisons: comparisons,
        exact_pair_agreement_rate: (r0_r1_exact + r0_r2_exact + r1_r2_exact) as f64
            / comparisons.max(1) as f64,
        mean_vote_entropy_bits: mean_entropy,
    }
}

fn pair_agreement_refs(rows: &[&EdgeVote], a: usize, b: usize) -> usize {
    rows.iter()
        .filter(|row| row.votes_r0_r1_r2[a] == row.votes_r0_r1_r2[b])
        .count()
}

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

fn unanimous_triangles(rows: &[EdgeVote]) -> Vec<Triangle> {
    let mut by_graph: BTreeMap<String, Vec<&EdgeVote>> = BTreeMap::new();
    for row in rows.iter().filter(|row| row.target_class != "DISPUTED") {
        by_graph.entry(row.graph.clone()).or_default().push(row);
    }
    let mut triangles = Vec::new();
    for (graph, edges) in by_graph {
        let nodes: Vec<String> = edges
            .iter()
            .flat_map(|edge| edge.nodes.iter().cloned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut edge_map: HashMap<(String, String), &EdgeVote> = HashMap::new();
        for edge in &edges {
            edge_map.insert(ordered_pair(&edge.nodes[0], &edge.nodes[1]), edge);
        }
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                for k in (j + 1)..nodes.len() {
                    let keys = [
                        ordered_pair(&nodes[i], &nodes[j]),
                        ordered_pair(&nodes[i], &nodes[k]),
                        ordered_pair(&nodes[j], &nodes[k]),
                    ];
                    let (Some(e01), Some(e02), Some(e12)) = (
                        edge_map.get(&keys[0]),
                        edge_map.get(&keys[1]),
                        edge_map.get(&keys[2]),
                    ) else {
                        continue;
                    };
                    let labels = [
                        if e01.target_class == "UNANIMOUS_SAME" {
                            Label::Same
                        } else {
                            Label::Different
                        },
                        if e02.target_class == "UNANIMOUS_SAME" {
                            Label::Same
                        } else {
                            Label::Different
                        },
                        if e12.target_class == "UNANIMOUS_SAME" {
                            Label::Same
                        } else {
                            Label::Different
                        },
                    ];
                    let pattern: String = labels.iter().map(|label| label.code()).collect();
                    let same_count = labels.iter().filter(|label| **label == Label::Same).count();
                    triangles.push(Triangle {
                        graph: graph.clone(),
                        nodes: [nodes[i].clone(), nodes[j].clone(), nodes[k].clone()],
                        packet_ids: [
                            e01.packet_id.clone(),
                            e02.packet_id.clone(),
                            e12.packet_id.clone(),
                        ],
                        unanimous_labels: labels,
                        pattern,
                        // Descriptive equivalence-pattern diagnostic only; this
                        // does not assert that natural compatibility is transitive.
                        two_same_one_different: same_count == 2,
                    });
                }
            }
        }
    }
    triangles
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(args.len() == 14, "usage: lt9_la2p1o2_reliability <packets> <author> <luna-a> <luna-b> <private-ledger> <comparison-receipt> <analysis-receipt> <rubric> <pre-review-root> <validation-receipt> <sufficiency-receipt> <analyzer-manifest> <new-output-dir>");
    let paths: Vec<PathBuf> = args[1..13].iter().map(PathBuf::from).collect();
    let [packets_path, author_path, luna_a_path, luna_b_path, metadata_path, comparison_path, analysis_path, rubric_path, root_path, validation_path, sufficiency_path, manifest_path] =
        <[PathBuf; 12]>::try_from(paths).expect("twelve input paths");
    let output_dir = PathBuf::from(&args[13]);
    ensure!(
        !output_dir.exists(),
        "refusing to overwrite {}",
        output_dir.display()
    );

    let packets: Vec<Packet> = read_json(&packets_path)?;
    let rater_maps = [
        label_map(&author_path)?,
        label_map(&luna_a_path)?,
        label_map(&luna_b_path)?,
    ];
    let metadata: Vec<Metadata> = read_json(&metadata_path)?;
    let comparison: Value = read_json(&comparison_path)?;
    let analysis: Value = read_json(&analysis_path)?;
    let root: Value = read_json(&root_path)?;
    let validation: Value = read_json(&validation_path)?;
    let sufficiency: Value = read_json(&sufficiency_path)?;
    ensure!(packets.len() == 30, "expected exact sealed 30-packet set");
    let packet_ids: HashSet<String> = packets
        .iter()
        .map(|packet| packet.packet_id.clone())
        .collect();
    ensure!(
        packet_ids.len() == 30,
        "duplicate packet ID in frozen packets"
    );
    for (index, map) in rater_maps.iter().enumerate() {
        ensure!(
            map.len() == 30 && map.keys().all(|id| packet_ids.contains(id)),
            "reviewer {index} ID set mismatch"
        );
    }
    ensure!(metadata.len() == 30, "private metadata row count mismatch");
    let metadata_ids: HashSet<String> = metadata.iter().map(|row| row.packet_id.clone()).collect();
    ensure!(
        metadata_ids.len() == 30 && metadata_ids == packet_ids,
        "private metadata ID set mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["packets"] == sha256(&packets_path)?,
        "comparison packet hash mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["author"] == sha256(&author_path)?,
        "comparison author hash mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["luna_a"] == sha256(&luna_a_path)?,
        "comparison Luna A hash mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["luna_b"] == sha256(&luna_b_path)?,
        "comparison Luna B hash mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["analysis"] == sha256(&analysis_path)?,
        "comparison analysis hash mismatch"
    );
    ensure!(
        comparison["inputs_sha256"]["rubric"] == sha256(&rubric_path)?,
        "comparison rubric hash mismatch"
    );
    ensure!(
        root["packets_sha256"] == sha256(&packets_path)?,
        "pre-review root packet hash mismatch"
    );
    ensure!(
        root["rubric_sha256"] == sha256(&rubric_path)?,
        "pre-review root rubric hash mismatch"
    );
    ensure!(
        root["private_ledger_sha256"] == sha256(&metadata_path)?,
        "pre-review root metadata hash mismatch"
    );
    ensure!(
        validation["frozen_packets_sha256"] == sha256(&packets_path)?,
        "validation packet hash mismatch"
    );
    ensure!(
        validation["judgments_sha256"] == sha256(&author_path)?,
        "validation author-label hash mismatch"
    );
    ensure!(
        validation["ids_match_exactly"] == true && validation["unfilled_count"] == 0,
        "author labels are not fully validated"
    );
    ensure!(
        sufficiency["packets_sha256"] == sha256(&packets_path)?,
        "sufficiency packet hash mismatch"
    );
    ensure!(
        sufficiency["judgments_sha256"] == sha256(&author_path)?,
        "sufficiency author-label hash mismatch"
    );
    ensure!(
        sufficiency["validation_receipt_sha256"] == sha256(&validation_path)?,
        "sufficiency validation hash mismatch"
    );
    ensure!(
        sufficiency["private_ledger_sha256"] == sha256(&metadata_path)?,
        "sufficiency metadata hash mismatch"
    );
    ensure!(
        analysis["pre_review_root_sha256"] == sha256(&root_path)?,
        "analysis root hash mismatch"
    );
    ensure!(
        analysis["validation_sha256"] == sha256(&validation_path)?,
        "analysis validation hash mismatch"
    );
    ensure!(
        analysis["sufficiency_sha256"] == sha256(&sufficiency_path)?,
        "analysis sufficiency hash mismatch"
    );
    ensure!(
        analysis["judgments_sha256"] == sha256(&author_path)?,
        "analysis author-label hash mismatch"
    );
    ensure!(
        analysis["private_ledger_sha256"] == sha256(&metadata_path)?,
        "analysis metadata hash mismatch"
    );
    ensure!(
        analysis["authority_updated"] == false && analysis["retrieval_run"] == false,
        "P1O2 boundary receipt indicates downstream actions"
    );

    let mut edges = Vec::with_capacity(30);
    for row in metadata {
        ensure!(
            packet_ids.contains(&row.packet_id),
            "metadata packet missing from frozen set"
        );
        let votes = [
            *rater_maps[0]
                .get(&row.packet_id)
                .context("author vote missing")?,
            *rater_maps[1]
                .get(&row.packet_id)
                .context("Luna A vote missing")?,
            *rater_maps[2]
                .get(&row.packet_id)
                .context("Luna B vote missing")?,
        ];
        let mut pattern = String::new();
        for label in votes {
            pattern.push(label.code());
        }
        let class = target_class(&votes);
        edges.push(EdgeVote {
            packet_id: row.packet_id,
            candidate_id: row.candidate_id,
            graph: row.split,
            overlap_band: row.overlap_band,
            nodes: [row.left_occurrence.node_id, row.right_occurrence.node_id],
            votes_r0_r1_r2: votes,
            vote_pattern: pattern,
            vote_entropy_bits: entropy(&votes),
            target_class: class.to_owned(),
        });
    }
    edges.sort_by(|a, b| a.packet_id.cmp(&b.packet_id));

    let mut rater_counts = [Counts::default(); 3];
    let mut consensus_counts = Counts::default();
    let mut vote_pattern_counts = BTreeMap::new();
    let mut unanimous_entropy = Vec::new();
    let mut disputed_entropy = Vec::new();
    for edge in &edges {
        for (idx, label) in edge.votes_r0_r1_r2.iter().enumerate() {
            rater_counts[idx].add(*label);
        }
        match edge.target_class.as_str() {
            "UNANIMOUS_SAME" => {
                consensus_counts.add(Label::Same);
                unanimous_entropy.push(edge.vote_entropy_bits);
            }
            "UNANIMOUS_DIFFERENT" => {
                consensus_counts.add(Label::Different);
                unanimous_entropy.push(edge.vote_entropy_bits);
            }
            _ => {
                consensus_counts.add(Label::Unknown);
                disputed_entropy.push(edge.vote_entropy_bits);
            }
        }
        *vote_pattern_counts
            .entry(edge.vote_pattern.clone())
            .or_default() += 1;
    }

    let mut candidate_groups: BTreeMap<String, Vec<&EdgeVote>> = BTreeMap::new();
    let mut overlap_groups: BTreeMap<String, Vec<&EdgeVote>> = BTreeMap::new();
    let mut graph_groups: BTreeMap<String, Vec<&EdgeVote>> = BTreeMap::new();
    for edge in &edges {
        candidate_groups
            .entry(edge.candidate_id.clone())
            .or_default()
            .push(edge);
        overlap_groups
            .entry(edge.overlap_band.clone())
            .or_default()
            .push(edge);
        graph_groups
            .entry(edge.graph.clone())
            .or_default()
            .push(edge);
    }
    let by_candidate = candidate_groups
        .iter()
        .map(|(key, group)| summarize(key, group))
        .collect();
    let by_overlap_band = overlap_groups
        .iter()
        .map(|(key, group)| summarize(key, group))
        .collect();
    let by_graph = graph_groups
        .iter()
        .map(|(key, group)| summarize(key, group))
        .collect();
    let pairwise_agreement = PairwiseMatrix {
        r0_vs_r1: pair_agreement(&edges, 0, 1),
        r0_vs_r2: pair_agreement(&edges, 0, 2),
        r1_vs_r2: pair_agreement(&edges, 1, 2),
    };
    let mean_entropy =
        edges.iter().map(|edge| edge.vote_entropy_bits).sum::<f64>() / edges.len() as f64;
    let mean = |values: &[f64]| {
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    };
    let triangles = unanimous_triangles(&edges);
    let mut triangle_patterns = BTreeMap::new();
    let mut two_same_one_different = 0;
    for triangle in &triangles {
        *triangle_patterns
            .entry(format!("{},{}", triangle.graph, triangle.pattern))
            .or_default() += 1;
        two_same_one_different += usize::from(triangle.two_same_one_different);
    }

    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_reliability.rs");
    let lock_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let binary_path = env::current_exe()?;
    let receipt = ReliabilityReceipt {
        schema: "phoenix.lexical.lt9-la2-p1p2-label-reliability/v1",
        date: "2026-09-24",
        status: "LABEL_SIDE_PROXY_RELIABILITY_SEALED",
        rater_order: ["R0_AUTHOR", "R1_LUNA_A", "R2_LUNA_B"],
        interpretation_limit: "two independent agent contexts share one model family; R0 is experiment author; these are proxy-review diagnostics, not independent-human reliability evidence",
        packets_sha256: sha256(&packets_path)?,
        author_review_sha256: sha256(&author_path)?,
        luna_a_review_sha256: sha256(&luna_a_path)?,
        luna_b_review_sha256: sha256(&luna_b_path)?,
        comparison_receipt_sha256: sha256(&comparison_path)?,
        analysis_receipt_sha256: sha256(&analysis_path)?,
        rubric_sha256: sha256(&rubric_path)?,
        pre_review_root_sha256: sha256(&root_path)?,
        validation_receipt_sha256: sha256(&validation_path)?,
        sufficiency_receipt_sha256: sha256(&sufficiency_path)?,
        metadata_ledger_sha256: sha256(&metadata_path)?,
        analyzer_source_sha256: sha256(&source_path)?,
        analyzer_manifest_sha256: sha256(&manifest_path)?,
        analyzer_lockfile_sha256: sha256(&lock_path)?,
        analyzer_binary_sha256: sha256(&binary_path)?,
        packet_count: edges.len(),
        label_counts_by_reviewer: rater_counts,
        consensus_counts,
        vote_pattern_counts,
        pairwise_agreement,
        fleiss_kappa_nominal: fleiss_kappa(&edges),
        mean_edge_vote_entropy_bits: mean_entropy,
        unanimous_edge_vote_entropy_bits: mean(&unanimous_entropy).unwrap_or(0.0),
        disputed_edge_vote_entropy_bits: mean(&disputed_entropy).unwrap_or(0.0),
        by_candidate,
        by_overlap_band,
        by_graph,
        edge_votes: edges,
        unanimous_edge_triangle_count: triangles.len(),
        unanimous_triangle_pattern_counts: triangle_patterns,
        unanimous_two_same_one_different_triangle_count: two_same_one_different,
        unanimous_triangles: triangles,
        private_join_scope: "packet_id, candidate_id, graph split, overlap band, and occurrence node IDs only",
        feature_values_read: false,
        feature_fit: false,
        authority_updated: false,
        retrieval_run: false,
    };
    fs::create_dir_all(&output_dir)?;
    let path = output_dir.join("reliability-receipt.json");
    fs::write(&path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    println!("reliability_receipt_sha256={}", sha256(&path)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disagreement_becomes_unknown_only_in_consensus_target() {
        assert_eq!(target_class(&[Label::Same; 3]), "UNANIMOUS_SAME");
        assert_eq!(target_class(&[Label::Different; 3]), "UNANIMOUS_DIFFERENT");
        assert_eq!(
            target_class(&[Label::Same, Label::Same, Label::Different]),
            "DISPUTED"
        );
    }

    #[test]
    fn vote_entropy_distinguishes_unanimity_and_two_to_one() {
        assert_eq!(entropy(&[Label::Same; 3]), 0.0);
        assert!(
            (entropy(&[Label::Same, Label::Same, Label::Different]) - 0.918295834).abs() < 1e-8
        );
    }

    #[test]
    fn fleiss_kappa_is_one_for_identical_raters() {
        let row = |packet_id: &str, label| EdgeVote {
            packet_id: packet_id.to_owned(),
            candidate_id: "test".to_owned(),
            graph: "fit".to_owned(),
            overlap_band: "low".to_owned(),
            nodes: ["a".to_owned(), "b".to_owned()],
            votes_r0_r1_r2: [label; 3],
            vote_pattern: "SSS".to_owned(),
            vote_entropy_bits: 0.0,
            target_class: target_class(&[label; 3]).to_owned(),
        };
        let rows = [row("1", Label::Same), row("2", Label::Different)];
        assert_eq!(fleiss_kappa(&rows), Some(1.0));
    }

    #[test]
    fn pairwise_confusion_preserves_label_direction() {
        let row = EdgeVote {
            packet_id: "1".to_owned(),
            candidate_id: "test".to_owned(),
            graph: "fit".to_owned(),
            overlap_band: "low".to_owned(),
            nodes: ["a".to_owned(), "b".to_owned()],
            votes_r0_r1_r2: [Label::Same, Label::Different, Label::Unknown],
            vote_pattern: "SDU".to_owned(),
            vote_entropy_bits: entropy(&[Label::Same, Label::Different, Label::Unknown]),
            target_class: "DISPUTED".to_owned(),
        };
        let agreement = pair_agreement(&[row], 0, 1);
        assert_eq!(agreement.confusion, [[0, 1, 0], [0, 0, 0], [0, 0, 0]]);
        assert_eq!(agreement.same_different_reversals, 1);
    }

    #[test]
    fn triangle_diagnostic_counts_only_two_same_one_different_pattern() {
        let same_count =
            |labels: [Label; 3]| labels.iter().filter(|label| **label == Label::Same).count();
        assert_ne!(
            same_count([Label::Different, Label::Different, Label::Same]),
            2
        );
        assert_eq!(same_count([Label::Same, Label::Same, Label::Different]), 2);
    }
}
