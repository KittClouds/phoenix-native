//! P1O2 deterministic CART and pairwise-metric core.
use super::PairFeatures;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Label {
    Same,
    Different,
    Unknown,
}

#[derive(Clone)]
pub(super) struct Example {
    pub(super) packet_id: String,
    pub(super) candidate_id: String,
    pub(super) split: String,
    pub(super) overlap_band: String,
    pub(super) left_node: String,
    pub(super) right_node: String,
    pub(super) label: Label,
    pub(super) features: BTreeMap<String, f64>,
}

struct CandidateSplit<'a> {
    gain: f64,
    feature: String,
    threshold: f64,
    left: Vec<&'a Example>,
    right: Vec<&'a Example>,
}

#[derive(Clone, Copy)]
pub(super) enum View {
    FullLocal,
    NoExactOverlap,
}

impl View {
    fn name(self) -> &'static str {
        match self {
            Self::FullLocal => "P_full_local",
            Self::NoExactOverlap => "P_no_exact_overlap",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Tree {
    Leaf {
        prediction: Label,
        rows: usize,
        counts: LabelCounts,
    },
    Split {
        feature: String,
        threshold: f64,
        rows: usize,
        left: Box<Tree>,
        right: Box<Tree>,
    },
}

#[derive(Clone, Copy, Default, Serialize)]
pub(super) struct LabelCounts {
    pub(super) same: usize,
    pub(super) different: usize,
    pub(super) unknown: usize,
}

impl LabelCounts {
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

    fn predict(self) -> Label {
        let max = self.same.max(self.different).max(self.unknown);
        let winners = [
            (Label::Same, self.same),
            (Label::Different, self.different),
            (Label::Unknown, self.unknown),
        ]
        .into_iter()
        .filter(|(_, count)| *count == max)
        .count();
        if winners != 1 {
            Label::Unknown
        } else if self.same == max {
            Label::Same
        } else if self.different == max {
            Label::Different
        } else {
            Label::Unknown
        }
    }
}

#[derive(Serialize)]
struct Confusion {
    actual_same: LabelCounts,
    actual_different: LabelCounts,
    actual_unknown: LabelCounts,
}

impl Confusion {
    fn new() -> Self {
        Self {
            actual_same: LabelCounts::default(),
            actual_different: LabelCounts::default(),
            actual_unknown: LabelCounts::default(),
        }
    }

    fn add(&mut self, actual: Label, predicted: Label) {
        match actual {
            Label::Same => self.actual_same.add(predicted),
            Label::Different => self.actual_different.add(predicted),
            Label::Unknown => self.actual_unknown.add(predicted),
        }
    }

    fn row(&self, label: Label) -> LabelCounts {
        match label {
            Label::Same => self.actual_same,
            Label::Different => self.actual_different,
            Label::Unknown => self.actual_unknown,
        }
    }
}

#[derive(Serialize)]
struct Metrics {
    rows: usize,
    accuracy: Option<f64>,
    observed_class_balanced_accuracy: Option<f64>,
    recall_same: Option<f64>,
    recall_different: Option<f64>,
    recall_unknown: Option<f64>,
    false_same_given_different: Option<f64>,
    unknown_to_same: usize,
    unknown_to_different: usize,
    low_overlap_same_rows: usize,
    low_overlap_same_recall: Option<f64>,
    high_overlap_different_rows: usize,
    high_overlap_different_recall: Option<f64>,
    predicted_counts: LabelCounts,
    confusion: Confusion,
}

#[derive(Serialize)]
pub(super) struct ViewReceipt {
    view: &'static str,
    inferential_status: &'static str,
    fit_rows: usize,
    fit_classes_present: usize,
    fit_accuracy: Option<f64>,
    holdout: Metrics,
    tree: Tree,
}

#[derive(Serialize)]
pub(super) struct Triangle {
    graph: String,
    nodes: [String; 3],
    edge_packet_ids: [String; 3],
    edge_labels: [Label; 3],
    label_pattern: String,
}

fn bool_number(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

pub(super) fn contains_candidate_token(value: &str, a: &str, b: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| token.eq_ignore_ascii_case(a) || token.eq_ignore_ascii_case(b))
}

pub(super) fn make_features(pair: &PairFeatures, view: View) -> BTreeMap<String, f64> {
    let mut result = BTreeMap::new();
    for (index, role) in ["before", "between", "after"].into_iter().enumerate() {
        result.insert(
            format!("structure:role_{role}_abs_delta"),
            f64::from(pair.role_count_abs_delta[index]),
        );
    }
    result.insert(
        "structure:token_count_abs_delta".to_owned(),
        f64::from(pair.token_count_abs_delta),
    );
    result.insert(
        "structure:near_count_abs_delta".to_owned(),
        f64::from(pair.near_count_abs_delta),
    );
    result.insert(
        "cue:support_equal".to_owned(),
        bool_number(pair.support_cue_equal),
    );
    result.insert(
        "cue:contradiction_equal".to_owned(),
        bool_number(pair.contradiction_cue_equal),
    );
    result.insert(
        "field:same_kind".to_owned(),
        bool_number(pair.same_field_kind),
    );
    if matches!(view, View::FullLocal) {
        result.insert("overlap:token_jaccard".to_owned(), pair.token_jaccard);
        result.insert("overlap:bigram_jaccard".to_owned(), pair.bigram_jaccard);
        result.insert("overlap:trigram_jaccard".to_owned(), pair.trigram_jaccard);
        for token in &pair.shared_tokens {
            result.insert(format!("shared_token:{token}"), 1.0);
        }
        for token in &pair.shared_role_tokens {
            result.insert(format!("shared_role_token:{token}"), 1.0);
        }
        for gram in &pair.shared_bigrams {
            result.insert(format!("shared_bigram:{gram}"), 1.0);
        }
        for gram in &pair.shared_trigrams {
            result.insert(format!("shared_trigram:{gram}"), 1.0);
        }
    }
    result
}

fn gini(rows: &[&Example]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut counts = LabelCounts::default();
    for row in rows {
        counts.add(row.label);
    }
    let n = counts.total() as f64;
    1.0 - [counts.same, counts.different, counts.unknown]
        .into_iter()
        .map(|count| {
            let p = count as f64 / n;
            p * p
        })
        .sum::<f64>()
}

fn fit_tree(rows: &[&Example], features: &[String], depth: usize) -> (Tree, bool) {
    let mut counts = LabelCounts::default();
    for row in rows {
        counts.add(row.label);
    }
    if depth >= 3
        || counts.same == counts.total()
        || counts.different == counts.total()
        || counts.unknown == counts.total()
    {
        return (
            Tree::Leaf {
                prediction: counts.predict(),
                rows: rows.len(),
                counts,
            },
            false,
        );
    }

    let parent = gini(rows);
    let mut best: Option<CandidateSplit<'_>> = None;
    for feature in features {
        let mut values: Vec<f64> = rows
            .iter()
            .map(|row| *row.features.get(feature).unwrap_or(&0.0))
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| a.total_cmp(b).is_eq());
        if values.len() < 2 {
            continue;
        }
        for threshold in values.iter().take(values.len() - 1).copied() {
            let mut left = Vec::new();
            let mut right = Vec::new();
            for row in rows {
                if *row.features.get(feature).unwrap_or(&0.0) <= threshold {
                    left.push(*row);
                } else {
                    right.push(*row);
                }
            }
            if left.len() < 3 || right.len() < 3 {
                continue;
            }
            let weighted = (left.len() as f64 * gini(&left) + right.len() as f64 * gini(&right))
                / rows.len() as f64;
            let gain = parent - weighted;
            if gain <= 0.0 {
                continue;
            }
            let replace = best.as_ref().is_none_or(|best| {
                gain > best.gain
                    || (gain == best.gain
                        && (feature < &best.feature
                            || (feature == &best.feature && threshold < best.threshold)))
            });
            if replace {
                best = Some(CandidateSplit {
                    gain,
                    feature: feature.clone(),
                    threshold,
                    left,
                    right,
                });
            }
        }
    }
    let Some(best) = best else {
        return (
            Tree::Leaf {
                prediction: counts.predict(),
                rows: rows.len(),
                counts,
            },
            false,
        );
    };
    let (left, _) = fit_tree(&best.left, features, depth + 1);
    let (right, _) = fit_tree(&best.right, features, depth + 1);
    (
        Tree::Split {
            feature: best.feature,
            threshold: best.threshold,
            rows: rows.len(),
            left: Box::new(left),
            right: Box::new(right),
        },
        depth == 0,
    )
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

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn metrics(rows: &[&Example], tree: &Tree) -> Metrics {
    let mut confusion = Confusion::new();
    let mut predicted = LabelCounts::default();
    let mut low_same = (0usize, 0usize);
    let mut high_different = (0usize, 0usize);
    let mut correct = 0usize;
    let mut unknown_to_same = 0usize;
    let mut unknown_to_different = 0usize;
    let mut diff_as_same = 0usize;
    for row in rows {
        let p = predict(tree, row);
        confusion.add(row.label, p);
        predicted.add(p);
        correct += usize::from(p == row.label);
        if row.label == Label::Unknown && p == Label::Same {
            unknown_to_same += 1;
        }
        if row.label == Label::Unknown && p == Label::Different {
            unknown_to_different += 1;
        }
        if row.label == Label::Different && p == Label::Same {
            diff_as_same += 1;
        }
        if row.label == Label::Same && row.overlap_band == "low" {
            low_same.0 += usize::from(p == Label::Same);
            low_same.1 += 1;
        }
        if row.label == Label::Different && row.overlap_band == "high" {
            high_different.0 += usize::from(p == Label::Different);
            high_different.1 += 1;
        }
    }
    let recall = |label| {
        let row = confusion.row(label);
        let correct = match label {
            Label::Same => row.same,
            Label::Different => row.different,
            Label::Unknown => row.unknown,
        };
        rate(correct, row.total())
    };
    let observed_recalls: Vec<f64> = [Label::Same, Label::Different, Label::Unknown]
        .into_iter()
        .filter_map(recall)
        .collect();
    Metrics {
        rows: rows.len(),
        accuracy: rate(correct, rows.len()),
        observed_class_balanced_accuracy: (!observed_recalls.is_empty())
            .then(|| observed_recalls.iter().sum::<f64>() / observed_recalls.len() as f64),
        recall_same: recall(Label::Same),
        recall_different: recall(Label::Different),
        recall_unknown: recall(Label::Unknown),
        false_same_given_different: rate(diff_as_same, confusion.actual_different.total()),
        unknown_to_same,
        unknown_to_different,
        low_overlap_same_rows: low_same.1,
        low_overlap_same_recall: rate(low_same.0, low_same.1),
        high_overlap_different_rows: high_different.1,
        high_overlap_different_recall: rate(high_different.0, high_different.1),
        predicted_counts: predicted,
        confusion,
    }
}

pub(super) fn label_counts(rows: &[&Example]) -> LabelCounts {
    let mut counts = LabelCounts::default();
    for row in rows {
        counts.add(row.label);
    }
    counts
}

fn classes_present(counts: LabelCounts) -> usize {
    usize::from(counts.same > 0)
        + usize::from(counts.different > 0)
        + usize::from(counts.unknown > 0)
}

pub(super) fn analyze_view(rows: &[Example], view: View) -> ViewReceipt {
    let fit: Vec<&Example> = rows.iter().filter(|row| row.split == "fit").collect();
    let holdout: Vec<&Example> = rows.iter().filter(|row| row.split == "holdout").collect();
    let features: Vec<String> = fit
        .iter()
        .flat_map(|row| row.features.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let fit_counts = label_counts(&fit);
    let (tree, root_split) = fit_tree(&fit, &features, 0);
    let status = if classes_present(fit_counts) < 2 || !root_split {
        "UNDERPOWERED_CONSTANT_DIAGNOSTIC"
    } else {
        "FROZEN_HOLDOUT_DIAGNOSTIC"
    };
    let fit_correct = fit
        .iter()
        .filter(|row| predict(&tree, row) == row.label)
        .count();
    ViewReceipt {
        view: view.name(),
        inferential_status: status,
        fit_rows: fit.len(),
        fit_classes_present: classes_present(fit_counts),
        fit_accuracy: rate(fit_correct, fit.len()),
        holdout: metrics(&holdout, &tree),
        tree,
    }
}

pub(super) fn triangles(rows: &[Example]) -> Vec<Triangle> {
    let mut by_graph: BTreeMap<String, Vec<&Example>> = BTreeMap::new();
    for row in rows {
        by_graph.entry(row.split.clone()).or_default().push(row);
    }
    let mut result = Vec::new();
    for (graph, edges) in by_graph {
        let nodes: Vec<String> = edges
            .iter()
            .flat_map(|edge| [edge.left_node.clone(), edge.right_node.clone()])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut edge_map: HashMap<(String, String), &Example> = HashMap::new();
        for edge in &edges {
            let key = ordered_pair(&edge.left_node, &edge.right_node);
            edge_map.insert(key, edge);
        }
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                for k in (j + 1)..nodes.len() {
                    let triple = [&nodes[i], &nodes[j], &nodes[k]];
                    let pairs = [
                        ordered_pair(triple[0], triple[1]),
                        ordered_pair(triple[0], triple[2]),
                        ordered_pair(triple[1], triple[2]),
                    ];
                    let Some(e01) = edge_map.get(&pairs[0]) else {
                        continue;
                    };
                    let Some(e02) = edge_map.get(&pairs[1]) else {
                        continue;
                    };
                    let Some(e12) = edge_map.get(&pairs[2]) else {
                        continue;
                    };
                    let edges3 = [*e01, *e02, *e12];
                    let labels = [edges3[0].label, edges3[1].label, edges3[2].label];
                    if labels.contains(&Label::Unknown) {
                        continue;
                    }
                    let pattern = labels
                        .iter()
                        .map(|label| match label {
                            Label::Same => 'S',
                            Label::Different => 'D',
                            Label::Unknown => 'U',
                        })
                        .collect();
                    result.push(Triangle {
                        graph: graph.clone(),
                        nodes: [triple[0].clone(), triple[1].clone(), triple[2].clone()],
                        edge_packet_ids: [
                            edges3[0].packet_id.clone(),
                            edges3[1].packet_id.clone(),
                            edges3[2].packet_id.clone(),
                        ],
                        edge_labels: labels,
                        label_pattern: pattern,
                    });
                }
            }
        }
    }
    result
}

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(label: Label, feature: f64) -> Example {
        Example {
            packet_id: String::new(),
            candidate_id: String::new(),
            split: "fit".to_owned(),
            overlap_band: String::new(),
            left_node: String::new(),
            right_node: String::new(),
            label,
            features: BTreeMap::from([("f".to_owned(), feature)]),
        }
    }

    #[test]
    fn leaf_ties_resolve_to_unknown() {
        let rows = [row(Label::Same, 0.0), row(Label::Different, 0.0)];
        let counts = label_counts(&rows.iter().collect::<Vec<_>>());
        assert_eq!(counts.predict(), Label::Unknown);
    }

    #[test]
    fn tree_respects_three_row_child_floor() {
        let rows = [
            row(Label::Same, 0.0),
            row(Label::Same, 0.0),
            row(Label::Same, 0.0),
            row(Label::Same, 0.0),
            row(Label::Different, 1.0),
            row(Label::Different, 1.0),
            row(Label::Different, 1.0),
        ];
        let refs: Vec<_> = rows.iter().collect();
        let (tree, split) = fit_tree(&refs, &["f".to_owned()], 0);
        assert!(split);
        assert_eq!(predict(&tree, &rows[6]), Label::Different);
    }

    #[test]
    fn no_exact_overlap_view_excludes_identity_features() {
        let pair = PairFeatures {
            shared_tokens: vec!["common".to_owned()],
            shared_role_tokens: vec!["before:common".to_owned()],
            shared_bigrams: vec!["a b".to_owned()],
            shared_trigrams: vec!["a b c".to_owned()],
            token_jaccard: 0.5,
            bigram_jaccard: 0.2,
            trigram_jaccard: 0.1,
            role_count_abs_delta: [1, 0, 2],
            token_count_abs_delta: 3,
            near_count_abs_delta: 1,
            support_cue_equal: true,
            contradiction_cue_equal: false,
            same_field_kind: true,
        };
        let reduced = make_features(&pair, View::NoExactOverlap);
        assert!(reduced.keys().all(|key| key.starts_with("structure:")
            || key.starts_with("cue:")
            || key.starts_with("field:")));
        assert_eq!(reduced.len(), 8);
    }

    #[test]
    fn candidate_firewall_checks_role_and_gram_tokens() {
        assert!(contains_candidate_token("before:save", "save", "spare"));
        assert!(contains_candidate_token(
            "saving the spare tire",
            "save",
            "spare"
        ));
        assert!(!contains_candidate_token("before:banking", "bank", "water"));
    }
}
