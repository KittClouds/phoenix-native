//! Read-only miss taxonomy for the Phoenix literal BEIR lane.
//!
//! This is an autopsy, not a serving change. It compares judged relevant
//! documents absent from Phoenix's bounded top-100 with the exhaustive literal
//! oracle and an independent BM25 baseline. The small stem-overlap signal is a
//! heuristic diagnostic only; it is not a morphology implementation.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchScratch,
};
use serde::{Deserialize, Serialize};

const TOP_K: usize = 100;
const MAXIMUM_QUERY_GROUPS: usize = 128;
// Must match QpsConfig::default used by the quality frontier evaluator.
const COVERAGE_FLOOR: f32 = 0.2;
const SAMPLE_LIMIT: usize = 32;

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

#[derive(Default)]
struct Bm25Index {
    postings: HashMap<String, Vec<(u32, u32)>>,
    lengths: Vec<u32>,
    average_length: f32,
}

#[derive(Debug, Default, Serialize)]
struct Counts {
    relevant_documents: u64,
    phoenix_top100: u64,
    missed_by_phoenix: u64,
    bm25_top100_any_missed: u64,
    literal_absent: u64,
    coverage_filtered: u64,
    ranked_below_top100: u64,
    ranked_below_top100_bm25_found: u64,
    eligible_missing_from_exhaustive: u64,
    stem_overlap_with_literal_absence: u64,
}

#[derive(Debug, Default, Serialize)]
struct RankDepth {
    rank_101_200: u64,
    rank_201_500: u64,
    rank_501_1000: u64,
    rank_1001_5000: u64,
    rank_over_5000: u64,
}

impl RankDepth {
    fn observe(&mut self, rank: usize) {
        match rank {
            101..=200 => self.rank_101_200 += 1,
            201..=500 => self.rank_201_500 += 1,
            501..=1000 => self.rank_501_1000 += 1,
            1001..=5000 => self.rank_1001_5000 += 1,
            _ => self.rank_over_5000 += 1,
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct FeatureMeans {
    documents: u64,
    lexical_score: f32,
    coverage: f32,
    proximity: f32,
    order: f32,
    phrase: f32,
    segment: f32,
    exact_field: f32,
}

impl FeatureMeans {
    fn observe(&mut self, hit: &phoenix_lexical_qps::SearchHit) {
        self.documents += 1;
        self.lexical_score += hit.lexical_score;
        self.coverage += hit.coverage;
        self.proximity += hit.proximity;
        self.order += hit.order;
        self.phrase += hit.phrase;
        self.segment += hit.segment;
        self.exact_field += hit.exact_field;
    }

    fn finish(&mut self) {
        let divisor = self.documents.max(1) as f32;
        self.lexical_score /= divisor;
        self.coverage /= divisor;
        self.proximity /= divisor;
        self.order /= divisor;
        self.phrase /= divisor;
        self.segment /= divisor;
        self.exact_field /= divisor;
    }
}

#[derive(Debug, Default, Serialize)]
struct LexicalCensus {
    documents: u64,
    max_idf: f32,
    mean_idf: f32,
    max_tf: f32,
    mean_tf: f32,
    document_length: f32,
    strongest_single_term: f32,
    contribution_entropy: f32,
    title_hits: f32,
    phoenix_lexical_score: f32,
    phoenix_coverage: f32,
}

impl LexicalCensus {
    fn observe(&mut self, values: LexicalValues) {
        self.documents += 1;
        self.max_idf += values.max_idf;
        self.mean_idf += values.mean_idf;
        self.max_tf += values.max_tf;
        self.mean_tf += values.mean_tf;
        self.document_length += values.document_length;
        self.strongest_single_term += values.strongest_single_term;
        self.contribution_entropy += values.contribution_entropy;
        self.title_hits += values.title_hits;
        self.phoenix_lexical_score += values.phoenix_lexical_score;
        self.phoenix_coverage += values.phoenix_coverage;
    }

    fn finish(&mut self) {
        let divisor = self.documents.max(1) as f32;
        self.max_idf /= divisor;
        self.mean_idf /= divisor;
        self.max_tf /= divisor;
        self.mean_tf /= divisor;
        self.document_length /= divisor;
        self.strongest_single_term /= divisor;
        self.contribution_entropy /= divisor;
        self.title_hits /= divisor;
        self.phoenix_lexical_score /= divisor;
        self.phoenix_coverage /= divisor;
    }
}

#[derive(Clone, Copy)]
struct LexicalValues {
    max_idf: f32,
    mean_idf: f32,
    max_tf: f32,
    mean_tf: f32,
    document_length: f32,
    strongest_single_term: f32,
    contribution_entropy: f32,
    title_hits: f32,
    phoenix_lexical_score: f32,
    phoenix_coverage: f32,
}

#[derive(Debug, Serialize)]
struct MissSample {
    query_id: String,
    document_id: String,
    category: &'static str,
    query: String,
    title: String,
    query_terms: usize,
    matched_terms: usize,
    coverage: f32,
    bm25_top100: bool,
    exhaustive_rank: Option<usize>,
    heuristic_stem_overlap: bool,
}

#[derive(Debug, Serialize)]
struct Receipt {
    dataset: String,
    documents: usize,
    judged_queries: usize,
    coverage_floor: f32,
    counts: Counts,
    rank_depth: RankDepth,
    bm25_recovery_feature_means: FeatureMeans,
    both_missed_feature_means: FeatureMeans,
    bm25_recovery_lexical_census: LexicalCensus,
    both_missed_lexical_census: LexicalCensus,
    samples: Vec<MissSample>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: qps_miss_taxonomy <dataset-root> [output-json]")?,
    );
    let output = args.next().map(PathBuf::from);
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let documents = read_corpus(&root.join("corpus.jsonl"))?;
    let queries = read_queries(&root.join("queries.jsonl"))?;
    let qrels = read_qrels(&root.join("qrels").join("test.tsv"))?;
    let bm25 = build_bm25(&documents);
    let lexical = build_qps(&documents)?;
    let document_by_id = documents
        .iter()
        .enumerate()
        .map(|(index, document)| (document.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut bounded = Vec::with_capacity(TOP_K);
    let mut exhaustive = Vec::with_capacity(documents.len().min(4096));
    let mut counts = Counts::default();
    let mut rank_depth = RankDepth::default();
    let mut bm25_recovery_features = FeatureMeans::default();
    let mut both_missed_features = FeatureMeans::default();
    let mut bm25_recovery_census = LexicalCensus::default();
    let mut both_missed_census = LexicalCensus::default();
    let mut samples = Vec::with_capacity(SAMPLE_LIMIT);

    for query in queries {
        let Some(relevance) = qrels.get(&query.id) else {
            continue;
        };
        let relevant = relevance
            .iter()
            .filter_map(|(id, score)| (*score > 0).then_some(id))
            .collect::<Vec<_>>();
        if relevant.is_empty() {
            continue;
        }
        let _ = lexical.search_into(&query.text, TOP_K, &mut scratch, &mut bounded)?;
        let _ = lexical.search_exhaustive_into(
            &query.text,
            documents.len(),
            &mut scratch,
            &mut exhaustive,
        )?;
        let phoenix_top = bounded
            .iter()
            .map(|hit| hit.external_id)
            .collect::<HashSet<_>>();
        let bm25_top = bm25
            .search(&query.text, TOP_K)
            .into_iter()
            .collect::<HashSet<_>>();
        let query_terms = tokenize(&query.text);
        let query_stems = query_terms
            .iter()
            .map(|term| stem(term))
            .collect::<Vec<_>>();
        counts.relevant_documents += relevant.len() as u64;
        for document_id in relevant {
            let Some(&document_index) = document_by_id.get(document_id.as_str()) else {
                continue;
            };
            let external_id = document_index as u64;
            if phoenix_top.contains(&external_id) {
                counts.phoenix_top100 += 1;
                continue;
            }
            counts.missed_by_phoenix += 1;
            let document = &documents[document_index];
            let document_tokens = tokenize(&format!("{} {}", document.title, document.text));
            let document_terms = document_tokens.iter().cloned().collect::<HashSet<_>>();
            let mut term_frequencies = HashMap::<String, u32>::new();
            for term in &document_tokens {
                *term_frequencies.entry(term.clone()).or_default() += 1;
            }
            let title_terms = tokenize(&document.title)
                .into_iter()
                .collect::<HashSet<_>>();
            let matched_terms = query_terms
                .iter()
                .filter(|term| document_terms.contains(*term))
                .count();
            let coverage = matched_terms as f32 / query_terms.len().max(1) as f32;
            let stem_overlap = query_stems.iter().any(|term| {
                !document_terms.contains(term)
                    && document_terms
                        .iter()
                        .any(|candidate| stem(candidate) == *term)
            });
            let bm25_top100 = bm25_top.contains(&external_id);
            if bm25_top100 {
                counts.bm25_top100_any_missed += 1;
            }
            let exhaustive_hit = exhaustive
                .iter()
                .find(|hit| hit.external_id == external_id)
                .copied();
            let exhaustive_rank = exhaustive
                .iter()
                .position(|hit| hit.external_id == external_id)
                .map(|rank| rank + 1);
            let category = if matched_terms == 0 {
                counts.literal_absent += 1;
                if stem_overlap {
                    counts.stem_overlap_with_literal_absence += 1;
                }
                "literal_absent"
            } else if coverage < COVERAGE_FLOOR {
                counts.coverage_filtered += 1;
                "coverage_filtered"
            } else if exhaustive_rank.is_some() {
                counts.ranked_below_top100 += 1;
                if bm25_top100 {
                    counts.ranked_below_top100_bm25_found += 1;
                }
                if let Some(rank) = exhaustive_rank {
                    rank_depth.observe(rank);
                    if let Some(hit) = exhaustive_hit {
                        if bm25_top100 {
                            bm25_recovery_features.observe(&hit);
                            bm25_recovery_census.observe(lexical_values(
                                &bm25,
                                &query_terms,
                                &term_frequencies,
                                &title_terms,
                                document_tokens.len(),
                                &hit,
                            ));
                        } else {
                            both_missed_features.observe(&hit);
                            both_missed_census.observe(lexical_values(
                                &bm25,
                                &query_terms,
                                &term_frequencies,
                                &title_terms,
                                document_tokens.len(),
                                &hit,
                            ));
                        }
                    }
                }
                "ranked_below_top100"
            } else {
                counts.eligible_missing_from_exhaustive += 1;
                "eligible_missing_from_exhaustive"
            };
            if samples.len() < SAMPLE_LIMIT {
                samples.push(MissSample {
                    query_id: query.id.clone(),
                    document_id: document.id.clone(),
                    category,
                    query: query.text.clone(),
                    title: document.title.clone(),
                    query_terms: query_terms.len(),
                    matched_terms,
                    coverage,
                    bm25_top100,
                    exhaustive_rank,
                    heuristic_stem_overlap: stem_overlap,
                });
            }
        }
    }

    bm25_recovery_features.finish();
    both_missed_features.finish();
    bm25_recovery_census.finish();
    both_missed_census.finish();
    let receipt = Receipt {
        dataset,
        documents: documents.len(),
        judged_queries: qrels.len(),
        coverage_floor: COVERAGE_FLOOR,
        counts,
        rank_depth,
        bm25_recovery_feature_means: bm25_recovery_features,
        both_missed_feature_means: both_missed_features,
        bm25_recovery_lexical_census: bm25_recovery_census,
        both_missed_lexical_census: both_missed_census,
        samples,
    };
    let json = serde_json::to_string_pretty(&receipt)?;
    if let Some(path) = output {
        fs::write(&path, json.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    }
    println!("{}", json);
    Ok(())
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
        let mut frequencies = HashMap::<String, u32>::new();
        for token in tokens {
            *frequencies.entry(token).or_default() += 1;
        }
        for (term, frequency) in frequencies {
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

impl Bm25Index {
    fn idf(&self, term: &str) -> f32 {
        let document_frequency = self
            .postings
            .get(term)
            .map_or(0.0, |postings| postings.len() as f32);
        let total_documents = self.lengths.len() as f32;
        ((total_documents - document_frequency + 0.5) / (document_frequency + 0.5) + 1.0).ln()
    }

    fn term_contribution(&self, term: &str, frequency: u32, document_length: usize) -> f32 {
        let idf = self.idf(term);
        let denominator = frequency as f32
            + 1.2 * (0.25 + 0.75 * document_length as f32 / self.average_length.max(1.0));
        idf * (frequency as f32 * 2.2) / denominator
    }

    fn search(&self, query: &str, limit: usize) -> Vec<u64> {
        let mut scores = HashMap::<u32, f32>::new();
        for term in tokenize(query) {
            let Some(postings) = self.postings.get(&term) else {
                continue;
            };
            let df = postings.len() as f32;
            let n = self.lengths.len() as f32;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for &(document, frequency) in postings {
                let length = self.lengths[document as usize] as f32;
                let denominator =
                    frequency as f32 + 1.2 * (0.25 + 0.75 * length / self.average_length.max(1.0));
                let contribution = idf * (frequency as f32 * 2.2) / denominator;
                *scores.entry(document).or_default() += contribution;
            }
        }
        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_unstable_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(document, _)| document as u64)
            .collect()
    }
}

fn lexical_values(
    bm25: &Bm25Index,
    query_terms: &[String],
    term_frequencies: &HashMap<String, u32>,
    title_terms: &HashSet<String>,
    document_length: usize,
    hit: &phoenix_lexical_qps::SearchHit,
) -> LexicalValues {
    let mut idfs = Vec::new();
    let mut term_frequencies_present = Vec::new();
    let mut contributions = Vec::new();
    let mut title_hits = 0_u32;
    for term in query_terms.iter().collect::<HashSet<_>>() {
        let frequency = term_frequencies.get(term).copied().unwrap_or(0);
        if frequency == 0 {
            continue;
        }
        let idf = bm25.idf(term);
        let contribution = bm25.term_contribution(term, frequency, document_length);
        idfs.push(idf);
        term_frequencies_present.push(frequency as f32);
        contributions.push(contribution);
        if title_terms.contains(term.as_str()) {
            title_hits += 1;
        }
    }
    let max_idf = idfs.iter().copied().fold(0.0_f32, f32::max);
    let mean_idf = mean(&idfs);
    let max_tf = term_frequencies_present
        .iter()
        .copied()
        .fold(0.0_f32, f32::max);
    let mean_tf = mean(&term_frequencies_present);
    let strongest_single_term = contributions.iter().copied().fold(0.0_f32, f32::max);
    let contribution_entropy = normalized_entropy(&contributions);
    LexicalValues {
        max_idf,
        mean_idf,
        max_tf,
        mean_tf,
        document_length: document_length as f32,
        strongest_single_term,
        contribution_entropy,
        title_hits: title_hits as f32,
        phoenix_lexical_score: hit.lexical_score,
        phoenix_coverage: hit.coverage,
    }
}

fn mean(values: &[f32]) -> f32 {
    values.iter().copied().sum::<f32>() / values.len().max(1) as f32
}

fn normalized_entropy(values: &[f32]) -> f32 {
    if values.len() <= 1 {
        return 0.0;
    }
    let total = values.iter().copied().sum::<f32>();
    if total <= 0.0 {
        return 0.0;
    }
    let entropy = values
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .map(|value| {
            let probability = value / total;
            -probability * probability.ln()
        })
        .sum::<f32>();
    (entropy / (values.len() as f32).ln()).clamp(0.0, 1.0)
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

fn stem(token: &str) -> String {
    let mut result = token.to_owned();
    for suffix in ["ingly", "edly", "ing", "ed", "es", "ly", "s"] {
        if result.len() > suffix.len() + 2 && result.ends_with(suffix) {
            result.truncate(result.len() - suffix.len());
            break;
        }
    }
    result
}
