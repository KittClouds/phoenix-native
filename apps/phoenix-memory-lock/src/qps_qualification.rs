use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use phoenix_lexical_qps::{
    DocumentInput, Expansion, FieldConfig, IndexStats, QpsBuilder, QpsConfig, QpsIndex, QueryGroup,
    SearchHit, SearchReceipt, SearchScratch, MAXIMUM_QUERY_GROUPS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod concurrency;
mod workload_concurrency;

pub use concurrency::run as run_concurrent;
pub use workload_concurrency::run as run_workload_concurrent;

const CONTRACT: &str = "phoenix.memory.qps-mixed-qualification/v1";
const ENGINE: &str = "phoenix-qps-v2.01-standalone";
const TOP_K: usize = 10;
const FIELDS: [FieldConfig; 2] = [
    FieldConfig::new("title", 2.5, 0.35, 0.35),
    FieldConfig::new("body", 1.0, 0.75, 0.10),
];

pub fn run(suite_path: &Path, repetitions: usize) -> Result<QualificationReceipt> {
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    let suite_bytes = fs::read(suite_path)
        .with_context(|| format!("read qualification suite {}", suite_path.display()))?;
    let suite: QualificationSuite = serde_json::from_slice(&suite_bytes)
        .with_context(|| format!("decode qualification suite {}", suite_path.display()))?;
    validate_suite(&suite)?;

    let build_started = Instant::now();
    let index = build_index(&suite)?;
    let build_nanos = elapsed_nanos(build_started);
    let index_stats = index.stats();
    let mut all_latency = Vec::with_capacity(suite.queries.len() * repetitions);
    let mut shape_samples = ShapeSamples::default();
    let mut source_samples = SourceSamples::default();
    let mut metrics = QualityAccumulator::default();
    let mut scratch_growths = 0_u64;
    let mut determinism_failures = 0_u64;
    let mut maximum_candidates = 0_u32;
    let mut maximum_reranked = 0_u32;
    let mut maximum_query_groups = 0_u16;
    let document_by_id = suite
        .documents
        .iter()
        .enumerate()
        .map(|(index, document)| (document.stable_id.as_str(), index))
        .collect::<HashMap<_, _>>();

    let mut scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::<SearchHit>::with_capacity(TOP_K);
    for query in &suite.queries {
        let expected_index = query
            .expected
            .as_deref()
            .map(|expected| document_by_id[expected]);
        let source = expected_index.map(|index| suite.documents[index].source);
        let prepared = PreparedQuery::new(query);
        let group_views = prepared.group_views();
        let warm_receipt = prepared.search(&group_views, &index, &mut scratch, &mut hits)?;
        let expected_ranking = hit_documents(&hits);
        metrics.observe(query.shape, expected_index, &hits);
        maximum_candidates = maximum_candidates.max(warm_receipt.candidates);
        maximum_reranked = maximum_reranked.max(warm_receipt.reranked_candidates);
        maximum_query_groups = maximum_query_groups.max(warm_receipt.query_groups);

        for _ in 0..repetitions {
            let started = Instant::now();
            let receipt = prepared.search(&group_views, &index, &mut scratch, &mut hits)?;
            let nanos = elapsed_nanos(started);
            all_latency.push(nanos);
            shape_samples.observe(query.shape, nanos);
            if let Some(source) = source {
                source_samples.observe(source, nanos);
            }
            scratch_growths += u64::from(receipt.allocations_grew);
            maximum_candidates = maximum_candidates.max(receipt.candidates);
            maximum_reranked = maximum_reranked.max(receipt.reranked_candidates);
            maximum_query_groups = maximum_query_groups.max(receipt.query_groups);
            if !ranking_matches(&hits, &expected_ranking) {
                determinism_failures += 1;
            }
        }
    }

    let quality = metrics.finish();
    let latency = LatencyPercentiles::from_samples(&mut all_latency);
    let gates = QualificationGates {
        hit_at_10_at_least_098: quality.hit_at_10 >= 0.98,
        mrr_at_least_090: quality.mean_reciprocal_rank >= 0.90,
        top_1_at_least_085: quality.top_1_accuracy >= 0.85,
        no_result_accuracy_is_one: quality.no_result_accuracy == 1.0,
        p99_at_most_one_millisecond: latency.p99 <= 1_000_000,
        zero_warm_capacity_growth: scratch_growths == 0,
        deterministic_rankings: determinism_failures == 0,
        all_requested_shapes_present: metrics.has_all_shapes(),
        documents_and_conversations_present: suite
            .documents
            .iter()
            .any(|document| document.source == SourceKind::Document)
            && suite
                .documents
                .iter()
                .any(|document| document.source == SourceKind::Conversation),
    };
    let qualified_for_standalone_use = gates.all_pass();
    let binary = binary_identity()?;

    Ok(QualificationReceipt {
        contract: CONTRACT,
        engine: ENGINE,
        authority: "standalone-qps-no-bm25-runtime-arm",
        suite_path: suite_path.display().to_string(),
        suite_bytes: suite_bytes.len(),
        suite_sha256: hex_sha256(&suite_bytes),
        binary,
        repetitions,
        top_k: TOP_K,
        documents: suite.documents.len(),
        document_sources: count_sources(&suite.documents),
        queries: suite.queries.len(),
        build_nanos,
        index: index_stats.into(),
        quality,
        latency,
        latency_by_shape: shape_samples.finish(),
        latency_by_source: source_samples.finish(),
        scratch_capacity_growths: scratch_growths,
        determinism_failures,
        maximum_candidates,
        maximum_reranked,
        maximum_query_groups,
        gates,
        qualified_for_standalone_use,
    })
}

fn build_index(suite: &QualificationSuite) -> Result<QpsIndex> {
    let config = QpsConfig {
        maximum_candidate_pool: 160,
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(Vec::from(FIELDS).into_boxed_slice(), config)?;
    for (index, document) in suite.documents.iter().enumerate() {
        builder.insert(DocumentInput {
            external_id: index as u64,
            fields: &[document.title.as_str(), document.body.as_str()],
        })?;
    }
    builder.build().map_err(Into::into)
}

fn validate_suite(suite: &QualificationSuite) -> Result<()> {
    if suite.contract != CONTRACT {
        bail!("unsupported qualification contract {}", suite.contract);
    }
    if suite.documents.is_empty() || suite.queries.is_empty() {
        bail!("qualification suite requires documents and queries");
    }
    let mut documents = HashSet::with_capacity(suite.documents.len());
    for document in &suite.documents {
        if document.stable_id.trim().is_empty()
            || document.title.trim().is_empty()
            || document.body.trim().is_empty()
            || !documents.insert(document.stable_id.as_str())
        {
            bail!("qualification suite has an invalid or duplicate document");
        }
    }
    let mut queries = HashSet::with_capacity(suite.queries.len());
    for query in &suite.queries {
        if query.stable_id.trim().is_empty() || !queries.insert(query.stable_id.as_str()) {
            bail!("qualification suite has an invalid or duplicate query");
        }
        if let Some(expected) = &query.expected {
            if !documents.contains(expected.as_str()) {
                bail!("query {} expects an unknown document", query.stable_id);
            }
        } else if query.shape != QueryShape::NoResult {
            bail!("only no-result queries may omit an expected document");
        }
        match query.shape {
            QueryShape::Fuzzy if query.groups.is_empty() => {
                bail!("fuzzy query {} requires expansion groups", query.stable_id)
            }
            QueryShape::NoResult if query.query.trim().is_empty() => {
                bail!(
                    "no-result query {} must still contain terms",
                    query.stable_id
                )
            }
            _ if query.query.trim().is_empty() && query.groups.is_empty() => {
                bail!("query {} has no search input", query.stable_id)
            }
            _ => {}
        }
        for group in &query.groups {
            if group.is_empty() || group.len() > 16 {
                bail!("query {} has an invalid expansion group", query.stable_id);
            }
        }
    }
    Ok(())
}

struct PreparedQuery<'a> {
    query: &'a QualificationQuery,
    expansions: Vec<Vec<Expansion<'a>>>,
}

impl<'a> PreparedQuery<'a> {
    fn new(query: &'a QualificationQuery) -> Self {
        let expansions = query
            .groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|expansion| Expansion {
                        term: expansion.term.as_str(),
                        quality: expansion.quality,
                    })
                    .collect()
            })
            .collect();
        Self { query, expansions }
    }

    fn group_views(&self) -> Vec<QueryGroup<'_>> {
        self.expansions
            .iter()
            .map(|expansions| QueryGroup { expansions })
            .collect()
    }

    fn search(
        &self,
        groups: &[QueryGroup<'_>],
        index: &QpsIndex,
        scratch: &mut SearchScratch,
        hits: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt> {
        if self.expansions.is_empty() {
            return index
                .search_into(&self.query.query, TOP_K, scratch, hits)
                .map_err(Into::into);
        }
        index
            .search_groups_into(groups, TOP_K, scratch, hits)
            .map_err(Into::into)
    }
}

fn hit_documents(hits: &[SearchHit]) -> Vec<u64> {
    hits.iter().map(|hit| hit.external_id).collect()
}

fn ranking_matches(hits: &[SearchHit], expected: &[u64]) -> bool {
    hits.len() == expected.len()
        && hits
            .iter()
            .zip(expected)
            .all(|(hit, expected)| hit.external_id == *expected)
}

#[derive(Default)]
struct QualityAccumulator {
    answerable: usize,
    hits_at_10: usize,
    reciprocal_rank: f64,
    top_1: usize,
    no_result: usize,
    correct_no_result: usize,
    shapes: [usize; 4],
}

impl QualityAccumulator {
    fn observe(&mut self, shape: QueryShape, expected: Option<usize>, hits: &[SearchHit]) {
        self.shapes[shape.index()] += 1;
        let Some(expected) = expected else {
            self.no_result += 1;
            self.correct_no_result += usize::from(hits.is_empty());
            return;
        };
        self.answerable += 1;
        if let Some(rank) = hits
            .iter()
            .position(|hit| hit.external_id as usize == expected)
        {
            self.hits_at_10 += 1;
            self.reciprocal_rank += 1.0 / (rank + 1) as f64;
            self.top_1 += usize::from(rank == 0);
        }
    }

    fn has_all_shapes(&self) -> bool {
        self.shapes.iter().all(|count| *count > 0)
    }

    fn finish(&self) -> QualityReceipt {
        QualityReceipt {
            answerable_queries: self.answerable,
            no_result_queries: self.no_result,
            hit_at_10: ratio(self.hits_at_10, self.answerable),
            mean_reciprocal_rank: self.reciprocal_rank / self.answerable.max(1) as f64,
            top_1_accuracy: ratio(self.top_1, self.answerable),
            no_result_accuracy: ratio(self.correct_no_result, self.no_result),
            queries_by_shape: QueryShapeCounts {
                ordinary: self.shapes[QueryShape::Ordinary.index()],
                phrase: self.shapes[QueryShape::Phrase.index()],
                fuzzy: self.shapes[QueryShape::Fuzzy.index()],
                no_result: self.shapes[QueryShape::NoResult.index()],
            },
        }
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    numerator as f64 / denominator.max(1) as f64
}

#[derive(Default)]
struct ShapeSamples([Vec<u64>; 4]);

impl ShapeSamples {
    fn observe(&mut self, shape: QueryShape, nanos: u64) {
        self.0[shape.index()].push(nanos);
    }

    fn finish(self) -> Vec<LatencyBucket> {
        self.0
            .into_iter()
            .enumerate()
            .filter_map(|(index, mut samples)| {
                (!samples.is_empty()).then(|| LatencyBucket {
                    bucket: QueryShape::from_index(index).label(),
                    samples: samples.len(),
                    latency: LatencyPercentiles::from_samples(&mut samples),
                })
            })
            .collect()
    }
}

#[derive(Default)]
struct SourceSamples([Vec<u64>; 2]);

impl SourceSamples {
    fn observe(&mut self, source: SourceKind, nanos: u64) {
        self.0[source.index()].push(nanos);
    }

    fn finish(self) -> Vec<LatencyBucket> {
        self.0
            .into_iter()
            .enumerate()
            .filter_map(|(index, mut samples)| {
                (!samples.is_empty()).then(|| LatencyBucket {
                    bucket: SourceKind::from_index(index).label(),
                    samples: samples.len(),
                    latency: LatencyPercentiles::from_samples(&mut samples),
                })
            })
            .collect()
    }
}

fn count_sources(documents: &[QualificationDocument]) -> SourceCounts {
    SourceCounts {
        documents: documents
            .iter()
            .filter(|document| document.source == SourceKind::Document)
            .count(),
        conversations: documents
            .iter()
            .filter(|document| document.source == SourceKind::Conversation)
            .count(),
    }
}

fn binary_identity() -> Result<BinaryIdentity> {
    let path = std::env::current_exe().context("resolve qualification binary")?;
    let bytes =
        fs::read(&path).with_context(|| format!("read qualification binary {}", path.display()))?;
    Ok(BinaryIdentity {
        path: path.display().to_string(),
        bytes: bytes.len(),
        sha256: hex_sha256(&bytes),
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    samples
        .get((samples.len().saturating_sub(1) * percentile) / 100)
        .copied()
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct QualificationSuite {
    contract: String,
    documents: Vec<QualificationDocument>,
    queries: Vec<QualificationQuery>,
}

#[derive(Debug, Deserialize)]
struct QualificationDocument {
    stable_id: String,
    source: SourceKind,
    title: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct QualificationQuery {
    stable_id: String,
    shape: QueryShape,
    query: String,
    #[serde(default)]
    groups: Vec<Vec<QualificationExpansion>>,
    expected: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QualificationExpansion {
    term: String,
    quality: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum QueryShape {
    Ordinary,
    Phrase,
    Fuzzy,
    NoResult,
}

impl QueryShape {
    const fn index(self) -> usize {
        self as usize
    }

    const fn from_index(index: usize) -> Self {
        [Self::Ordinary, Self::Phrase, Self::Fuzzy, Self::NoResult][index]
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Phrase => "phrase",
            Self::Fuzzy => "fuzzy",
            Self::NoResult => "no_result",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
    Document,
    Conversation,
}

impl SourceKind {
    const fn index(self) -> usize {
        self as usize
    }

    const fn from_index(index: usize) -> Self {
        [Self::Document, Self::Conversation][index]
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Conversation => "conversation",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QualificationReceipt {
    contract: &'static str,
    engine: &'static str,
    authority: &'static str,
    suite_path: String,
    suite_bytes: usize,
    suite_sha256: String,
    binary: BinaryIdentity,
    repetitions: usize,
    top_k: usize,
    documents: usize,
    document_sources: SourceCounts,
    queries: usize,
    build_nanos: u64,
    index: IndexStatsReceipt,
    quality: QualityReceipt,
    latency: LatencyPercentiles,
    latency_by_shape: Vec<LatencyBucket>,
    latency_by_source: Vec<LatencyBucket>,
    scratch_capacity_growths: u64,
    determinism_failures: u64,
    maximum_candidates: u32,
    maximum_reranked: u32,
    maximum_query_groups: u16,
    gates: QualificationGates,
    qualified_for_standalone_use: bool,
}

#[derive(Debug, Serialize)]
struct BinaryIdentity {
    path: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct SourceCounts {
    documents: usize,
    conversations: usize,
}

#[derive(Debug, Serialize)]
struct IndexStatsReceipt {
    documents: usize,
    terms: usize,
    posting_rows: usize,
    positions: usize,
    estimated_bytes: usize,
}

impl From<IndexStats> for IndexStatsReceipt {
    fn from(stats: IndexStats) -> Self {
        Self {
            documents: stats.documents,
            terms: stats.terms,
            posting_rows: stats.posting_rows,
            positions: stats.positions,
            estimated_bytes: stats.estimated_bytes,
        }
    }
}

#[derive(Debug, Serialize)]
struct QualityReceipt {
    answerable_queries: usize,
    no_result_queries: usize,
    hit_at_10: f64,
    mean_reciprocal_rank: f64,
    top_1_accuracy: f64,
    no_result_accuracy: f64,
    queries_by_shape: QueryShapeCounts,
}

#[derive(Debug, Serialize)]
struct QueryShapeCounts {
    ordinary: usize,
    phrase: usize,
    fuzzy: usize,
    no_result: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct LatencyPercentiles {
    median: u64,
    p95: u64,
    p99: u64,
    max: u64,
}

impl LatencyPercentiles {
    fn from_samples(samples: &mut [u64]) -> Self {
        samples.sort_unstable();
        Self {
            median: percentile(samples, 50),
            p95: percentile(samples, 95),
            p99: percentile(samples, 99),
            max: samples.last().copied().unwrap_or(0),
        }
    }
}

#[derive(Debug, Serialize)]
struct LatencyBucket {
    bucket: &'static str,
    samples: usize,
    latency: LatencyPercentiles,
}

#[derive(Debug, Serialize)]
struct QualificationGates {
    hit_at_10_at_least_098: bool,
    mrr_at_least_090: bool,
    top_1_at_least_085: bool,
    no_result_accuracy_is_one: bool,
    p99_at_most_one_millisecond: bool,
    zero_warm_capacity_growth: bool,
    deterministic_rankings: bool,
    all_requested_shapes_present: bool,
    documents_and_conversations_present: bool,
}

impl QualificationGates {
    fn all_pass(&self) -> bool {
        self.hit_at_10_at_least_098
            && self.mrr_at_least_090
            && self.top_1_at_least_085
            && self.no_result_accuracy_is_one
            && self.p99_at_most_one_millisecond
            && self.zero_warm_capacity_growth
            && self.deterministic_rankings
            && self.all_requested_shapes_present
            && self.documents_and_conversations_present
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use std::path::Path;

    #[test]
    fn frozen_mixed_suite_qualifies_without_a_second_search_arm() {
        let suite = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../memory-lock/qps-mixed-qualification-v1.json");
        let receipt = run(&suite, 4).expect("run mixed QPS qualification");
        assert_eq!(receipt.authority, "standalone-qps-no-bm25-runtime-arm");
        assert!(receipt.qualified_for_standalone_use);
        assert_eq!(receipt.quality.hit_at_10, 1.0);
        assert_eq!(receipt.quality.mean_reciprocal_rank, 1.0);
        assert_eq!(receipt.quality.no_result_accuracy, 1.0);
        assert_eq!(receipt.scratch_capacity_growths, 0);
        assert_eq!(receipt.determinism_failures, 0);
    }
}
