use super::{FeatureMap, Row, TreeReceipt, MAX_DEPTH, MIN_LEAF, UNKNOWN};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub(super) enum Node {
    Leaf(usize),
    Split {
        feature: String,
        branches: BTreeMap<u16, Box<Node>>,
        fallback: usize,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Tree {
    pub(super) root: Node,
    pub(super) split_features: Vec<String>,
}

pub(super) fn majority(rows: &[Row], indices: &[usize], unknown_class: usize) -> usize {
    let mut counts = [0usize; 4];
    for i in indices {
        counts[rows[*i].label.min(UNKNOWN)] += 1;
    }
    let max = counts.iter().copied().max().unwrap_or(0);
    let tied = counts
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (*c == max).then_some(i))
        .collect::<Vec<_>>();
    if tied.contains(&unknown_class) {
        unknown_class
    } else {
        *tied.first().unwrap_or(&unknown_class)
    }
}

pub(super) fn gini(labels: &[usize]) -> f64 {
    if labels.is_empty() {
        return 0.0;
    }
    let mut count = [0usize; 4];
    for label in labels {
        count[(*label).min(UNKNOWN)] += 1;
    }
    1.0 - count
        .iter()
        .map(|n| {
            let p = *n as f64 / labels.len() as f64;
            p * p
        })
        .sum::<f64>()
}

pub(super) fn train_node(
    rows: &[Row],
    indices: &[usize],
    depth: u8,
    max_depth: u8,
    unknown_class: usize,
    splits: &mut Vec<String>,
) -> Node {
    let fallback = majority(rows, indices, unknown_class);
    if depth >= max_depth
        || indices.len() < MIN_LEAF * 2
        || gini(&indices.iter().map(|i| rows[*i].label).collect::<Vec<_>>()) == 0.0
    {
        return Node::Leaf(fallback);
    }
    let mut names = BTreeSet::new();
    for i in indices {
        names.extend(rows[*i].features.0.keys().cloned());
    }
    let parent = gini(&indices.iter().map(|i| rows[*i].label).collect::<Vec<_>>());
    let mut best: Option<(f64, String, BTreeMap<u16, Vec<usize>>)> = None;
    for name in names {
        let mut branches = BTreeMap::<u16, Vec<usize>>::new();
        for i in indices {
            let value = rows[*i].features.0.get(&name).copied().unwrap_or(0);
            branches.entry(value).or_default().push(*i);
        }
        if branches.len() < 2 || branches.values().any(|v| v.len() < MIN_LEAF) {
            continue;
        }
        let weighted = branches
            .values()
            .map(|part| {
                let labels = part.iter().map(|i| rows[*i].label).collect::<Vec<_>>();
                (part.len() as f64 / indices.len() as f64) * gini(&labels)
            })
            .sum::<f64>();
        let gain = parent - weighted;
        if gain > 1e-12
            && best.as_ref().map_or(true, |b| {
                gain > b.0 + 1e-12 || ((gain - b.0).abs() <= 1e-12 && name < b.1)
            })
        {
            best = Some((gain, name, branches));
        }
    }
    let Some((_, name, branches)) = best else {
        return Node::Leaf(fallback);
    };
    splits.push(name.clone());
    let branches = branches
        .into_iter()
        .map(|(value, part)| {
            let child = train_node(rows, &part, depth + 1, max_depth, unknown_class, splits);
            (value, Box::new(child))
        })
        .collect();
    Node::Split {
        feature: name,
        branches,
        fallback,
    }
}

pub(super) fn train_tree(rows: &[Row], max_depth: u8, unknown_class: usize) -> Tree {
    let indices = (0..rows.len()).collect::<Vec<_>>();
    let mut split_features = Vec::new();
    let root = train_node(
        rows,
        &indices,
        0,
        max_depth,
        unknown_class,
        &mut split_features,
    );
    Tree {
        root,
        split_features,
    }
}

pub(super) fn predict(node: &Node, features: &FeatureMap) -> usize {
    match node {
        Node::Leaf(value) => *value,
        Node::Split {
            feature,
            branches,
            fallback,
        } => {
            let value = features.0.get(feature).copied().unwrap_or(0);
            branches
                .get(&value)
                .map_or(*fallback, |child| predict(child, features))
        }
    }
}

pub(super) fn tree_receipt(
    tree: &Tree,
    train_rows: usize,
    test: &[(u8, usize, FeatureMap)],
) -> TreeReceipt {
    let mut confusion = vec![vec![0u64; 4]; 4];
    let mut correct = 0u64;
    let mut groups = BTreeMap::<u8, (u64, u64)>::new();
    for (group, expected, features) in test {
        let predicted = predict(&tree.root, features).min(UNKNOWN);
        confusion[*expected][predicted] += 1;
        correct += u64::from(predicted == *expected);
        let entry = groups.entry(*group).or_default();
        entry.1 += 1;
        entry.0 += u64::from(predicted == *expected);
    }
    let accuracy = correct as f64 / test.len().max(1) as f64;
    let group_macro = groups
        .values()
        .map(|(c, n)| *c as f64 / *n as f64)
        .sum::<f64>()
        / groups.len().max(1) as f64;
    TreeReceipt {
        max_depth: MAX_DEPTH,
        class_axis: [String::new(), String::new(), String::new(), String::new()],
        train_rows,
        test_rows: test.len(),
        split_features: tree.split_features.clone(),
        heldout_accuracy: accuracy,
        heldout_group_macro_accuracy: group_macro,
        class_confusion: confusion,
    }
}
