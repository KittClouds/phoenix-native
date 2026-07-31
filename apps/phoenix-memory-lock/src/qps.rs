use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, SearchHit, SearchScratch,
    MAXIMUM_QUERY_GROUPS,
};
use serde::Serialize;

use crate::artifact::{read_artifact, write_artifact};
use crate::baseline::verify_binding;
use crate::model::{
    FreezeManifest, RankedSession, RetrievalArtifact, RetrievalCase, WorkloadArtifact,
    RETRIEVAL_CONTRACT, RETRIEVAL_MAGIC, WORKLOAD_CONTRACT, WORKLOAD_MAGIC,
};

const ENGINE: &str = "phoenix-qps-v2.01-shadow";
const PATH_ID: &str = "qps/v2.01/shadow";
const BM25_COMPARATOR: &str = "precomputed-impact-csc-equivalent/v1";
const WARNING_NANOS: i64 = 100_000;
const PAIRED_HARD_NANOS: i64 = 150_000;
const FIELD: FieldConfig = FieldConfig::new("session", 1.0, 0.75, 0.0);
const K1: f64 = 1.2;
const B: f64 = 0.75;

pub fn run(
    manifest: &FreezeManifest,
    workload_path: &Path,
    output_path: &Path,
    top_k: usize,
    repetitions: usize,
    profile: QpsAblationProfile,
) -> Result<QpsShadowRunReceipt> {
    if top_k == 0 || top_k > 1_024 {
        bail!("top-k must be in 1..=1024");
    }
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    if workload.contract != WORKLOAD_CONTRACT {
        bail!("unsupported workload contract {}", workload.contract);
    }
    verify_binding(manifest, &workload.source)?;

    let mut qps_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut bm25_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut paired_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut accumulation = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut selection = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut coherence = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut ordering = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut output_cases = Vec::with_capacity(workload.cases.len());
    let mut shape_counts = QueryShapeCounts::default();
    let mut scale_samples = ScaleSamples::default();
    let mut query_group_samples = QueryGroupSamples::default();
    let mut scratch_growths = 0_u64;
    let mut max_candidates = 0_u32;
    let mut max_reranked = 0_u32;
    let mut max_query_groups = 0_u16;
    let mut posting_rows_visited = 0_u64;
    let mut position_values_visited = 0_u64;
    let mut max_posting_rows_visited = 0_u32;
    let mut max_position_values_visited = 0_u32;

    for case in &workload.cases {
        shape_counts.observe(&case.question);
        let documents = case
            .sessions
            .iter()
            .map(|session| {
                session
                    .turns
                    .iter()
                    .map(|turn| turn.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>();
        let qps = build_qps(&documents, profile)?;
        let scale_bucket = ScaleBucket::from_documents(documents.len());
        let bm25 = Bm25TurboIndex::build(&documents);
        let query_terms = unique_tokens(&case.question);
        let mut qps_scratch = SearchScratch::with_document_capacity(documents.len(), 32);
        let mut qps_hits = Vec::<SearchHit>::with_capacity(top_k);
        let mut bm25_scratch = Bm25TurboScratch::with_document_capacity(documents.len());
        let mut bm25_hits = Vec::with_capacity(documents.len());

        // Warm caller-owned buffers and instruction/data paths before sampling.
        let warm_receipt =
            qps.search_into(&case.question, top_k, &mut qps_scratch, &mut qps_hits)?;
        let query_group_bucket = QueryGroupBucket::from_groups(warm_receipt.query_groups);
        bm25.search_into(&query_terms, top_k, &mut bm25_scratch, &mut bm25_hits);

        for repetition in 0..repetitions {
            let (bm25_nanos, qps_nanos, receipt) = if repetition & 1 == 0 {
                let bm25_started = Instant::now();
                bm25.search_into(&query_terms, top_k, &mut bm25_scratch, &mut bm25_hits);
                let bm25_nanos = elapsed_nanos(bm25_started);
                let qps_started = Instant::now();
                let receipt =
                    qps.search_into(&case.question, top_k, &mut qps_scratch, &mut qps_hits)?;
                (bm25_nanos, elapsed_nanos(qps_started), receipt)
            } else {
                let qps_started = Instant::now();
                let receipt =
                    qps.search_into(&case.question, top_k, &mut qps_scratch, &mut qps_hits)?;
                let qps_nanos = elapsed_nanos(qps_started);
                let bm25_started = Instant::now();
                bm25.search_into(&query_terms, top_k, &mut bm25_scratch, &mut bm25_hits);
                (elapsed_nanos(bm25_started), qps_nanos, receipt)
            };
            qps_samples.push(qps_nanos);
            scale_samples.observe(scale_bucket, qps_nanos);
            bm25_samples.push(bm25_nanos);
            let paired_nanos = saturating_i64(qps_nanos) - saturating_i64(bm25_nanos);
            paired_samples.push(paired_nanos);
            query_group_samples.observe(query_group_bucket, qps_nanos, bm25_nanos, paired_nanos);
            accumulation.push(receipt.stages.accumulation);
            selection.push(receipt.stages.selection);
            coherence.push(receipt.stages.coherence);
            ordering.push(receipt.stages.ordering);
            scratch_growths += u64::from(receipt.allocations_grew);
            max_candidates = max_candidates.max(receipt.candidates);
            max_reranked = max_reranked.max(receipt.reranked_candidates);
            max_query_groups = max_query_groups.max(receipt.query_groups);
            posting_rows_visited =
                posting_rows_visited.saturating_add(u64::from(receipt.posting_rows_visited));
            position_values_visited =
                position_values_visited.saturating_add(u64::from(receipt.position_values_visited));
            max_posting_rows_visited = max_posting_rows_visited.max(receipt.posting_rows_visited);
            max_position_values_visited =
                max_position_values_visited.max(receipt.position_values_visited);
        }
        let ranked_sessions = qps_hits
            .iter()
            .map(|hit| {
                let index = usize::try_from(hit.external_id).context("QPS document ID overflow")?;
                let stable_id = case
                    .sessions
                    .get(index)
                    .context("QPS returned an unknown session")?
                    .stable_id
                    .clone();
                Ok(RankedSession {
                    stable_id,
                    score_bits: f64::from(hit.score).to_bits(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        output_cases.push(RetrievalCase {
            question_id: case.question_id.clone(),
            ranked_sessions,
        });
    }

    let artifact = RetrievalArtifact {
        contract: RETRIEVAL_CONTRACT.to_owned(),
        source: workload.source,
        engine: profile.engine().to_owned(),
        top_k: u32::try_from(top_k).context("top-k overflow")?,
        cases: output_cases,
    };
    write_artifact(output_path, RETRIEVAL_MAGIC, &artifact)?;

    let qps_latency = Percentiles::from_u64(&mut qps_samples);
    let bm25_latency = Percentiles::from_u64(&mut bm25_samples);
    let paired_overhead = SignedPercentiles::from_i64(&mut paired_samples);
    let aggregate_p99_overhead = saturating_i64(qps_latency.p99) - saturating_i64(bm25_latency.p99);
    Ok(QpsShadowRunReceipt {
        contract: "phoenix.memory.qps-shadow-lock/v1",
        path_id: PATH_ID,
        profile: profile.as_str(),
        authority: "none-shadow-artifact-only",
        quality_evaluated: false,
        concurrency_evaluated: false,
        execution_model: "single-owner-coordinator",
        bm25_comparator: BM25_COMPARATOR,
        cases: workload.cases.len(),
        repetitions,
        top_k,
        rerank_pool: 160,
        output_path: output_path.display().to_string(),
        qps_latency,
        bm25_latency,
        paired_overhead,
        aggregate_p99_overhead_nanos: aggregate_p99_overhead,
        accumulation: Percentiles::from_u64(&mut accumulation),
        selection: Percentiles::from_u64(&mut selection),
        coherence: Percentiles::from_u64(&mut coherence),
        ordering: Percentiles::from_u64(&mut ordering),
        shape_counts,
        corpus_scale: scale_samples.finish(),
        query_group_scale: query_group_samples.finish(),
        scratch_capacity_growths: scratch_growths,
        max_candidates,
        max_reranked,
        posting_rows_visited,
        position_values_visited,
        max_posting_rows_visited,
        max_position_values_visited,
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        max_query_groups_observed: max_query_groups,
        warning_gate_passed: aggregate_p99_overhead <= WARNING_NANOS,
        paired_hard_gate_passed: paired_overhead.p99 <= PAIRED_HARD_NANOS,
    })
}

pub(crate) fn build_qps(
    documents: &[String],
    profile: QpsAblationProfile,
) -> Result<phoenix_lexical_qps::QpsIndex> {
    let mut config = QpsConfig {
        maximum_candidate_pool: 160,
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        ..QpsConfig::default()
    };
    profile.apply(&mut config);
    let mut builder = QpsBuilder::new(Vec::from([FIELD]).into_boxed_slice(), config)?;
    for (index, text) in documents.iter().enumerate() {
        let fields = [text.as_str()];
        builder.insert(DocumentInput {
            external_id: index as u64,
            fields: &fields,
        })?;
    }
    builder.build().map_err(Into::into)
}

struct Bm25TurboIndex {
    terms: Box<[u64]>,
    column_offsets: Box<[u32]>,
    impacts: Box<[Bm25Impact]>,
}

struct Bm25BuildDocument {
    term_count: u32,
    counts: Vec<(u64, u32)>,
}

#[derive(Clone, Copy)]
struct Bm25Impact {
    document: u32,
    score: f64,
}

struct Bm25TurboScratch {
    scores: Vec<f64>,
    stamps: Vec<u32>,
    touched: Vec<u32>,
    epoch: u32,
}

impl Bm25TurboScratch {
    fn with_document_capacity(documents: usize) -> Self {
        Self {
            scores: vec![0.0; documents],
            stamps: vec![0; documents],
            touched: Vec::with_capacity(documents),
            epoch: 0,
        }
    }

    fn begin_query(&mut self) {
        self.touched.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamps.fill(0);
            self.epoch = 1;
        }
    }
}

impl Bm25TurboIndex {
    fn build(documents: &[String]) -> Self {
        let mut output = Vec::with_capacity(documents.len());
        let mut document_frequency = HashMap::<u64, u32>::new();
        let mut total_terms = 0_u64;
        for text in documents {
            let mut tokens = Vec::new();
            append_tokens(text, &mut tokens);
            tokens.sort_unstable();
            let counts = compress_counts(&tokens);
            for &(term, _) in &counts {
                *document_frequency.entry(term).or_insert(0) += 1;
            }
            total_terms += tokens.len() as u64;
            output.push(Bm25BuildDocument {
                term_count: tokens.len() as u32,
                counts,
            });
        }
        let average_length = if output.is_empty() {
            1.0
        } else {
            total_terms as f64 / output.len() as f64
        };
        let mut columns = HashMap::<u64, Vec<Bm25Impact>>::with_capacity(document_frequency.len());
        let document_count = output.len() as f64;
        for (document, record) in output.iter().enumerate() {
            let length_ratio = f64::from(record.term_count) / average_length;
            for &(term, frequency) in &record.counts {
                let df = f64::from(document_frequency[&term]);
                let idf = (1.0 + (document_count - df + 0.5) / (df + 0.5)).ln();
                let tf = f64::from(frequency);
                let score = idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * length_ratio));
                columns.entry(term).or_default().push(Bm25Impact {
                    document: document as u32,
                    score,
                });
            }
        }
        let mut columns = columns.into_iter().collect::<Vec<_>>();
        columns.sort_unstable_by_key(|(term, _)| *term);
        let mut terms = Vec::with_capacity(columns.len());
        let mut column_offsets = Vec::with_capacity(columns.len() + 1);
        let impact_count = columns.iter().map(|(_, rows)| rows.len()).sum();
        let mut impacts = Vec::with_capacity(impact_count);
        column_offsets.push(0);
        for (term, rows) in columns {
            terms.push(term);
            impacts.extend(rows);
            column_offsets.push(impacts.len() as u32);
        }
        Self {
            terms: terms.into_boxed_slice(),
            column_offsets: column_offsets.into_boxed_slice(),
            impacts: impacts.into_boxed_slice(),
        }
    }

    fn search_into(
        &self,
        query: &[u64],
        top_k: usize,
        scratch: &mut Bm25TurboScratch,
        output: &mut Vec<(usize, f64)>,
    ) {
        scratch.begin_query();
        output.clear();
        for term in query {
            let Ok(column) = self.terms.binary_search(term) else {
                continue;
            };
            let start = self.column_offsets[column] as usize;
            let end = self.column_offsets[column + 1] as usize;
            for impact in &self.impacts[start..end] {
                let document = impact.document as usize;
                if scratch.stamps[document] != scratch.epoch {
                    scratch.stamps[document] = scratch.epoch;
                    scratch.scores[document] = 0.0;
                    scratch.touched.push(impact.document);
                }
                scratch.scores[document] += impact.score;
            }
        }
        output.extend(scratch.touched.iter().map(|&document| {
            let index = document as usize;
            (index, scratch.scores[index])
        }));
        output.sort_unstable_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        output.truncate(top_k.min(output.len()));
    }
}

fn unique_tokens(text: &str) -> Vec<u64> {
    let mut tokens = Vec::new();
    append_tokens(text, &mut tokens);
    tokens.sort_unstable();
    tokens.dedup();
    tokens
}

fn append_tokens(text: &str, output: &mut Vec<u64>) {
    for token in text
        .as_bytes()
        .split(|byte| !byte.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for &byte in token {
            hash ^= u64::from(byte.to_ascii_lowercase());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        output.push(hash);
    }
}

fn compress_counts(tokens: &[u64]) -> Vec<(u64, u32)> {
    let mut counts = Vec::with_capacity(tokens.len());
    for &token in tokens {
        if let Some((last, count)) = counts.last_mut() {
            if *last == token {
                *count += 1;
                continue;
            }
        }
        counts.push((token, 1));
    }
    counts
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Percentiles {
    pub median: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

impl Percentiles {
    fn from_u64(samples: &mut [u64]) -> Self {
        samples.sort_unstable();
        Self {
            median: percentile(samples, 50),
            p95: percentile(samples, 95),
            p99: percentile(samples, 99),
            max: samples.last().copied().unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct SignedPercentiles {
    pub median: i64,
    pub p95: i64,
    pub p99: i64,
    pub max: i64,
}

impl SignedPercentiles {
    fn from_i64(samples: &mut [i64]) -> Self {
        samples.sort_unstable();
        Self {
            median: signed_percentile(samples, 50),
            p95: signed_percentile(samples, 95),
            p99: signed_percentile(samples, 99),
            max: samples.last().copied().unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct QueryShapeCounts {
    pub single_token: u64,
    pub multi_token: u64,
    pub phrase_like: u64,
    pub no_result: u64,
}

impl QueryShapeCounts {
    fn observe(&mut self, query: &str) {
        let count = query.split_ascii_whitespace().take(3).count();
        match count {
            0 => self.no_result += 1,
            1 => self.single_token += 1,
            _ if query.contains('"') => self.phrase_like += 1,
            _ => self.multi_token += 1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QpsShadowRunReceipt {
    pub contract: &'static str,
    pub path_id: &'static str,
    pub profile: &'static str,
    pub authority: &'static str,
    pub quality_evaluated: bool,
    pub concurrency_evaluated: bool,
    pub execution_model: &'static str,
    pub bm25_comparator: &'static str,
    pub cases: usize,
    pub repetitions: usize,
    pub top_k: usize,
    pub rerank_pool: usize,
    pub output_path: String,
    pub qps_latency: Percentiles,
    pub bm25_latency: Percentiles,
    pub paired_overhead: SignedPercentiles,
    pub aggregate_p99_overhead_nanos: i64,
    pub accumulation: Percentiles,
    pub selection: Percentiles,
    pub coherence: Percentiles,
    pub ordering: Percentiles,
    pub shape_counts: QueryShapeCounts,
    pub corpus_scale: Vec<ScaleBucketReceipt>,
    pub query_group_scale: Vec<QueryGroupBucketReceipt>,
    pub scratch_capacity_growths: u64,
    pub max_candidates: u32,
    pub max_reranked: u32,
    pub posting_rows_visited: u64,
    pub position_values_visited: u64,
    pub max_posting_rows_visited: u32,
    pub max_position_values_visited: u32,
    pub maximum_query_groups: usize,
    pub max_query_groups_observed: u16,
    pub warning_gate_passed: bool,
    pub paired_hard_gate_passed: bool,
}

#[derive(Clone, Copy)]
enum ScaleBucket {
    Tiny,
    Small,
    Medium,
    Large,
}

impl ScaleBucket {
    const fn from_documents(documents: usize) -> Self {
        match documents {
            0..=32 => Self::Tiny,
            33..=128 => Self::Small,
            129..=512 => Self::Medium,
            _ => Self::Large,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default)]
struct ScaleSamples {
    samples: [Vec<u64>; 4],
}

impl ScaleSamples {
    fn observe(&mut self, bucket: ScaleBucket, nanos: u64) {
        self.samples[bucket.index()].push(nanos);
    }

    fn finish(self) -> Vec<ScaleBucketReceipt> {
        let labels = ["0-32", "33-128", "129-512", "513+"];
        self.samples
            .into_iter()
            .zip(labels)
            .filter_map(|(mut samples, documents)| {
                (!samples.is_empty()).then(|| ScaleBucketReceipt {
                    documents,
                    samples: samples.len(),
                    latency: Percentiles::from_u64(&mut samples),
                })
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct ScaleBucketReceipt {
    pub documents: &'static str,
    pub samples: usize,
    pub latency: Percentiles,
}

#[derive(Clone, Copy)]
enum QueryGroupBucket {
    One,
    TwoToFour,
    FiveToEight,
    NineToSixteen,
    SeventeenToThirtyTwo,
    ThirtyThreeToSixtyFour,
    SixtyFiveToOneTwentyEight,
}

impl QueryGroupBucket {
    const fn from_groups(groups: u16) -> Self {
        match groups {
            0..=1 => Self::One,
            2..=4 => Self::TwoToFour,
            5..=8 => Self::FiveToEight,
            9..=16 => Self::NineToSixteen,
            17..=32 => Self::SeventeenToThirtyTwo,
            33..=64 => Self::ThirtyThreeToSixtyFour,
            _ => Self::SixtyFiveToOneTwentyEight,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default)]
struct QueryGroupSamples {
    qps: [Vec<u64>; 7],
    bm25: [Vec<u64>; 7],
    paired: [Vec<i64>; 7],
}

impl QueryGroupSamples {
    fn observe(
        &mut self,
        bucket: QueryGroupBucket,
        qps_nanos: u64,
        bm25_nanos: u64,
        paired_nanos: i64,
    ) {
        let index = bucket.index();
        self.qps[index].push(qps_nanos);
        self.bm25[index].push(bm25_nanos);
        self.paired[index].push(paired_nanos);
    }

    fn finish(self) -> Vec<QueryGroupBucketReceipt> {
        let labels = ["1", "2-4", "5-8", "9-16", "17-32", "33-64", "65-128"];
        self.qps
            .into_iter()
            .zip(self.bm25)
            .zip(self.paired)
            .zip(labels)
            .filter_map(|(((mut qps, mut bm25), mut paired), query_groups)| {
                (!qps.is_empty()).then(|| QueryGroupBucketReceipt {
                    query_groups,
                    samples: qps.len(),
                    qps_latency: Percentiles::from_u64(&mut qps),
                    bm25_latency: Percentiles::from_u64(&mut bm25),
                    paired_overhead: SignedPercentiles::from_i64(&mut paired),
                })
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct QueryGroupBucketReceipt {
    pub query_groups: &'static str,
    pub samples: usize,
    pub qps_latency: Percentiles,
    pub bm25_latency: Percentiles,
    pub paired_overhead: SignedPercentiles,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QpsAblationProfile {
    Full,
    LexicalCoverage,
    NoProximity,
    NoOrderPhrase,
    NoSegment,
    NoCoverage,
}

impl QpsAblationProfile {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "full" => Ok(Self::Full),
            "lexical-coverage" => Ok(Self::LexicalCoverage),
            "no-proximity" => Ok(Self::NoProximity),
            "no-order-phrase" => Ok(Self::NoOrderPhrase),
            "no-segment" => Ok(Self::NoSegment),
            "no-coverage" => Ok(Self::NoCoverage),
            _ => bail!(
                "unknown QPS profile {value:?}; expected full, lexical-coverage, \
                 no-proximity, no-order-phrase, no-segment, or no-coverage"
            ),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::LexicalCoverage => "lexical-coverage",
            Self::NoProximity => "no-proximity",
            Self::NoOrderPhrase => "no-order-phrase",
            Self::NoSegment => "no-segment",
            Self::NoCoverage => "no-coverage",
        }
    }

    const fn engine(self) -> &'static str {
        match self {
            Self::Full => ENGINE,
            Self::LexicalCoverage => "phoenix-qps-v2.01-ablation-lexical-coverage",
            Self::NoProximity => "phoenix-qps-v2.01-ablation-no-proximity",
            Self::NoOrderPhrase => "phoenix-qps-v2.01-ablation-no-order-phrase",
            Self::NoSegment => "phoenix-qps-v2.01-ablation-no-segment",
            Self::NoCoverage => "phoenix-qps-v2.01-ablation-no-coverage",
        }
    }

    fn apply(self, config: &mut QpsConfig) {
        match self {
            Self::Full => {}
            Self::LexicalCoverage => {
                config.proximity_weight = 0.0;
                config.order_weight = 0.0;
                config.phrase_weight = 0.0;
                config.segment_weight = 0.0;
            }
            Self::NoProximity => config.proximity_weight = 0.0,
            Self::NoOrderPhrase => {
                config.order_weight = 0.0;
                config.phrase_weight = 0.0;
            }
            Self::NoSegment => config.segment_weight = 0.0,
            Self::NoCoverage => {
                config.coverage_floor = 0.0;
                config.coverage_exponent = 0.0;
            }
        }
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    samples
        .get((samples.len().saturating_sub(1) * percentile) / 100)
        .copied()
        .unwrap_or(0)
}

fn signed_percentile(samples: &[i64], percentile: usize) -> i64 {
    samples
        .get((samples.len().saturating_sub(1) * percentile) / 100)
        .copied()
        .unwrap_or(0)
}

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{read_artifact, write_artifact};
    use crate::model::{
        GoldArtifact, GoldCase, HistorySession, HistoryTurn, SourceBinding, WorkloadCase,
        GOLD_CONTRACT, GOLD_MAGIC,
    };
    use crate::verify;
    use tempfile::tempdir;

    #[test]
    fn shadow_runner_is_deterministic_bounded_and_cannot_open_gold() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../memory-lock/longmemeval-cleaned-v1.json");
        let (manifest, _) =
            verify::load_and_verify(&manifest_path).expect("frozen manifest verifies");
        let dataset = &manifest.benchmark.datasets[0];
        let binding = SourceBinding {
            freeze_id: manifest.freeze_id.clone(),
            variant: dataset.variant.clone(),
            filename: dataset.filename.clone(),
            bytes: dataset.bytes,
            sha256: dataset.sha256.clone(),
        };
        let workload = WorkloadArtifact {
            contract: WORKLOAD_CONTRACT.to_owned(),
            source: binding.clone(),
            cases: vec![WorkloadCase {
                question_id: "qps-ordinary-1".to_owned(),
                question_type: "single-session-user".to_owned(),
                question: "Where was the jasmine green tea ordered?".to_owned(),
                question_date: "2026-07-30".to_owned(),
                sessions: vec![
                    session("s-ocean", "We discussed the ocean and blue boats."),
                    session(
                        "s-tea",
                        "Mira ordered jasmine green tea at the Rome market.",
                    ),
                    session("s-engine", "Bob repaired an engine after work."),
                ],
            }],
        };
        let gold = GoldArtifact {
            contract: GOLD_CONTRACT.to_owned(),
            source: binding,
            cases: vec![GoldCase {
                question_id: "qps-ordinary-1".to_owned(),
                question_type: "single-session-user".to_owned(),
                answer: "Rome market".to_owned(),
                answer_session_ids: vec!["s-tea".to_owned()],
            }],
        };
        let directory = tempdir().expect("temporary benchmark directory");
        let workload_path = directory.path().join("ordinary.plmw");
        let gold_path = directory.path().join("ordinary.plmg");
        let output_path = directory.path().join("qps.plmr");
        write_artifact(&workload_path, WORKLOAD_MAGIC, &workload).expect("write workload");
        write_artifact(&gold_path, GOLD_MAGIC, &gold).expect("write gold");

        let receipt = run(
            &manifest,
            &workload_path,
            &output_path,
            3,
            8,
            QpsAblationProfile::Full,
        )
        .expect("run QPS shadow lock");
        assert_eq!(receipt.path_id, PATH_ID);
        assert_eq!(receipt.authority, "none-shadow-artifact-only");
        assert!(!receipt.quality_evaluated);
        assert_eq!(receipt.scratch_capacity_growths, 0);
        assert!(receipt.max_reranked <= 160);
        let retrieval: RetrievalArtifact =
            read_artifact(&output_path, RETRIEVAL_MAGIC).expect("read QPS retrieval");
        assert_eq!(retrieval.engine, ENGINE);
        assert_eq!(retrieval.cases[0].ranked_sessions[0].stable_id, "s-tea");
        let error = run(
            &manifest,
            &gold_path,
            &directory.path().join("forbidden.plmr"),
            3,
            1,
            QpsAblationProfile::Full,
        )
        .expect_err("gold cannot enter QPS shadow retrieval");
        assert!(error.to_string().contains("artifact type mismatch"));
    }

    #[test]
    fn precomputed_csc_bm25_matches_the_reference_equation() {
        let documents = vec![
            "red dragon armor".to_owned(),
            "red red padding dragon armor later".to_owned(),
            "unrelated memory policy".to_owned(),
        ];
        let query = unique_tokens("red dragon armor");
        let index = Bm25TurboIndex::build(&documents);
        let mut scratch = Bm25TurboScratch::with_document_capacity(documents.len());
        let mut hits = Vec::with_capacity(documents.len());
        index.search_into(&query, documents.len(), &mut scratch, &mut hits);
        assert_eq!(hits.first().map(|(document, _)| *document), Some(0));

        let token_rows = documents
            .iter()
            .map(|document| {
                let mut tokens = Vec::new();
                append_tokens(document, &mut tokens);
                tokens
            })
            .collect::<Vec<_>>();
        let average_length =
            token_rows.iter().map(Vec::len).sum::<usize>() as f64 / token_rows.len() as f64;
        for (document, score) in hits {
            let expected = query
                .iter()
                .map(|term| {
                    let frequency = token_rows[document]
                        .iter()
                        .filter(|candidate| *candidate == term)
                        .count() as f64;
                    if frequency == 0.0 {
                        return 0.0;
                    }
                    let document_frequency = token_rows
                        .iter()
                        .filter(|tokens| tokens.contains(term))
                        .count() as f64;
                    let idf = (1.0
                        + (token_rows.len() as f64 - document_frequency + 0.5)
                            / (document_frequency + 0.5))
                        .ln();
                    let length_ratio = token_rows[document].len() as f64 / average_length;
                    idf * (frequency * (K1 + 1.0)) / (frequency + K1 * (1.0 - B + B * length_ratio))
                })
                .sum::<f64>();
            assert!((score - expected).abs() < 1.0e-12);
        }
    }

    fn session(stable_id: &str, content: &str) -> HistorySession {
        HistorySession {
            stable_id: stable_id.to_owned(),
            date: "2026-07-30".to_owned(),
            turns: vec![HistoryTurn {
                role: "user".to_owned(),
                content: content.to_owned(),
            }],
        }
    }
}
