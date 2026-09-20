//! Read-only BEIR quality/cost frontier for Phoenix QPS.
//!
//! The transport lanes intentionally use a duplicated exact expansion as a
//! control. No synonym or fuzzy policy is invented here; a future transport
//! result must use a separately frozen expansion artifact.

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use phoenix_lexical_qps::{
    DocumentInput, Expansion, FeatureNormalizationV3, FieldConfig, LinearRankerV3, QpsBuilder,
    QpsConfig, QpsIndex, QueryGroup, SearchHit, SearchScratch, RANK_EVIDENCE_V3_FEATURE_COUNT,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;
const DEFAULT_REPETITIONS: usize = 3;
const MAXIMUM_QUERY_GROUPS: usize = 128;

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

#[derive(Clone, Copy, Debug)]
enum Lane {
    Bm25,
    PhoenixLiteral,
    PhoenixPositional,
    PhoenixV3,
    PhoenixTransportControl,
    PhoenixTransportV3Control,
}

impl Lane {
    const ALL: [Self; 6] = [
        Self::Bm25,
        Self::PhoenixLiteral,
        Self::PhoenixPositional,
        Self::PhoenixV3,
        Self::PhoenixTransportControl,
        Self::PhoenixTransportV3Control,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::PhoenixLiteral => "phoenix_literal",
            Self::PhoenixPositional => "phoenix_literal_positional",
            Self::PhoenixV3 => "phoenix_literal_v3_synthetic",
            Self::PhoenixTransportControl => "phoenix_transport_exact_control",
            Self::PhoenixTransportV3Control => "phoenix_transport_v3_exact_control",
        }
    }
}

#[derive(Default)]
struct Bm25Index {
    postings: HashMap<String, Vec<(u32, u32)>>,
    lengths: Vec<u32>,
    average_length: f32,
}

#[derive(Clone, Copy, Debug)]
struct ScoredDoc {
    doc: u32,
    score: f32,
}

#[derive(Debug, serde::Serialize)]
struct LaneReceipt {
    lane: &'static str,
    queries: usize,
    repetitions: usize,
    ndcg_at_10: f64,
    recall_at_100: f64,
    mrr: f64,
    map: f64,
    mean_nanos: u64,
    median_nanos: u64,
    p95_nanos: u64,
    p99_nanos: u64,
    max_nanos: u64,
}

#[derive(Debug, serde::Serialize)]
struct DatasetReceipt {
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
    repetitions: usize,
    lanes: Vec<LaneReceipt>,
    transport_control_note: &'static str,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: qps_quality_frontier <dataset-root> [repetitions] [output-json]")?,
    );
    let repetitions = args
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()
        .context("repetitions must be an integer")?
        .unwrap_or(DEFAULT_REPETITIONS);
    if repetitions == 0 || repetitions > 32 {
        bail!("repetitions must be in 1..=32");
    }
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
    let bm25 = build_bm25(&documents);
    let lexical = build_qps(&documents, false)?;
    let positional = build_qps(&documents, true)?;
    let v3 = synthetic_v3_ranker()?;

    let mut lanes = Vec::with_capacity(Lane::ALL.len());
    for lane in Lane::ALL {
        let receipt = run_lane(
            lane,
            &documents,
            &queries,
            &qrels,
            &bm25,
            &lexical,
            &positional,
            &v3,
            repetitions,
        )?;
        println!(
            "{}: nDCG@10={:.5} recall@100={:.5} MRR={:.5} MAP={:.5} median={}ns p95={}ns",
            receipt.lane,
            receipt.ndcg_at_10,
            receipt.recall_at_100,
            receipt.mrr,
            receipt.map,
            receipt.median_nanos,
            receipt.p95_nanos,
        );
        lanes.push(receipt);
    }

    let receipt = DatasetReceipt {
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
        repetitions,
        lanes,
        transport_control_note:
            "Transport lanes duplicate each exact query term at quality 0.999; no synonym/fuzzy expansion policy is claimed.",
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

fn build_bm25(documents: &[Document]) -> Bm25Index {
    let mut index = Bm25Index {
        postings: HashMap::with_capacity(documents.len().saturating_mul(2)),
        lengths: Vec::with_capacity(documents.len()),
        average_length: 0.0,
    };
    for (doc, document) in documents.iter().enumerate() {
        let tokens = tokenize(&format!("{} {}", document.title, document.text));
        index.average_length += tokens.len() as f32;
        index.lengths.push(tokens.len() as u32);
        let mut term_frequency = HashMap::<String, u32>::new();
        for token in tokens {
            *term_frequency.entry(token).or_default() += 1;
        }
        for (term, frequency) in term_frequency {
            index
                .postings
                .entry(term)
                .or_default()
                .push((doc as u32, frequency));
        }
    }
    index.average_length /= documents.len().max(1) as f32;
    index
}

fn build_qps(documents: &[Document], positional: bool) -> Result<QpsIndex> {
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

fn synthetic_v3_ranker() -> Result<LinearRankerV3> {
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    weights[0] = 8.0;
    weights[6] = 3.0;
    weights[9] = 2.0;
    weights[11] = 1.0;
    weights[13] = 1.0;
    LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights)
        .map_err(anyhow::Error::msg)
}

#[allow(clippy::too_many_arguments)]
fn run_lane(
    lane: Lane,
    documents: &[Document],
    queries: &[Query],
    qrels: &Qrels,
    bm25: &Bm25Index,
    lexical: &QpsIndex,
    positional: &QpsIndex,
    v3: &LinearRankerV3,
    repetitions: usize,
) -> Result<LaneReceipt> {
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::with_capacity(TOP_K);
    let mut latencies = Vec::with_capacity(queries.len().saturating_mul(repetitions));
    let mut quality = QualityAccumulator::default();
    for query in queries {
        let relevance = qrels.get(&query.id).cloned().unwrap_or_default();
        if relevance.is_empty() {
            continue;
        }
        let first = execute(
            lane,
            query,
            bm25,
            lexical,
            positional,
            v3,
            &mut scratch,
            &mut hits,
        )?;
        quality.observe(documents, &relevance, &first);
        for _ in 0..repetitions {
            let started = Instant::now();
            let _ = execute(
                lane,
                query,
                bm25,
                lexical,
                positional,
                v3,
                &mut scratch,
                &mut hits,
            )?;
            latencies.push(elapsed_nanos(started));
        }
    }
    let latency = Percentiles::from(&mut latencies);
    Ok(LaneReceipt {
        lane: lane.label(),
        queries: quality.queries,
        repetitions,
        ndcg_at_10: quality.ndcg / quality.queries.max(1) as f64,
        recall_at_100: quality.recall / quality.queries.max(1) as f64,
        mrr: quality.mrr / quality.queries.max(1) as f64,
        map: quality.map / quality.queries.max(1) as f64,
        mean_nanos: latency.mean,
        median_nanos: latency.median,
        p95_nanos: latency.p95,
        p99_nanos: latency.p99,
        max_nanos: latency.max,
    })
}

fn execute<'a>(
    lane: Lane,
    query: &Query,
    bm25: &Bm25Index,
    lexical: &QpsIndex,
    positional: &QpsIndex,
    v3: &LinearRankerV3,
    scratch: &mut SearchScratch,
    hits: &'a mut Vec<SearchHit>,
) -> Result<Vec<u64>> {
    hits.clear();
    match lane {
        Lane::Bm25 => Ok(bm25.search(&query.text, TOP_K)),
        Lane::PhoenixLiteral => lexical
            .search_into(&query.text, TOP_K, scratch, hits)
            .map(|_| hit_ids(hits))
            .map_err(Into::into),
        Lane::PhoenixPositional => positional
            .search_into(&query.text, TOP_K, scratch, hits)
            .map(|_| hit_ids(hits))
            .map_err(Into::into),
        Lane::PhoenixV3 => positional
            .search_v3_into(&query.text, TOP_K, v3, scratch, hits)
            .map(|_| hit_ids(hits))
            .map_err(Into::into),
        Lane::PhoenixTransportControl => transport_search(positional, query, scratch, hits, None),
        Lane::PhoenixTransportV3Control => {
            transport_search(positional, query, scratch, hits, Some(v3))
        }
    }
}

fn transport_search(
    index: &QpsIndex,
    query: &Query,
    scratch: &mut SearchScratch,
    hits: &mut Vec<SearchHit>,
    v3: Option<&LinearRankerV3>,
) -> Result<Vec<u64>> {
    let tokens = tokenize(&query.text);
    let terms = tokens.into_iter().map(|token| token).collect::<Vec<_>>();
    let expansion_groups = terms
        .iter()
        .map(|term| {
            vec![
                Expansion {
                    term: term.as_str(),
                    quality: 1.0,
                },
                Expansion {
                    term: term.as_str(),
                    quality: 0.999,
                },
            ]
        })
        .collect::<Vec<_>>();
    let groups = expansion_groups
        .iter()
        .map(|expansions| QueryGroup { expansions })
        .collect::<Vec<_>>();
    if groups.is_empty() || groups.len() > MAXIMUM_QUERY_GROUPS {
        return Ok(Vec::new());
    }
    match v3 {
        Some(model) => index
            .search_groups_v3_into(&groups, TOP_K, model, scratch, hits)
            .map(|_| hit_ids(hits))
            .map_err(Into::into),
        None => index
            .search_groups_into(&groups, TOP_K, scratch, hits)
            .map(|_| hit_ids(hits))
            .map_err(Into::into),
    }
}

fn hit_ids(hits: &[SearchHit]) -> Vec<u64> {
    hits.iter().map(|hit| hit.external_id).collect()
}

impl Bm25Index {
    fn search(&self, query: &str, limit: usize) -> Vec<u64> {
        let mut scores = HashMap::<u32, f32>::new();
        let query_terms = tokenize(query);
        for term in query_terms {
            let Some(postings) = self.postings.get(&term) else {
                continue;
            };
            let document_frequency = postings.len() as f32;
            let total_documents = self.lengths.len() as f32;
            let idf = ((total_documents - document_frequency + 0.5) / (document_frequency + 0.5)
                + 1.0)
                .ln();
            for &(document, frequency) in postings {
                let length = self.lengths[document as usize] as f32;
                let denominator = frequency as f32
                    + 1.2 * (1.0 - 0.75 + 0.75 * length / self.average_length.max(1.0));
                let contribution = idf * (frequency as f32 * 2.2) / denominator;
                *scores.entry(document).or_default() += contribution;
            }
        }
        let mut ranked = scores
            .into_iter()
            .map(|(doc, score)| ScoredDoc { doc, score })
            .collect::<Vec<_>>();
        ranked.sort_unstable_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.doc.cmp(&right.doc))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|item| item.doc as u64)
            .collect()
    }
}

#[derive(Default)]
struct QualityAccumulator {
    queries: usize,
    ndcg: f64,
    recall: f64,
    mrr: f64,
    map: f64,
}

impl QualityAccumulator {
    fn observe(
        &mut self,
        documents: &[Document],
        relevance: &HashMap<String, u32>,
        ranking: &[u64],
    ) {
        self.queries += 1;
        let relevant = relevance.len();
        let mut dcg = 0.0;
        let mut found = 0;
        let mut reciprocal = 0.0;
        let mut precision_sum = 0.0;
        for (rank, document) in ranking.iter().enumerate() {
            let Some(document) = documents.get(*document as usize) else {
                continue;
            };
            let key = &document.id;
            let Some(score) = relevance.get(key) else {
                continue;
            };
            if rank < 10 {
                dcg += (2.0_f64.powi(*score as i32) - 1.0) / (rank as f64 + 2.0).log2();
            }
            found += 1;
            if reciprocal == 0.0 {
                reciprocal = 1.0 / (rank + 1) as f64;
            }
            precision_sum += found as f64 / (rank + 1) as f64;
        }
        let mut ideal = relevance.values().copied().collect::<Vec<_>>();
        ideal.sort_unstable_by(|left, right| right.cmp(left));
        let idcg = ideal
            .into_iter()
            .take(10)
            .enumerate()
            .map(|(rank, score)| (2.0_f64.powi(score as i32) - 1.0) / (rank as f64 + 2.0).log2())
            .sum::<f64>();
        self.ndcg += if idcg > 0.0 { dcg / idcg } else { 0.0 };
        self.recall += found as f64 / relevant.max(1) as f64;
        self.mrr += reciprocal;
        self.map += precision_sum / relevant.max(1) as f64;
    }
}

struct Percentiles {
    mean: u64,
    median: u64,
    p95: u64,
    p99: u64,
    max: u64,
}

impl Percentiles {
    fn from(samples: &mut [u64]) -> Self {
        samples.sort_unstable();
        let total = samples.iter().copied().sum::<u64>();
        let at = |percentile: usize| {
            samples
                .get((samples.len().saturating_sub(1) * percentile) / 100)
                .copied()
                .unwrap_or(0)
        };
        Self {
            mean: total / samples.len().max(1) as u64,
            median: at(50),
            p95: at(95),
            p99: at(99),
            max: samples.last().copied().unwrap_or(0),
        }
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

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {} for hash", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
