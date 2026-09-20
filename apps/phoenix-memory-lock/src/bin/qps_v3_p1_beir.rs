//! V3-P1 positional residual authority microscope.
//!
//! This evaluator is deliberately separate from serving. It requires a
//! post-reliability authority receipt and a scientific sufficiency receipt,
//! rebuilds the frozen literal QPS candidate universe, chooses each residual
//! arm's alpha from grouped train authority only, checks grouped dev authority,
//! and evaluates the frozen arm on BEIR dev/test.

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchScratch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;
const AUTHORITY_LIMIT: usize = 4096;
const MAXIMUM_QUERY_GROUPS: usize = 128;
const FEATURE_COUNT: usize = 30;
const ALPHA_GRID: [f32; 6] = [0.0, 0.025, 0.05, 0.10, 0.15, 0.20];
const RESIDUAL_BOUND: f32 = 0.20;

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

#[derive(Clone)]
struct Candidate {
    external_id: u64,
    document_id: String,
    baseline_rank: usize,
    values: [f32; FEATURE_COUNT],
    relevant: bool,
}

struct QueryCase {
    query_id: String,
    candidates: Vec<Candidate>,
    relevant_documents: usize,
}

#[derive(Debug, Deserialize)]
struct HumanPair {
    split: String,
    query_id: String,
    positive_document_id: String,
    negative_document_id: String,
}

#[derive(Debug, Deserialize)]
struct AuthorityReceipt {
    status: String,
    queries_with_two_and_zero: usize,
    authoritative_pairs: usize,
}

#[derive(Debug, Deserialize)]
struct GateReceipt {
    minimum_authoritative_queries: usize,
    eligible_datasets: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
enum Arm {
    Anchor,
    P1CompleteSpan,
    P2OrderedSpan,
    P3OrderedFraction,
    P4ExactPhrase,
    P5ExactField,
}

impl Arm {
    const ALL: [Self; 6] = [
        Self::Anchor,
        Self::P1CompleteSpan,
        Self::P2OrderedSpan,
        Self::P3OrderedFraction,
        Self::P4ExactPhrase,
        Self::P5ExactField,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Anchor => "r0_bm25f_anchor",
            Self::P1CompleteSpan => "p1_complete_span_quality",
            Self::P2OrderedSpan => "p2_ordered_span_quality",
            Self::P3OrderedFraction => "p3_ordered_fraction",
            Self::P4ExactPhrase => "p4_exact_phrase",
            Self::P5ExactField => "p5_exact_field",
        }
    }

    const fn feature_definition(self) -> &'static str {
        match self {
            Self::Anchor => "BM25F feature 0",
            Self::P1CompleteSpan => "values[10] complete_span_quality",
            Self::P2OrderedSpan => "values[11] ordered_span_quality",
            Self::P3OrderedFraction => "values[12] ordered_fraction",
            Self::P4ExactPhrase => "values[13] exact_phrase",
            Self::P5ExactField => "values[15] exact_field",
        }
    }
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
    relevant_in_candidate_pool: usize,
    old_top10_new_below10: usize,
    old_top100_new_below100: usize,
    old_over100_new_top10: usize,
}

#[derive(Debug, Default, Serialize)]
struct PairConcordance {
    groups: usize,
    pairs: usize,
    correct_pairs: usize,
    ties: usize,
    mean_query_concordance: f64,
    unavailable_pairs: usize,
}

#[derive(Debug, Serialize)]
struct ArmReceipt {
    arm: Arm,
    label: &'static str,
    feature_definition: &'static str,
    selected_alpha: f32,
    train_authority: PairConcordance,
    dev_authority: PairConcordance,
    dev_metrics: Metrics,
    test_metrics: Metrics,
    test_migrations: Migrations,
}

#[derive(Debug, Serialize)]
struct DoseReceipt {
    arm: Arm,
    alpha: f32,
    train_authority: PairConcordance,
    dev_authority: PairConcordance,
    test_metrics: Metrics,
    test_migrations: Migrations,
}

#[derive(Debug, Serialize)]
struct Receipt {
    contract: &'static str,
    dataset: String,
    status: &'static str,
    candidate_generator: &'static str,
    authority_pairs_sha256: String,
    authority_receipt_sha256: String,
    gate_receipt_sha256: String,
    corpus_sha256: String,
    queries_sha256: String,
    train_qrels_sha256: String,
    dev_qrels_sha256: String,
    test_qrels_sha256: String,
    minimum_authoritative_queries: usize,
    authoritative_queries: usize,
    authoritative_pairs: usize,
    alpha_grid: Vec<f32>,
    residual_bound: f32,
    candidate_pool: usize,
    authority_candidate_pool: usize,
    dose_response: Vec<DoseReceipt>,
    arms: Vec<ArmReceipt>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(args.next().context(
        "usage: qps_v3_p1_beir <dataset-root> <pairs-json> <authority-json> <gate-json> <output-json>",
    )?);
    let pairs_path = PathBuf::from(args.next().context("missing pairs json")?);
    let authority_path = PathBuf::from(args.next().context("missing authority json")?);
    let gate_path = PathBuf::from(args.next().context("missing gate json")?);
    let output_path = PathBuf::from(args.next().context("missing output json")?);
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let authority_bytes = fs::read(&authority_path)?;
    let authority: AuthorityReceipt = serde_json::from_slice(&authority_bytes)?;
    let gate_bytes = fs::read(&gate_path)?;
    let gate: GateReceipt = serde_json::from_slice(&gate_bytes)?;
    if authority.status != "authority_available"
        || authority.queries_with_two_and_zero < gate.minimum_authoritative_queries
    {
        bail!("authority receipt does not pass technical and scientific gates");
    }
    if !gate.eligible_datasets.iter().any(|name| name == &dataset) {
        bail!("dataset is not present in scientific gate eligible_datasets");
    }

    let pair_bytes = fs::read(&pairs_path)?;
    let pairs: Vec<HumanPair> = serde_json::from_slice(&pair_bytes)?;
    if pairs.is_empty() || pairs.len() != authority.authoritative_pairs {
        bail!("authority pair count does not match authority receipt");
    }

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
    let train_qrels = read_qrels(&train_qrels_path)?;
    let dev_qrels = read_qrels(&dev_qrels_path)?;
    let test_qrels = read_qrels(&test_qrels_path)?;
    let index = build_qps(&documents)?;
    let document_ids = documents
        .iter()
        .map(|document| document.id.clone())
        .collect::<Vec<_>>();
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let dev_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &dev_qrels,
        &mut scratch,
        TOP_K,
    )?;
    let test_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &test_qrels,
        &mut scratch,
        TOP_K,
    )?;
    let train_pairs = pairs_for_split(&pairs, "train");
    let dev_pairs = pairs_for_split(&pairs, "dev");
    let authority_qrels = authority_qrels(&pairs, &train_qrels, &dev_qrels);
    let authority_cases = collect_cases(
        &index,
        &document_ids,
        &query_by_id,
        &authority_qrels,
        &mut scratch,
        AUTHORITY_LIMIT,
    )?;
    let case_index = index_cases(&authority_cases);
    let mut arms = Vec::with_capacity(Arm::ALL.len());
    for arm in Arm::ALL {
        let selected_alpha = select_alpha(arm, &train_pairs, &case_index);
        arms.push(ArmReceipt {
            arm,
            label: arm.label(),
            feature_definition: arm.feature_definition(),
            selected_alpha,
            train_authority: concordance(arm, selected_alpha, &train_pairs, &case_index),
            dev_authority: concordance(arm, selected_alpha, &dev_pairs, &case_index),
            dev_metrics: evaluate(&dev_cases, arm, selected_alpha).0,
            test_metrics: evaluate(&test_cases, arm, selected_alpha).0,
            test_migrations: evaluate(&test_cases, arm, selected_alpha).1,
        });
    }
    let mut dose_response = Vec::with_capacity((Arm::ALL.len() - 1) * ALPHA_GRID.len());
    for arm in Arm::ALL {
        if arm == Arm::Anchor {
            continue;
        }
        for alpha in ALPHA_GRID {
            let (test_metrics, test_migrations) = evaluate(&test_cases, arm, alpha);
            dose_response.push(DoseReceipt {
                arm,
                alpha,
                train_authority: concordance(arm, alpha, &train_pairs, &case_index),
                dev_authority: concordance(arm, alpha, &dev_pairs, &case_index),
                test_metrics,
                test_migrations,
            });
        }
    }

    let receipt = Receipt {
        contract: "phoenix.qps.v3-p1-positional-authority-microscope/v1",
        dataset,
        status: "v3_p1_positional_authority",
        candidate_generator: "frozen_literal_qps_top100_evidence",
        authority_pairs_sha256: sha256_bytes(&pair_bytes),
        authority_receipt_sha256: sha256_bytes(&authority_bytes),
        gate_receipt_sha256: sha256_bytes(&gate_bytes),
        corpus_sha256: sha256_file(&corpus_path)?,
        queries_sha256: sha256_file(&queries_path)?,
        train_qrels_sha256: sha256_file(&train_qrels_path)?,
        dev_qrels_sha256: sha256_file(&dev_qrels_path)?,
        test_qrels_sha256: sha256_file(&test_qrels_path)?,
        minimum_authoritative_queries: gate.minimum_authoritative_queries,
        authoritative_queries: authority.queries_with_two_and_zero,
        authoritative_pairs: pairs.len(),
        alpha_grid: ALPHA_GRID.to_vec(),
        residual_bound: RESIDUAL_BOUND,
        candidate_pool: TOP_K,
        authority_candidate_pool: AUTHORITY_LIMIT,
        dose_response,
        arms,
    };
    let json = serde_json::to_vec_pretty(&receipt)?;
    fs::write(&output_path, &json)?;
    println!("{}", String::from_utf8_lossy(&json));
    Ok(())
}

fn pairs_for_split<'a>(pairs: &'a [HumanPair], split: &str) -> Vec<&'a HumanPair> {
    pairs.iter().filter(|pair| pair.split == split).collect()
}

fn authority_qrels(pairs: &[HumanPair], train_qrels: &Qrels, dev_qrels: &Qrels) -> Qrels {
    let mut selected = Qrels::new();
    for pair in pairs {
        let source = if pair.split == "train" {
            train_qrels
        } else {
            dev_qrels
        };
        if let Some(relevance) = source.get(&pair.query_id) {
            selected
                .entry(pair.query_id.clone())
                .or_insert_with(|| relevance.clone());
        }
    }
    selected
}

fn index_cases<'a>(cases: &'a [QueryCase]) -> HashMap<String, &'a QueryCase> {
    cases
        .iter()
        .map(|case| (case.query_id.clone(), case))
        .collect()
}

fn select_alpha(arm: Arm, pairs: &[&HumanPair], cases: &HashMap<String, &QueryCase>) -> f32 {
    if arm == Arm::Anchor {
        return 0.0;
    }
    let mut best: f32 = 0.0;
    let mut best_score = f64::NEG_INFINITY;
    for alpha in ALPHA_GRID {
        let stats = concordance(arm, alpha, pairs, cases);
        if stats.groups == 0 {
            continue;
        }
        let score = stats.mean_query_concordance;
        let tie = (score - best_score).abs() < 1.0e-12;
        if score > best_score
            || (tie && alpha.abs() < best.abs())
            || (tie && alpha.abs() == best.abs() && alpha < best)
        {
            best = alpha;
            best_score = score;
        }
    }
    best
}

fn concordance(
    arm: Arm,
    alpha: f32,
    pairs: &[&HumanPair],
    cases: &HashMap<String, &QueryCase>,
) -> PairConcordance {
    let mut by_query = HashMap::<String, (usize, usize, usize)>::new();
    let mut unavailable = 0;
    for pair in pairs {
        let Some(case) = cases.get(&pair.query_id) else {
            unavailable += 1;
            continue;
        };
        let positive = find_document(case, pair, true);
        let negative = find_document(case, pair, false);
        let Some((positive, negative)) = positive.zip(negative) else {
            unavailable += 1;
            continue;
        };
        let positive_score = score(positive, arm, alpha);
        let negative_score = score(negative, arm, alpha);
        let correct = if positive_score > negative_score
            || (positive_score.to_bits() == negative_score.to_bits()
                && positive.external_id < negative.external_id)
        {
            1
        } else {
            0
        };
        let tie = usize::from(positive_score.to_bits() == negative_score.to_bits());
        let entry = by_query.entry(pair.query_id.clone()).or_default();
        entry.0 += correct;
        entry.1 += 1;
        entry.2 += tie;
    }
    let mut mean = 0.0;
    let mut pairs_count = 0;
    let mut correct_pairs = 0;
    let mut ties = 0;
    for (correct, count, tie) in by_query.values() {
        mean += *correct as f64 / *count as f64;
        pairs_count += count;
        correct_pairs += correct;
        ties += tie;
    }
    PairConcordance {
        groups: by_query.len(),
        pairs: pairs_count,
        correct_pairs,
        ties,
        mean_query_concordance: if by_query.is_empty() {
            0.0
        } else {
            mean / by_query.len() as f64
        },
        unavailable_pairs: unavailable,
    }
}

fn find_document<'a>(
    case: &'a QueryCase,
    pair: &HumanPair,
    positive: bool,
) -> Option<&'a Candidate> {
    let id = if positive {
        &pair.positive_document_id
    } else {
        &pair.negative_document_id
    };
    case.candidates
        .iter()
        .find(|candidate| candidate.document_id == *id)
}

fn score(candidate: &Candidate, arm: Arm, alpha: f32) -> f32 {
    let residual = match arm {
        Arm::Anchor => 0.0,
        Arm::P1CompleteSpan => candidate.values[10],
        Arm::P2OrderedSpan => candidate.values[11],
        Arm::P3OrderedFraction => candidate.values[12],
        Arm::P4ExactPhrase => candidate.values[13],
        Arm::P5ExactField => candidate.values[15],
    };
    candidate.values[0] + alpha * residual
}

fn evaluate(cases: &[QueryCase], arm: Arm, alpha: f32) -> (Metrics, Migrations) {
    let mut metrics = Metrics::default();
    let mut migrations = Migrations::default();
    for case in cases {
        let mut ranking = case.candidates.iter().collect::<Vec<_>>();
        ranking.sort_unstable_by(|left, right| {
            score(left, arm, alpha)
                .total_cmp(&score(right, arm, alpha))
                .reverse()
                .then_with(|| left.external_id.cmp(&right.external_id))
        });
        if case.relevant_documents == 0 {
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
        let ideal = case.relevant_documents.min(10);
        let idcg = (0..ideal)
            .map(|rank| 1.0 / (rank as f64 + 2.0).log2())
            .sum::<f64>();
        metrics.ndcg_at_10 += if idcg > 0.0 { dcg / idcg } else { 0.0 };
        metrics.recall_at_100 +=
            found.min(case.relevant_documents) as f64 / case.relevant_documents as f64;
        metrics.mrr += reciprocal;
        metrics.map += precision_sum / case.relevant_documents as f64;
        for candidate in &case.candidates {
            if !candidate.relevant {
                continue;
            }
            migrations.relevant_in_candidate_pool += 1;
            let old_rank = candidate.baseline_rank;
            let new_rank = ranking
                .iter()
                .position(|item| item.external_id == candidate.external_id)
                .unwrap_or(old_rank)
                + 1;
            if old_rank <= 10 && new_rank > 10 {
                migrations.old_top10_new_below10 += 1;
            }
            if old_rank <= 100 && new_rank > 100 {
                migrations.old_top100_new_below100 += 1;
            }
            if old_rank > 100 && new_rank <= 10 {
                migrations.old_over100_new_top10 += 1;
            }
        }
    }
    let divisor = metrics.queries.max(1) as f64;
    metrics.ndcg_at_10 /= divisor;
    metrics.recall_at_100 /= divisor;
    metrics.mrr /= divisor;
    metrics.map /= divisor;
    (metrics, migrations)
}

fn collect_cases(
    index: &QpsIndex,
    document_ids: &[String],
    queries: &HashMap<String, Query>,
    qrels: &Qrels,
    scratch: &mut SearchScratch,
    limit: usize,
) -> Result<Vec<QueryCase>> {
    let mut cases = Vec::with_capacity(qrels.len());
    let mut hits = Vec::with_capacity(limit);
    for (query_id, relevance) in qrels {
        let Some(query) = queries.get(query_id) else {
            continue;
        };
        hits.clear();
        if limit == TOP_K {
            index.search_evidence_into(&query.text, limit, scratch, &mut hits)?;
        } else {
            index.search_exhaustive_evidence_into(&query.text, limit, scratch, &mut hits)?;
        }
        let candidates = hits
            .iter()
            .enumerate()
            .map(|(rank, hit)| Candidate {
                external_id: hit.external_id,
                document_id: document_ids[hit.external_id as usize].clone(),
                baseline_rank: rank + 1,
                values: hit.rank_evidence_v3.values,
                relevant: relevance
                    .get(&document_ids[hit.external_id as usize])
                    .is_some_and(|score| *score > 0),
            })
            .collect::<Vec<_>>();
        cases.push(QueryCase {
            query_id: query_id.clone(),
            candidates,
            relevant_documents: relevance.values().filter(|score| **score > 0).count(),
        });
    }
    Ok(cases)
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
            let row: CorpusRow = serde_json::from_str(
                &value.with_context(|| format!("read corpus line {}", line + 1))?,
            )
            .with_context(|| format!("decode corpus line {}", line + 1))?;
            Ok(Document {
                id: row._id,
                title: row.title,
                text: row.text,
            })
        })
        .collect()
}

fn read_queries(path: &Path) -> Result<Vec<Query>> {
    BufReader::new(File::open(path).with_context(|| format!("open queries {}", path.display()))?)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let row: QueryRow = serde_json::from_str(
                &value.with_context(|| format!("read query line {}", line + 1))?,
            )
            .with_context(|| format!("decode query line {}", line + 1))?;
            Ok(Query {
                id: row._id,
                text: row.text,
            })
        })
        .collect()
}

fn read_qrels(path: &Path) -> Result<Qrels> {
    let file = File::open(path).with_context(|| format!("open qrels {}", path.display()))?;
    let mut qrels = Qrels::new();
    for (line, value) in BufReader::new(file).lines().enumerate() {
        let value = value?;
        if line == 0 && value.starts_with("query-id") {
            continue;
        }
        let mut columns = value.split('\t');
        let query = columns.next().context("missing query id")?.to_owned();
        let document = columns.next().context("missing document id")?.to_owned();
        let score = columns.next().context("missing score")?.parse::<u32>()?;
        qrels.entry(query).or_default().insert(document, score);
    }
    Ok(qrels)
}

fn sha256_file(path: &Path) -> Result<String> {
    Ok(sha256_bytes(&fs::read(path)?))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
