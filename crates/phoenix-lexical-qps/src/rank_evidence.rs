use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

pub const RANK_EVIDENCE_V3_SCHEMA_VERSION: u16 = 3;
pub const RANK_EVIDENCE_V3_FEATURE_COUNT: usize = 30;
pub const RANK_EVIDENCE_V3_FIELD_SLOTS: usize = 4;
pub const RANK_EVIDENCE_V3_FEATURE_NAMES: [&str; RANK_EVIDENCE_V3_FEATURE_COUNT] = [
    "bm25f_lexical",
    "field_lexical_0",
    "field_lexical_1",
    "field_lexical_2",
    "field_lexical_3",
    "field_lexical_overflow",
    "weighted_group_coverage",
    "matched_group_fraction",
    "missing_group_absence",
    "complete_coverage",
    "complete_span_quality",
    "ordered_span_quality",
    "ordered_fraction",
    "exact_phrase",
    "exact_group_fraction",
    "exact_field",
    "best_expansion_quality",
    "mean_expansion_quality",
    "minimum_expansion_quality",
    "rarest_matched_term",
    "mean_matched_term_rarity",
    "document_length_prior",
    "candidate_score_percentile",
    "original_candidate_rank",
    "query_group_count",
    "single_group_flag",
    "expansion_query_flag",
    "long_query_flag",
    "all_exact_groups_flag",
    "field_coverage_fraction",
];

pub const QUERY_FLAG_SINGLE_GROUP: u16 = 1 << 0;
pub const QUERY_FLAG_HAS_EXPANSIONS: u16 = 1 << 1;
pub const QUERY_FLAG_LONG: u16 = 1 << 2;
pub const QUERY_FLAG_ALL_EXACT_GROUPS: u16 = 1 << 3;

/// Lower numeric values are constitutionally stronger and always sort first.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(u8)]
pub enum RelevanceTier {
    ConfiguredExactIdentifier = 1,
    CompleteExactGroups = 2,
    CompleteExpandedGroups = 3,
    AdmissiblePartial = 4,
    Rejected = 5,
}

impl RelevanceTier {
    /// Constitutional ordering: tier first, learned score only within a tier,
    /// then stable external document identity for exact ties.
    pub fn compare_ranked(
        self,
        score: f32,
        document_identity: u64,
        other: Self,
        other_score: f32,
        other_document_identity: u64,
    ) -> Ordering {
        self.cmp(&other)
            .then_with(|| other_score.total_cmp(&score))
            .then_with(|| document_identity.cmp(&other_document_identity))
    }
}

/// Fixed-width primitive evidence for V3. No slot contains the V2 final score
/// or the old candidate-strength feature. All floating-point values are finite
/// and normalized to `[0, 1]`; integer counts preserve exact audit detail.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct RankEvidenceV3 {
    pub schema_version: u16,
    pub query_groups: u16,
    pub matched_groups: u16,
    pub missing_groups: u16,
    pub query_flags: u16,
    pub field_count: u16,
    pub values: [f32; RANK_EVIDENCE_V3_FEATURE_COUNT],
}

impl RankEvidenceV3 {
    pub const BM25F_LEXICAL: usize = 0;
    pub const FIELD_LEXICAL_0: usize = 1;
    pub const FIELD_LEXICAL_1: usize = 2;
    pub const FIELD_LEXICAL_2: usize = 3;
    pub const FIELD_LEXICAL_3: usize = 4;
    pub const FIELD_LEXICAL_OVERFLOW: usize = 5;
    pub const WEIGHTED_GROUP_COVERAGE: usize = 6;
    pub const MATCHED_GROUP_FRACTION: usize = 7;
    pub const MISSING_GROUP_ABSENCE: usize = 8;
    pub const COMPLETE_COVERAGE: usize = 9;
    pub const COMPLETE_SPAN_QUALITY: usize = 10;
    pub const ORDERED_SPAN_QUALITY: usize = 11;
    pub const ORDERED_FRACTION: usize = 12;
    pub const EXACT_PHRASE: usize = 13;
    pub const EXACT_GROUP_FRACTION: usize = 14;
    pub const EXACT_FIELD: usize = 15;
    pub const BEST_EXPANSION_QUALITY: usize = 16;
    pub const MEAN_EXPANSION_QUALITY: usize = 17;
    pub const MINIMUM_EXPANSION_QUALITY: usize = 18;
    pub const RAREST_MATCHED_TERM: usize = 19;
    pub const MEAN_MATCHED_TERM_RARITY: usize = 20;
    pub const DOCUMENT_LENGTH_PRIOR: usize = 21;
    pub const CANDIDATE_SCORE_PERCENTILE: usize = 22;
    pub const ORIGINAL_CANDIDATE_RANK: usize = 23;
    pub const QUERY_GROUP_COUNT: usize = 24;
    pub const SINGLE_GROUP_FLAG: usize = 25;
    pub const EXPANSION_QUERY_FLAG: usize = 26;
    pub const LONG_QUERY_FLAG: usize = 27;
    pub const ALL_EXACT_GROUPS_FLAG: usize = 28;
    pub const FIELD_COVERAGE_FRACTION: usize = 29;

    pub(crate) fn from_inputs(inputs: RankEvidenceInputs) -> Self {
        let query_groups = inputs.query_groups.max(1);
        let matched_groups = inputs.matched_groups.min(query_groups);
        let missing_groups = query_groups - matched_groups;
        let matched_fraction = matched_groups as f32 / query_groups as f32;
        let complete = matched_groups == query_groups;
        let mut flags = 0_u16;
        if query_groups == 1 {
            flags |= QUERY_FLAG_SINGLE_GROUP;
        }
        if inputs.has_expansions {
            flags |= QUERY_FLAG_HAS_EXPANSIONS;
        }
        if query_groups > 16 {
            flags |= QUERY_FLAG_LONG;
        }
        if inputs.all_exact_groups {
            flags |= QUERY_FLAG_ALL_EXACT_GROUPS;
        }
        let mut values = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
        values[Self::BM25F_LEXICAL] = unit_saturating(inputs.lexical);
        for (slot, value) in inputs.field_lexical.into_iter().enumerate() {
            values[Self::FIELD_LEXICAL_0 + slot] = unit_saturating(value);
        }
        values[Self::FIELD_LEXICAL_OVERFLOW] = unit_saturating(inputs.field_lexical_overflow);
        values[Self::WEIGHTED_GROUP_COVERAGE] = unit(inputs.weighted_coverage);
        values[Self::MATCHED_GROUP_FRACTION] = matched_fraction;
        values[Self::MISSING_GROUP_ABSENCE] = 1.0 - missing_groups as f32 / query_groups as f32;
        values[Self::COMPLETE_COVERAGE] = f32::from(complete);
        values[Self::COMPLETE_SPAN_QUALITY] =
            span_quality(inputs.minimum_complete_span, query_groups, complete);
        values[Self::ORDERED_SPAN_QUALITY] =
            span_quality(inputs.minimum_ordered_span, query_groups, complete);
        values[Self::ORDERED_FRACTION] = unit(inputs.ordered_fraction);
        values[Self::EXACT_PHRASE] = f32::from(inputs.exact_phrase);
        values[Self::EXACT_GROUP_FRACTION] = inputs.exact_groups as f32 / query_groups as f32;
        values[Self::EXACT_FIELD] = f32::from(inputs.exact_field);
        values[Self::BEST_EXPANSION_QUALITY] = unit(inputs.best_expansion_quality);
        values[Self::MEAN_EXPANSION_QUALITY] = unit(inputs.mean_expansion_quality);
        values[Self::MINIMUM_EXPANSION_QUALITY] = unit(inputs.minimum_expansion_quality);
        values[Self::RAREST_MATCHED_TERM] = unit(inputs.rarest_matched_term);
        values[Self::MEAN_MATCHED_TERM_RARITY] = unit(inputs.mean_matched_term_rarity);
        values[Self::DOCUMENT_LENGTH_PRIOR] = 1.0 / (1.0 + inputs.document_tokens as f32).sqrt();
        values[Self::CANDIDATE_SCORE_PERCENTILE] =
            rank_percentile(inputs.candidate_rank, inputs.candidate_pool_size);
        values[Self::ORIGINAL_CANDIDATE_RANK] =
            rank_percentile(inputs.candidate_rank, inputs.maximum_candidate_pool);
        values[Self::QUERY_GROUP_COUNT] = query_groups as f32 / inputs.maximum_query_groups as f32;
        values[Self::SINGLE_GROUP_FLAG] = f32::from(flags & QUERY_FLAG_SINGLE_GROUP != 0);
        values[Self::EXPANSION_QUERY_FLAG] = f32::from(flags & QUERY_FLAG_HAS_EXPANSIONS != 0);
        values[Self::LONG_QUERY_FLAG] = f32::from(flags & QUERY_FLAG_LONG != 0);
        values[Self::ALL_EXACT_GROUPS_FLAG] = f32::from(flags & QUERY_FLAG_ALL_EXACT_GROUPS != 0);
        values[Self::FIELD_COVERAGE_FRACTION] = unit(inputs.field_coverage_fraction);
        Self {
            schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
            query_groups: u16::try_from(query_groups).unwrap_or(u16::MAX),
            matched_groups: u16::try_from(matched_groups).unwrap_or(u16::MAX),
            missing_groups: u16::try_from(missing_groups).unwrap_or(u16::MAX),
            query_flags: flags,
            field_count: u16::try_from(inputs.field_count).unwrap_or(u16::MAX),
            values,
        }
    }

    pub fn is_valid(self) -> bool {
        self.schema_version == RANK_EVIDENCE_V3_SCHEMA_VERSION
            && self.query_groups > 0
            && self.matched_groups.saturating_add(self.missing_groups) == self.query_groups
            && self.field_count > 0
            && self
                .values
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    }

    pub fn relevance_tier(self, configured_exact_identifier: bool) -> RelevanceTier {
        if !self.is_valid() {
            return RelevanceTier::Rejected;
        }
        let complete = self.missing_groups == 0;
        let all_exact = self.values[Self::EXACT_GROUP_FRACTION] >= 1.0 - f32::EPSILON;
        if configured_exact_identifier && complete && all_exact {
            RelevanceTier::ConfiguredExactIdentifier
        } else if complete && all_exact {
            RelevanceTier::CompleteExactGroups
        } else if complete {
            RelevanceTier::CompleteExpandedGroups
        } else {
            RelevanceTier::AdmissiblePartial
        }
    }

    #[inline]
    pub fn difference(self, other: Self) -> [f32; RANK_EVIDENCE_V3_FEATURE_COUNT] {
        let mut difference = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
        for (slot, (positive, negative)) in difference
            .iter_mut()
            .zip(self.values.into_iter().zip(other.values))
        {
            *slot = positive - negative;
        }
        difference
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RankEvidenceInputs {
    pub lexical: f32,
    pub field_lexical: [f32; RANK_EVIDENCE_V3_FIELD_SLOTS],
    pub field_lexical_overflow: f32,
    pub weighted_coverage: f32,
    pub query_groups: usize,
    pub matched_groups: usize,
    pub exact_groups: usize,
    pub minimum_complete_span: u32,
    pub minimum_ordered_span: u32,
    pub ordered_fraction: f32,
    pub exact_phrase: bool,
    pub exact_field: bool,
    pub best_expansion_quality: f32,
    pub mean_expansion_quality: f32,
    pub minimum_expansion_quality: f32,
    pub rarest_matched_term: f32,
    pub mean_matched_term_rarity: f32,
    pub document_tokens: u32,
    pub candidate_rank: usize,
    pub candidate_pool_size: usize,
    pub maximum_candidate_pool: usize,
    pub maximum_query_groups: usize,
    pub field_count: usize,
    pub field_coverage_fraction: f32,
    pub has_expansions: bool,
    pub all_exact_groups: bool,
}

#[inline]
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[inline]
fn unit_saturating(value: f32) -> f32 {
    let positive = if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    positive / (1.0 + positive)
}

#[inline]
fn rank_percentile(rank: usize, population: usize) -> f32 {
    if population <= 1 {
        1.0
    } else {
        1.0 - rank.min(population - 1) as f32 / (population - 1) as f32
    }
}

#[inline]
fn span_quality(span: u32, query_groups: usize, complete: bool) -> f32 {
    if !complete || span == u32::MAX {
        return 0.0;
    }
    let excess = span.saturating_sub(query_groups as u32) as f32;
    1.0 / (1.0 + excess)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(span: u32) -> RankEvidenceV3 {
        RankEvidenceV3::from_inputs(RankEvidenceInputs {
            lexical: 4.0,
            field_lexical: [3.0, 1.0, 0.0, 0.0],
            field_lexical_overflow: 0.0,
            weighted_coverage: 1.0,
            query_groups: 3,
            matched_groups: 3,
            exact_groups: 3,
            minimum_complete_span: span,
            minimum_ordered_span: span,
            ordered_fraction: 1.0,
            exact_phrase: span == 3,
            exact_field: false,
            best_expansion_quality: 1.0,
            mean_expansion_quality: 1.0,
            minimum_expansion_quality: 1.0,
            rarest_matched_term: 0.8,
            mean_matched_term_rarity: 0.5,
            document_tokens: 12,
            candidate_rank: 0,
            candidate_pool_size: 160,
            maximum_candidate_pool: 160,
            maximum_query_groups: 128,
            field_count: 2,
            field_coverage_fraction: 1.0,
            has_expansions: false,
            all_exact_groups: true,
        })
    }

    #[test]
    fn primitive_schema_is_bounded_and_excludes_v2_composites() {
        let exact = evidence(3);
        let scattered = evidence(12);
        assert!(exact.is_valid());
        assert!(scattered.is_valid());
        assert!(
            exact.values[RankEvidenceV3::COMPLETE_SPAN_QUALITY]
                > scattered.values[RankEvidenceV3::COMPLETE_SPAN_QUALITY]
        );
        assert_eq!(RANK_EVIDENCE_V3_FEATURE_COUNT, 30);
    }

    #[test]
    fn constitutional_tiers_are_strictly_ordered_before_score() {
        let exact = evidence(3);
        let mut expanded = exact;
        expanded.values[RankEvidenceV3::EXACT_GROUP_FRACTION] = 0.5;
        let mut partial = exact;
        partial.matched_groups = 2;
        partial.missing_groups = 1;
        assert_eq!(
            exact.relevance_tier(true),
            RelevanceTier::ConfiguredExactIdentifier
        );
        assert_eq!(
            exact.relevance_tier(false),
            RelevanceTier::CompleteExactGroups
        );
        assert_eq!(
            expanded.relevance_tier(false),
            RelevanceTier::CompleteExpandedGroups
        );
        assert_eq!(
            partial.relevance_tier(false),
            RelevanceTier::AdmissiblePartial
        );
        assert_eq!(
            RelevanceTier::CompleteExactGroups.compare_ranked(
                0.01,
                9,
                RelevanceTier::AdmissiblePartial,
                1_000_000.0,
                1,
            ),
            Ordering::Less
        );
        assert_eq!(
            RelevanceTier::CompleteExactGroups.compare_ranked(
                4.0,
                2,
                RelevanceTier::CompleteExactGroups,
                4.0,
                3,
            ),
            Ordering::Less
        );
    }
}
