//! Read-only query-regime census for Phoenix literal versus broad positional
//! retrieval. This is diagnostic only: it does not change serving policy,
//! ranker weights, thresholds, or any frozen qualification artifact.

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, SearchHit, SearchScratch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;
const MAXIMUM_QUERY_GROUPS: usize = 128;
const CLASS_EPSILON: f64 = 1.0e-12;

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

#[derive(Debug, Serialize)]
struct QueryRowReceipt {
    query_id: String,
    query_tokens: usize,
    unique_query_tokens: usize,
    adjacent_token_pairs: usize,
    query_has_quotes: bool,
    query_has_hyphen: bool,
    class: &'static str,
    literal_ndcg_at_10: f64,
    positional_ndcg_at_10: f64,
    ndcg_delta: f64,
    literal_recall_at_100: f64,
    positional_recall_at_100: f64,
    recall_delta: f64,
    literal_top1_score: f64,
    literal_top1_top2_margin: f64,
    literal_top1_coverage: f64,
    literal_top10_mean_coverage: f64,
    literal_top1_rarity: f64,
    literal_top10_mean_rarity: f64,
    literal_top1_field_coverage: f64,
    literal_top10_mean_field_coverage: f64,
    literal_top1_complete_span: f64,
    literal_top1_ordered_span: f64,
    literal_top1_ordered_fraction: f64,
    literal_top1_exact_phrase: f64,
    positional_top1_order: f64,
    positional_top1_proximity: f64,
    positional_top1_phrase: f64,
    relevant_documents: usize,
    literal_relevant_in_top_10: usize,
    positional_relevant_in_top_10: usize,
    literal_relevant_in_top_100: usize,
    positional_relevant_in_top_100: usize,
}

#[derive(Default, Debug, Serialize)]
struct ClassAggregate {
    queries: usize,
    mean_ndcg_delta: f64,
    mean_recall_delta: f64,
    mean_query_tokens: f64,
    mean_adjacent_token_pairs: f64,
    mean_top1_coverage: f64,
    mean_top10_coverage: f64,
    mean_top1_rarity: f64,
    mean_top10_rarity: f64,
    mean_top1_field_coverage: f64,
    mean_top10_field_coverage: f64,
    mean_top1_complete_span: f64,
    mean_top1_ordered_span: f64,
    mean_top1_ordered_fraction: f64,
    mean_top1_exact_phrase: f64,
    mean_top1_margin: f64,
    mean_literal_relevant_top_10: f64,
    mean_positional_relevant_top_10: f64,
    quoted_query_count: usize,
    hyphenated_query_count: usize,
}

impl ClassAggregate {
    fn add(&mut self, row: &QueryRowReceipt) {
        self.queries += 1;
        let n = self.queries as f64;
        let update = |mean: &mut f64, value: f64| {
            *mean += (value - *mean) / n;
        };
        update(&mut self.mean_ndcg_delta, row.ndcg_delta);
        update(&mut self.mean_recall_delta, row.recall_delta);
        update(&mut self.mean_query_tokens, row.query_tokens as f64);
        update(
            &mut self.mean_adjacent_token_pairs,
            row.adjacent_token_pairs as f64,
        );
        update(&mut self.mean_top1_coverage, row.literal_top1_coverage);
        update(
            &mut self.mean_top10_coverage,
            row.literal_top10_mean_coverage,
        );
        update(&mut self.mean_top1_rarity, row.literal_top1_rarity);
        update(&mut self.mean_top10_rarity, row.literal_top10_mean_rarity);
        update(
            &mut self.mean_top1_field_coverage,
            row.literal_top1_field_coverage,
        );
        update(
            &mut self.mean_top10_field_coverage,
            row.literal_top10_mean_field_coverage,
        );
        update(
            &mut self.mean_top1_complete_span,
            row.literal_top1_complete_span,
        );
        update(
            &mut self.mean_top1_ordered_span,
            row.literal_top1_ordered_span,
        );
        update(
            &mut self.mean_top1_ordered_fraction,
            row.literal_top1_ordered_fraction,
        );
        update(
            &mut self.mean_top1_exact_phrase,
            row.literal_top1_exact_phrase,
        );
        update(&mut self.mean_top1_margin, row.literal_top1_top2_margin);
        update(
            &mut self.mean_literal_relevant_top_10,
            row.literal_relevant_in_top_10 as f64,
        );
        update(
            &mut self.mean_positional_relevant_top_10,
            row.positional_relevant_in_top_10 as f64,
        );
        if row.query_has_quotes {
            self.quoted_query_count += 1;
        }
        if row.query_has_hyphen {
            self.hyphenated_query_count += 1;
        }
    }
}

#[derive(Debug, Serialize)]
struct CensusReceipt {
    schema: &'static str,
    dataset: String,
    corpus_path: String,
    queries_path: String,
    qrels_path: String,
    corpus_sha256: String,
    queries_sha256: String,
    qrels_sha256: String,
    documents: usize,
    queries: usize,
    judged_queries: usize,
    compared_queries: usize,
    classification_epsilon: f64,
    literal_lane: &'static str,
    positional_lane: &'static str,
    query_rows: Vec<QueryRowReceipt>,
    class_counts: BTreeMap<&'static str, usize>,
    class_aggregates: BTreeMap<&'static str, ClassAggregate>,
    interpretation: &'static str,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: qps_positional_census <dataset-root> [output-json]")?,
    );
    let output = args.next().map(PathBuf::from);
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let corpus_path = root.join("corpus.jsonl");
    let queries_path = root.join("queries.jsonl");
    let qrels_path = root.join("qrels").join("test.tsv");
    let documents = read_corpus(&corpus_path)?;
    let queries = read_queries(&queries_path)?;
    let qrels = read_qrels(&qrels_path)?;
    let literal = build_qps(&documents, false)?;
    let positional = build_qps(&documents, true)?;
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut literal_hits = Vec::with_capacity(256);
    let mut positional_hits = Vec::with_capacity(256);
    let mut rows = Vec::new();
    let mut aggregates = BTreeMap::<&'static str, ClassAggregate>::new();
    let mut class_counts = BTreeMap::<&'static str, usize>::new();

    for query in &queries {
        let Some(relevance) = qrels.get(&query.id) else {
            continue;
        };
        if relevance.is_empty() {
            continue;
        }
        literal_hits.clear();
        positional_hits.clear();
        literal
            .search_evidence_into(&query.text, TOP_K, &mut scratch, &mut literal_hits)
            .with_context(|| format!("literal search for query {}", query.id))?;
        positional
            .search_evidence_into(&query.text, TOP_K, &mut scratch, &mut positional_hits)
            .with_context(|| format!("positional search for query {}", query.id))?;
        let literal_top = &literal_hits[..literal_hits.len().min(TOP_K)];
        let positional_top = &positional_hits[..positional_hits.len().min(TOP_K)];
        let literal_ndcg = ndcg_at_10(&documents, relevance, literal_top);
        let positional_ndcg = ndcg_at_10(&documents, relevance, positional_top);
        let literal_recall = recall_at_100(&documents, relevance, literal_top);
        let positional_recall = recall_at_100(&documents, relevance, positional_top);
        let ndcg_delta = positional_ndcg - literal_ndcg;
        let recall_delta = positional_recall - literal_recall;
        let class = if ndcg_delta > CLASS_EPSILON {
            "POSITION_HELPED"
        } else if ndcg_delta < -CLASS_EPSILON {
            "POSITION_HURT"
        } else {
            "POSITION_NEUTRAL"
        };
        let tokens = tokenize(&query.text);
        let unique_tokens = tokens
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        let top1 = literal_top.first().copied();
        let top1_score = top1.map(|hit| hit.score).unwrap_or(0.0);
        let second_score = literal_top
            .get(1)
            .map(|hit| hit.score)
            .unwrap_or(top1_score);
        let row = QueryRowReceipt {
            query_id: query.id.clone(),
            query_tokens: tokens.len(),
            unique_query_tokens: unique_tokens,
            adjacent_token_pairs: tokens.len().saturating_sub(1),
            query_has_quotes: query.text.contains('"'),
            query_has_hyphen: query.text.contains('-'),
            class,
            literal_ndcg_at_10: literal_ndcg,
            positional_ndcg_at_10: positional_ndcg,
            ndcg_delta,
            literal_recall_at_100: literal_recall,
            positional_recall_at_100: positional_recall,
            recall_delta,
            literal_top1_score: f64::from(top1_score),
            literal_top1_top2_margin: f64::from(top1_score - second_score),
            literal_top1_coverage: f64::from(top1.map(|hit| hit.coverage).unwrap_or(0.0)),
            literal_top10_mean_coverage: mean(literal_top.iter().map(|hit| hit.coverage)),
            literal_top1_rarity: evidence_value_opt(top1, 19),
            literal_top10_mean_rarity: mean(
                literal_top.iter().map(|hit| evidence_value(hit, 19) as f32),
            ),
            literal_top1_field_coverage: evidence_value_opt(top1, 29),
            literal_top10_mean_field_coverage: mean(
                literal_top.iter().map(|hit| evidence_value(hit, 29) as f32),
            ),
            literal_top1_complete_span: evidence_value_opt(top1, 10),
            literal_top1_ordered_span: evidence_value_opt(top1, 11),
            literal_top1_ordered_fraction: evidence_value_opt(top1, 12),
            literal_top1_exact_phrase: evidence_value_opt(top1, 13),
            positional_top1_order: f64::from(
                positional_top.first().map(|hit| hit.order).unwrap_or(0.0),
            ),
            positional_top1_proximity: f64::from(
                positional_top
                    .first()
                    .map(|hit| hit.proximity)
                    .unwrap_or(0.0),
            ),
            positional_top1_phrase: f64::from(
                positional_top.first().map(|hit| hit.phrase).unwrap_or(0.0),
            ),
            relevant_documents: relevance.len(),
            literal_relevant_in_top_10: relevant_count(
                &documents,
                relevance,
                &literal_top[..literal_top.len().min(10)],
            ),
            positional_relevant_in_top_10: relevant_count(
                &documents,
                relevance,
                &positional_top[..positional_top.len().min(10)],
            ),
            literal_relevant_in_top_100: relevant_count(&documents, relevance, literal_top),
            positional_relevant_in_top_100: relevant_count(&documents, relevance, positional_top),
        };
        *class_counts.entry(class).or_default() += 1;
        aggregates.entry(class).or_default().add(&row);
        rows.push(row);
    }

    let receipt = CensusReceipt {
        schema: "phoenix.qps.positional-regime-census/v1",
        dataset,
        corpus_path: corpus_path.display().to_string(),
        queries_path: queries_path.display().to_string(),
        qrels_path: qrels_path.display().to_string(),
        corpus_sha256: sha256_file(&corpus_path)?,
        queries_sha256: sha256_file(&queries_path)?,
        qrels_sha256: sha256_file(&qrels_path)?,
        documents: documents.len(),
        queries: queries.len(),
        judged_queries: qrels.len(),
        compared_queries: rows.len(),
        classification_epsilon: CLASS_EPSILON,
        literal_lane: "phoenix_literal (same configuration as quality frontier)",
        positional_lane: "phoenix_literal_positional (same configuration as quality frontier)",
        query_rows: rows,
        class_counts,
        class_aggregates: aggregates,
        interpretation: "Diagnostic query-regime census only. No gate, weight, serving policy, or ranker artifact is changed.",
    };
    let json = serde_json::to_string_pretty(&receipt)?;
    if let Some(path) = output {
        fs::write(&path, json.as_bytes())
            .with_context(|| format!("write receipt {}", path.display()))?;
    }
    println!("{}", json);
    Ok(())
}

fn read_corpus(path: &Path) -> Result<Vec<Document>> {
    let file = File::open(path).with_context(|| format!("open corpus {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let value = value.with_context(|| format!("read corpus line {}", line + 1))?;
            let row: CorpusRow = serde_json::from_str(&value)
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
    let file = File::open(path).with_context(|| format!("open queries {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let value = value.with_context(|| format!("read query line {}", line + 1))?;
            let row: QueryRow = serde_json::from_str(&value)
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
    let mut result = Qrels::new();
    for (line, value) in BufReader::new(file).lines().enumerate() {
        let value = value.with_context(|| format!("read qrels line {}", line + 1))?;
        if line == 0 && value.starts_with("query-id") {
            continue;
        }
        let mut columns = value.split('\t');
        let query = columns.next().context("qrels query id missing")?;
        let document = columns.next().context("qrels document id missing")?;
        let score = columns
            .next()
            .context("qrels score missing")?
            .parse::<u32>()
            .with_context(|| format!("invalid qrels score on line {}", line + 1))?;
        result
            .entry(query.to_owned())
            .or_default()
            .insert(document.to_owned(), score);
    }
    Ok(result)
}

fn build_qps(documents: &[Document], positional: bool) -> Result<phoenix_lexical_qps::QpsIndex> {
    let fields = [
        FieldConfig::new("title", 2.5, 0.35, if positional { 0.35 } else { 0.0 }),
        FieldConfig::new("body", 1.0, 0.75, if positional { 0.10 } else { 0.0 }),
    ];
    let mut config = QpsConfig {
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        maximum_candidate_pool: 256,
        ..QpsConfig::default()
    };
    if !positional {
        config.proximity_weight = 0.0;
        config.order_weight = 0.0;
        config.phrase_weight = 0.0;
        config.segment_weight = 0.0;
    }
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config)?;
    for (index, document) in documents.iter().enumerate() {
        builder.insert(DocumentInput {
            external_id: index as u64,
            fields: &[document.title.as_str(), document.text.as_str()],
        })?;
    }
    builder.build().map_err(Into::into)
}

fn ndcg_at_10(
    documents: &[Document],
    relevance: &HashMap<String, u32>,
    ranking: &[SearchHit],
) -> f64 {
    let mut dcg = 0.0;
    for (rank, hit) in ranking.iter().take(10).enumerate() {
        let Some(document) = documents.get(hit.external_id as usize) else {
            continue;
        };
        let Some(score) = relevance.get(&document.id) else {
            continue;
        };
        dcg += (2.0_f64.powi(*score as i32) - 1.0) / (rank as f64 + 2.0).log2();
    }
    let mut ideal = relevance.values().copied().collect::<Vec<_>>();
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    let idcg = ideal
        .into_iter()
        .take(10)
        .enumerate()
        .map(|(rank, score)| (2.0_f64.powi(score as i32) - 1.0) / (rank as f64 + 2.0).log2())
        .sum::<f64>();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

fn recall_at_100(
    documents: &[Document],
    relevance: &HashMap<String, u32>,
    ranking: &[SearchHit],
) -> f64 {
    relevant_count(documents, relevance, ranking) as f64 / relevance.len().max(1) as f64
}

fn relevant_count(
    documents: &[Document],
    relevance: &HashMap<String, u32>,
    ranking: &[SearchHit],
) -> usize {
    ranking
        .iter()
        .filter_map(|hit| documents.get(hit.external_id as usize))
        .filter(|document| relevance.contains_key(&document.id))
        .count()
}

fn evidence_value(hit: &SearchHit, index: usize) -> f64 {
    f64::from(
        hit.rank_evidence_v3
            .values
            .get(index)
            .copied()
            .unwrap_or(0.0),
    )
}

fn evidence_value_opt(hit: Option<SearchHit>, index: usize) -> f64 {
    hit.map(|value| evidence_value(&value, index))
        .unwrap_or(0.0)
}

fn mean<I>(values: I) -> f64
where
    I: Iterator<Item = f32>,
{
    let mut count = 0_u64;
    let mut total = 0.0_f64;
    for value in values {
        count += 1;
        total += f64::from(value);
    }
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() {
            for lower in character.to_lowercase() {
                current.push(lower);
            }
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {} for hash", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
