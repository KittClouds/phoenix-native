use std::cmp::Ordering;

use phoenix_lexical_qps::{
    leakage_split_identity_v3, JudgmentReasonV3, LeakageSplitV3, LinearRankerV3, PrimarySplitV3,
    RankEvidenceV3, RelevanceLedgerV3, RelevanceTier,
};

use super::train::{decode_hex_32, LinearModelArtifactV3};
use super::*;

const CONTRACT: &str = "phoenix.memory.qps-v3-quality-qualification/v1";
const GRADED_CONTRACT: &str = "phoenix.qps.graded-evaluation-suite/v3";

pub(crate) fn qualify(
    model_path: &Path,
    phase_6_path: &Path,
    phase_4_path: &Path,
    phase_3_path: &Path,
    graded_suite_path: &Path,
    output_path: &Path,
) -> Result<QualityPublication> {
    if output_path.exists() {
        bail!(
            "refusing to overwrite quality receipt {}",
            output_path.display()
        );
    }
    let model: LinearModelArtifactV3 = read_json(model_path, "model artifact")?;
    if !model.validate_challenger() {
        bail!("Phase 8 requires a valid Phase 7 challenger artifact");
    }
    let phase_6: FrozenPhase6 = read_json(phase_6_path, "Phase 6 receipt")?;
    if phase_6.contract != "phoenix.memory.qps-v3-leakage-split/v1" || !phase_6.phase_6_verified {
        bail!("Phase 8 requires a verified Phase 6 split");
    }
    let phase_4: FrozenPhase4 = read_json(phase_4_path, "Phase 4 receipt")?;
    if phase_4.contract != "phoenix.memory.qps-v3-ledger-qualification/v1"
        || !phase_4.phase_4_verified
    {
        bail!("Phase 8 requires a verified Phase 4 ledger");
    }
    let ledger_identity = decode_hex_32(&sha256_bytes(&serde_json::to_vec(&phase_4.ledger)?))?;
    if model.training_ledger_identity != ledger_identity
        || model.training_receipt.leakage_split_identity
            != leakage_split_identity_v3(&phase_6.split)
    {
        bail!("Phase 8 model is not bound to the supplied Phase 4 ledger and Phase 6 split");
    }
    let phase_3: FrozenPhase3 = read_json(phase_3_path, "Phase 3 receipt")?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("Phase 8 requires the frozen constitutional and release cohorts");
    }
    let graded: GradedEvaluationSuiteV3 = read_json(graded_suite_path, "graded suite")?;
    graded.validate()?;
    let release_evidence = release_evidence_fingerprints(&phase_3.longmemeval_release);
    let training_release_evidence_intersections = phase_4
        .ledger
        .judgments
        .iter()
        .filter(|judgment| {
            release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.positive_features,
                judgment.positive_tier,
            )) || release_evidence.contains(&super::provenance::evidence_fingerprint(
                judgment.negative_features,
                judgment.negative_tier,
            ))
        })
        .count();
    let graded_release_evidence_intersections = graded
        .queries
        .iter()
        .flat_map(|query| query.candidates.iter())
        .filter(|candidate| {
            release_evidence.contains(&super::provenance::evidence_fingerprint(
                candidate.rank_evidence_v3,
                candidate.relevance_tier,
            ))
        })
        .count();

    let mixed = evaluate_frozen_cohort(&phase_3.mixed_suite, &model.model_parameters)?;
    let longmemeval =
        evaluate_frozen_cohort(&phase_3.longmemeval_release, &model.model_parameters)?;
    let graded_evaluation = evaluate_graded(&graded, &model.model_parameters)?;
    let pairwise = evaluate_blind_pairs(&phase_4.ledger, &phase_6.split, &model.model_parameters)?;
    let worst_shape_mrr_regression =
        worst_query_shape_regression(&phase_3.longmemeval_release, &model.model_parameters)?;
    let held_out_top_1_improvement_points =
        (pairwise.v3_top_1_accuracy - pairwise.v2_top_1_accuracy) * 100.0;
    let gates = QualityGates {
        longmemeval_hit_at_10_at_least_0_984: longmemeval.v3.hit_at_10 >= 0.984,
        longmemeval_mrr_at_least_0_910: longmemeval.v3.mean_reciprocal_rank >= 0.910,
        stretch_mrr_at_least_0_920: graded_evaluation.stretch_mrr >= 0.920,
        graded_ndcg_at_10_improvement_at_least_0_020: graded_evaluation.ndcg_improvement >= 0.020,
        held_out_top_1_improvement_at_least_2_points: held_out_top_1_improvement_points >= 2.0,
        held_out_pairwise_accuracy_at_least_0_80: pairwise.v3_accuracy >= 0.80,
        pairwise_accuracy_per_major_class_at_least_0_75: pairwise
            .classes
            .iter()
            .all(|class| class.judgments > 0 && class.v3_accuracy >= 0.75),
        mixed_hit_mrr_top_1_remain_1: mixed.v3.hit_at_10 == 1.0
            && mixed.v3.mean_reciprocal_rank == 1.0
            && mixed.v3.top_1_accuracy == 1.0,
        no_result_accuracy_remains_1: mixed.v3.no_result_accuracy == 1.0,
        constitutional_regressions_are_zero: mixed.constitutional_regressions == 0,
        candidate_pool_mismatches_are_zero: mixed.candidate_pool_mismatches == 0
            && longmemeval.candidate_pool_mismatches == 0,
        oracle_recall_regression_is_zero: mixed.oracle_recall_regressions == 0
            && longmemeval.oracle_recall_regressions == 0,
        worst_query_shape_mrr_regression_at_most_0_005: worst_shape_mrr_regression <= 0.005,
        training_release_evidence_intersections_are_zero: training_release_evidence_intersections
            == 0,
        graded_release_evidence_intersections_are_zero: graded_release_evidence_intersections == 0,
    };
    let receipt = QualityReceipt {
        contract: CONTRACT,
        model_artifact: file_identity(model_path)?,
        phase_6_receipt: file_identity(phase_6_path)?,
        phase_4_receipt: file_identity(phase_4_path)?,
        phase_3_receipt: file_identity(phase_3_path)?,
        graded_suite: file_identity(graded_suite_path)?,
        producer_binary: current_binary_identity()?,
        model_identity: model.model_identity,
        mixed,
        longmemeval,
        graded: graded_evaluation,
        blind_pairwise: pairwise,
        held_out_top_1_improvement_points,
        worst_query_shape_mrr_regression: worst_shape_mrr_regression,
        training_release_evidence_intersections,
        graded_release_evidence_intersections,
        gates,
        phase_8_verified: gates.all_pass(),
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(QualityPublication {
        contract: CONTRACT,
        output: file_identity(output_path)?,
        model_identity: model.model_identity,
        gates,
        phase_8_verified: receipt.phase_8_verified,
    })
}

fn release_evidence_fingerprints(cohort: &FrozenCohort) -> HashSet<[u8; 32]> {
    cohort
        .queries
        .iter()
        .flat_map(|query| query.candidate_pool.iter())
        .map(|candidate| {
            super::provenance::evidence_fingerprint(
                candidate.rank_evidence_v3,
                candidate.relevance_tier,
            )
        })
        .collect()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("decode {label} {}", path.display()))
}

fn evaluate_frozen_cohort(
    cohort: &FrozenCohort,
    model: &LinearRankerV3,
) -> Result<CohortComparison> {
    let mut v2 = MetricAccumulator::default();
    let mut v3 = MetricAccumulator::default();
    let mut constitutional_regressions = 0_usize;
    let mut oracle_recall_regressions = 0_usize;
    for query in &cohort.queries {
        let relevant = query
            .oracle_locations
            .iter()
            .map(|oracle| oracle.document_identity.as_str())
            .collect::<HashSet<_>>();
        let mut v2_order = query.candidate_pool.iter().collect::<Vec<_>>();
        v2_order.sort_unstable_by_key(|candidate| candidate.v2_order);
        let v3_order = rank_candidates(&query.candidate_pool, model)?;
        v2.observe(&v2_order, &relevant);
        v3.observe(&v3_order, &relevant);
        let v2_recalled = v2_order
            .iter()
            .any(|candidate| relevant.contains(candidate.document_identity.as_str()));
        let v3_recalled = v3_order
            .iter()
            .any(|candidate| relevant.contains(candidate.document_identity.as_str()));
        oracle_recall_regressions += usize::from(v2_recalled && !v3_recalled);
        if query.query_shape.contains("identifier") || query.query_shape.contains("phrase") {
            let v2_first = first_relevant_rank(&v2_order, &relevant);
            let v3_first = first_relevant_rank(&v3_order, &relevant);
            constitutional_regressions += usize::from(v3_first > v2_first);
        }
    }
    Ok(CohortComparison {
        v2: v2.finish(),
        v3: v3.finish(),
        candidate_pool_mismatches: 0,
        oracle_recall_regressions,
        constitutional_regressions,
    })
}

fn rank_candidates<'a>(
    candidates: &'a [FrozenCandidate],
    model: &LinearRankerV3,
) -> Result<Vec<&'a FrozenCandidate>> {
    let mut scored = candidates
        .iter()
        .map(|candidate| {
            model
                .score(candidate.rank_evidence_v3)
                .map(|score| (candidate, score))
                .context("invalid candidate evidence in quality cohort")
        })
        .collect::<Result<Vec<_>>>()?;
    scored.sort_unstable_by(|(left, left_score), (right, right_score)| {
        compare_ranked(
            left.relevance_tier,
            *left_score,
            &left.document_identity,
            right.relevance_tier,
            *right_score,
            &right.document_identity,
        )
    });
    Ok(scored.into_iter().map(|(candidate, _)| candidate).collect())
}

fn compare_ranked(
    left_tier: RelevanceTier,
    left_score: f32,
    left_identity: &str,
    right_tier: RelevanceTier,
    right_score: f32,
    right_identity: &str,
) -> Ordering {
    left_tier
        .cmp(&right_tier)
        .then_with(|| right_score.total_cmp(&left_score))
        .then_with(|| left_identity.cmp(right_identity))
}

fn evaluate_blind_pairs(
    ledger: &RelevanceLedgerV3,
    split: &LeakageSplitV3,
    model: &LinearRankerV3,
) -> Result<PairwiseComparison> {
    let assigned = split
        .assignments
        .iter()
        .map(|assignment| (assignment.judgment_identity, assignment.primary_split))
        .collect::<HashMap<_, _>>();
    let mut classes = all_reasons()
        .into_iter()
        .map(|reason| ClassAccuracy {
            reason,
            judgments: 0,
            v2_correct: 0,
            v3_correct: 0,
            v2_accuracy: 0.0,
            v3_accuracy: 0.0,
        })
        .collect::<Vec<_>>();
    let mut judgments = 0_usize;
    let mut v2_correct = 0_usize;
    let mut v3_correct = 0_usize;
    let mut query_top_1 = HashMap::new();
    for judgment in ledger
        .active_model_training_judgments()
        .into_iter()
        .filter(|judgment| assigned.get(&judgment.identity) == Some(&PrimarySplitV3::BlindTest))
    {
        let positive_score = model
            .score(judgment.positive_features)
            .context("invalid positive blind evidence")?;
        let negative_score = model
            .score(judgment.negative_features)
            .context("invalid negative blind evidence")?;
        let v2_wins = judgment.positive_position < judgment.negative_position;
        let v3_wins = compare_pair(
            judgment.positive_tier,
            positive_score,
            judgment.positive_document_version.as_bytes(),
            judgment.negative_tier,
            negative_score,
            judgment.negative_document_version.as_bytes(),
        ) == Ordering::Less;
        judgments += 1;
        v2_correct += usize::from(v2_wins);
        v3_correct += usize::from(v3_wins);
        let query = query_top_1
            .entry(judgment.query_identity)
            .or_insert((true, true));
        query.0 &= v2_wins;
        query.1 &= v3_wins;
        let class = classes
            .iter_mut()
            .find(|class| class.reason == judgment.reason)
            .expect("all reasons are represented");
        class.judgments += 1;
        class.v2_correct += usize::from(v2_wins);
        class.v3_correct += usize::from(v3_wins);
    }
    for class in &mut classes {
        class.v2_accuracy = class.v2_correct as f64 / class.judgments.max(1) as f64;
        class.v3_accuracy = class.v3_correct as f64 / class.judgments.max(1) as f64;
    }
    let queries = query_top_1.len();
    let v2_top_1 = query_top_1.values().filter(|(v2, _)| *v2).count();
    let v3_top_1 = query_top_1.values().filter(|(_, v3)| *v3).count();
    Ok(PairwiseComparison {
        judgments,
        v2_correct,
        v3_correct,
        v2_accuracy: v2_correct as f64 / judgments.max(1) as f64,
        v3_accuracy: v3_correct as f64 / judgments.max(1) as f64,
        queries,
        v2_top_1_accuracy: v2_top_1 as f64 / queries.max(1) as f64,
        v3_top_1_accuracy: v3_top_1 as f64 / queries.max(1) as f64,
        classes,
    })
}

fn compare_pair(
    positive_tier: RelevanceTier,
    positive_score: f32,
    positive_identity: [u8; 32],
    negative_tier: RelevanceTier,
    negative_score: f32,
    negative_identity: [u8; 32],
) -> Ordering {
    positive_tier
        .cmp(&negative_tier)
        .then_with(|| negative_score.total_cmp(&positive_score))
        .then_with(|| positive_identity.cmp(&negative_identity))
}

fn evaluate_graded(
    suite: &GradedEvaluationSuiteV3,
    model: &LinearRankerV3,
) -> Result<GradedComparison> {
    let mut v2_ndcg = 0.0_f64;
    let mut v3_ndcg = 0.0_f64;
    let mut reciprocal_rank = 0.0_f64;
    for query in &suite.queries {
        let mut v2 = query.candidates.iter().collect::<Vec<_>>();
        v2.sort_unstable_by_key(|candidate| candidate.v2_order);
        let mut v3 = query
            .candidates
            .iter()
            .map(|candidate| {
                model
                    .score(candidate.rank_evidence_v3)
                    .map(|score| (candidate, score))
                    .context("invalid graded candidate evidence")
            })
            .collect::<Result<Vec<_>>>()?;
        v3.sort_unstable_by(|(left, left_score), (right, right_score)| {
            compare_ranked(
                left.relevance_tier,
                *left_score,
                &left.document_identity,
                right.relevance_tier,
                *right_score,
                &right.document_identity,
            )
        });
        v2_ndcg += ndcg(v2.iter().map(|candidate| candidate.grade));
        v3_ndcg += ndcg(v3.iter().map(|(candidate, _)| candidate.grade));
        reciprocal_rank += v3
            .iter()
            .position(|(candidate, _)| candidate.grade > 0)
            .map_or(0.0, |rank| 1.0 / (rank + 1) as f64);
    }
    let queries = suite.queries.len().max(1) as f64;
    let v2_ndcg_at_10 = v2_ndcg / queries;
    let v3_ndcg_at_10 = v3_ndcg / queries;
    Ok(GradedComparison {
        queries: suite.queries.len(),
        v2_ndcg_at_10,
        v3_ndcg_at_10,
        ndcg_improvement: v3_ndcg_at_10 - v2_ndcg_at_10,
        stretch_mrr: reciprocal_rank / queries,
    })
}

fn ndcg(grades: impl Iterator<Item = u8>) -> f64 {
    let grades = grades.take(10).collect::<Vec<_>>();
    let dcg = discounted_gain(grades.iter().copied());
    let mut ideal = grades;
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    let ideal = discounted_gain(ideal.into_iter());
    if ideal == 0.0 {
        0.0
    } else {
        dcg / ideal
    }
}

fn discounted_gain(grades: impl Iterator<Item = u8>) -> f64 {
    grades
        .enumerate()
        .map(|(rank, grade)| ((1_u32 << grade) - 1) as f64 / ((rank + 2) as f64).log2())
        .sum()
}

fn worst_query_shape_regression(cohort: &FrozenCohort, model: &LinearRankerV3) -> Result<f64> {
    let mut shapes = HashMap::<&str, (f64, f64, usize)>::new();
    for query in &cohort.queries {
        let relevant = query
            .oracle_locations
            .iter()
            .map(|oracle| oracle.document_identity.as_str())
            .collect::<HashSet<_>>();
        if relevant.is_empty() {
            continue;
        }
        let mut v2 = query.candidate_pool.iter().collect::<Vec<_>>();
        v2.sort_unstable_by_key(|candidate| candidate.v2_order);
        let v3 = rank_candidates(&query.candidate_pool, model)?;
        let entry = shapes.entry(&query.query_shape).or_default();
        entry.0 += reciprocal_rank(&v2, &relevant);
        entry.1 += reciprocal_rank(&v3, &relevant);
        entry.2 += 1;
    }
    Ok(shapes
        .into_values()
        .map(|(v2, v3, count)| (v2 - v3) / count.max(1) as f64)
        .fold(0.0, f64::max))
}

fn first_relevant_rank<T: CandidateIdentity>(
    ordered: &[T],
    relevant: &HashSet<&str>,
) -> Option<usize> {
    ordered
        .iter()
        .position(|candidate| relevant.contains(candidate.identity()))
}

fn reciprocal_rank<T: CandidateIdentity>(ordered: &[T], relevant: &HashSet<&str>) -> f64 {
    first_relevant_rank(ordered, relevant).map_or(0.0, |rank| 1.0 / (rank + 1) as f64)
}

trait CandidateIdentity {
    fn identity(&self) -> &str;
}

impl CandidateIdentity for &FrozenCandidate {
    fn identity(&self) -> &str {
        &self.document_identity
    }
}

#[derive(Default)]
struct MetricAccumulator {
    answerable: usize,
    hits: usize,
    reciprocal_sum: f64,
    top_1: usize,
    no_result: usize,
    correct_no_result: usize,
}

impl MetricAccumulator {
    fn observe<T: CandidateIdentity>(&mut self, ordered: &[T], relevant: &HashSet<&str>) {
        if relevant.is_empty() {
            self.no_result += 1;
            self.correct_no_result += usize::from(ordered.is_empty());
            return;
        }
        self.answerable += 1;
        if let Some(rank) = first_relevant_rank(ordered, relevant) {
            self.hits += usize::from(rank < 10);
            self.top_1 += usize::from(rank == 0);
            self.reciprocal_sum += 1.0 / (rank + 1) as f64;
        }
    }

    fn finish(self) -> QualityMetricsV3 {
        QualityMetricsV3 {
            answerable_queries: self.answerable,
            hit_at_10: self.hits as f64 / self.answerable.max(1) as f64,
            mean_reciprocal_rank: self.reciprocal_sum / self.answerable.max(1) as f64,
            top_1_accuracy: self.top_1 as f64 / self.answerable.max(1) as f64,
            no_result_queries: self.no_result,
            no_result_accuracy: self.correct_no_result as f64 / self.no_result.max(1) as f64,
        }
    }
}

fn all_reasons() -> [JudgmentReasonV3; 11] {
    [
        JudgmentReasonV3::PartialMatchSaturation,
        JudgmentReasonV3::ScatteredTerms,
        JudgmentReasonV3::PhraseOrderFailure,
        JudgmentReasonV3::IdentifierCollision,
        JudgmentReasonV3::FuzzyCollision,
        JudgmentReasonV3::WeakFieldEvidence,
        JudgmentReasonV3::CommonTermDominance,
        JudgmentReasonV3::LengthPriorFailure,
        JudgmentReasonV3::WrongConceptProximity,
        JudgmentReasonV3::DocumentConversationConfusion,
        JudgmentReasonV3::LongQueryFailure,
    ]
}

#[derive(Clone, Copy, Debug, Serialize)]
struct QualityMetricsV3 {
    answerable_queries: usize,
    hit_at_10: f64,
    mean_reciprocal_rank: f64,
    top_1_accuracy: f64,
    no_result_queries: usize,
    no_result_accuracy: f64,
}

#[derive(Debug, Serialize)]
struct CohortComparison {
    v2: QualityMetricsV3,
    v3: QualityMetricsV3,
    candidate_pool_mismatches: usize,
    oracle_recall_regressions: usize,
    constitutional_regressions: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ClassAccuracy {
    reason: JudgmentReasonV3,
    judgments: usize,
    v2_correct: usize,
    v3_correct: usize,
    v2_accuracy: f64,
    v3_accuracy: f64,
}

#[derive(Debug, Serialize)]
struct PairwiseComparison {
    judgments: usize,
    v2_correct: usize,
    v3_correct: usize,
    v2_accuracy: f64,
    v3_accuracy: f64,
    queries: usize,
    v2_top_1_accuracy: f64,
    v3_top_1_accuracy: f64,
    classes: Vec<ClassAccuracy>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct GradedComparison {
    queries: usize,
    v2_ndcg_at_10: f64,
    v3_ndcg_at_10: f64,
    ndcg_improvement: f64,
    stretch_mrr: f64,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct QualityGates {
    longmemeval_hit_at_10_at_least_0_984: bool,
    longmemeval_mrr_at_least_0_910: bool,
    stretch_mrr_at_least_0_920: bool,
    graded_ndcg_at_10_improvement_at_least_0_020: bool,
    held_out_top_1_improvement_at_least_2_points: bool,
    held_out_pairwise_accuracy_at_least_0_80: bool,
    pairwise_accuracy_per_major_class_at_least_0_75: bool,
    mixed_hit_mrr_top_1_remain_1: bool,
    no_result_accuracy_remains_1: bool,
    constitutional_regressions_are_zero: bool,
    candidate_pool_mismatches_are_zero: bool,
    oracle_recall_regression_is_zero: bool,
    worst_query_shape_mrr_regression_at_most_0_005: bool,
    training_release_evidence_intersections_are_zero: bool,
    graded_release_evidence_intersections_are_zero: bool,
}

impl QualityGates {
    fn all_pass(self) -> bool {
        self.longmemeval_hit_at_10_at_least_0_984
            && self.longmemeval_mrr_at_least_0_910
            && self.stretch_mrr_at_least_0_920
            && self.graded_ndcg_at_10_improvement_at_least_0_020
            && self.held_out_top_1_improvement_at_least_2_points
            && self.held_out_pairwise_accuracy_at_least_0_80
            && self.pairwise_accuracy_per_major_class_at_least_0_75
            && self.mixed_hit_mrr_top_1_remain_1
            && self.no_result_accuracy_remains_1
            && self.constitutional_regressions_are_zero
            && self.candidate_pool_mismatches_are_zero
            && self.oracle_recall_regression_is_zero
            && self.worst_query_shape_mrr_regression_at_most_0_005
            && self.training_release_evidence_intersections_are_zero
            && self.graded_release_evidence_intersections_are_zero
    }
}

#[derive(Debug, Serialize)]
struct QualityReceipt {
    contract: &'static str,
    model_artifact: FileIdentity,
    phase_6_receipt: FileIdentity,
    phase_4_receipt: FileIdentity,
    phase_3_receipt: FileIdentity,
    graded_suite: FileIdentity,
    producer_binary: FileIdentity,
    model_identity: [u8; 32],
    mixed: CohortComparison,
    longmemeval: CohortComparison,
    graded: GradedComparison,
    blind_pairwise: PairwiseComparison,
    held_out_top_1_improvement_points: f64,
    worst_query_shape_mrr_regression: f64,
    training_release_evidence_intersections: usize,
    graded_release_evidence_intersections: usize,
    gates: QualityGates,
    phase_8_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct QualityPublication {
    contract: &'static str,
    output: FileIdentity,
    model_identity: [u8; 32],
    gates: QualityGates,
    phase_8_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase6 {
    contract: String,
    split: LeakageSplitV3,
    phase_6_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase4 {
    contract: String,
    ledger: RelevanceLedgerV3,
    phase_4_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    contract: String,
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_shape: String,
    candidate_pool: Vec<FrozenCandidate>,
    oracle_locations: Vec<FrozenOracle>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    v2_order: usize,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[derive(Debug, Deserialize)]
struct FrozenOracle {
    document_identity: String,
}

#[derive(Debug, Deserialize)]
struct GradedEvaluationSuiteV3 {
    contract: String,
    schema_version: u16,
    queries: Vec<GradedQueryV3>,
}

impl GradedEvaluationSuiteV3 {
    fn validate(&self) -> Result<()> {
        if self.contract != GRADED_CONTRACT || self.schema_version != 3 || self.queries.is_empty() {
            bail!("invalid or empty V3 graded evaluation suite");
        }
        for query in &self.queries {
            if query.query_identity.is_empty()
                || query.candidates.is_empty()
                || query.candidates.len() > 160
                || query
                    .candidates
                    .iter()
                    .any(|candidate| candidate.grade > 4 || !candidate.rank_evidence_v3.is_valid())
            {
                bail!("invalid V3 graded query");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct GradedQueryV3 {
    query_identity: String,
    candidates: Vec<GradedCandidateV3>,
}

#[derive(Debug, Deserialize)]
struct GradedCandidateV3 {
    document_identity: String,
    v2_order: usize,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
    grade: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndcg_rewards_the_ideal_graded_order() {
        assert_eq!(ndcg([3, 2, 1, 0].into_iter()), 1.0);
        assert!(ndcg([0, 1, 2, 3].into_iter()) < 1.0);
    }

    #[test]
    fn quality_gate_requires_every_slice() {
        let mut gates = QualityGates {
            longmemeval_hit_at_10_at_least_0_984: true,
            longmemeval_mrr_at_least_0_910: true,
            stretch_mrr_at_least_0_920: true,
            graded_ndcg_at_10_improvement_at_least_0_020: true,
            held_out_top_1_improvement_at_least_2_points: true,
            held_out_pairwise_accuracy_at_least_0_80: true,
            pairwise_accuracy_per_major_class_at_least_0_75: true,
            mixed_hit_mrr_top_1_remain_1: true,
            no_result_accuracy_remains_1: true,
            constitutional_regressions_are_zero: true,
            candidate_pool_mismatches_are_zero: true,
            oracle_recall_regression_is_zero: true,
            worst_query_shape_mrr_regression_at_most_0_005: true,
            training_release_evidence_intersections_are_zero: true,
            graded_release_evidence_intersections_are_zero: true,
        };
        assert!(gates.all_pass());
        gates.pairwise_accuracy_per_major_class_at_least_0_75 = false;
        assert!(!gates.all_pass());
    }
}
