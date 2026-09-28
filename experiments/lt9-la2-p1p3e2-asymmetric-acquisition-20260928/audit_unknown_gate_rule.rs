//! Rule-path-only audit of synthetic no-context and one-sided context inputs.
//!
//! Reuses the frozen E1 local features and tree. It does not read labels,
//! retrain the tree, or apply relation authority. Outcomes are conditional on
//! a relation having already passed the E1 support gate; current E1 support is
//! not established for these E2 relation groups.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_analyze_core.rs"]
mod analysis_core;
#[path = "../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_features.rs"]
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
struct InputRow {
    example_id: String,
    variant: String,
    target: Option<String>,
    #[serde(default)]
    status: Option<String>,
    model_input: ModelInput,
    metadata: Metadata,
}

#[derive(Deserialize)]
struct ModelInput {
    #[serde(default)]
    query_contexts: Vec<String>,
    #[serde(default)]
    document_contexts: Vec<String>,
}

#[derive(Deserialize)]
struct Metadata {
    base_group_id: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum GateAction {
    Allow,
    Refuse,
    Abstain,
    MixedAbstain,
}

#[derive(Default, Serialize)]
struct ActionCounts {
    allow: usize,
    refuse: usize,
    abstain: usize,
    mixed_abstain: usize,
}

impl ActionCounts {
    fn add(&mut self, action: GateAction) {
        match action {
            GateAction::Allow => self.allow += 1,
            GateAction::Refuse => self.refuse += 1,
            GateAction::Abstain => self.abstain += 1,
            GateAction::MixedAbstain => self.mixed_abstain += 1,
        }
    }
}

#[derive(Serialize)]
struct ExampleResult {
    example_id: String,
    base_group_id: String,
    variant: String,
    target: Option<String>,
    source_status: Option<String>,
    pair_evaluations: usize,
    actions: ActionCounts,
    aggregate_action: GateAction,
    current_e1_action_without_relation_support: GateAction,
}

#[derive(Serialize)]
struct BaseResult {
    base_group_id: String,
    variants: usize,
    variant_actions: BTreeMap<String, GateAction>,
    any_allow: bool,
    any_refuse: bool,
    any_abstain: bool,
    mixed_across_variants: bool,
    synthetic_unknown_any_allow: bool,
    synthetic_unknown_any_refuse: bool,
    synthetic_unknown_all_abstain: bool,
    one_sided_any_allow: bool,
    one_sided_any_refuse: bool,
    one_sided_all_abstain: bool,
}

#[derive(Serialize)]
struct AuditReceipt {
    schema: &'static str,
    status: &'static str,
    interpretation: &'static str,
    same_field_assumption: &'static str,
    bank_sha256: String,
    one_sided_audit_sha256: String,
    e1_receipt_sha256: String,
    relation_support_applied: bool,
    feature_view: &'static str,
    input_examples: usize,
    input_base_pairs: usize,
    by_variant: BTreeMap<String, ActionCounts>,
    examples: Vec<ExampleResult>,
    base_pairs: Vec<BaseResult>,
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn read_jsonl(path: &Path) -> Result<Vec<InputRow>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).with_context(|| format!("parse {}", path.display())))
        .collect()
}

fn tree_action(tree: &Value, features: &BTreeMap<String, f64>) -> Result<GateAction> {
    let mut node = tree;
    loop {
        let kind = node
            .get("kind")
            .and_then(Value::as_str)
            .context("tree node missing kind")?;
        if kind == "leaf" {
            let prediction = node
                .get("prediction")
                .and_then(Value::as_str)
                .context("leaf missing prediction")?;
            return Ok(match prediction {
                "DIFFERENT" => GateAction::Refuse,
                "UNKNOWN" => GateAction::Abstain,
                "SAME" => {
                    let counts = node.get("counts").context("leaf missing counts")?;
                    let same = counts.get("same").and_then(Value::as_u64).unwrap_or(0);
                    let different = counts.get("different").and_then(Value::as_u64).unwrap_or(0);
                    let unknown = counts.get("unknown").and_then(Value::as_u64).unwrap_or(0);
                    if same > 0 && different == 0 && unknown == 0 {
                        GateAction::Allow
                    } else {
                        GateAction::Abstain
                    }
                }
                other => anyhow::bail!("unexpected E1 leaf prediction: {other}"),
            });
        }
        ensure!(kind == "split", "unexpected E1 tree node kind: {kind}");
        let feature = node
            .get("feature")
            .and_then(Value::as_str)
            .context("split missing feature")?;
        let threshold = node
            .get("threshold")
            .and_then(Value::as_f64)
            .context("split missing threshold")?;
        let observed = *features.get(feature).unwrap_or(&0.0);
        node = if observed <= threshold {
            node.get("left").context("split missing left child")?
        } else {
            node.get("right").context("split missing right child")?
        };
    }
}

fn pair_action(tree: &Value, left: &str, right: &str) -> Result<GateAction> {
    // The E2 packet builder enforces candidate masking. Empty candidate strings
    // therefore preserve the exact feature path without exposing relation IDs.
    let left_features = features::context_features(
        &p1o1::Occurrence {
            field: "context".to_owned(),
            excerpt: left.to_owned(),
        },
        "",
        "",
    );
    let right_features = features::context_features(
        &p1o1::Occurrence {
            field: "context".to_owned(),
            excerpt: right.to_owned(),
        },
        "",
        "",
    );
    let pair = features::pair_features(&left_features, &right_features);
    let feature_row = analysis_core::make_features(&pair, analysis_core::View::FullLocal);
    tree_action(tree, &feature_row)
}

fn aggregate(actions: &BTreeSet<GateAction>) -> GateAction {
    if actions.len() != 1 {
        return GateAction::MixedAbstain;
    }
    *actions.iter().next().expect("nonempty action set")
}

fn evaluate_row(tree: &Value, row: InputRow) -> Result<ExampleResult> {
    let left = if row.model_input.query_contexts.is_empty() {
        vec![String::new()]
    } else {
        row.model_input.query_contexts
    };
    let right = if row.model_input.document_contexts.is_empty() {
        vec![String::new()]
    } else {
        row.model_input.document_contexts
    };
    let mut actions = ActionCounts::default();
    let mut distinct = BTreeSet::new();
    let mut pair_evaluations = 0usize;
    for query in &left {
        for document in &right {
            let action = pair_action(tree, query, document)?;
            actions.add(action);
            distinct.insert(action);
            pair_evaluations += 1;
        }
    }
    let aggregate_action = aggregate(&distinct);
    Ok(ExampleResult {
        example_id: row.example_id,
        base_group_id: row.metadata.base_group_id,
        variant: row.variant,
        target: row.target,
        source_status: row.status,
        pair_evaluations,
        actions,
        aggregate_action,
        // E2 relation IDs have no entries in the E1 support map, so the exact
        // current wrapper would abstain before reaching this diagnostic tree.
        current_e1_action_without_relation_support: GateAction::Abstain,
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(
        args.len() == 5,
        "usage: lt9_la2p1p3e2_unknown_gate_audit <unknown-bank.jsonl> <one-sided-audit.jsonl> <e1-transport-receipt.json> <output.json>"
    );
    let bank_path = Path::new(&args[1]);
    let one_sided_path = Path::new(&args[2]);
    let e1_path = Path::new(&args[3]);
    let output_path = Path::new(&args[4]);
    ensure!(!output_path.exists(), "refusing to overwrite audit output");

    let mut rows = read_jsonl(bank_path)?;
    rows.extend(read_jsonl(one_sided_path)?);
    ensure!(!rows.is_empty(), "no audit rows");
    let receipt: Value = serde_json::from_slice(&fs::read(e1_path)?)?;
    let tree = receipt
        .get("full_local_tree")
        .context("E1 receipt missing full_local_tree")?;

    let mut examples = rows
        .into_iter()
        .map(|row| evaluate_row(tree, row))
        .collect::<Result<Vec<_>>>()?;
    examples.sort_by(|a, b| {
        (&a.base_group_id, &a.variant, &a.example_id).cmp(&(
            &b.base_group_id,
            &b.variant,
            &b.example_id,
        ))
    });

    let mut by_variant = BTreeMap::<String, ActionCounts>::new();
    let mut by_base = BTreeMap::<String, Vec<&ExampleResult>>::new();
    for example in &examples {
        by_variant
            .entry(example.variant.clone())
            .or_default()
            .add(example.aggregate_action);
        by_base
            .entry(example.base_group_id.clone())
            .or_default()
            .push(example);
    }
    let base_pairs = by_base
        .into_iter()
        .map(|(base_group_id, group)| {
            let actions: BTreeSet<_> = group.iter().map(|row| row.aggregate_action).collect();
            let variant_actions: BTreeMap<_, _> = group
                .iter()
                .map(|row| (row.variant.clone(), row.aggregate_action))
                .collect();
            let unknown: Vec<_> = group
                .iter()
                .filter(|row| {
                    matches!(
                        row.variant.as_str(),
                        "BOTH_SIDES_ABSENT" | "BOTH_ENDPOINT_MARKERS_ONLY"
                    )
                })
                .collect();
            let one_sided: Vec<_> = group
                .iter()
                .filter(|row| {
                    matches!(
                        row.variant.as_str(),
                        "QUERY_SIDE_ABSENT" | "DOCUMENT_SIDE_ABSENT"
                    )
                })
                .collect();
            BaseResult {
                base_group_id,
                variants: group.len(),
                variant_actions,
                any_allow: actions.contains(&GateAction::Allow),
                any_refuse: actions.contains(&GateAction::Refuse),
                any_abstain: actions.contains(&GateAction::Abstain)
                    || actions.contains(&GateAction::MixedAbstain),
                mixed_across_variants: actions.len() > 1,
                synthetic_unknown_any_allow: unknown
                    .iter()
                    .any(|row| row.aggregate_action == GateAction::Allow),
                synthetic_unknown_any_refuse: unknown
                    .iter()
                    .any(|row| row.aggregate_action == GateAction::Refuse),
                synthetic_unknown_all_abstain: unknown.len() == 2
                    && unknown
                        .iter()
                        .all(|row| row.aggregate_action == GateAction::Abstain),
                one_sided_any_allow: one_sided
                    .iter()
                    .any(|row| row.aggregate_action == GateAction::Allow),
                one_sided_any_refuse: one_sided
                    .iter()
                    .any(|row| row.aggregate_action == GateAction::Refuse),
                one_sided_all_abstain: one_sided.len() == 2
                    && one_sided
                        .iter()
                        .all(|row| row.aggregate_action == GateAction::Abstain),
            }
        })
        .collect::<Vec<_>>();
    let audit = AuditReceipt {
        schema: "phoenix.lexical.p1p3e2-unknown-gate-rule-audit/v2",
        status: "FROZEN_TREE_RULE_PATH_DIAGNOSTIC_NOT_GATE_QUALIFICATION",
        interpretation: "Tree outputs are conditional on relation support; the current E1 wrapper abstains for all these unseen relation IDs.",
        same_field_assumption: "Both excerpts treated as the same context field because source packets omit field provenance.",
        bank_sha256: sha256(bank_path)?,
        one_sided_audit_sha256: sha256(one_sided_path)?,
        e1_receipt_sha256: sha256(e1_path)?,
        relation_support_applied: false,
        feature_view: "P_full_local using frozen E1 feature/tree implementation",
        input_examples: examples.len(),
        input_base_pairs: base_pairs.len(),
        by_variant,
        examples,
        base_pairs,
    };
    let bytes = serde_json::to_vec_pretty(&audit)?;
    fs::write(output_path, bytes)?;
    println!(
        "wrote {} examples={} base_pairs={} bank_sha256={}",
        output_path.display(),
        audit.input_examples,
        audit.input_base_pairs,
        audit.bank_sha256
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pure_same_leaf_allows() {
        let mut features = BTreeMap::new();
        let pure_same = serde_json::json!({
            "kind": "leaf",
            "prediction": "SAME",
            "counts": {"same": 4, "different": 0, "unknown": 0}
        });
        let mixed_same = serde_json::json!({
            "kind": "leaf",
            "prediction": "SAME",
            "counts": {"same": 4, "different": 1, "unknown": 0}
        });
        assert_eq!(
            tree_action(&pure_same, &features).unwrap(),
            GateAction::Allow
        );
        assert_eq!(
            tree_action(&mixed_same, &features).unwrap(),
            GateAction::Abstain
        );
        let different = serde_json::json!({"kind": "leaf", "prediction": "DIFFERENT"});
        assert_eq!(
            tree_action(&different, &features).unwrap(),
            GateAction::Refuse
        );
    }

    #[test]
    fn heterogeneous_context_pair_actions_abstain_in_aggregate() {
        let one = BTreeSet::from([GateAction::Abstain]);
        assert_eq!(aggregate(&one), GateAction::Abstain);
        let mixed = BTreeSet::from([GateAction::Allow, GateAction::Refuse]);
        assert_eq!(aggregate(&mixed), GateAction::MixedAbstain);
    }
}
