//! Frozen, authority-free P1N3 fit/holdout probe.
//! The holdout is read only after a fixed fit-only tree recipe is applied.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use memmap2::MmapOptions;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::path::Path;

const MAX_DEPTH: usize = 3;
const MIN_CHILD_ROWS: usize = 3;
const DATE: &str = "2026-09-23";
const LABELS: [&str; 3] = ["SAME", "DIFFERENT", "UNKNOWN"];

#[derive(Clone)]
struct Row {
    candidate: String,
    overlap_band: String,
    split: String,
    truth: usize,
    full: HashMap<String, f64>,
    no_overlap: HashMap<String, f64>,
}

#[derive(Clone, Serialize)]
struct Counts {
    total: usize,
    same: usize,
    different: usize,
    unknown: usize,
    low_overlap: usize,
    high_overlap: usize,
}

#[derive(Clone, Serialize)]
struct Metrics {
    total: usize,
    scored: usize,
    accuracy: Option<f64>,
    balanced_accuracy_observed_classes: Option<f64>,
    per_class_recall: [Option<f64>; 3],
    confusion_truth_by_prediction: [[usize; 3]; 3],
    false_same_rate_on_different: Option<f64>,
    hard_positive_same_recall: Option<f64>,
    hard_positive_count: usize,
    hard_negative_different_recall: Option<f64>,
    hard_negative_count: usize,
}

#[derive(Clone, Serialize)]
struct ViewReport {
    fit: Counts,
    holdout: Counts,
    overall_holdout: Metrics,
    by_candidate: Vec<CandidateReport>,
    macro_accuracy_over_candidates_with_multiple_holdout_classes: Option<f64>,
    macro_false_same_rate_over_candidates_with_different_holdouts: Option<f64>,
}

#[derive(Clone, Serialize)]
struct CandidateReport {
    candidate: String,
    fit: Counts,
    holdout: Counts,
    model_status: String,
    holdout_metrics: Metrics,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    analysis_plan: &'static str,
    analysis_plan_sha256: String,
    evaluator_source_sha256: String,
    evaluator_binary_sha256: String,
    input_sha256: InputHashes,
    frozen_model: ModelSpec,
    reviewer_note: &'static str,
    unknown_gold_evaluation: &'static str,
    p_full_local: ViewReport,
    p_no_exact_overlap: ViewReport,
    transitivity_policy: &'static str,
    authority_or_retrieval_run: bool,
}

#[derive(Serialize)]
struct InputHashes {
    review_pass1: String,
    validation_receipt: String,
    sealed_packets: String,
    private_ledger: String,
    pre_review_root: String,
}

#[derive(Serialize)]
struct ModelSpec {
    per_candidate_models: usize,
    criterion: &'static str,
    max_depth: usize,
    min_rows_per_child: usize,
    class_weights: &'static str,
    split_tie_break: &'static str,
    leaf_tie_prediction: &'static str,
    threshold_source: &'static str,
    holdout_uses: &'static str,
}

#[derive(Clone)]
enum Node {
    Leaf(usize),
    Split {
        feature: usize,
        threshold: f64,
        left: Box<Node>,
        right: Box<Node>,
    },
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 7,
        "usage: lt9_la2p1n3_eval <review-pass1.json> <validation-receipt.json> <sealed-packets.json> <private-ledger.json> <pre-review-root.json> <report.json>"
    );
    run(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
        Path::new(&args[4]),
        Path::new(&args[5]),
        Path::new(&args[6]),
    )
}

fn run(
    review_path: &Path,
    validation_path: &Path,
    packet_path: &Path,
    ledger_path: &Path,
    root_path: &Path,
    out: &Path,
) -> Result<()> {
    let review_bytes = mmap_bytes(review_path)?;
    let validation_bytes = mmap_bytes(validation_path)?;
    let packet_bytes = mmap_bytes(packet_path)?;
    let ledger_bytes = mmap_bytes(ledger_path)?;
    let root_bytes = mmap_bytes(root_path)?;
    let review: Vec<Value> =
        serde_json::from_slice(&review_bytes).context("decode human labels")?;
    let validation: Value =
        serde_json::from_slice(&validation_bytes).context("decode label-validation receipt")?;
    let packets: Vec<Value> =
        serde_json::from_slice(&packet_bytes).context("decode sealed packets")?;
    let ledger: Vec<Value> =
        serde_json::from_slice(&ledger_bytes).context("decode private features")?;
    let root: Value = serde_json::from_slice(&root_bytes).context("decode pre-review root")?;

    let hashes = InputHashes {
        review_pass1: sha256(&review_bytes),
        validation_receipt: sha256(&validation_bytes),
        sealed_packets: sha256(&packet_bytes),
        private_ledger: sha256(&ledger_bytes),
        pre_review_root: sha256(&root_bytes),
    };
    let plan_path = Path::new("docs/LT9_LA2_P1N3_ANALYSIS_PLAN_20260923.md");
    let plan_bytes = std::fs::read(plan_path).context("read frozen analysis plan")?;
    let binary_path = env::current_exe().context("locate evaluator binary")?;
    let binary_bytes = std::fs::read(&binary_path).context("read evaluator binary for seal")?;
    ensure!(
        root["packets_sha256"].as_str() == Some(hashes.sealed_packets.as_str()),
        "sealed packet hash differs from pre-review root"
    );
    ensure!(
        root["private_ledger_sha256"].as_str() == Some(hashes.private_ledger.as_str()),
        "private ledger hash differs from pre-review root"
    );
    ensure!(
        root["judgments_completed"].as_bool() == Some(false),
        "pre-review root must remain the pre-judgment root"
    );
    ensure!(
        validation["status"].as_str() == Some("validated_human_pass1_no_probe_run"),
        "human-label validation receipt is not in the expected sealed state"
    );
    ensure!(
        validation["review_pass1_sha256"].as_str() == Some(hashes.review_pass1.as_str()),
        "human-label file hash differs from validation receipt"
    );
    ensure!(
        validation["base_packet_sha256"].as_str() == Some(hashes.sealed_packets.as_str()),
        "validation receipt refers to different sealed packets"
    );
    let rows = join_rows(&review, &packets, &ledger)?;
    let (full, no_overlap) = (evaluate_view(&rows, true), evaluate_view(&rows, false));
    let report = Report {
        schema: "phoenix.lexical.lt9-la2-p1n3-fit-holdout/v1",
        date: DATE,
        status: "single_frozen_holdout_evaluation",
        analysis_plan: "docs/LT9_LA2_P1N3_ANALYSIS_PLAN_20260923.md",
        analysis_plan_sha256: sha256(&plan_bytes),
        evaluator_source_sha256: sha256(include_bytes!("lt9_la2p1n3_eval.rs")),
        evaluator_binary_sha256: sha256(&binary_bytes),
        input_sha256: hashes,
        frozen_model: ModelSpec {
            per_candidate_models: 3,
            criterion: "gini impurity",
            max_depth: MAX_DEPTH,
            min_rows_per_child: MIN_CHILD_ROWS,
            class_weights: "unweighted",
            split_tie_break: "lexicographic feature name, then lower threshold",
            leaf_tie_prediction: "UNKNOWN",
            threshold_source: "fit rows only",
            holdout_uses: "one score pass; no selection or tuning",
        },
        reviewer_note: "Human experiment author reviewed packets; prior hypothesis awareness limits reviewer independence.",
        unknown_gold_evaluation: if rows.iter().any(|r| r.truth == 2) {
            "UNKNOWN judgments observed; see per-class counts"
        } else {
            "not estimable: no human UNKNOWN judgments in this sample"
        },
        p_full_local: full,
        p_no_exact_overlap: no_overlap,
        transitivity_policy: "pairwise only; no semantic closure or union-find",
        authority_or_retrieval_run: false,
    };
    let json = serde_json::to_vec_pretty(&report)?;
    std::fs::write(out, json).with_context(|| format!("write report {}", out.display()))?;
    println!(
        "holdout_report={} review_sha256={}",
        out.display(),
        report.input_sha256.review_pass1
    );
    Ok(())
}

fn mmap_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mapped = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("mmap {}", path.display()))?;
    Ok(mapped.to_vec())
}

fn join_rows(review: &[Value], packets: &[Value], ledger: &[Value]) -> Result<Vec<Row>> {
    ensure!(
        review.len() == 96 && packets.len() == 96 && ledger.len() == 96,
        "expected frozen 96-row inputs"
    );
    let mut packet_by_id = HashMap::with_capacity(packets.len());
    for packet in packets {
        packet_by_id.insert(required_str(packet, "packet_id")?, packet);
    }
    let mut ledger_by_id = HashMap::with_capacity(ledger.len());
    for row in ledger {
        ledger_by_id.insert(required_str(row, "packet_id")?, row);
    }
    let mut seen = HashSet::with_capacity(review.len());
    let mut result = Vec::with_capacity(review.len());
    for judgment in review {
        let id = required_str(judgment, "packet_id")?;
        ensure!(
            seen.insert(id.to_owned()),
            "duplicate review packet ID {id}"
        );
        let packet = packet_by_id
            .get(id)
            .context("review ID absent from sealed packets")?;
        let private = ledger_by_id
            .get(id)
            .context("review ID absent from private ledger")?;
        ensure!(
            judgment["lexical_pair"] == packet["lexical_pair"],
            "visible pair changed for {id}"
        );
        ensure!(
            judgment["contexts"] == packet["contexts"],
            "visible context changed for {id}"
        );
        let truth = label_index(required_str(judgment, "judgment")?)?;
        let pair = &private["pair_features"];
        result.push(Row {
            candidate: required_str(private, "candidate_id")?.to_owned(),
            overlap_band: required_str(private, "overlap_band")?.to_owned(),
            split: required_str(private, "split")?.to_owned(),
            truth,
            full: full_features(pair)?,
            no_overlap: no_overlap_features(pair)?,
        });
    }
    ensure!(seen.len() == packets.len(), "review ID set incomplete");
    Ok(result)
}

fn full_features(pair: &Value) -> Result<HashMap<String, f64>> {
    let mut out = structural_features(pair)?;
    for key in ["token_jaccard", "bigram_jaccard", "trigram_jaccard"] {
        out.insert(format!("overlap:{key}"), required_num(pair, key)?);
    }
    for (key, prefix) in [
        ("shared_tokens", "token"),
        ("shared_role_tokens", "role"),
        ("shared_bigrams", "bigram"),
        ("shared_trigrams", "trigram"),
    ] {
        let values = pair[key]
            .as_array()
            .with_context(|| format!("{key} must be an array"))?;
        for value in values {
            let identity = value.as_str().context("shared identity must be text")?;
            out.insert(format!("identity:{prefix}:{identity}"), 1.0);
        }
    }
    Ok(out)
}

fn no_overlap_features(pair: &Value) -> Result<HashMap<String, f64>> {
    structural_features(pair)
}

fn structural_features(pair: &Value) -> Result<HashMap<String, f64>> {
    let mut out = HashMap::with_capacity(12);
    let deltas = pair["role_count_abs_delta"]
        .as_array()
        .context("role_count_abs_delta must be an array")?;
    ensure!(deltas.len() == 3, "role_count_abs_delta length must be 3");
    for (i, value) in deltas.iter().enumerate() {
        out.insert(
            format!("structure:role_count_delta_{i}"),
            value.as_f64().context("role delta")?,
        );
    }
    for key in ["token_count_abs_delta", "distance_bin_abs_delta"] {
        out.insert(format!("structure:{key}"), required_num(pair, key)?);
    }
    for key in [
        "support_cue_equal",
        "contradiction_cue_equal",
        "same_field_kind",
    ] {
        out.insert(format!("structure:{key}"), required_bool(pair, key)?);
    }
    Ok(out)
}

fn evaluate_view(rows: &[Row], full: bool) -> ViewReport {
    let mut candidates: Vec<String> = rows.iter().map(|r| r.candidate.clone()).collect();
    candidates.sort();
    candidates.dedup();
    let mut reports = Vec::with_capacity(candidates.len());
    let mut all_fit = Vec::new();
    let mut all_holdout = Vec::new();
    let mut all_holdout_truth = Vec::new();
    let mut all_holdout_predictions = Vec::new();
    let mut accs = Vec::new();
    let mut false_same = Vec::new();
    for candidate in candidates {
        let selected: Vec<&Row> = rows.iter().filter(|r| r.candidate == candidate).collect();
        let fit: Vec<&Row> = selected
            .iter()
            .copied()
            .filter(|r| r.split == "fit")
            .collect();
        let test: Vec<&Row> = selected
            .iter()
            .copied()
            .filter(|r| r.split == "holdout")
            .collect();
        let feature_maps: Vec<&HashMap<String, f64>> = fit
            .iter()
            .map(|r| if full { &r.full } else { &r.no_overlap })
            .collect();
        let feature_names = feature_union(&feature_maps);
        let x_fit = matrix(&fit, &feature_names, full);
        let y_fit: Vec<usize> = fit.iter().map(|r| r.truth).collect();
        let model = fit_tree(&x_fit, &y_fit, &feature_names, 0);
        let x_test = matrix(&test, &feature_names, full);
        let y_test: Vec<usize> = test.iter().map(|r| r.truth).collect();
        let predictions: Vec<usize> = x_test.iter().map(|x| predict(&model, x)).collect();
        let metrics = metrics(&test, &y_test, &predictions);
        let fit_counts = counts(&fit);
        let test_counts = counts(&test);
        let observed_fit_classes = fit_counts.same.gt(&0) as usize
            + fit_counts.different.gt(&0) as usize
            + fit_counts.unknown.gt(&0) as usize;
        let status = if observed_fit_classes < 2 {
            "underpowered_single_fit_class"
        } else if matches!(model, Node::Leaf(_)) {
            "fit_majority_leaf_no_legal_split"
        } else {
            "fit_tree_scored_once"
        }
        .to_string();
        let observed_holdout_classes = usize::from(test_counts.same > 0)
            + usize::from(test_counts.different > 0)
            + usize::from(test_counts.unknown > 0);
        if observed_holdout_classes >= 2 {
            accs.push(metrics.accuracy.unwrap_or(0.0));
        }
        if let Some(value) = metrics.false_same_rate_on_different {
            false_same.push(value);
        }
        all_fit.extend(fit);
        all_holdout.extend(test.iter().copied());
        all_holdout_truth.extend(y_test.iter().copied());
        all_holdout_predictions.extend(predictions.iter().copied());
        reports.push(CandidateReport {
            candidate,
            fit: fit_counts,
            holdout: test_counts,
            model_status: status,
            holdout_metrics: metrics,
        });
    }
    let overall = metrics(&all_holdout, &all_holdout_truth, &all_holdout_predictions);
    ViewReport {
        fit: counts(&all_fit),
        holdout: counts(&all_holdout),
        overall_holdout: overall,
        by_candidate: reports,
        macro_accuracy_over_candidates_with_multiple_holdout_classes: mean_opt(&accs),
        macro_false_same_rate_over_candidates_with_different_holdouts: mean_opt(&false_same),
    }
}

fn feature_union(rows: &[&HashMap<String, f64>]) -> Vec<String> {
    let mut names: Vec<String> = rows.iter().flat_map(|m| m.keys().cloned()).collect();
    names.sort();
    names.dedup();
    names
}

fn matrix(rows: &[&Row], names: &[String], full: bool) -> Vec<Vec<f64>> {
    rows.iter()
        .map(|row| {
            let map = if full { &row.full } else { &row.no_overlap };
            names
                .iter()
                .map(|name| map.get(name).copied().unwrap_or(0.0))
                .collect()
        })
        .collect()
}

fn fit_tree(x: &[Vec<f64>], y: &[usize], names: &[String], depth: usize) -> Node {
    let prediction = majority(y);
    if depth >= MAX_DEPTH || y.len() < MIN_CHILD_ROWS * 2 || gini(y) == 0.0 || x.is_empty() {
        return Node::Leaf(prediction);
    }
    let parent_impurity = gini(y);
    let mut best: Option<(f64, usize, f64, Vec<usize>, Vec<usize>)> = None;
    for feature in 0..names.len() {
        let mut order: Vec<usize> = (0..x.len()).collect();
        order.sort_by(|a, b| x[*a][feature].total_cmp(&x[*b][feature]));
        for boundary in 1..order.len() {
            let left_value = x[order[boundary - 1]][feature];
            let right_value = x[order[boundary]][feature];
            if left_value == right_value
                || boundary < MIN_CHILD_ROWS
                || order.len() - boundary < MIN_CHILD_ROWS
            {
                continue;
            }
            let threshold = left_value + (right_value - left_value) / 2.0;
            let left: Vec<usize> = order[..boundary].iter().map(|i| y[*i]).collect();
            let right: Vec<usize> = order[boundary..].iter().map(|i| y[*i]).collect();
            let gain = parent_impurity
                - (left.len() as f64 / y.len() as f64) * gini(&left)
                - (right.len() as f64 / y.len() as f64) * gini(&right);
            if gain <= 1e-12 {
                continue;
            }
            let replace = match &best {
                None => true,
                Some((best_gain, best_feature, best_threshold, _, _)) => {
                    gain > *best_gain + 1e-12
                        || ((gain - *best_gain).abs() <= 1e-12
                            && (names[feature].cmp(&names[*best_feature]) == Ordering::Less
                                || (feature == *best_feature && threshold < *best_threshold)))
                }
            };
            if replace {
                best = Some((
                    gain,
                    feature,
                    threshold,
                    order[..boundary].to_vec(),
                    order[boundary..].to_vec(),
                ));
            }
        }
    }
    let Some((_, feature, threshold, left_ids, right_ids)) = best else {
        return Node::Leaf(prediction);
    };
    let left_x: Vec<Vec<f64>> = left_ids.iter().map(|i| x[*i].clone()).collect();
    let left_y: Vec<usize> = left_ids.iter().map(|i| y[*i]).collect();
    let right_x: Vec<Vec<f64>> = right_ids.iter().map(|i| x[*i].clone()).collect();
    let right_y: Vec<usize> = right_ids.iter().map(|i| y[*i]).collect();
    Node::Split {
        feature,
        threshold,
        left: Box::new(fit_tree(&left_x, &left_y, names, depth + 1)),
        right: Box::new(fit_tree(&right_x, &right_y, names, depth + 1)),
    }
}

fn predict(node: &Node, x: &[f64]) -> usize {
    match node {
        Node::Leaf(label) => *label,
        Node::Split {
            feature,
            threshold,
            left,
            right,
        } => {
            if x[*feature] <= *threshold {
                predict(left, x)
            } else {
                predict(right, x)
            }
        }
    }
}

fn gini(labels: &[usize]) -> f64 {
    if labels.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 3];
    for label in labels {
        counts[*label] += 1;
    }
    let n = labels.len() as f64;
    1.0 - counts
        .iter()
        .map(|n_class| (*n_class as f64 / n).powi(2))
        .sum::<f64>()
}

fn majority(labels: &[usize]) -> usize {
    let mut counts = [0usize; 3];
    for label in labels {
        counts[*label] += 1;
    }
    let max = *counts.iter().max().unwrap_or(&0);
    let winners: Vec<usize> = (0..3).filter(|i| counts[*i] == max).collect();
    if winners.len() == 1 {
        winners[0]
    } else {
        2
    }
}

fn metrics(rows: &[&Row], y: &[usize], p: &[usize]) -> Metrics {
    let mut confusion = [[0usize; 3]; 3];
    let mut hard_pos = [0usize; 2];
    let mut hard_neg = [0usize; 2];
    for (i, row) in rows.iter().enumerate() {
        confusion[y[i]][p[i]] += 1;
        if row.overlap_band == "low" && y[i] == 0 {
            hard_pos[0] += 1;
            hard_pos[1] += usize::from(p[i] == 0);
        }
        if row.overlap_band == "high" && y[i] == 1 {
            hard_neg[0] += 1;
            hard_neg[1] += usize::from(p[i] == 1);
        }
    }
    metrics_from_confusion(rows.len(), confusion, hard_pos, hard_neg)
}

fn metrics_from_confusion(
    total: usize,
    confusion: [[usize; 3]; 3],
    hard_pos: [usize; 2],
    hard_neg: [usize; 2],
) -> Metrics {
    let scored: usize = confusion.iter().flatten().sum();
    let correct = (0..3).map(|i| confusion[i][i]).sum::<usize>();
    let recalls: [Option<f64>; 3] = std::array::from_fn(|class| {
        let n = confusion[class].iter().sum::<usize>();
        (n > 0).then_some(confusion[class][class] as f64 / n as f64)
    });
    let observed: Vec<f64> = recalls.iter().flatten().copied().collect();
    let different = confusion[1].iter().sum::<usize>();
    Metrics {
        total,
        scored,
        accuracy: (scored > 0).then_some(correct as f64 / scored as f64),
        balanced_accuracy_observed_classes: (!observed.is_empty())
            .then_some(observed.iter().sum::<f64>() / observed.len() as f64),
        per_class_recall: recalls,
        confusion_truth_by_prediction: confusion,
        false_same_rate_on_different: (different > 0)
            .then_some(confusion[1][0] as f64 / different as f64),
        hard_positive_same_recall: (hard_pos[0] > 0)
            .then_some(hard_pos[1] as f64 / hard_pos[0] as f64),
        hard_positive_count: hard_pos[0],
        hard_negative_different_recall: (hard_neg[0] > 0)
            .then_some(hard_neg[1] as f64 / hard_neg[0] as f64),
        hard_negative_count: hard_neg[0],
    }
}

fn counts(rows: &[&Row]) -> Counts {
    Counts {
        total: rows.len(),
        same: rows.iter().filter(|r| r.truth == 0).count(),
        different: rows.iter().filter(|r| r.truth == 1).count(),
        unknown: rows.iter().filter(|r| r.truth == 2).count(),
        low_overlap: rows.iter().filter(|r| r.overlap_band == "low").count(),
        high_overlap: rows.iter().filter(|r| r.overlap_band == "high").count(),
    }
}

fn label_index(label: &str) -> Result<usize> {
    LABELS
        .iter()
        .position(|candidate| *candidate == label)
        .context("invalid label")
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("missing string field {key}"))
}

fn required_num(value: &Value, key: &str) -> Result<f64> {
    value[key]
        .as_f64()
        .with_context(|| format!("missing numeric field {key}"))
}

fn required_bool(value: &Value, key: &str) -> Result<f64> {
    Ok(
        if value[key]
            .as_bool()
            .with_context(|| format!("missing bool field {key}"))?
        {
            1.0
        } else {
            0.0
        },
    )
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mean_opt(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cart_separates_a_fixed_binary_feature() {
        let x = vec![
            vec![0.0],
            vec![0.1],
            vec![0.2],
            vec![1.0],
            vec![1.1],
            vec![1.2],
        ];
        let y = vec![1, 1, 1, 0, 0, 0];
        let model = fit_tree(&x, &y, &["x".to_string()], 0);
        assert_eq!(predict(&model, &[0.05]), 1);
        assert_eq!(predict(&model, &[1.15]), 0);
    }

    #[test]
    fn tied_leaf_abstains_unknown() {
        assert_eq!(majority(&[0, 1]), 2);
        assert_eq!(majority(&[0, 0, 1]), 0);
    }

    #[test]
    fn no_overlap_view_contains_no_identity_or_jaccard_fields() {
        let pair = json!({
            "shared_tokens":["foo"], "shared_role_tokens":["before:foo"],
            "shared_bigrams":["foo bar"], "shared_trigrams":["foo bar baz"],
            "token_jaccard":0.5,"bigram_jaccard":0.2,"trigram_jaccard":0.1,
            "role_count_abs_delta":[0,1,2],"token_count_abs_delta":1,"distance_bin_abs_delta":0,
            "support_cue_equal":true,"contradiction_cue_equal":false,"same_field_kind":true
        });
        let features = no_overlap_features(&pair).unwrap();
        assert!(features.keys().all(|k| k.starts_with("structure:")));
        assert_eq!(features.len(), 8);
    }

    #[test]
    fn false_same_rate_uses_different_gold_denominator() {
        let metrics = metrics_from_confusion(3, [[1, 0, 0], [2, 1, 0], [0, 0, 0]], [0, 0], [0, 0]);
        assert_eq!(metrics.false_same_rate_on_different, Some(2.0 / 3.0));
    }

    #[test]
    fn absent_unknown_class_has_no_recall_estimate() {
        let metrics = metrics_from_confusion(2, [[1, 0, 0], [0, 1, 0], [0, 0, 0]], [0, 0], [0, 0]);
        assert_eq!(metrics.per_class_recall[2], None);
    }
}
