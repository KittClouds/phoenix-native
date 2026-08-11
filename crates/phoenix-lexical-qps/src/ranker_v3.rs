use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use crate::{
    LeakageSplitV3, PrimarySplitV3, RankEvidenceV3, RelevanceLedgerV3,
    RANK_EVIDENCE_V3_FEATURE_COUNT,
};
#[cfg(test)]
use crate::{RANK_EVIDENCE_V3_FEATURE_NAMES, RANK_EVIDENCE_V3_SCHEMA_VERSION};

pub const LINEAR_RANKER_V3_VERSION: u16 = 3;
pub const RANK_EVIDENCE_V3_SCHEMA_IDENTITY: [u8; 32] = [
    125, 14, 178, 88, 193, 159, 39, 243, 158, 150, 16, 174, 70, 3, 100, 13, 215, 186, 159, 206,
    179, 217, 154, 107, 184, 214, 37, 253, 176, 46, 214, 229,
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeatureNormalizationV3 {
    pub offsets: [f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
    pub scales: [f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
}

impl FeatureNormalizationV3 {
    /// RankEvidenceV3 is already schema-normalized into `[0, 1]`. Keeping the
    /// identity transform in the artifact makes that frozen contract explicit.
    pub const fn identity() -> Self {
        Self {
            offsets: [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT],
            scales: [1.0; RANK_EVIDENCE_V3_FEATURE_COUNT],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.offsets.iter().all(|value| *value == 0.0)
            && self.scales.iter().all(|value| *value == 1.0)
    }

    #[inline]
    fn normalize(&self, index: usize, value: f32) -> f32 {
        ((value - self.offsets[index]) * self.scales[index]).clamp(0.0, 1.0)
    }
}

impl Default for FeatureNormalizationV3 {
    fn default() -> Self {
        Self::identity()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearRankerV3 {
    pub version: u16,
    pub feature_schema_identity: [u8; 32],
    pub normalization: FeatureNormalizationV3,
    pub weights: [f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
}

impl LinearRankerV3 {
    pub fn from_weights(
        normalization: FeatureNormalizationV3,
        weights: [f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
    ) -> Result<Self, &'static str> {
        if !normalization.is_valid()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight < 0.0)
            || weights.iter().all(|weight| *weight == 0.0)
        {
            return Err("invalid monotonic V3 linear ranker");
        }
        Ok(Self {
            version: LINEAR_RANKER_V3_VERSION,
            feature_schema_identity: rank_evidence_schema_identity_v3(),
            normalization,
            weights,
        })
    }

    pub fn is_valid(&self) -> bool {
        self.version == LINEAR_RANKER_V3_VERSION
            && self.feature_schema_identity == rank_evidence_schema_identity_v3()
            && self.normalization.is_valid()
            && self
                .weights
                .iter()
                .all(|weight| weight.is_finite() && *weight >= 0.0)
            && self.weights.iter().any(|weight| *weight > 0.0)
    }

    /// Fixed-width slice walk: 30 multiply-add operations, with no
    /// allocation, hashing, I/O, locks, dispatch, or access to the V2 score.
    #[inline]
    pub fn score(&self, evidence: RankEvidenceV3) -> Option<f32> {
        if !self.is_valid() || !evidence.is_valid() {
            return None;
        }
        Some(self.score_prevalidated(evidence))
    }

    #[inline]
    pub(crate) fn score_prevalidated(&self, evidence: RankEvidenceV3) -> f32 {
        debug_assert!(self.is_valid());
        debug_assert!(evidence.is_valid());
        let mut score = 0.0;
        for (weight, value) in self.weights.iter().zip(evidence.values) {
            score = weight.mul_add(value, score);
        }
        debug_assert!(score.is_finite());
        score
    }

    pub fn identity(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix-qps-linear-ranker-v3\0");
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&self.feature_schema_identity);
        for value in self.normalization.offsets {
            hasher.update(&value.to_bits().to_le_bytes());
        }
        for value in self.normalization.scales {
            hasher.update(&value.to_bits().to_le_bytes());
        }
        for weight in self.weights {
            hasher.update(&weight.to_bits().to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearTrainingConfigV3 {
    pub epochs: u16,
    pub learning_rate: f32,
    pub l2_penalty: f32,
}

impl LinearTrainingConfigV3 {
    fn validate(self) -> Result<(), &'static str> {
        if self.epochs == 0
            || self.epochs > 16_384
            || !self.learning_rate.is_finite()
            || !(0.0..=1.0).contains(&self.learning_rate)
            || self.learning_rate == 0.0
            || !self.l2_penalty.is_finite()
            || !(0.0..=0.1).contains(&self.l2_penalty)
        {
            return Err("invalid V3 linear training configuration");
        }
        Ok(())
    }
}

impl Default for LinearTrainingConfigV3 {
    fn default() -> Self {
        Self {
            epochs: 128,
            learning_rate: 0.025,
            l2_penalty: 0.0001,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearTrainingReceiptV3 {
    pub training_ledger_identity: [u8; 32],
    pub leakage_split_identity: [u8; 32],
    pub feature_schema_identity: [u8; 32],
    pub model_identity: [u8; 32],
    pub training_judgments: usize,
    pub epochs: u16,
    pub learning_rate: f32,
    pub l2_penalty: f32,
    pub initial_pairwise_loss: f32,
    pub final_pairwise_loss: f32,
    pub correctly_ordered: usize,
    pub pairwise_accuracy: f32,
}

pub fn train_linear_ranker_v3(
    ledger: &RelevanceLedgerV3,
    split: &LeakageSplitV3,
    training_ledger_identity: [u8; 32],
    config: LinearTrainingConfigV3,
) -> Result<(LinearRankerV3, LinearTrainingReceiptV3), &'static str> {
    ledger.validate()?;
    config.validate()?;
    if training_ledger_identity == [0; 32]
        || split.audit.eligible_judgments == 0
        || !split.audit.is_qualified()
    {
        return Err("V3 training requires a qualified leakage split and ledger identity");
    }
    let assignment = split
        .assignments
        .iter()
        .map(|value| (value.judgment_identity, value.primary_split))
        .collect::<HashMap<_, _>>();
    let mut training = ledger
        .active_model_training_judgments()
        .into_iter()
        .filter(|judgment| assignment.get(&judgment.identity) == Some(&PrimarySplitV3::Training))
        .collect::<Vec<_>>();
    if training.is_empty() {
        return Err("V3 training split contains no eligible judgments");
    }
    training.sort_unstable_by(|left, right| {
        left.query_identity
            .cmp(&right.query_identity)
            .then_with(|| {
                left.positive_document_version
                    .cmp(&right.positive_document_version)
            })
            .then_with(|| {
                left.negative_document_version
                    .cmp(&right.negative_document_version)
            })
            .then_with(|| left.identity.cmp(&right.identity))
    });
    let normalization = FeatureNormalizationV3::identity();
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    let initial_pairwise_loss = pairwise_loss(&weights, normalization, &training);
    // The objective contains one L2 term for the complete ledger. SGD visits
    // every judgment once per epoch, so distribute its gradient across those
    // visits instead of applying the full penalty once per row.
    let l2_gradient_per_judgment = distributed_l2_gradient(config.l2_penalty, training.len());
    for _ in 0..config.epochs {
        for judgment in &training {
            let difference = normalized_difference(
                normalization,
                judgment.positive_features,
                judgment.negative_features,
            );
            let margin = dot(&weights, &difference);
            let pair_weight = judgment.weight * judgment.confidence;
            let pressure = pair_weight / (1.0 + margin.exp());
            for (weight, delta) in weights.iter_mut().zip(difference) {
                *weight +=
                    config.learning_rate * (pressure * delta - l2_gradient_per_judgment * *weight);
                *weight = weight.max(0.0);
            }
        }
    }
    let model = LinearRankerV3::from_weights(normalization, weights)?;
    let final_pairwise_loss = pairwise_loss(&weights, normalization, &training);
    let correctly_ordered = training
        .iter()
        .filter(|judgment| {
            model
                .score(judgment.positive_features)
                .unwrap_or(f32::NEG_INFINITY)
                > model
                    .score(judgment.negative_features)
                    .unwrap_or(f32::NEG_INFINITY)
        })
        .count();
    let leakage_split_identity = leakage_split_identity_v3(split);
    let receipt = LinearTrainingReceiptV3 {
        training_ledger_identity,
        leakage_split_identity,
        feature_schema_identity: rank_evidence_schema_identity_v3(),
        model_identity: model.identity(),
        training_judgments: training.len(),
        epochs: config.epochs,
        learning_rate: config.learning_rate,
        l2_penalty: config.l2_penalty,
        initial_pairwise_loss,
        final_pairwise_loss,
        correctly_ordered,
        pairwise_accuracy: correctly_ordered as f32 / training.len() as f32,
    };
    Ok((model, receipt))
}

#[inline]
fn distributed_l2_gradient(l2_penalty: f32, judgments: usize) -> f32 {
    (2.0 * l2_penalty) / judgments.max(1) as f32
}

pub const fn rank_evidence_schema_identity_v3() -> [u8; 32] {
    RANK_EVIDENCE_V3_SCHEMA_IDENTITY
}

#[cfg(test)]
fn computed_rank_evidence_schema_identity_v3() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-qps-rank-evidence-schema-v3\0");
    hasher.update(&RANK_EVIDENCE_V3_SCHEMA_VERSION.to_le_bytes());
    for name in RANK_EVIDENCE_V3_FEATURE_NAMES {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub fn leakage_split_identity_v3(split: &LeakageSplitV3) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-qps-leakage-split-identity-v3\0");
    for assignment in &split.assignments {
        hasher.update(&assignment.judgment_identity.as_bytes());
        hasher.update(&[match assignment.primary_split {
            PrimarySplitV3::Training => 1,
            PrimarySplitV3::Development => 2,
            PrimarySplitV3::BlindTest => 3,
        }]);
    }
    *hasher.finalize().as_bytes()
}

fn normalized_difference(
    normalization: FeatureNormalizationV3,
    positive: RankEvidenceV3,
    negative: RankEvidenceV3,
) -> [f32; RANK_EVIDENCE_V3_FEATURE_COUNT] {
    let mut difference = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    for (index, value) in difference.iter_mut().enumerate() {
        *value = normalization.normalize(index, positive.values[index])
            - normalization.normalize(index, negative.values[index]);
    }
    difference
}

fn dot(
    weights: &[f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
    values: &[f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
) -> f32 {
    weights
        .iter()
        .zip(values)
        .fold(0.0, |score, (weight, value)| weight.mul_add(*value, score))
}

fn pairwise_loss(
    weights: &[f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
    normalization: FeatureNormalizationV3,
    judgments: &[&crate::PairwiseJudgmentV3],
) -> f32 {
    judgments
        .iter()
        .map(|judgment| {
            let difference = normalized_difference(
                normalization,
                judgment.positive_features,
                judgment.negative_features,
            );
            let negative_margin = -dot(weights, &difference);
            let softplus = if negative_margin > 20.0 {
                negative_margin
            } else {
                negative_margin.exp().ln_1p()
            };
            judgment.weight * judgment.confidence * softplus
        })
        .sum::<f32>()
        / judgments.len().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity, PairwiseJudgmentDraftV3,
        PairwiseJudgmentV3, SplitGroupProvenanceV3,
    };

    #[test]
    fn schema_identity_is_stable_and_v2_composites_are_absent() {
        assert_eq!(
            rank_evidence_schema_identity_v3(),
            computed_rank_evidence_schema_identity_v3()
        );
        assert!(!RANK_EVIDENCE_V3_FEATURE_NAMES.contains(&"baseline_score"));
        assert!(!RANK_EVIDENCE_V3_FEATURE_NAMES.contains(&"candidate_strength"));
    }

    #[test]
    fn scorer_is_monotonic_and_rejects_invalid_models() {
        let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
        weights[RankEvidenceV3::BM25F_LEXICAL] = 1.0;
        let model =
            LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights).unwrap();
        let mut low = evidence();
        low.values[RankEvidenceV3::BM25F_LEXICAL] = 0.2;
        let mut high = low;
        high.values[RankEvidenceV3::BM25F_LEXICAL] = 0.8;
        assert!(model.score(high) > model.score(low));
        assert!(LinearRankerV3::from_weights(
            FeatureNormalizationV3::identity(),
            [-1.0; RANK_EVIDENCE_V3_FEATURE_COUNT]
        )
        .is_err());
    }

    #[test]
    fn deterministic_training_improves_pairwise_loss_from_zero_initialization() {
        let ledger = training_ledger(55);
        let split = LeakageSplitV3::build(&ledger).unwrap();
        let first =
            train_linear_ranker_v3(&ledger, &split, [7; 32], LinearTrainingConfigV3::default())
                .unwrap();
        let second =
            train_linear_ranker_v3(&ledger, &split, [7; 32], LinearTrainingConfigV3::default())
                .unwrap();
        assert_eq!(first, second);
        assert!(first.1.final_pairwise_loss < first.1.initial_pairwise_loss);
        assert_eq!(first.1.training_judgments, 33);
        assert_eq!(first.1.correctly_ordered, 33);
        assert!(first.0.weights.iter().all(|weight| *weight >= 0.0));
    }

    #[test]
    fn l2_gradient_is_applied_once_per_epoch_not_once_per_row() {
        assert_eq!(distributed_l2_gradient(0.1, 4), 0.05);
        assert_eq!(distributed_l2_gradient(0.1, 1), 0.2);
        assert_eq!(distributed_l2_gradient(0.1, 0), 0.2);
    }

    fn training_ledger(count: u8) -> RelevanceLedgerV3 {
        let mut ledger = RelevanceLedgerV3::default();
        for value in 1..=count {
            let positive = keyed(value.saturating_add(80));
            let negative = keyed(value.saturating_add(120));
            let mut positive_features = evidence();
            positive_features.values[RankEvidenceV3::BM25F_LEXICAL] = 0.9;
            let mut negative_features = evidence();
            negative_features.values[RankEvidenceV3::BM25F_LEXICAL] = 0.1;
            let judgment = PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
                workspace_identity: keyed(250),
                query_identity: keyed(value),
                positive_document_version: positive,
                negative_document_version: negative,
                positive_features,
                negative_features,
                positive_tier: crate::RelevanceTier::CompleteExactGroups,
                negative_tier: crate::RelevanceTier::CompleteExactGroups,
                candidate_pool: vec![positive, negative].into_boxed_slice(),
                positive_position: 0,
                negative_position: 1,
                split_groups: SplitGroupProvenanceV3 {
                    query_family_identity: scoped_keyed(value, 1),
                    positive_source_identity: scoped_keyed(value, 2),
                    negative_source_identity: scoped_keyed(value, 3),
                    positive_near_duplicate_cluster_identity: scoped_keyed(value, 4),
                    negative_near_duplicate_cluster_identity: scoped_keyed(value, 5),
                    entity_or_identifier_family_identity: scoped_keyed(value, 6),
                    collection_cohort_identity: scoped_keyed(value, 7),
                    collected_at_unix_seconds: 1_700_000_000 + u64::from(value),
                },
                frozen_holdout: None,
                v2_model_identity: [2; 32],
                challenger_model_identity: [3; 32],
                reason: major_reason(value),
                source: JudgmentSourceV3::CuratedRegressionCase,
                confidence: 1.0,
                weight: 1.0,
                index_generation: u64::from(value),
                supersedes: None,
                contradicts: Box::new([]),
            });
            ledger.append(judgment).unwrap();
        }
        ledger
    }

    fn keyed(value: u8) -> KeyedIdentity {
        KeyedIdentity::from_bytes([value; 32])
    }

    fn scoped_keyed(value: u8, domain: u8) -> KeyedIdentity {
        let mut bytes = [value; 32];
        bytes[1] = domain;
        KeyedIdentity::from_bytes(bytes)
    }

    fn major_reason(value: u8) -> JudgmentReasonV3 {
        const REASONS: [JudgmentReasonV3; 11] = [
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
        ];
        REASONS[usize::from(value - 1) % REASONS.len()]
    }

    fn evidence() -> RankEvidenceV3 {
        RankEvidenceV3 {
            schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
            query_groups: 1,
            matched_groups: 1,
            missing_groups: 0,
            query_flags: 0,
            field_count: 1,
            values: [0.5; RANK_EVIDENCE_V3_FEATURE_COUNT],
        }
    }
}
