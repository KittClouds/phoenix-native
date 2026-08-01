use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

pub const RANK_FEATURE_COUNT: usize = 12;
const MODEL_VERSION: u16 = 1;

/// Bounded evidence vector produced from the same posting encounter as the
/// baseline QPS score. All values are finite and normalized into `[0, 1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct RankFeatureVector(pub [f32; RANK_FEATURE_COUNT]);

impl RankFeatureVector {
    pub const BASELINE: usize = 0;
    pub const LEXICAL: usize = 1;
    pub const COVERAGE: usize = 2;
    pub const COMPLETE_COVERAGE: usize = 3;
    pub const PROXIMITY: usize = 4;
    pub const ORDER: usize = 5;
    pub const PHRASE: usize = 6;
    pub const SEGMENT: usize = 7;
    pub const EXACT_FIELD: usize = 8;
    pub const CANDIDATE_STRENGTH: usize = 9;
    pub const LENGTH_PRIOR: usize = 10;
    pub const EXPANSION_QUALITY: usize = 11;

    pub fn is_valid(self) -> bool {
        self.0
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    }

    #[inline]
    pub(crate) fn difference(self, other: Self) -> [f32; RANK_FEATURE_COUNT] {
        let mut difference = [0.0; RANK_FEATURE_COUNT];
        for (slot, (positive, negative)) in
            difference.iter_mut().zip(self.0.into_iter().zip(other.0))
        {
            *slot = positive - negative;
        }
        difference
    }
}

/// Tiny deterministic linear ranker. Evaluation is twelve multiply-adds per
/// reranked candidate and performs no allocation, hashing, dispatch or I/O.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearRankerV1 {
    version: u16,
    enabled: bool,
    weights: [f32; RANK_FEATURE_COUNT],
}

impl LinearRankerV1 {
    pub const fn disabled() -> Self {
        let mut weights = [0.0; RANK_FEATURE_COUNT];
        weights[RankFeatureVector::BASELINE] = 1.0;
        Self {
            version: MODEL_VERSION,
            enabled: false,
            weights,
        }
    }

    pub fn from_weights(weights: [f32; RANK_FEATURE_COUNT]) -> Result<Self, &'static str> {
        if weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight < 0.0)
        {
            return Err("ranker weights must be finite and non-negative");
        }
        if weights.iter().all(|weight| *weight == 0.0) {
            return Err("ranker must retain at least one evidence signal");
        }
        Ok(Self {
            version: MODEL_VERSION,
            enabled: true,
            weights,
        })
    }

    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    pub const fn weights(self) -> [f32; RANK_FEATURE_COUNT] {
        self.weights
    }

    #[inline]
    pub fn score(self, baseline: f32, features: RankFeatureVector) -> f32 {
        if !self.enabled {
            return baseline;
        }
        let mut score = 0.0;
        for (weight, feature) in self.weights.into_iter().zip(features.0) {
            score = weight.mul_add(feature, score);
        }
        score
    }

    pub fn identity(self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"phoenix-qps-linear-ranker-v1\0");
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&[u8::from(self.enabled)]);
        for weight in self.weights {
            hasher.update(&weight.to_bits().to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

impl Default for LinearRankerV1 {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardNegativeReason {
    PartialMatchSaturation,
    ScatteredTerms,
    IdentifierCollision,
    FuzzyCollision,
    WeakFieldEvidence,
    CommonTermDominance,
    ProductionCorrection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HardNegativeJudgment {
    pub query_hash: [u8; 32],
    pub positive_document_hash: [u8; 32],
    pub negative_document_hash: [u8; 32],
    pub positive: RankFeatureVector,
    pub negative: RankFeatureVector,
    pub reason: HardNegativeReason,
    pub weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HardNegativeLedgerV1 {
    pub judgments: Vec<HardNegativeJudgment>,
}

impl HardNegativeLedgerV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.judgments.is_empty() {
            return Err("hard-negative ledger is empty");
        }
        for judgment in &self.judgments {
            if judgment.query_hash == [0; 32]
                || judgment.positive_document_hash == [0; 32]
                || judgment.negative_document_hash == [0; 32]
                || judgment.positive_document_hash == judgment.negative_document_hash
                || !judgment.positive.is_valid()
                || !judgment.negative.is_valid()
                || !judgment.weight.is_finite()
                || judgment.weight <= 0.0
            {
                return Err("hard-negative ledger contains an invalid judgment");
            }
        }
        Ok(())
    }

    pub fn train(
        &self,
        config: RankerTrainingConfig,
    ) -> Result<(LinearRankerV1, RankerTrainingReceipt), &'static str> {
        self.validate()?;
        config.validate()?;
        let mut order = (0..self.judgments.len()).collect::<Vec<_>>();
        order.sort_unstable_by(|left, right| {
            compare_judgments(&self.judgments[*left], &self.judgments[*right])
        });
        let mut weights = [0.0; RANK_FEATURE_COUNT];
        weights[RankFeatureVector::BASELINE] = 1.0;
        let initial_loss = pairwise_loss(&weights, &self.judgments);
        for _ in 0..config.epochs {
            for &index in &order {
                let judgment = &self.judgments[index];
                let difference = judgment.positive.difference(judgment.negative);
                let margin = dot(&weights, &difference);
                let pressure = judgment.weight / (1.0 + margin.exp());
                for (weight, delta) in weights.iter_mut().zip(difference) {
                    *weight +=
                        config.learning_rate * (pressure * delta - config.l2_penalty * *weight);
                    // All exposed signals are encoded so that more evidence is
                    // better. Projection preserves that monotonic contract.
                    *weight = weight.max(0.0);
                }
            }
        }
        let model = LinearRankerV1::from_weights(weights)?;
        let final_loss = pairwise_loss(&weights, &self.judgments);
        let correctly_ordered = self
            .judgments
            .iter()
            .filter(|judgment| {
                model.score(0.0, judgment.positive) > model.score(0.0, judgment.negative)
            })
            .count();
        Ok((
            model,
            RankerTrainingReceipt {
                judgments: self.judgments.len(),
                epochs: config.epochs,
                initial_pairwise_loss: initial_loss,
                final_pairwise_loss: final_loss,
                correctly_ordered,
                model_identity: model.identity(),
            },
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankerTrainingConfig {
    pub epochs: u16,
    pub learning_rate: f32,
    pub l2_penalty: f32,
}

impl RankerTrainingConfig {
    fn validate(self) -> Result<(), &'static str> {
        if self.epochs == 0
            || self.epochs > 16_384
            || !self.learning_rate.is_finite()
            || !(0.0..=1.0).contains(&self.learning_rate)
            || self.learning_rate == 0.0
            || !self.l2_penalty.is_finite()
            || !(0.0..=0.1).contains(&self.l2_penalty)
        {
            return Err("invalid ranker training configuration");
        }
        Ok(())
    }
}

impl Default for RankerTrainingConfig {
    fn default() -> Self {
        Self {
            epochs: 128,
            learning_rate: 0.025,
            l2_penalty: 0.0001,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankerTrainingReceipt {
    pub judgments: usize,
    pub epochs: u16,
    pub initial_pairwise_loss: f32,
    pub final_pairwise_loss: f32,
    pub correctly_ordered: usize,
    pub model_identity: [u8; 32],
}

fn compare_judgments(left: &HardNegativeJudgment, right: &HardNegativeJudgment) -> Ordering {
    left.query_hash
        .cmp(&right.query_hash)
        .then_with(|| {
            left.positive_document_hash
                .cmp(&right.positive_document_hash)
        })
        .then_with(|| {
            left.negative_document_hash
                .cmp(&right.negative_document_hash)
        })
}

fn dot(weights: &[f32; RANK_FEATURE_COUNT], values: &[f32; RANK_FEATURE_COUNT]) -> f32 {
    weights
        .iter()
        .zip(values)
        .fold(0.0, |score, (weight, value)| weight.mul_add(*value, score))
}

fn pairwise_loss(weights: &[f32; RANK_FEATURE_COUNT], judgments: &[HardNegativeJudgment]) -> f32 {
    judgments
        .iter()
        .map(|judgment| {
            let margin = dot(weights, &judgment.positive.difference(judgment.negative));
            judgment.weight * (-margin).exp().ln_1p()
        })
        .sum::<f32>()
        / judgments.len().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector(values: [f32; RANK_FEATURE_COUNT]) -> RankFeatureVector {
        RankFeatureVector(values)
    }

    #[test]
    fn deterministic_training_learns_the_frozen_pair() {
        let ledger = HardNegativeLedgerV1 {
            judgments: vec![HardNegativeJudgment {
                query_hash: [1; 32],
                positive_document_hash: [2; 32],
                negative_document_hash: [3; 32],
                positive: vector([0.5, 0.4, 1.0, 1.0, 0.8, 0.8, 0.8, 0.2, 0.0, 0.6, 0.3, 1.0]),
                negative: vector([0.6, 0.7, 0.4, 0.0, 0.1, 0.0, 0.0, 0.2, 0.0, 0.8, 0.3, 1.0]),
                reason: HardNegativeReason::PartialMatchSaturation,
                weight: 1.0,
            }],
        };
        let (first, first_receipt) = ledger.train(RankerTrainingConfig::default()).unwrap();
        let (second, second_receipt) = ledger.train(RankerTrainingConfig::default()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first_receipt, second_receipt);
        assert!(first_receipt.final_pairwise_loss < first_receipt.initial_pairwise_loss);
        assert_eq!(first_receipt.correctly_ordered, 1);
    }

    #[test]
    fn invalid_or_non_monotonic_models_fail_closed() {
        assert!(LinearRankerV1::from_weights([-1.0; RANK_FEATURE_COUNT]).is_err());
        assert!(LinearRankerV1::from_weights([0.0; RANK_FEATURE_COUNT]).is_err());
        let mut nan = [0.0; RANK_FEATURE_COUNT];
        nan[0] = f32::NAN;
        assert!(LinearRankerV1::from_weights(nan).is_err());
    }
}
