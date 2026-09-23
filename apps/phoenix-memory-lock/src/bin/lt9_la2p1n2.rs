#![allow(dead_code)]

#[path = "lt9_la2p1m1_core.rs"]
mod core;
#[path = "lt9_la2p1n2_data.rs"]
mod data;
#[path = "lt9_la2p1n2_tree.rs"]
mod tree;
use data::{candidate_words, family_route, generate_stimuli, local_features, pair_rows, relation};
use tree::{predict, train_tree, tree_receipt, Tree};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

const RESULT_SCHEMA: &str = "phoenix.lexical.lt9-la2-p1n2-result/v1";
const TRAIN_GROUPS: u8 = 8;
const GROUPS: u8 = 12;
const MAX_DEPTH: u8 = 3;
const MIN_LEAF: usize = 2;
const UNKNOWN: usize = 3;
const SAME: usize = 0;
const DIFFERENT: usize = 1;
const PAIR_UNKNOWN: usize = 2;
const SOURCE_POS: usize = 5;
const TARGET_POS: usize = 7;
const SLOT_POSITIONS: [usize; 6] = [2, 3, 4, 6, 8, 9];
const NEUTRAL_WORDS: [&str; 16] = [
    "the", "near", "beside", "with", "around", "plain", "local", "object", "area", "some", "and",
    "then", "quiet", "small", "common", "context",
];

#[derive(Clone, Copy)]
struct Relation {
    candidate_index: usize,
    id: &'static str,
    frame_a: usize,
    frame_b: usize,
    cues_a: &'static [&'static str],
    cues_b: &'static [&'static str],
}

const RELATIONS: [Relation; 3] = [
    Relation {
        candidate_index: 11,
        id: "bank_to_water",
        frame_a: 1,
        frame_b: 0,
        cues_a: &[
            "river",
            "shore",
            "current",
            "flood",
            "stream",
            "erosion",
            "watershed",
            "estuary",
            "tidal",
            "wetland",
            "channel",
            "riparian",
        ],
        cues_b: &[
            "deposit", "teller", "savings", "lender", "mortgage", "account", "interest", "payment",
            "borrower", "branch", "capital", "finance",
        ],
    },
    Relation {
        candidate_index: 2,
        id: "car_to_vehicle",
        frame_a: 2,
        frame_b: 0,
        cues_a: &[
            "driver",
            "road",
            "engine",
            "traffic",
            "garage",
            "steering",
            "tire",
            "motor",
            "highway",
            "wheel",
            "brake",
            "transport",
        ],
        cues_b: &[
            "loan",
            "credit",
            "lease",
            "interest",
            "payment",
            "borrower",
            "lender",
            "installment",
            "finance",
            "account",
            "purchase",
            "debt",
        ],
    },
    Relation {
        candidate_index: 9,
        id: "insurance_to_coverage",
        frame_a: 0,
        frame_b: 2,
        cues_a: &[
            "policy",
            "premium",
            "claim",
            "insurer",
            "benefit",
            "underwrite",
            "deductible",
            "liability",
            "protection",
            "reimbursement",
            "payout",
            "actuary",
        ],
        cues_b: &[
            "collision",
            "garage",
            "driver",
            "road",
            "vehicle",
            "repair",
            "traffic",
            "motor",
            "crash",
            "tire",
            "transport",
            "highway",
        ],
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
enum Condition {
    ABase,
    ATopicSwap,
    ATopicAmplify,
    BBase,
    BTopicSwap,
    BTopicAmplify,
    CueRemoved,
    Conflict,
}

#[derive(Clone, Debug, Serialize)]
struct Stimulus {
    candidate: usize,
    group: u8,
    condition: Condition,
    label: Option<usize>,
    tokens: Vec<String>,
    title_end: usize,
}

#[derive(Clone, Debug, Default)]
struct FeatureMap(BTreeMap<String, u16>);

impl FeatureMap {
    fn add(&mut self, key: impl Into<String>) {
        let value = self.0.entry(key.into()).or_default();
        *value = value.saturating_add(1);
    }
    fn set(&mut self, key: impl Into<String>, value: u16) {
        self.0.insert(key.into(), value);
    }
}

#[derive(Clone, Serialize)]
struct TreeReceipt {
    max_depth: u8,
    class_axis: [String; 4],
    train_rows: usize,
    test_rows: usize,
    split_features: Vec<String>,
    heldout_accuracy: f64,
    heldout_group_macro_accuracy: f64,
    class_confusion: Vec<Vec<u64>>,
}

#[derive(Clone, Serialize)]
struct CandidateReceipt {
    candidate_id: String,
    train_groups: u8,
    heldout_groups: u8,
    heldout_stimuli: usize,
    current_family_accuracy: f64,
    context_only_family_accuracy: f64,
    current_family_topic_invariance: Option<f64>,
    context_only_topic_invariance: Option<f64>,
    local_probe: TreeReceipt,
    local_topic_invariance: Option<f64>,
    local_sense_sensitivity: Option<f64>,
    local_ambiguity_calibration: f64,
    local_condition_accuracy: BTreeMap<String, f64>,
    compatibility_probe: TreeReceipt,
    compatibility_group_macro_by_class: [f64; 3],
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    branch: String,
    protocol_sha256: String,
    prerun_manifest_sha256: String,
    source_sha256: String,
    data_sha256: String,
    tree_sha256: String,
    core_sha256: String,
    cargo_manifest_sha256: String,
    cargo_lock_sha256: String,
    binary_sha256: String,
    stimuli_sha256: String,
    scope: Scope,
    total_stimuli: usize,
    total_base_groups: usize,
    feature_firewall: FeatureFirewall,
    candidates: Vec<CandidateReceipt>,
}

#[derive(Serialize)]
struct Scope {
    natural_corpus_read: bool,
    validity_or_relevance_labels_read: bool,
    p1m2r_or_p1n1_outcome_receipts_read: bool,
    webis_touche_read: bool,
    natural_authority_updated: bool,
    retrieval_or_ranking_run: bool,
    router_selected_or_promoted: bool,
}

#[derive(Serialize)]
struct FeatureFirewall {
    candidate_tokens_in_local_feature_keys: u64,
    candidate_id_in_local_feature_keys: u64,
    unknown_condition_count: u64,
    topic_perturbations_per_local_frame: u64,
}

#[derive(Deserialize)]
struct PreRunManifest {
    schema: String,
    date: String,
    branch: String,
    protocol_sha256: String,
    source_sha256: String,
    data_sha256: String,
    tree_sha256: String,
    core_sha256: String,
    cargo_manifest_sha256: String,
    cargo_lock_sha256: String,
    train_groups: u8,
    total_groups: u8,
    tree_max_depth: u8,
    min_leaf: usize,
}

#[derive(Clone)]
struct Row {
    group: u8,
    label: usize,
    features: FeatureMap,
}

fn topic_invariance_rate(predictions: &BTreeMap<(u8, Condition), usize>) -> Option<f64> {
    let mut stable = 0u64;
    let mut total = 0u64;
    for group in TRAIN_GROUPS..GROUPS {
        for (base, variants) in [
            (
                Condition::ABase,
                [Condition::ATopicSwap, Condition::ATopicAmplify],
            ),
            (
                Condition::BBase,
                [Condition::BTopicSwap, Condition::BTopicAmplify],
            ),
        ] {
            if let Some(base_prediction) = predictions.get(&(group, base)) {
                for variant in variants {
                    if let Some(variant_prediction) = predictions.get(&(group, variant)) {
                        stable += u64::from(base_prediction == variant_prediction);
                        total += 1;
                    }
                }
            }
        }
    }
    (total > 0).then_some(stable as f64 / total as f64)
}

fn condition_name(condition: Condition) -> &'static str {
    match condition {
        Condition::ABase => "a_base",
        Condition::ATopicSwap => "a_topic_swap",
        Condition::ATopicAmplify => "a_topic_amplify",
        Condition::BBase => "b_base",
        Condition::BTopicSwap => "b_topic_swap",
        Condition::BTopicAmplify => "b_topic_amplify",
        Condition::CueRemoved => "cue_removed",
        Condition::Conflict => "conflict",
    }
}

fn score_family_view(stimuli: &[Stimulus], remove_candidates: bool) -> (f64, Option<f64>) {
    let test = stimuli
        .iter()
        .filter(|s| s.group >= TRAIN_GROUPS)
        .collect::<Vec<_>>();
    let correct = test
        .iter()
        .filter(|s| family_route(s, remove_candidates) == s.label)
        .count();
    let accuracy = correct as f64 / test.len().max(1) as f64;
    let mut route_map = BTreeMap::new();
    for s in &test {
        route_map.insert(
            (s.group, s.condition),
            family_route(s, remove_candidates).unwrap_or(UNKNOWN),
        );
    }
    let invariance = topic_invariance_rate(&route_map);
    (accuracy, invariance)
}

fn classification_receipt(
    stimuli: &[Stimulus],
    candidate: usize,
) -> (
    TreeReceipt,
    f64,
    Option<f64>,
    Option<f64>,
    f64,
    BTreeMap<String, f64>,
    Tree,
) {
    let train = stimuli
        .iter()
        .filter(|s| s.candidate == candidate && s.group < TRAIN_GROUPS)
        .map(|s| Row {
            group: s.group,
            label: s.label.unwrap_or(UNKNOWN),
            features: local_features(s),
        })
        .collect::<Vec<_>>();
    let test_stimuli = stimuli
        .iter()
        .filter(|s| s.candidate == candidate && s.group >= TRAIN_GROUPS)
        .collect::<Vec<_>>();
    let tree = train_tree(&train, MAX_DEPTH, UNKNOWN);
    let test_tuples = test_stimuli
        .iter()
        .map(|s| (s.group, s.label.unwrap_or(UNKNOWN), local_features(s)))
        .collect::<Vec<_>>();
    let receipt = tree_receipt(&tree, train.len(), &test_tuples);
    let mut receipt = receipt;
    receipt.class_axis = [
        "finance".to_string(),
        "geography".to_string(),
        "transport".to_string(),
        "unknown".to_string(),
    ];
    let group_macro_accuracy = receipt.heldout_group_macro_accuracy;
    let mut predictions = BTreeMap::new();
    let mut condition_counts = BTreeMap::<String, (u64, u64)>::new();
    for s in &test_stimuli {
        let pred = predict(&tree.root, &local_features(s)).min(UNKNOWN);
        predictions.insert((s.group, s.condition), pred);
        let entry = condition_counts
            .entry(condition_name(s.condition).to_string())
            .or_default();
        entry.1 += 1;
        entry.0 += u64::from(pred == s.label.unwrap_or(UNKNOWN));
    }
    let condition_accuracy = condition_counts
        .into_iter()
        .map(|(name, (yes, n))| (name, yes as f64 / n as f64))
        .collect();
    let topic = topic_invariance_rate(&predictions);
    let mut sense_good = 0u64;
    let mut sense_total = 0u64;
    for group in TRAIN_GROUPS..GROUPS {
        let a = predictions.get(&(group, Condition::ABase));
        let b = predictions.get(&(group, Condition::BBase));
        if let (Some(a), Some(b)) = (a, b) {
            sense_total += 1;
            sense_good += u64::from(
                *a == relation(candidate).frame_a && *b == relation(candidate).frame_b && a != b,
            );
        }
    }
    let ambiguity_cases = test_stimuli
        .iter()
        .filter(|s| matches!(s.condition, Condition::CueRemoved | Condition::Conflict))
        .collect::<Vec<_>>();
    let unknown_correct = ambiguity_cases
        .iter()
        .filter(|s| predictions.get(&(s.group, s.condition)) == Some(&UNKNOWN))
        .count();
    let ambiguity = unknown_correct as f64 / ambiguity_cases.len().max(1) as f64;
    (
        receipt,
        group_macro_accuracy,
        topic,
        (sense_total > 0).then_some(sense_good as f64 / sense_total as f64),
        ambiguity,
        condition_accuracy,
        tree,
    )
}

fn compatibility_receipt(stimuli: &[Stimulus], candidate: usize) -> (TreeReceipt, [f64; 3]) {
    let train = pair_rows(stimuli, candidate, true);
    let test = pair_rows(stimuli, candidate, false);
    let tree = train_tree(&train, MAX_DEPTH, PAIR_UNKNOWN);
    let tuples = test
        .iter()
        .map(|r| (r.group, r.label, r.features.clone()))
        .collect::<Vec<_>>();
    let mut receipt = tree_receipt(&tree, train.len(), &tuples);
    receipt.max_depth = MAX_DEPTH;
    receipt.class_axis = [
        "same".to_string(),
        "different".to_string(),
        "unknown".to_string(),
        "unused".to_string(),
    ];
    let mut group_class = BTreeMap::<(u8, usize), (u64, u64)>::new();
    for row in &test {
        let pred = predict(&tree.root, &row.features).min(3);
        let key = (row.group, row.label);
        let e = group_class.entry(key).or_default();
        e.1 += 1;
        e.0 += u64::from(pred == row.label);
    }
    let mut sums = [0.0; 3];
    let mut n = [0u64; 3];
    for ((_, class), (ok, total)) in group_class {
        if class < 3 {
            sums[class] += ok as f64 / total as f64;
            n[class] += 1;
        }
    }
    let rates = std::array::from_fn(|i| if n[i] > 0 { sums[i] / n[i] as f64 } else { 0.0 });
    (receipt, rates)
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn key_contains_exact_word(key: &str, word: &str) -> bool {
    key.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|piece| piece.eq_ignore_ascii_case(word))
}

fn run(repo: &Path, output: &Path) -> Result<()> {
    let protocol_path = repo.join("docs/LT9_LA2_P1N2_LOCAL_SENSE_OBSERVABILITY_20260923.md");
    let manifest_path = repo.join("experiments/lt9-la2-p1n2/pre-run-manifest-20260923.json");
    let protocol = fs::read(&protocol_path)?;
    let manifest_bytes = fs::read(&manifest_path)?;
    let protocol_sha256 = hash_bytes(&protocol);
    let manifest_sha256 = hash_bytes(&manifest_bytes);
    let manifest: PreRunManifest = serde_json::from_slice(&manifest_bytes)?;
    let source_sha256 = hash_bytes(include_bytes!("lt9_la2p1n2.rs"));
    let data_sha256 = hash_bytes(include_bytes!("lt9_la2p1n2_data.rs"));
    let tree_sha256 = hash_bytes(include_bytes!("lt9_la2p1n2_tree.rs"));
    let core_sha256 = hash_bytes(include_bytes!("lt9_la2p1m1_core.rs"));
    let cargo_manifest_sha256 =
        hash_bytes(&fs::read(repo.join("experiments/lt9-la2-p1n2/Cargo.toml"))?);
    let cargo_lock_sha256 =
        hash_bytes(&fs::read(repo.join("experiments/lt9-la2-p1n2/Cargo.lock"))?);
    ensure!(
        manifest.schema == "phoenix.lexical.lt9-la2-p1n2-prerun/v1",
        "unexpected pre-run schema"
    );
    ensure!(manifest.date == "2026-09-23", "pre-run date mismatch");
    ensure!(
        manifest.branch == "codex/phoenix-native-p1n2-local-sense-20260923",
        "branch mismatch"
    );
    ensure!(
        manifest.protocol_sha256 == protocol_sha256,
        "protocol hash mismatch"
    );
    ensure!(
        manifest.source_sha256 == source_sha256,
        "source hash mismatch"
    );
    ensure!(
        manifest.data_sha256 == data_sha256,
        "data source hash mismatch"
    );
    ensure!(
        manifest.tree_sha256 == tree_sha256,
        "tree source hash mismatch"
    );
    ensure!(
        manifest.cargo_manifest_sha256 == cargo_manifest_sha256,
        "cargo manifest hash mismatch"
    );
    ensure!(
        manifest.cargo_lock_sha256 == cargo_lock_sha256,
        "cargo lock hash mismatch"
    );
    ensure!(
        manifest.core_sha256 == core_sha256,
        "frozen core hash mismatch"
    );
    ensure!(
        manifest.train_groups == TRAIN_GROUPS && manifest.total_groups == GROUPS,
        "group split mismatch"
    );
    ensure!(
        manifest.tree_max_depth == MAX_DEPTH && manifest.min_leaf == MIN_LEAF,
        "tree contract mismatch"
    );
    let stimuli = generate_stimuli();
    let stimuli_json = serde_json::to_vec(&stimuli)?;
    let stimuli_sha256 = hash_bytes(&stimuli_json);
    let mut candidates = Vec::new();
    let mut candidate_tokens_in_keys = 0u64;
    let mut candidate_id_in_keys = 0u64;
    for r in &RELATIONS {
        let candidate_stimuli = stimuli
            .iter()
            .filter(|s| s.candidate == r.candidate_index)
            .cloned()
            .collect::<Vec<_>>();
        for s in &candidate_stimuli {
            let f = local_features(s);
            let (source, target) = candidate_words(s.candidate);
            candidate_tokens_in_keys +=
                f.0.keys()
                    .filter(|k| {
                        key_contains_exact_word(k, source) || key_contains_exact_word(k, target)
                    })
                    .count() as u64;
            candidate_id_in_keys += f.0.keys().filter(|k| k.contains(r.id)).count() as u64;
        }
        let (local_probe, _, local_topic, local_sense, ambiguity, condition_accuracy, _tree) =
            classification_receipt(&stimuli, r.candidate_index);
        let (compatibility_probe, compatibility_group_macro_by_class) =
            compatibility_receipt(&stimuli, r.candidate_index);
        let (current_family_accuracy, current_family_topic_invariance) =
            score_family_view(&candidate_stimuli, false);
        let (context_only_family_accuracy, context_only_topic_invariance) =
            score_family_view(&candidate_stimuli, true);
        candidates.push(CandidateReceipt {
            candidate_id: r.id.to_string(),
            train_groups: TRAIN_GROUPS,
            heldout_groups: GROUPS - TRAIN_GROUPS,
            heldout_stimuli: candidate_stimuli
                .iter()
                .filter(|s| s.group >= TRAIN_GROUPS)
                .count(),
            current_family_accuracy,
            context_only_family_accuracy,
            current_family_topic_invariance,
            context_only_topic_invariance,
            local_probe,
            local_topic_invariance: local_topic,
            local_sense_sensitivity: local_sense,
            local_ambiguity_calibration: ambiguity,
            local_condition_accuracy: condition_accuracy,
            compatibility_probe,
            compatibility_group_macro_by_class,
        });
    }
    let binary_sha256 = hash_current_exe()?;
    let receipt = Receipt {
        schema: RESULT_SCHEMA,
        date: "2026-09-23",
        branch: manifest.branch,
        protocol_sha256,
        prerun_manifest_sha256: manifest_sha256,
        source_sha256,
        data_sha256,
        tree_sha256,
        core_sha256,
        cargo_manifest_sha256,
        cargo_lock_sha256,
        binary_sha256,
        stimuli_sha256,
        scope: Scope {
            natural_corpus_read: false,
            validity_or_relevance_labels_read: false,
            p1m2r_or_p1n1_outcome_receipts_read: false,
            webis_touche_read: false,
            natural_authority_updated: false,
            retrieval_or_ranking_run: false,
            router_selected_or_promoted: false,
        },
        total_stimuli: stimuli.len(),
        total_base_groups: RELATIONS.len() * usize::from(GROUPS),
        feature_firewall: FeatureFirewall {
            candidate_tokens_in_local_feature_keys: candidate_tokens_in_keys,
            candidate_id_in_local_feature_keys: candidate_id_in_keys,
            unknown_condition_count: stimuli.iter().filter(|s| s.label.is_none()).count() as u64,
            topic_perturbations_per_local_frame: 2,
        },
        candidates,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(output).with_context(|| format!("create {}", output.display()))?;
    serde_json::to_writer_pretty(&mut file, &receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn hash_current_exe() -> Result<String> {
    let bytes = fs::read(env::current_exe()?)?;
    Ok(hash_bytes(&bytes))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let Some(mode) = args.next() else {
        bail!("usage: lt9_la2p1n2 --run <repo-root> <output.json>")
    };
    ensure!(mode == "--run", "only --run is supported");
    let Some(repo) = args.next() else {
        bail!("missing repository root")
    };
    let Some(output) = args.next() else {
        bail!("missing output path")
    };
    ensure!(args.next().is_none(), "unexpected extra argument");
    run(Path::new(&repo), Path::new(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pair_tokens_are_not_local_features() {
        let s = generate_stimuli()
            .into_iter()
            .find(|s| s.candidate == 11 && s.condition == Condition::ABase)
            .unwrap();
        let f = local_features(&s);
        assert!(f.0.keys().all(|key| !key_contains_exact_word(key, "bank")
            && !key_contains_exact_word(key, "water")
            && !key.contains("bank_to_water")));
    }

    #[test]
    fn topic_variants_preserve_local_frame_features() {
        let all = generate_stimuli();
        let find = |condition| {
            all.iter()
                .find(|s| s.candidate == 11 && s.group == 8 && s.condition == condition)
                .unwrap()
        };
        let a = local_features(find(Condition::ABase));
        let b = local_features(find(Condition::ATopicSwap));
        let c = local_features(find(Condition::ATopicAmplify));
        let local = |m: &FeatureMap| {
            m.0.iter()
                .filter(|(k, _)| k.ends_with("@nearby") || k.starts_with("immediate:"))
                .map(|(k, v)| (k.clone(), *v))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(local(&a), local(&b));
        assert_eq!(local(&a), local(&c));
    }

    #[test]
    fn local_ngrams_do_not_cross_title_body_boundary() {
        let stimulus = generate_stimuli()
            .into_iter()
            .find(|s| s.candidate == 2 && s.group == 2 && s.condition == Condition::ABase)
            .unwrap();
        assert_eq!(stimulus.title_end, 9);
        let crossing = format!(
            "ngram2:{}_{}@field=title",
            stimulus.tokens[8], stimulus.tokens[9]
        );
        assert!(!local_features(&stimulus).0.contains_key(&crossing));
    }

    #[test]
    fn sense_swap_changes_label_without_changing_candidate_words() {
        let all = generate_stimuli();
        let a = all
            .iter()
            .find(|s| s.candidate == 2 && s.group == 4 && s.condition == Condition::ABase)
            .unwrap();
        let b = all
            .iter()
            .find(|s| s.candidate == 2 && s.group == 4 && s.condition == Condition::BBase)
            .unwrap();
        assert_eq!(
            (&a.tokens[SOURCE_POS], &a.tokens[TARGET_POS]),
            (&b.tokens[SOURCE_POS], &b.tokens[TARGET_POS])
        );
        assert_ne!(a.label, b.label);
    }

    #[test]
    fn ambiguity_is_explicit_and_pair_labels_fail_closed() {
        let all = generate_stimuli();
        let removed = all
            .iter()
            .find(|s| s.candidate == 9 && s.group == 9 && s.condition == Condition::CueRemoved)
            .unwrap();
        let conflict = all
            .iter()
            .find(|s| s.candidate == 9 && s.group == 9 && s.condition == Condition::Conflict)
            .unwrap();
        assert_eq!(removed.label, None);
        assert_eq!(conflict.label, None);
        assert_eq!(data::label_for_pair(Some(0), Some(0)), SAME);
        assert_eq!(data::label_for_pair(Some(0), Some(2)), DIFFERENT);
        assert_eq!(data::label_for_pair(Some(0), None), PAIR_UNKNOWN);
    }
}
