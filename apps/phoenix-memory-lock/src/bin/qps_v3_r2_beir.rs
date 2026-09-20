//! Diagnostic-only BEIR R2 anchored residual experiment.
//!
//! Candidate generation and Phoenix evidence remain frozen. BM25F feature 0 is
//! the fixed score anchor; learned weights are bounded signed residuals. BEIR
//! qrels are positive-only, so unjudged candidates remain weak negatives and
//! this artifact is non-promotable until it is replaced by an authoritative
//! judgment contract.

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blake3::Hasher;
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchScratch,
    RANK_EVIDENCE_V3_FEATURE_NAMES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;
const MAXIMUM_QUERY_GROUPS: usize = 128;
const MAX_NEGATIVES_PER_SOURCE: usize = 6;
const FEATURE_COUNT: usize = 30;
const RESIDUAL_LEXICAL_FEATURES: [usize; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21];
const RESIDUAL_FULL_FEATURES: [usize; 23] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
];
const RESIDUAL_BOUNDS: [f32; 3] = [0.05, 0.10, 0.20];

#[derive(Debug, Deserialize)]
struct CorpusRow {
    _id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct QueryRow {
    _id: String,
    text: String,
}

#[derive(Clone, Debug)]
struct Document {
    id: String,
    title: String,
    text: String,
}

#[derive(Clone, Debug)]
struct Query {
    id: String,
    text: String,
}

type Qrels = HashMap<String, HashMap<String, u32>>;

#[derive(Clone, Copy)]
struct Candidate {
    external_id: u64,
    baseline_rank: usize,
    values: [f32; FEATURE_COUNT],
    relevant: bool,
}

struct QueryCase {
    candidates: Vec<Candidate>,
    relevant_documents: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct TrainConfig {
    epochs: usize,
    learning_rate: f32,
    l2_penalty: f32,
    residual_bound: f32,
}

#[derive(Clone)]
struct Model {
    weights: [f32; FEATURE_COUNT],
    features: &'static [usize],
    config: TrainConfig,
}

#[derive(Clone, Copy)]
struct Pair {
    positive: [f32; FEATURE_COUNT],
    negative: [f32; FEATURE_COUNT],
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
struct Metrics {
    queries: usize,
    ndcg_at_10: f64,
    recall_at_100: f64,
    mrr: f64,
    map: f64,
}

#[derive(Debug, Default, Serialize)]
struct Migrations {
    relevant_in_candidate_pool: u64,
    old_over_100_new_top100: u64,
    old_101_200_new_top100: u64,
    old_201_500_new_top100: u64,
    old_501_1000_new_top100: u64,
    old_1001_5000_new_top100: u64,
    old_over_5000_new_top100: u64,
    old_top10_new_below10: u64,
    old_top100_new_below100: u64,
    old_over100_new_top10: u64,
}

#[derive(Debug, Serialize)]
struct ArmReceipt {
    arm: String,
    metrics: Metrics,
    migrations: Migrations,
    model_identity: Option<String>,
    weights: Option<Vec<f32>>,
}

#[derive(Debug, Default, Serialize)]
struct NegativeReceipt {
    pairs: usize,
    phoenix_only: usize,
    bm25f_only: usize,
    shared: usize,
}

#[derive(Debug, Default, Serialize)]
struct SplitReceipt {
    queries: usize,
    candidate_documents: usize,
    relevant_documents: usize,
    relevant_documents_in_pool: usize,
    queries_with_relevant_candidate: usize,
    negatives: NegativeReceipt,
}

#[derive(Debug, Serialize)]
struct AuthorityReceipt {
    feature: usize,
    name: &'static str,
    selected_weight: f32,
    dev_ndcg_at_10: f64,
    dev_delta_ndcg_vs_anchor: f64,
    dev_top10_loss: u64,
    accepted_by_guard: bool,
}

#[derive(Debug, Serialize)]
struct Receipt {
    contract: &'static str,
    dataset: String,
    candidate_generator: &'static str,
    negative_policy: &'static str,
    corpus_sha256: String,
    queries_sha256: String,
    train_qrels_sha256: String,
    dev_qrels_sha256: String,
    test_qrels_sha256: String,
    documents: usize,
    train: SplitReceipt,
    dev: SplitReceipt,
    test: SplitReceipt,
    selected_lexical_config: TrainConfig,
    selected_full_config: TrainConfig,
    authority_census: Vec<AuthorityReceipt>,
    arms: Vec<ArmReceipt>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: qps_v3_r2_beir <dataset-root> [output-json]")?,
    );
    let output = args.next().map(PathBuf::from);
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let corpus_path = root.join("corpus.jsonl");
    let queries_path = root.join("queries.jsonl");
    let train_qrels_path = root.join("qrels").join("train.tsv");
    let dev_qrels_path = root.join("qrels").join("dev.tsv");
    let test_qrels_path = root.join("qrels").join("test.tsv");
    let documents = read_corpus(&corpus_path)?;
    let queries = read_queries(&queries_path)?;
    let query_by_id = queries
        .into_iter()
        .map(|query| (query.id.clone(), query))
        .collect::<HashMap<_, _>>();
    let (train_qrels, dev_qrels, train_qrels_sha256, dev_qrels_sha256) =
        load_train_dev_qrels(&train_qrels_path, &dev_qrels_path)?;
    let test_qrels = read_qrels(&test_qrels_path)?;
    let index = build_qps(&documents)?;
    let document_ids = documents
        .iter()
        .map(|document| document.id.clone())
        .collect::<Vec<_>>();
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let train_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &train_qrels,
        &mut scratch,
    )?;
    let dev_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &dev_qrels,
        &mut scratch,
    )?;
    let test_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &test_qrels,
        &mut scratch,
    )?;
    let train_summary = summarize_cases(&train_cases);
    let dev_summary = summarize_cases(&dev_cases);
    let test_summary = summarize_cases(&test_cases);
    let lexical_model = select_model(&train_cases, &dev_cases, &RESIDUAL_LEXICAL_FEATURES)?;
    let full_model = select_model(&train_cases, &dev_cases, &RESIDUAL_FULL_FEATURES)?;
    let authority_census = authority_census(&dev_cases);
    let arms = vec![
        evaluate_arm("phoenix_literal", &test_cases, None, ArmKind::Baseline),
        evaluate_arm("phoenix_bm25f_anchor", &test_cases, None, ArmKind::Bm25f),
        evaluate_arm(
            "r2_anchored_lexical_residual",
            &test_cases,
            Some(&lexical_model),
            ArmKind::Model,
        ),
        evaluate_arm(
            "r2_anchored_full_nonpositional_residual",
            &test_cases,
            Some(&full_model),
            ArmKind::Model,
        ),
    ];
    let receipt = Receipt {
        contract: "phoenix.qps.beir-r2-anchored-residual/v1",
        dataset,
        candidate_generator: "phoenix_literal_search_evidence_top100_candidate_pool",
        negative_policy: "query_grouped_phoenix_and_bm25f_hard_negatives_from_unjudged_candidates_diagnostic_only",
        corpus_sha256: sha256_file(&corpus_path)?,
        queries_sha256: sha256_file(&queries_path)?,
        train_qrels_sha256,
        dev_qrels_sha256,
        test_qrels_sha256: sha256_file(&test_qrels_path)?,
        documents: documents.len(),
        train: train_summary,
        dev: dev_summary,
        test: test_summary,
        selected_lexical_config: lexical_model.config,
        selected_full_config: full_model.config,
        authority_census,
        arms,
    };
    let json = serde_json::to_string_pretty(&receipt)?;
    if let Some(path) = output {
        fs::write(&path, json.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    }
    println!("{}", json);
    Ok(())
}

#[derive(Clone, Copy)]
enum ArmKind {
    Baseline,
    Bm25f,
    Model,
}

fn collect_cases(
    index: &QpsIndex,
    document_ids: &[String],
    queries: &HashMap<String, Query>,
    qrels: &Qrels,
    scratch: &mut SearchScratch,
) -> Result<Vec<QueryCase>> {
    let mut cases = Vec::with_capacity(qrels.len());
    let mut hits = Vec::with_capacity(256);
    for (query_id, relevance) in qrels {
        let Some(query) = queries.get(query_id) else {
            continue;
        };
        hits.clear();
        index.search_evidence_into(&query.text, TOP_K, scratch, &mut hits)?;
        let relevant_documents = relevance.values().filter(|score| **score > 0).count();
        let candidates = hits
            .iter()
            .enumerate()
            .map(|(rank, hit)| Candidate {
                external_id: hit.external_id,
                baseline_rank: rank + 1,
                values: hit.rank_evidence_v3.values,
                relevant: relevance
                    .get(&document_ids[hit.external_id as usize])
                    .is_some_and(|score| *score > 0),
            })
            .collect::<Vec<_>>();
        cases.push(QueryCase {
            candidates,
            relevant_documents,
        });
    }
    Ok(cases)
}

fn summarize_cases(cases: &[QueryCase]) -> SplitReceipt {
    let mut receipt = SplitReceipt::default();
    for case in cases {
        receipt.queries += 1;
        receipt.candidate_documents += case.candidates.len();
        receipt.relevant_documents += case.relevant_documents;
        let relevant_in_pool = case
            .candidates
            .iter()
            .filter(|candidate| candidate.relevant)
            .count();
        receipt.relevant_documents_in_pool += relevant_in_pool;
        receipt.queries_with_relevant_candidate += usize::from(relevant_in_pool > 0);
        let (_, negatives) = build_pairs(std::slice::from_ref(case), &RESIDUAL_LEXICAL_FEATURES);
        receipt.negatives.pairs += negatives.pairs;
        receipt.negatives.phoenix_only += negatives.phoenix_only;
        receipt.negatives.bm25f_only += negatives.bm25f_only;
        receipt.negatives.shared += negatives.shared;
    }
    receipt
}

fn select_model(
    train_cases: &[QueryCase],
    dev_cases: &[QueryCase],
    features: &'static [usize],
) -> Result<Model> {
    let configs = [
        TrainConfig {
            epochs: 64,
            learning_rate: 0.01,
            l2_penalty: 0.001,
            residual_bound: RESIDUAL_BOUNDS[0],
        },
        TrainConfig {
            epochs: 128,
            learning_rate: 0.005,
            l2_penalty: 0.001,
            residual_bound: RESIDUAL_BOUNDS[1],
        },
        TrainConfig {
            epochs: 256,
            learning_rate: 0.0025,
            l2_penalty: 0.002,
            residual_bound: RESIDUAL_BOUNDS[2],
        },
    ];
    let anchor = evaluate_ranked(dev_cases, None, ArmKind::Bm25f);
    let anchor_migrations = migration_receipt(dev_cases, None, ArmKind::Bm25f);
    let loss_guard = (anchor_migrations.relevant_in_candidate_pool / 20).max(8);
    let mut best = Model::zero(features, configs[0]);
    let mut best_metrics = anchor;
    let mut best_guard = true;
    for config in configs {
        let model = train_model(train_cases, features, config)?;
        let metrics = evaluate_model(dev_cases, &model);
        let migrations = migration_receipt(dev_cases, Some(&model), ArmKind::Model);
        let guard = migrations.old_top10_new_below10 <= loss_guard;
        if guard && (!best_guard || metrics.ndcg_at_10 > best_metrics.ndcg_at_10)
            || guard == best_guard && metrics.ndcg_at_10 > best_metrics.ndcg_at_10
        {
            best = model;
            best_metrics = metrics;
            best_guard = guard;
        }
    }
    Ok(best)
}

fn train_model(
    cases: &[QueryCase],
    features: &'static [usize],
    config: TrainConfig,
) -> Result<Model> {
    let (pairs, _) = build_pairs(cases, features);
    if pairs.is_empty() {
        anyhow::bail!("training produced no positive/hard-negative pairs")
    }
    let mut weights = [0.0_f32; FEATURE_COUNT];
    for _ in 0..config.epochs {
        for pair in &pairs {
            let base_difference = pair.positive[0] - pair.negative[0];
            let mut residual_difference = 0.0_f32;
            for &feature in features {
                residual_difference +=
                    weights[feature] * (pair.positive[feature] - pair.negative[feature]);
            }
            let probability = 1.0
                / (1.0
                    + (base_difference + residual_difference)
                        .clamp(-20.0, 20.0)
                        .exp());
            for &feature in features {
                let difference = pair.positive[feature] - pair.negative[feature];
                let gradient = probability * difference - config.l2_penalty * weights[feature];
                weights[feature] = (weights[feature] + config.learning_rate * gradient)
                    .clamp(-config.residual_bound, config.residual_bound);
            }
        }
    }
    Ok(Model {
        weights,
        features,
        config,
    })
}

impl Model {
    fn zero(features: &'static [usize], config: TrainConfig) -> Self {
        Self {
            weights: [0.0; FEATURE_COUNT],
            features,
            config,
        }
    }
}

fn build_pairs(cases: &[QueryCase], _features: &'static [usize]) -> (Vec<Pair>, NegativeReceipt) {
    let mut pairs = Vec::new();
    let mut receipt = NegativeReceipt::default();
    for case in cases {
        let positives = case
            .candidates
            .iter()
            .filter(|candidate| candidate.relevant)
            .collect::<Vec<_>>();
        let mut negatives = case
            .candidates
            .iter()
            .filter(|candidate| !candidate.relevant)
            .collect::<Vec<_>>();
        let mut selected = HashMap::<u64, (Candidate, u8)>::new();
        for candidate in negatives.iter().take(MAX_NEGATIVES_PER_SOURCE) {
            selected.insert(candidate.external_id, (**candidate, 1));
        }
        negatives.sort_unstable_by(|left, right| {
            right.values[0]
                .total_cmp(&left.values[0])
                .then_with(|| left.external_id.cmp(&right.external_id))
        });
        for candidate in negatives.iter().take(MAX_NEGATIVES_PER_SOURCE) {
            selected
                .entry(candidate.external_id)
                .and_modify(|(_, mask)| *mask |= 2)
                .or_insert((**candidate, 2));
        }
        let chosen = selected.values().copied().collect::<Vec<_>>();
        for (_, mask) in &chosen {
            receipt.pairs += positives.len();
            match mask {
                1 => receipt.phoenix_only += positives.len(),
                2 => receipt.bm25f_only += positives.len(),
                _ => receipt.shared += positives.len(),
            }
        }
        for positive in positives {
            for (negative, _source_mask) in &chosen {
                pairs.push(Pair {
                    positive: positive.values,
                    negative: negative.values,
                });
            }
        }
    }
    (pairs, receipt)
}

fn authority_census(cases: &[QueryCase]) -> Vec<AuthorityReceipt> {
    let anchor = evaluate_ranked(cases, None, ArmKind::Bm25f);
    let anchor_migrations = migration_receipt(cases, None, ArmKind::Bm25f);
    let guard = (anchor_migrations.relevant_in_candidate_pool / 20).max(8);
    let grid = [-0.10_f32, -0.05, -0.025, 0.0, 0.025, 0.05, 0.10];
    RESIDUAL_FULL_FEATURES
        .iter()
        .map(|&feature| {
            let mut selected_weight = 0.0;
            let mut selected_metrics = anchor;
            for weight in grid {
                let mut model = Model::zero(
                    &RESIDUAL_FULL_FEATURES,
                    TrainConfig {
                        epochs: 0,
                        learning_rate: 0.0,
                        l2_penalty: 0.0,
                        residual_bound: 0.10,
                    },
                );
                model.weights[feature] = weight;
                let metrics = evaluate_model(cases, &model);
                let migrations = migration_receipt(cases, Some(&model), ArmKind::Model);
                let allowed = migrations.old_top10_new_below10 <= guard;
                if allowed && metrics.ndcg_at_10 > selected_metrics.ndcg_at_10 {
                    selected_weight = weight;
                    selected_metrics = metrics;
                }
            }
            let mut selected_model = Model::zero(
                &RESIDUAL_FULL_FEATURES,
                TrainConfig {
                    epochs: 0,
                    learning_rate: 0.0,
                    l2_penalty: 0.0,
                    residual_bound: 0.10,
                },
            );
            selected_model.weights[feature] = selected_weight;
            let loss = migration_receipt(cases, Some(&selected_model), ArmKind::Model)
                .old_top10_new_below10;
            let accepted = selected_weight != 0.0;
            AuthorityReceipt {
                feature,
                name: RANK_EVIDENCE_V3_FEATURE_NAMES[feature],
                selected_weight,
                dev_ndcg_at_10: selected_metrics.ndcg_at_10,
                dev_delta_ndcg_vs_anchor: selected_metrics.ndcg_at_10 - anchor.ndcg_at_10,
                dev_top10_loss: loss,
                accepted_by_guard: accepted,
            }
        })
        .collect()
}

fn evaluate_arm(
    name: &str,
    cases: &[QueryCase],
    model: Option<&Model>,
    kind: ArmKind,
) -> ArmReceipt {
    let metrics = evaluate_ranked(cases, model, kind);
    let migrations = migration_receipt(cases, model, kind);
    let (model_identity, weights) = model.map_or((None, None), |model| {
        (Some(model_identity(model)), Some(model.weights.to_vec()))
    });
    ArmReceipt {
        arm: name.to_owned(),
        metrics,
        migrations,
        model_identity,
        weights,
    }
}

fn evaluate_model(cases: &[QueryCase], model: &Model) -> Metrics {
    evaluate_ranked(cases, Some(model), ArmKind::Model)
}

fn evaluate_ranked(cases: &[QueryCase], model: Option<&Model>, kind: ArmKind) -> Metrics {
    let mut metrics = Metrics::default();
    for case in cases {
        let ranking = ranked_candidates(case, model, kind);
        let relevant = case.relevant_documents;
        if relevant == 0 {
            continue;
        }
        metrics.queries += 1;
        let mut dcg = 0.0;
        let mut found = 0;
        let mut reciprocal = 0.0;
        let mut precision_sum = 0.0;
        for (rank, candidate) in ranking.iter().take(TOP_K).enumerate() {
            if !candidate.relevant {
                continue;
            }
            if rank < 10 {
                dcg += 1.0 / (rank as f64 + 2.0).log2();
            }
            found += 1;
            if reciprocal == 0.0 {
                reciprocal = 1.0 / (rank + 1) as f64;
            }
            precision_sum += found as f64 / (rank + 1) as f64;
        }
        let ideal_count = relevant.min(10);
        let idcg = (0..ideal_count)
            .map(|rank| 1.0 / (rank as f64 + 2.0).log2())
            .sum::<f64>();
        metrics.ndcg_at_10 += if idcg > 0.0 { dcg / idcg } else { 0.0 };
        metrics.recall_at_100 += found.min(relevant) as f64 / relevant as f64;
        metrics.mrr += reciprocal;
        metrics.map += precision_sum / relevant as f64;
    }
    let divisor = metrics.queries.max(1) as f64;
    metrics.ndcg_at_10 /= divisor;
    metrics.recall_at_100 /= divisor;
    metrics.mrr /= divisor;
    metrics.map /= divisor;
    metrics
}

fn ranked_candidates<'a>(
    case: &'a QueryCase,
    model: Option<&Model>,
    kind: ArmKind,
) -> Vec<&'a Candidate> {
    let mut ranking = case.candidates.iter().collect::<Vec<_>>();
    ranking.sort_unstable_by(|left, right| {
        score_candidate(left, model, kind)
            .total_cmp(&score_candidate(right, model, kind))
            .reverse()
            .then_with(|| left.external_id.cmp(&right.external_id))
    });
    ranking
}

fn score_candidate(candidate: &Candidate, model: Option<&Model>, kind: ArmKind) -> f32 {
    match kind {
        ArmKind::Baseline => -(candidate.baseline_rank as f32),
        ArmKind::Bm25f => candidate.values[0],
        ArmKind::Model => {
            let model = model.expect("model arm requires a model");
            model
                .features
                .iter()
                .fold(candidate.values[0], |score, &feature| {
                    score + model.weights[feature] * candidate.values[feature]
                })
        }
    }
}

fn migration_receipt(cases: &[QueryCase], model: Option<&Model>, kind: ArmKind) -> Migrations {
    let mut migrations = Migrations::default();
    for case in cases {
        let new = ranked_candidates(case, model, kind)
            .iter()
            .enumerate()
            .map(|(rank, candidate)| (candidate.external_id, rank + 1))
            .collect::<HashMap<_, _>>();
        for candidate in &case.candidates {
            if !candidate.relevant {
                continue;
            }
            let Some(&new_rank) = new.get(&candidate.external_id) else {
                continue;
            };
            migrations.relevant_in_candidate_pool += 1;
            let old_rank = candidate.baseline_rank;
            if old_rank > TOP_K && new_rank <= TOP_K {
                migrations.old_over_100_new_top100 += 1;
                match old_rank {
                    101..=200 => migrations.old_101_200_new_top100 += 1,
                    201..=500 => migrations.old_201_500_new_top100 += 1,
                    501..=1000 => migrations.old_501_1000_new_top100 += 1,
                    1001..=5000 => migrations.old_1001_5000_new_top100 += 1,
                    _ => migrations.old_over_5000_new_top100 += 1,
                }
            }
            if old_rank <= 10 && new_rank > 10 {
                migrations.old_top10_new_below10 += 1;
            }
            if old_rank <= TOP_K && new_rank > TOP_K {
                migrations.old_top100_new_below100 += 1;
            }
            if old_rank > TOP_K && new_rank <= 10 {
                migrations.old_over100_new_top10 += 1;
            }
        }
    }
    migrations
}

fn model_identity(model: &Model) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"phoenix-qps-beir-r2-anchored-residual\0");
    hasher.update(&model.config.residual_bound.to_bits().to_le_bytes());
    for &feature in model.features {
        hasher.update(&(feature as u32).to_le_bytes());
    }
    for weight in model.weights {
        hasher.update(&weight.to_bits().to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn build_qps(documents: &[Document]) -> Result<QpsIndex> {
    let fields = [
        FieldConfig::new("title", 2.5, 0.35, 0.0),
        FieldConfig::new("body", 1.0, 0.75, 0.0),
    ];
    let config = QpsConfig {
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        maximum_candidate_pool: 256,
        proximity_weight: 0.0,
        order_weight: 0.0,
        phrase_weight: 0.0,
        segment_weight: 0.0,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config)?;
    for (index, document) in documents.iter().enumerate() {
        builder.insert(DocumentInput {
            external_id: index as u64,
            fields: &[document.title.as_str(), document.text.as_str()],
        })?;
    }
    builder.build().map_err(Into::into)
}

fn read_corpus(path: &Path) -> Result<Vec<Document>> {
    BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let row: CorpusRow =
                serde_json::from_str(&value.with_context(|| format!("read line {}", line + 1))?)
                    .with_context(|| format!("decode line {}", line + 1))?;
            Ok(Document {
                id: row._id,
                title: row.title,
                text: row.text,
            })
        })
        .collect()
}

fn read_queries(path: &Path) -> Result<Vec<Query>> {
    BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let row: QueryRow =
                serde_json::from_str(&value.with_context(|| format!("read line {}", line + 1))?)
                    .with_context(|| format!("decode line {}", line + 1))?;
            Ok(Query {
                id: row._id,
                text: row.text,
            })
        })
        .collect()
}

fn read_qrels(path: &Path) -> Result<Qrels> {
    let mut result = Qrels::new();
    for (line, value) in BufReader::new(File::open(path)?).lines().enumerate() {
        let value = value.with_context(|| format!("read qrels line {}", line + 1))?;
        if line == 0 && value.starts_with("query-id") {
            continue;
        }
        let mut columns = value.split('\t');
        let query = columns.next().context("qrels query missing")?;
        let document = columns.next().context("qrels document missing")?;
        let score = columns
            .next()
            .context("qrels score missing")?
            .parse::<u32>()?;
        result
            .entry(query.to_owned())
            .or_default()
            .insert(document.to_owned(), score);
    }
    Ok(result)
}

fn load_train_dev_qrels(
    train_path: &Path,
    dev_path: &Path,
) -> Result<(Qrels, Qrels, String, String)> {
    let all_train = read_qrels(train_path)?;
    if dev_path.exists() {
        return Ok((
            all_train,
            read_qrels(dev_path)?,
            sha256_file(train_path)?,
            sha256_file(dev_path)?,
        ));
    }
    let mut train = Qrels::new();
    let mut dev = Qrels::new();
    for (query, judgments) in all_train {
        let bucket = blake3::hash(query.as_bytes()).as_bytes()[0] % 5;
        if bucket == 0 {
            dev.insert(query, judgments);
        } else {
            train.insert(query, judgments);
        }
    }
    let train_identity = sha256_qrels(&train);
    let dev_identity = sha256_qrels(&dev);
    Ok((train, dev, train_identity, dev_identity))
}

fn sha256_qrels(qrels: &Qrels) -> String {
    let mut rows = qrels
        .iter()
        .flat_map(|(query, judgments)| {
            judgments
                .iter()
                .map(move |(document, score)| (query.as_str(), document.as_str(), *score))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable();
    let mut hasher = Sha256::new();
    for (query, document, score) in rows {
        hasher.update(query.as_bytes());
        hasher.update(b"\t");
        hasher.update(document.as_bytes());
        hasher.update(b"\t");
        hasher.update(score.to_string().as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).with_context(|| format!("read {}", path.display()))?);
    Ok(format!("{:x}", hasher.finalize()))
}
