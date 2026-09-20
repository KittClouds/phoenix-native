use std::fmt;

use crate::ranker::{LinearRankerV1, RankFeatureVector};
use crate::RankEvidenceV3;

/// Maximum number of independently scored query groups. The coherence kernel
/// uses two packed `u64` lanes, keeping long-query bookkeeping on the stack.
pub const MAXIMUM_QUERY_GROUPS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct DocumentId(pub u32);

impl DocumentId {
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FieldConfig {
    pub name: &'static str,
    pub weight: f32,
    pub length_normalization: f32,
    pub exact_match_bonus: f32,
}

impl FieldConfig {
    pub const fn new(
        name: &'static str,
        weight: f32,
        length_normalization: f32,
        exact_match_bonus: f32,
    ) -> Self {
        Self {
            name,
            weight,
            length_normalization,
            exact_match_bonus,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct QpsConfig {
    pub k1: f32,
    pub coverage_floor: f32,
    pub coverage_exponent: f32,
    pub proximity_weight: f32,
    pub order_weight: f32,
    pub phrase_weight: f32,
    pub segment_weight: f32,
    pub proximity_decay_tokens: f32,
    /// Minimum lexical candidates retained before positional reranking.
    pub minimum_candidate_pool: usize,
    /// Candidate multiplier relative to requested top-k.
    pub candidate_pool_multiplier: usize,
    /// Hard bound on positional reranking work.
    pub maximum_candidate_pool: usize,
    /// Touched-document density at which a contiguous SIMD scan replaces the
    /// sparse touched-document selector.
    pub dense_simd_threshold: f32,
    pub maximum_query_groups: usize,
    pub maximum_expansions_per_group: usize,
    /// Optional deterministic learned composition over evidence already
    /// produced by QPS. Disabled means the frozen hand-tuned V2.01 score.
    pub learned_ranker: LinearRankerV1,
    /// V3 constitutional exact-identifier fields. Bit `n` protects field `n`.
    /// This does not participate in V2 retrieval, candidate selection, or its
    /// diagnostic score.
    pub v3_exact_identifier_fields: u64,
}

impl Default for QpsConfig {
    fn default() -> Self {
        Self {
            k1: 1.2,
            coverage_floor: 0.2,
            coverage_exponent: 2.0,
            proximity_weight: 0.35,
            order_weight: 0.15,
            phrase_weight: 0.30,
            segment_weight: 0.20,
            proximity_decay_tokens: 12.0,
            minimum_candidate_pool: 64,
            candidate_pool_multiplier: 16,
            maximum_candidate_pool: 256,
            dense_simd_threshold: 0.35,
            maximum_query_groups: 32,
            maximum_expansions_per_group: 16,
            learned_ranker: LinearRankerV1::disabled(),
            v3_exact_identifier_fields: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DocumentInput<'a> {
    pub external_id: u64,
    pub fields: &'a [&'a str],
}

#[derive(Clone, Copy, Debug)]
pub struct Expansion<'a> {
    pub term: &'a str,
    pub quality: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct QueryGroup<'a> {
    pub expansions: &'a [Expansion<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchHit {
    pub document: DocumentId,
    pub external_id: u64,
    pub score: f32,
    /// Frozen hand-designed V2 score retained only for diagnostics and the
    /// explicit rollback path. V3 never reads this value as a feature.
    pub v2_score: f32,
    pub lexical_score: f32,
    pub coverage: f32,
    pub proximity: f32,
    pub order: f32,
    pub phrase: f32,
    pub segment: f32,
    pub exact_field: f32,
    /// Experimental Phase 8.6 primitive: field-local compactness of all
    /// matched query groups. This is not part of the canonical V3 schema.
    pub matched_group_locality: f32,
    /// Experimental Phase 8.7 primitive: fraction of candidate-independent
    /// query rarity mass represented by matched groups. This is not part of
    /// the canonical V3 schema.
    pub rarity_weighted_group_coverage: f32,
    /// Fixed-width evidence used by the optional learned ranker. Keeping it on
    /// the returned hit makes failures auditable without retaining postings.
    pub rank_features: RankFeatureVector,
    /// Primitive-only V3 evidence. The V2 final score and candidate-strength
    /// composite are deliberately absent from this schema.
    pub rank_evidence_v3: RankEvidenceV3,
    /// Hard constitutional tier computed before any V3 learned score.
    pub relevance_tier: crate::RelevanceTier,
}

/// Offline-only, caller-owned capture of selected per-query-group lexical
/// evidence. Values are stored hit-major in one flat buffer so evidence
/// regeneration does not allocate one vector per candidate. This is not read
/// by candidate generation, V2 scoring, constitutional tiers, or V3 serving.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupStrengthBatch {
    group_count: usize,
    hit_count: usize,
    values: Vec<f32>,
}

impl GroupStrengthBatch {
    pub fn with_capacity(hit_capacity: usize, group_capacity: usize) -> Self {
        Self {
            group_count: 0,
            hit_count: 0,
            values: Vec::with_capacity(hit_capacity.saturating_mul(group_capacity)),
        }
    }

    pub fn group_count(&self) -> usize {
        self.group_count
    }

    pub fn hit_count(&self) -> usize {
        self.hit_count
    }

    pub fn strengths(&self, hit_index: usize) -> Option<&[f32]> {
        if hit_index >= self.hit_count {
            return None;
        }
        let start = hit_index * self.group_count;
        Some(&self.values[start..start + self.group_count])
    }

    pub(crate) fn prepare(&mut self, hit_count: usize, group_count: usize) {
        self.group_count = group_count;
        self.hit_count = hit_count;
        self.values.clear();
        self.values
            .resize(hit_count.saturating_mul(group_count), 0.0);
    }

    pub(crate) fn set(&mut self, hit_index: usize, group: usize, value: f32) {
        self.values[hit_index * self.group_count + group] = value;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchReceipt {
    pub query_groups: u16,
    /// Documents with at least one matching expansion posting.
    pub candidates: u32,
    /// Documents which passed the coverage gate.
    pub covered_candidates: u32,
    /// Documents whose position payloads were opened.
    pub reranked_candidates: u32,
    pub posting_rows_visited: u32,
    pub position_values_visited: u32,
    pub selection: CandidateSelection,
    pub allocations_grew: bool,
    pub stages: SearchStageNanos,
    /// REDLINE Phase 3: rows the classic accumulator would read (sum of
    /// involved posting-list lengths). Equals `posting_rows_visited` unless
    /// block-max skipping ran.
    pub posting_rows_available: u32,
    /// Posting blocks jumped without reading (block-max path only).
    pub blocks_skipped: u32,
    /// Pivot documents dead on arrival by coverage impossibility.
    pub coverage_impossible_deaths: u32,
    /// Pivot documents dead on arrival by score impossibility.
    pub score_bound_deaths: u32,
    /// REDLINE Phase 3B: literal documents finalized by the fused merge.
    /// Zero for the classic and block-max differential paths.
    pub literal_documents_finalized: u32,
    /// REDLINE Phase 3B: matched-group choices materialized for bounded
    /// survivors. Choices are never retained for rejected documents.
    pub literal_choice_records_materialized: u32,
    /// Documents which never entered corpus-indexed accumulation scratch on
    /// the fused path.
    pub literal_scratch_documents_avoided: u32,
    /// REDLINE Phase 4 transport-collapse metrics. These remain zero for
    /// literal-only queries.
    pub transport_raw_expansion_rows: u32,
    pub transport_unique_group_documents: u32,
    pub transport_nonliteral_winner_documents: u32,
    pub transport_literal_winner_documents: u32,
    pub transport_winning_expansion_rows: u32,
    pub transport_rows_never_winner: u32,
    pub transport_winner_rows_outside_pool: u32,
    pub transport_touched_uncovered: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchStageNanos {
    pub accumulation: u64,
    pub selection: u64,
    pub coherence: u64,
    pub ordering: u64,
    pub total: u64,
}

/// Read-only REDLINE Phase 3C attribution receipt. This diagnostic never
/// participates in serving; it separates literal merge, selection, and null
/// traversal costs before any Phase 4 transport work is attempted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Phase3cDiagnostics {
    pub posting_rows: u32,
    pub documents_finalized: u32,
    pub covered_candidates: u32,
    pub candidate_limit: u32,
    pub merge_flat_nanos: u64,
    pub flat_select_nanos: u64,
    pub classic_accum_nanos: u64,
    pub classic_heap_nanos: u64,
    pub merge_null_nanos: u64,
    pub classic_null_nanos: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CandidateSelection {
    #[default]
    SparseTouched,
    DenseSimd,
    Exhaustive,
}

#[derive(Debug, thiserror::Error)]
pub enum QpsError {
    #[error("at least one field configuration is required")]
    MissingFields,
    #[error("field {field} has invalid ranking parameters")]
    InvalidField { field: &'static str },
    #[error("invalid QPS configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("document fields did not match the configured field count")]
    FieldCountMismatch,
    #[error("duplicate external document ID {0}")]
    DuplicateExternalId(u64),
    #[error("document ID space is exhausted")]
    DocumentIdOverflow,
    #[error("search hit document ID is not present in this index")]
    InvalidDocument,
    #[error("packed index address space is exhausted")]
    IndexAddressOverflow,
    #[error("field ID space is exhausted")]
    FieldIdOverflow,
    #[error("segment ID space is exhausted")]
    SegmentIdOverflow,
    #[error("query has no groups")]
    EmptyQuery,
    #[error("query exceeds the configured group or expansion bound")]
    QueryTooLarge,
    #[error("query expansion quality must be finite and in (0, 1]")]
    InvalidExpansionQuality,
    #[error("each query expansion must normalize to exactly one token")]
    InvalidExpansionTerm,
    #[error("V3 ranker artifact is missing, corrupt, or incompatible")]
    InvalidV3Ranker,
}

impl fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}
