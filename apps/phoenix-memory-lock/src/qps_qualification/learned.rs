use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use phoenix_lexical_qps::{
    HardNegativeJudgment, HardNegativeLedgerV1, HardNegativeReason, LinearRankerV1,
    RankerTrainingConfig, RankerTrainingReceipt, SearchHit, SearchScratch, MAXIMUM_QUERY_GROUPS,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    build_index, build_index_with_ranker, elapsed_nanos, hex_sha256, percentile, validate_suite,
    PreparedQuery, QualificationSuite, TOP_K,
};

const CONTRACT: &str = "phoenix.memory.qps-learned-ranker-qualification/v1";

pub fn run(
    suite_path: &Path,
    output_path: &Path,
    repetitions: usize,
) -> Result<LearnedRankerQualificationReceipt> {
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    let suite_bytes = fs::read(suite_path)
        .with_context(|| format!("read qualification suite {}", suite_path.display()))?;
    let suite: QualificationSuite = serde_json::from_slice(&suite_bytes)
        .with_context(|| format!("decode qualification suite {}", suite_path.display()))?;
    validate_suite(&suite)?;
    let baseline = build_index(&suite)?;
    let ledger = mine_training_ledger(&suite, &baseline)?;
    let (model, training) = ledger
        .train(RankerTrainingConfig::default())
        .map_err(anyhow::Error::msg)?;
    let learned = build_index_with_ranker(&suite, model)?;
    let comparison = compare_holdout(&suite, &baseline, &learned, repetitions)?;
    let evaluation_cost = measure_ranker_evaluation_cost(model, &ledger, repetitions);
    let gates = LearnedRankerGates {
        heldout_mrr_not_lower: comparison.learned_mrr >= comparison.baseline_mrr,
        no_heldout_regressions: comparison.regressions == 0,
        p99_overhead_below_150_microseconds: comparison.p99_overhead_nanos <= 150_000,
        deterministic_model: model
            == ledger
                .train(RankerTrainingConfig::default())
                .map_err(anyhow::Error::msg)?
                .0,
        frozen_pool_unchanged: comparison.candidate_pool_mismatches == 0,
        ranker_160_candidate_p99_below_50_microseconds: evaluation_cost.p99_batch_nanos <= 50_000,
    };
    let artifact = LearnedRankerArtifactV1 {
        contract: CONTRACT,
        suite_sha256: hex_sha256(&suite_bytes),
        split: "sha256(stable_query_id)[0] % 4 == 0 is held out",
        ledger,
        model,
        training,
    };
    let artifact_bytes = serde_json::to_vec_pretty(&artifact)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create ranker artifact directory {}", parent.display()))?;
    }
    fs::write(output_path, &artifact_bytes)
        .with_context(|| format!("write ranker artifact {}", output_path.display()))?;
    Ok(LearnedRankerQualificationReceipt {
        contract: CONTRACT,
        suite_path: suite_path.display().to_string(),
        suite_sha256: artifact.suite_sha256,
        artifact_path: output_path.display().to_string(),
        artifact_sha256: hex_sha256(&artifact_bytes),
        artifact_bytes: artifact_bytes.len(),
        model_identity: hex(model.identity()),
        training,
        comparison,
        evaluation_cost,
        qualified: gates.all_pass(),
        gates,
    })
}

fn measure_ranker_evaluation_cost(
    model: LinearRankerV1,
    ledger: &HardNegativeLedgerV1,
    repetitions: usize,
) -> RankerEvaluationCost {
    const CANDIDATES_PER_BATCH: usize = 160;
    let features = ledger
        .judgments
        .iter()
        .flat_map(|judgment| [judgment.positive, judgment.negative])
        .collect::<Vec<_>>();
    let batches = repetitions.saturating_mul(16).clamp(256, 65_536);
    let mut samples = Vec::with_capacity(batches);
    let mut checksum = 0.0_f32;
    for batch in 0..batches {
        let started = Instant::now();
        let mut batch_score = 0.0_f32;
        for candidate in 0..CANDIDATES_PER_BATCH {
            let feature = features[(batch + candidate) % features.len()];
            batch_score += black_box(model).score(black_box(0.5), black_box(feature));
        }
        checksum += black_box(batch_score);
        samples.push(elapsed_nanos(started));
    }
    black_box(checksum);
    samples.sort_unstable();
    RankerEvaluationCost {
        batches,
        candidates_per_batch: CANDIDATES_PER_BATCH,
        p50_batch_nanos: percentile(&samples, 50),
        p95_batch_nanos: percentile(&samples, 95),
        p99_batch_nanos: percentile(&samples, 99),
    }
}

fn mine_training_ledger(
    suite: &QualificationSuite,
    index: &phoenix_lexical_qps::QpsIndex,
) -> Result<HardNegativeLedgerV1> {
    let mut scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::<SearchHit>::with_capacity(suite.documents.len());
    let mut judgments = Vec::new();
    for query in &suite.queries {
        let Some(expected) = query.expected.as_deref() else {
            continue;
        };
        if is_holdout(&query.stable_id) {
            continue;
        }
        let expected_index = suite
            .documents
            .iter()
            .position(|document| document.stable_id == expected)
            .context("training query expected document is missing")?;
        let prepared = PreparedQuery::new(query);
        let groups = prepared.group_views();
        prepared.search_exhaustive(
            &groups,
            index,
            suite.documents.len(),
            &mut scratch,
            &mut hits,
        )?;
        let positive = hits
            .iter()
            .find(|hit| hit.external_id as usize == expected_index)
            .with_context(|| format!("query {} did not retrieve its positive", query.stable_id))?;
        let Some(negative) = hits
            .iter()
            .find(|hit| hit.external_id as usize != expected_index)
        else {
            continue;
        };
        judgments.push(HardNegativeJudgment {
            query_hash: hash(&query.stable_id),
            positive_document_hash: hash(expected),
            negative_document_hash: hash(&suite.documents[negative.external_id as usize].stable_id),
            positive: positive.rank_features,
            negative: negative.rank_features,
            reason: reason(query.shape.label()),
            weight: 1.0,
        });
    }
    let ledger = HardNegativeLedgerV1 { judgments };
    ledger.validate().map_err(anyhow::Error::msg)?;
    Ok(ledger)
}

fn compare_holdout(
    suite: &QualificationSuite,
    baseline: &phoenix_lexical_qps::QpsIndex,
    learned: &phoenix_lexical_qps::QpsIndex,
    repetitions: usize,
) -> Result<HoldoutComparison> {
    let mut baseline_scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut learned_scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut baseline_hits = Vec::<SearchHit>::with_capacity(TOP_K);
    let mut learned_hits = Vec::<SearchHit>::with_capacity(TOP_K);
    let mut baseline_reciprocal = 0.0;
    let mut learned_reciprocal = 0.0;
    let mut answerable = 0;
    let mut regressions = 0;
    let mut candidate_pool_mismatches = 0;
    let mut overhead = Vec::new();
    for query in &suite.queries {
        if !is_holdout(&query.stable_id) {
            continue;
        }
        let prepared = PreparedQuery::new(query);
        let groups = prepared.group_views();
        let base_receipt =
            prepared.search(&groups, baseline, &mut baseline_scratch, &mut baseline_hits)?;
        let learned_receipt =
            prepared.search(&groups, learned, &mut learned_scratch, &mut learned_hits)?;
        candidate_pool_mismatches += usize::from(
            base_receipt.candidates != learned_receipt.candidates
                || base_receipt.reranked_candidates != learned_receipt.reranked_candidates,
        );
        if let Some(expected) = query.expected.as_deref() {
            answerable += 1;
            let expected_index = suite
                .documents
                .iter()
                .position(|document| document.stable_id == expected)
                .context("holdout expected document is missing")?
                as u64;
            let baseline_rank = rank(&baseline_hits, expected_index);
            let learned_rank = rank(&learned_hits, expected_index);
            baseline_reciprocal += reciprocal(baseline_rank);
            learned_reciprocal += reciprocal(learned_rank);
            regressions += usize::from(learned_rank > baseline_rank);
        }
        for _ in 0..repetitions {
            let started = Instant::now();
            prepared.search(&groups, baseline, &mut baseline_scratch, &mut baseline_hits)?;
            let baseline_nanos = elapsed_nanos(started);
            let started = Instant::now();
            prepared.search(&groups, learned, &mut learned_scratch, &mut learned_hits)?;
            overhead.push(elapsed_nanos(started).saturating_sub(baseline_nanos));
        }
    }
    overhead.sort_unstable();
    Ok(HoldoutComparison {
        heldout_queries: suite
            .queries
            .iter()
            .filter(|query| is_holdout(&query.stable_id))
            .count(),
        answerable_queries: answerable,
        baseline_mrr: baseline_reciprocal / answerable.max(1) as f64,
        learned_mrr: learned_reciprocal / answerable.max(1) as f64,
        regressions,
        candidate_pool_mismatches,
        p50_overhead_nanos: percentile(&overhead, 50),
        p95_overhead_nanos: percentile(&overhead, 95),
        p99_overhead_nanos: percentile(&overhead, 99),
    })
}

fn rank(hits: &[SearchHit], expected: u64) -> usize {
    hits.iter()
        .position(|hit| hit.external_id == expected)
        .map_or(usize::MAX, |rank| rank + 1)
}

fn reciprocal(rank: usize) -> f64 {
    if rank == usize::MAX {
        0.0
    } else {
        1.0 / rank as f64
    }
}

fn is_holdout(stable_id: &str) -> bool {
    hash(stable_id)[0] % 4 == 0
}

fn reason(shape: &str) -> HardNegativeReason {
    match shape {
        "phrase" => HardNegativeReason::ScatteredTerms,
        "fuzzy" => HardNegativeReason::FuzzyCollision,
        _ => HardNegativeReason::PartialMatchSaturation,
    }
}

fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Serialize)]
struct LearnedRankerArtifactV1 {
    contract: &'static str,
    suite_sha256: String,
    split: &'static str,
    ledger: HardNegativeLedgerV1,
    model: LinearRankerV1,
    training: RankerTrainingReceipt,
}

#[derive(Debug, Serialize)]
pub struct LearnedRankerQualificationReceipt {
    contract: &'static str,
    suite_path: String,
    suite_sha256: String,
    artifact_path: String,
    artifact_sha256: String,
    artifact_bytes: usize,
    model_identity: String,
    training: RankerTrainingReceipt,
    comparison: HoldoutComparison,
    evaluation_cost: RankerEvaluationCost,
    gates: LearnedRankerGates,
    qualified: bool,
}

#[derive(Debug, Serialize)]
struct HoldoutComparison {
    heldout_queries: usize,
    answerable_queries: usize,
    baseline_mrr: f64,
    learned_mrr: f64,
    regressions: usize,
    candidate_pool_mismatches: usize,
    p50_overhead_nanos: u64,
    p95_overhead_nanos: u64,
    p99_overhead_nanos: u64,
}

#[derive(Debug, Serialize)]
struct RankerEvaluationCost {
    batches: usize,
    candidates_per_batch: usize,
    p50_batch_nanos: u64,
    p95_batch_nanos: u64,
    p99_batch_nanos: u64,
}

#[derive(Debug, Serialize)]
struct LearnedRankerGates {
    heldout_mrr_not_lower: bool,
    no_heldout_regressions: bool,
    p99_overhead_below_150_microseconds: bool,
    deterministic_model: bool,
    frozen_pool_unchanged: bool,
    ranker_160_candidate_p99_below_50_microseconds: bool,
}

impl LearnedRankerGates {
    fn all_pass(&self) -> bool {
        self.heldout_mrr_not_lower
            && self.no_heldout_regressions
            && self.p99_overhead_below_150_microseconds
            && self.deterministic_model
            && self.frozen_pool_unchanged
            && self.ranker_160_candidate_p99_below_50_microseconds
    }
}
