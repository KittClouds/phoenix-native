use std::fmt;

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
    pub lexical_score: f32,
    pub coverage: f32,
    pub proximity: f32,
    pub order: f32,
    pub phrase: f32,
    pub segment: f32,
    pub exact_field: f32,
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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchStageNanos {
    pub accumulation: u64,
    pub selection: u64,
    pub coherence: u64,
    pub ordering: u64,
    pub total: u64,
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
}

impl fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}
