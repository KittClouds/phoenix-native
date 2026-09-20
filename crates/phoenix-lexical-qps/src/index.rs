use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::mem::size_of;
use std::time::Instant;

use compact_str::CompactString;
use hashbrown::HashMap;
use pulp::{Arch, Simd, WithSimd};

use crate::rank_evidence::{
    RankEvidenceInputs, RankEvidenceV3, RelevanceTier, RANK_EVIDENCE_V3_FIELD_SLOTS,
};
use crate::ranker::RankFeatureVector;
use crate::ranker_v3::LinearRankerV3;
use crate::score::{
    measure_field_with_evidence, ordered_fraction, Coherence, CoherenceSignals,
    FieldMeasurementOptions, GroupMask, PositionedGroups,
};
use crate::selection::{consider, finish, retain_dense_simd, retain_sparse, RankedCandidate};
use crate::tokenize::{tokenize_into, TokenOccurrence};
use crate::types::{
    CandidateSelection, DocumentId, FieldConfig, GroupStrengthBatch, Phase3cDiagnostics, QpsConfig,
    QpsError, QueryGroup, SearchHit, SearchReceipt, SearchStageNanos,
};

const NO_CHOICE: u32 = u32::MAX;

/// Allocation-free V3 rank kernel for a caller-owned candidate slice. The V2
/// diagnostic score remains on each hit but is never read by this function.
pub fn rerank_v3_in_place(
    model: &LinearRankerV3,
    candidates: &mut [SearchHit],
) -> Result<(), QpsError> {
    if !model.is_valid()
        || candidates
            .iter()
            .any(|candidate| !candidate.rank_evidence_v3.is_valid())
    {
        return Err(QpsError::InvalidV3Ranker);
    }
    rerank_v3_prevalidated(model, candidates, usize::MAX);
    Ok(())
}

/// Serving kernel for candidates whose evidence was produced by this QPS
/// index in the same query execution. The model is checked once; the
/// already-proven evidence schema is not redundantly rescanned.
pub fn rerank_v3_generated_candidates_in_place(
    model: &LinearRankerV3,
    candidates: &mut [SearchHit],
) -> Result<(), QpsError> {
    if !model.is_valid() {
        return Err(QpsError::InvalidV3Ranker);
    }
    rerank_v3_prevalidated(model, candidates, usize::MAX);
    Ok(())
}

/// Stable top-k serving variant. All candidates are scored, but only the best
/// `top_k` prefix is ordered; callers may truncate the tail without sorting it.
pub fn rerank_v3_generated_top_k_in_place(
    model: &LinearRankerV3,
    candidates: &mut [SearchHit],
    top_k: usize,
) -> Result<(), QpsError> {
    if !model.is_valid() {
        return Err(QpsError::InvalidV3Ranker);
    }
    rerank_v3_prevalidated(model, candidates, top_k);
    Ok(())
}

#[inline]
fn rerank_v3_prevalidated(model: &LinearRankerV3, candidates: &mut [SearchHit], top_k: usize) {
    debug_assert!(model.is_valid());
    Arch::new().dispatch(RankV3Scores { model, candidates });
    if top_k == 0 {
        return;
    }
    if top_k < candidates.len() {
        candidates.select_nth_unstable_by(top_k, compare_v3_hits);
        candidates[..top_k].sort_unstable_by(compare_v3_hits);
    } else {
        candidates.sort_unstable_by(compare_v3_hits);
    }
}

/// REDLINE Phase 1: V3 serving kernel over hot sort records. Scores the same
/// linear model over the same evidence values as `rerank_v3_prevalidated`;
/// only the score destination and comparator record type differ.
#[inline]
fn rerank_v3_hot(
    model: &LinearRankerV3,
    candidates: &mut [HotCandidate],
    cold: &[ColdEvidence],
    top_k: usize,
) {
    debug_assert!(model.is_valid());
    Arch::new().dispatch(RankV3HotScores {
        model,
        candidates,
        cold,
    });
    if top_k == 0 {
        return;
    }
    if top_k < candidates.len() {
        candidates.select_nth_unstable_by(top_k, compare_v3_hot);
        candidates[..top_k].sort_unstable_by(compare_v3_hot);
    } else {
        candidates.sort_unstable_by(compare_v3_hot);
    }
}

#[inline]
fn compare_v3_hot(left: &HotCandidate, right: &HotCandidate) -> Ordering {
    left.tier.compare_ranked(
        left.score,
        left.external_id,
        right.tier,
        right.score,
        right.external_id,
    )
}

#[inline]
fn compare_v3_hits(left: &SearchHit, right: &SearchHit) -> Ordering {
    left.relevance_tier.compare_ranked(
        left.score,
        left.external_id,
        right.relevance_tier,
        right.score,
        right.external_id,
    )
}

struct RankV3Scores<'a> {
    model: &'a LinearRankerV3,
    candidates: &'a mut [SearchHit],
}

struct RankV3HotScores<'a> {
    model: &'a LinearRankerV3,
    candidates: &'a mut [HotCandidate],
    cold: &'a [ColdEvidence],
}

impl WithSimd for RankV3Scores<'_> {
    type Output = ();

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) {
        let (packed_weights, tail_weights) = S::as_simd_f32s(&self.model.weights);
        for candidate in self.candidates {
            let (packed_values, tail_values) = S::as_simd_f32s(&candidate.rank_evidence_v3.values);
            let mut packed_sum = simd.splat_f32s(0.0);
            for (&weights, &values) in packed_weights.iter().zip(packed_values) {
                packed_sum = simd.mul_add_f32s(weights, values, packed_sum);
            }
            let mut score = simd.reduce_sum_f32s(packed_sum);
            for (&weight, &value) in tail_weights.iter().zip(tail_values) {
                score = weight.mul_add(value, score);
            }
            candidate.score = score;
        }
    }
}

impl WithSimd for RankV3HotScores<'_> {
    type Output = ();

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) {
        let (packed_weights, tail_weights) = S::as_simd_f32s(&self.model.weights);
        for candidate in self.candidates.iter_mut() {
            let values = &self.cold[candidate.evidence as usize]
                .rank_evidence_v3
                .values;
            let (packed_values, tail_values) = S::as_simd_f32s(values);
            let mut packed_sum = simd.splat_f32s(0.0);
            for (&weights, &values) in packed_weights.iter().zip(packed_values) {
                packed_sum = simd.mul_add_f32s(weights, values, packed_sum);
            }
            let mut score = simd.reduce_sum_f32s(packed_sum);
            for (&weight, &value) in tail_weights.iter().zip(tail_values) {
                score = weight.mul_add(value, score);
            }
            candidate.score = score;
        }
    }
}

#[inline]
fn coverage_factor(coverage: f32, exponent: f32) -> f32 {
    if exponent == 2.0 {
        coverage * coverage
    } else if exponent == 1.0 {
        coverage
    } else if exponent == 0.0 {
        1.0
    } else {
        coverage.powf(exponent)
    }
}

#[inline]
fn unit_saturating(value: f32) -> f32 {
    let positive = value.max(0.0);
    positive / (1.0 + positive)
}

/// REDLINE Phase 3: bounded candidate-pool limit shared by the classic
/// selector and the block-max heap cap (the latter passes `None` for the
/// covered count, which is unknown before traversal, and truncates after).
#[inline]
fn bounded_candidate_limit(
    config: &QpsConfig,
    group_count: usize,
    candidate_top_k: usize,
    covered: Option<usize>,
) -> usize {
    let requested = if group_count == 1 {
        // A single group has no inter-term proximity, order, or phrase
        // signal. Retain room for field-exact reranking without paying the
        // multi-term ambiguity budget.
        candidate_top_k.saturating_mul(4).max(candidate_top_k)
    } else {
        config
            .minimum_candidate_pool
            .max(candidate_top_k.saturating_mul(config.candidate_pool_multiplier))
    };
    let capped = requested.min(config.maximum_candidate_pool);
    match covered {
        Some(covered) => capped.min(covered),
        None => capped,
    }
}

/// Candidate-generation order is a V3 primitive. Sorting the bounded pool by
/// the already-computed generation score does not alter its set or V2's
/// later stable final ordering.
fn sort_pool_by_generation_score(scratch: &mut SearchScratch) {
    let candidate_scores = &scratch.candidate_scores;
    scratch.selected_candidates.sort_unstable_by(|left, right| {
        candidate_scores[*right as usize]
            .total_cmp(&candidate_scores[*left as usize])
            .then_with(|| left.cmp(right))
    });
}

struct RankFeatureInputs {
    baseline: f32,
    lexical: f32,
    coverage: f32,
    coherence: Coherence,
    candidate_score: f32,
    maximum_candidate_score: f32,
    token_count: u32,
    expansion_quality: f32,
}

#[derive(Clone, Copy, Debug)]
struct PrimitiveCoherence {
    matched_group_locality: f32,
    minimum_complete_span: u32,
    minimum_ordered_span: u32,
    ordered_fraction: f32,
    exact_phrase: bool,
    exact_field: bool,
    exact_identifier_field: bool,
}

#[derive(Clone, Copy, Debug)]
struct CandidateEvidenceContext {
    document: u32,
    candidate_rank: usize,
    candidate_pool_size: usize,
    lexical: f32,
    coverage: f32,
    coherence: PrimitiveCoherence,
}

impl Default for PrimitiveCoherence {
    fn default() -> Self {
        Self {
            matched_group_locality: 0.0,
            minimum_complete_span: u32::MAX,
            minimum_ordered_span: u32::MAX,
            ordered_fraction: 0.0,
            exact_phrase: false,
            exact_field: false,
            exact_identifier_field: false,
        }
    }
}

#[inline]
fn rank_features(inputs: RankFeatureInputs) -> RankFeatureVector {
    let candidate_strength = if inputs.maximum_candidate_score > 0.0 {
        (inputs.candidate_score / inputs.maximum_candidate_score).clamp(0.0, 1.0)
    } else {
        0.0
    };
    RankFeatureVector([
        unit_saturating(inputs.baseline),
        unit_saturating(inputs.lexical),
        inputs.coverage.clamp(0.0, 1.0),
        f32::from(inputs.coverage >= 1.0 - f32::EPSILON),
        inputs.coherence.proximity.clamp(0.0, 1.0),
        inputs.coherence.order.clamp(0.0, 1.0),
        inputs.coherence.phrase.clamp(0.0, 1.0),
        inputs.coherence.segment.clamp(0.0, 1.0),
        unit_saturating(inputs.coherence.exact_field),
        candidate_strength,
        1.0 / (1.0 + inputs.token_count as f32).sqrt(),
        inputs.expansion_quality,
    ])
}

#[derive(Clone, Debug)]
pub(crate) struct StoredField {
    pub terms: Box<[u32]>,
    pub segments: Box<[u16]>,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredDocument {
    pub external_id: u64,
    pub active: bool,
    pub fields: Box<[StoredField]>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DocumentMeta {
    pub external_id: u64,
    pub token_count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(crate) struct FieldRange {
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(crate) struct PostingRange {
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct PostingRecord {
    pub document: u32,
    /// Precomputed document-level BM25F impact.
    pub impact: f32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct PostingPosition {
    pub position: u32,
    pub segment: u16,
    pub field: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexStats {
    pub documents: usize,
    pub terms: usize,
    pub posting_rows: usize,
    pub positions: usize,
    pub estimated_bytes: usize,
}

pub struct QpsIndex {
    config: QpsConfig,
    field_configs: Box<[FieldConfig]>,
    term_ids: HashMap<CompactString, u32>,
    terms: Box<[CompactString]>,
    documents: Box<[DocumentMeta]>,
    field_ranges: Box<[FieldRange]>,
    posting_ranges: Box<[PostingRange]>,
    postings: Box<[PostingRecord]>,
    /// Exact per-field decomposition of each posting's BM25F impact. This is a
    /// cold sidecar so the hot posting row remains eight bytes.
    posting_field_impacts: Box<[f32]>,
    /// Normalized IDF primitive for each term.
    term_rarities: Box<[f32]>,
    /// Cold random-access offsets are split from hot accumulation rows. The
    /// next start (or the position-array end) closes each posting range.
    posting_position_starts: Box<[u32]>,
    posting_positions: Box<[PostingPosition]>,
    /// REDLINE Phase 3: block metadata per term posting list (64-row blocks).
    posting_block_ranges: Box<[PostingRange]>,
    posting_block_max: Box<[f32]>,
    posting_block_end: Box<[u32]>,
}

/// Posting rows per block-max block (REDLINE Phase 3, predeclared; 3B may
/// evaluate a second arm).
pub(crate) const REDLINE_BLOCK_ROWS: u32 = 64;
/// Row-count floor below which block-max traversal cannot pay for itself;
/// smaller workloads run the classic accumulator bit-identically.
pub(crate) const BMW_ROW_THRESHOLD: usize = 4096;
// Phase 3B is exact but demoted after Phase 3C attribution: literal serving
// stays on the classic accumulator while the fused path remains diagnostic.
const LITERAL_FUSED_SERVING_ENABLED: bool = false;
// Phase 3 BMW is retained as a qualification arm until its hot-loop latency
// gate passes. Unit tests exercise it; normal binaries fail closed to the
// already-qualified classic accumulator.
#[cfg(test)]
const BMW_SERVING_ENABLED: bool = true;
#[cfg(not(test))]
const BMW_SERVING_ENABLED: bool = false;

/// Phase 4 is a census, not serving work. Keep the receipt counters and
/// winner scans out of normal release queries; the release probe opts in with
/// `PHOENIX_QPS_PHASE4_DIAGNOSTICS=1`. Unit tests keep the census enabled so
/// the accounting assertions remain active.
#[inline]
fn phase4_diagnostics_requested() -> bool {
    cfg!(test) || std::env::var_os("PHOENIX_QPS_PHASE4_DIAGNOSTICS").is_some()
}

pub struct SearchScratch {
    epoch: u32,
    group_epoch: u32,
    document_stamp: Vec<u32>,
    group_stamp: Vec<u32>,
    lexical: Vec<f32>,
    coverage_weight: Vec<f32>,
    group_best_score: Vec<f32>,
    group_best_quality: Vec<f32>,
    group_best_choice: Vec<u32>,
    /// Resolved expansion index for the current group's winning posting.
    /// This keeps the Phase 4 census off the posting-range lookup path.
    group_best_expansion: Vec<u32>,
    candidate_scores: Vec<f32>,
    choices: Vec<u32>,
    choice_stamp: Vec<u32>,
    touched: Vec<u32>,
    group_documents: Vec<u32>,
    selected_candidates: Vec<u32>,
    candidate_heap: BinaryHeap<Reverse<RankedCandidate>>,
    dense_candidates: Vec<RankedCandidate>,
    query_expansions: Vec<ResolvedExpansion>,
    query_ranges: Vec<PostingRange>,
    query_group_rarity: Vec<f32>,
    query_tokens: Vec<TokenOccurrence>,
    query_token_buffer: String,
    position_epoch: u32,
    position_stamp: Vec<u32>,
    position_groups: Vec<GroupMask>,
    position_segments: Vec<u16>,
    position_fields: Vec<u16>,
    touched_positions: Vec<u32>,
    positioned_groups: Vec<PositionedGroups>,
    chosen_postings: Vec<Option<u32>>,
    /// Hot sort records (REDLINE Phase 1). The ordering kernels shuffle
    /// these instead of 240-byte `SearchHit` values; cold evidence lives in
    /// `cold_evidence` indexed by `HotCandidate::evidence`.
    hot_candidates: Vec<HotCandidate>,
    cold_evidence: Vec<ColdEvidence>,
    /// Running top scores for the REDLINE Phase 2 V2 score gate (best-first,
    /// capped at the output limit). Empty unless lazy evaluation is active.
    v2_top_scores: Vec<f32>,
    /// REDLINE Phase 3: per-group posting-list cursors for block-max
    /// traversal, plus concatenated per-group suffix-maximum arrays.
    wand_cursors: Vec<WandCursor>,
    wand_order: Vec<u32>,
    wand_suffix: Vec<f32>,
    /// REDLINE Phase 3B: bounded survivor slots for the fused literal merge.
    /// Slots are recycled when a candidate falls below the exact heap floor.
    literal_candidates: Vec<LiteralCandidate>,
    literal_free_slots: Vec<u32>,
    literal_choice_buffer: Vec<u32>,
    literal_choices_work: Vec<u32>,
    literal_matched_groups: Vec<u32>,
    literal_heap: BinaryHeap<Reverse<LiteralHeapEntry>>,
    literal_cursors: Vec<LiteralCursor>,
    /// REDLINE Phase 4: expansion-local winner census. Each byte corresponds
    /// to one resolved expansion in the current transport query.
    transport_winning_expansions: Vec<u8>,
    transport_pool_winners: Vec<u8>,
    transport_outside_winners: Vec<u8>,
}

/// REDLINE Phase 3: traversal state for one literal query group over its
/// term's posting list. `pos` is an absolute index into `postings`;
/// `block` indexes the term's block metadata; `curr` caches the document at
/// `pos` (`u32::MAX` when exhausted) so pivot scans read no postings.
#[derive(Clone, Copy, Debug, Default)]
struct WandCursor {
    quality: f32,
    range_start: u32,
    range_end: u32,
    pos: u32,
    curr: u32,
    block: u32,
    block_base: u32,
    suffix_base: u32,
}

/// One bounded survivor from the fused literal stream. Choice storage lives
/// in a fixed-stride scratch buffer so evicted documents do not leave behind
/// corpus-sized metadata.
#[derive(Clone, Copy, Debug, Default)]
struct LiteralCandidate {
    document: u32,
    score: f32,
    lexical: f32,
    coverage: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct LiteralCursor {
    quality: f32,
    pos: u32,
    end: u32,
    curr: u32,
}

#[derive(Clone, Copy, Debug)]
struct LiteralHeapEntry {
    ranked: RankedCandidate,
    slot: u32,
}

impl PartialEq for LiteralHeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.ranked == other.ranked && self.slot == other.slot
    }
}

impl Eq for LiteralHeapEntry {}

impl PartialOrd for LiteralHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LiteralHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ranked
            .cmp(&other.ranked)
            .then_with(|| self.slot.cmp(&other.slot))
    }
}

/// Outcome of block-max accumulation. `visited` counts posting rows
/// actually read; skipped rows are `available - visited` exactly.
#[derive(Clone, Copy, Debug, Default)]
struct BmwAccum {
    visited: u32,
    covered: u32,
    maximum_score: f32,
    blocks_skipped: u32,
    coverage_deaths: u32,
    score_deaths: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct LiteralAccum {
    visited: u32,
    covered: u32,
    maximum_score: f32,
    documents_finalized: u32,
}

/// Relative slack for MaxScore bound comparisons (same doctrine as the
/// Phase 2 score gate: strict proof only, ties evaluate).
const BMW_SLACK_REL: f64 = 1e-6;

/// Hot sort record: everything the ordering kernels compare, nothing more.
/// Eviction of any field here breaks a comparator; addition breaks the size
/// budget (asserted in tests).
#[derive(Clone, Copy, Debug)]
struct HotCandidate {
    external_id: u64,
    document: u32,
    evidence: u32,
    score: f32,
    tier: RelevanceTier,
}

/// Cold per-candidate evidence. Never sorted; only pushed during candidate
/// production and read (by index) during final materialization.
#[derive(Clone, Copy, Debug)]
struct ColdEvidence {
    v2_score: f32,
    lexical_score: f32,
    coverage: f32,
    proximity: f32,
    order: f32,
    phrase: f32,
    segment: f32,
    exact_field: f32,
    matched_group_locality: f32,
    rarity_weighted_group_coverage: f32,
    rank_features: RankFeatureVector,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[derive(Clone, Copy)]
struct ResolvedExpansion {
    term: u32,
    quality: f32,
}

#[derive(Clone, Copy)]
enum SearchMode {
    Bounded,
    Exhaustive,
    /// REDLINE Phase 3: bounded candidate semantics via the classic full-scan
    /// accumulator. Production paths never construct this; it exists so
    /// differential tests can prove the block-max path equivalent.
    #[cfg_attr(not(test), allow(dead_code))]
    Classic,
    /// REDLINE Phase 3B differential mode. It forces the fused literal
    /// kernel in unit tests while leaving the public bounded API unchanged.
    #[cfg(test)]
    Fused,
}

#[inline]
fn bounded_literal_mode(mode: SearchMode) -> bool {
    match mode {
        SearchMode::Bounded => true,
        #[cfg(test)]
        SearchMode::Fused => true,
        SearchMode::Exhaustive | SearchMode::Classic => false,
    }
}

#[inline]
fn forced_fused_mode(mode: SearchMode) -> bool {
    #[cfg(test)]
    {
        matches!(mode, SearchMode::Fused)
    }
    #[cfg(not(test))]
    {
        let _ = mode;
        false
    }
}

#[derive(Clone, Copy)]
struct SearchResolvedRequest<'a> {
    candidate_top_k: usize,
    output_limit: usize,
    mode: SearchMode,
    v3_ranker: Option<&'a LinearRankerV3>,
    collect_v3_primitive_evidence: bool,
}

impl SearchScratch {
    pub fn new() -> Self {
        Self {
            epoch: 0,
            group_epoch: 0,
            document_stamp: Vec::new(),
            group_stamp: Vec::new(),
            lexical: Vec::new(),
            coverage_weight: Vec::new(),
            group_best_score: Vec::new(),
            group_best_quality: Vec::new(),
            group_best_choice: Vec::new(),
            group_best_expansion: Vec::new(),
            candidate_scores: Vec::new(),
            choices: Vec::new(),
            choice_stamp: Vec::new(),
            touched: Vec::new(),
            group_documents: Vec::new(),
            selected_candidates: Vec::new(),
            candidate_heap: BinaryHeap::new(),
            dense_candidates: Vec::new(),
            query_expansions: Vec::new(),
            query_ranges: Vec::new(),
            query_group_rarity: Vec::new(),
            query_tokens: Vec::new(),
            query_token_buffer: String::with_capacity(64),
            position_epoch: 0,
            position_stamp: Vec::new(),
            position_groups: Vec::new(),
            position_segments: Vec::new(),
            position_fields: Vec::new(),
            touched_positions: Vec::new(),
            positioned_groups: Vec::new(),
            chosen_postings: Vec::new(),
            hot_candidates: Vec::new(),
            cold_evidence: Vec::new(),
            v2_top_scores: Vec::new(),
            wand_cursors: Vec::new(),
            wand_order: Vec::new(),
            wand_suffix: Vec::new(),
            literal_candidates: Vec::new(),
            literal_free_slots: Vec::new(),
            literal_choice_buffer: Vec::new(),
            literal_choices_work: Vec::new(),
            literal_matched_groups: Vec::new(),
            literal_heap: BinaryHeap::new(),
            literal_cursors: Vec::new(),
            transport_winning_expansions: Vec::new(),
            transport_pool_winners: Vec::new(),
            transport_outside_winners: Vec::new(),
        }
    }

    pub fn with_document_capacity(documents: usize, maximum_query_groups: usize) -> Self {
        let mut scratch = Self::new();
        scratch.prepare(documents, maximum_query_groups);
        scratch
    }

    fn prepare(&mut self, documents: usize, maximum_query_groups: usize) {
        if self.document_stamp.len() < documents {
            self.document_stamp.resize(documents, 0);
            self.group_stamp.resize(documents, 0);
            self.lexical.resize(documents, 0.0);
            self.coverage_weight.resize(documents, 0.0);
            self.group_best_score.resize(documents, 0.0);
            self.group_best_quality.resize(documents, 0.0);
            self.group_best_choice.resize(documents, NO_CHOICE);
            self.group_best_expansion.resize(documents, NO_CHOICE);
            self.candidate_scores.resize(documents, 0.0);
        }
        let choices = documents.saturating_mul(maximum_query_groups);
        if self.choices.len() < choices {
            self.choices.resize(choices, NO_CHOICE);
            self.choice_stamp.resize(choices, 0);
        }
        self.touched.reserve(documents.min(4_096));
        self.group_documents.reserve(documents.min(4_096));
        self.selected_candidates.reserve(documents.min(256));
        self.candidate_heap.reserve(documents.min(256));
        self.dense_candidates.reserve(documents.min(8_192));
        self.query_expansions.reserve(maximum_query_groups);
        self.query_ranges.reserve(maximum_query_groups);
        self.query_group_rarity.reserve(maximum_query_groups);
        self.query_tokens.reserve(maximum_query_groups);
        self.query_token_buffer.reserve(64);
        self.chosen_postings.reserve(maximum_query_groups);
        self.wand_cursors.reserve(maximum_query_groups);
        self.wand_order.reserve(maximum_query_groups);
        self.literal_candidates.reserve(256);
        self.literal_free_slots.reserve(256);
        self.literal_choices_work.reserve(maximum_query_groups);
        self.literal_matched_groups.reserve(maximum_query_groups);
        self.literal_heap.reserve(256);
        self.literal_cursors.reserve(maximum_query_groups);
        let expansion_capacity = maximum_query_groups.saturating_mul(64);
        self.transport_winning_expansions
            .reserve(expansion_capacity);
        self.transport_pool_winners.reserve(expansion_capacity);
        self.transport_outside_winners.reserve(expansion_capacity);
    }

    fn begin_query(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.document_stamp.fill(0);
            self.choice_stamp.fill(0);
            self.epoch = 1;
        }
        for document in self.touched.drain(..) {
            self.candidate_scores[document as usize] = 0.0;
        }
        self.selected_candidates.clear();
        self.candidate_heap.clear();
        self.dense_candidates.clear();
        self.query_expansions.clear();
        self.query_ranges.clear();
        self.query_group_rarity.clear();
        self.hot_candidates.clear();
        self.cold_evidence.clear();
        self.v2_top_scores.clear();
        self.wand_cursors.clear();
        self.wand_order.clear();
        self.wand_suffix.clear();
        self.literal_candidates.clear();
        self.literal_free_slots.clear();
        self.literal_choice_buffer.clear();
        self.literal_choices_work.clear();
        self.literal_matched_groups.clear();
        self.literal_heap.clear();
        self.literal_cursors.clear();
        self.transport_winning_expansions.clear();
        self.transport_pool_winners.clear();
        self.transport_outside_winners.clear();
    }

    fn begin_group(&mut self) {
        self.group_epoch = self.group_epoch.wrapping_add(1);
        if self.group_epoch == 0 {
            self.group_stamp.fill(0);
            self.group_epoch = 1;
        }
        self.group_documents.clear();
    }

    fn capacity_fingerprint(&self) -> usize {
        self.document_stamp.capacity()
            + self.group_stamp.capacity()
            + self.lexical.capacity()
            + self.coverage_weight.capacity()
            + self.group_best_score.capacity()
            + self.group_best_quality.capacity()
            + self.group_best_choice.capacity()
            + self.group_best_expansion.capacity()
            + self.candidate_scores.capacity()
            + self.choices.capacity()
            + self.choice_stamp.capacity()
            + self.touched.capacity()
            + self.group_documents.capacity()
            + self.selected_candidates.capacity()
            + self.candidate_heap.capacity()
            + self.dense_candidates.capacity()
            + self.query_expansions.capacity()
            + self.query_ranges.capacity()
            + self.literal_candidates.capacity()
            + self.literal_free_slots.capacity()
            + self.literal_choice_buffer.capacity()
            + self.literal_choices_work.capacity()
            + self.literal_matched_groups.capacity()
            + self.literal_heap.capacity()
            + self.literal_cursors.capacity()
            + self.transport_winning_expansions.capacity()
            + self.transport_pool_winners.capacity()
            + self.transport_outside_winners.capacity()
            + self.query_group_rarity.capacity()
            + self.query_tokens.capacity()
            + self.query_token_buffer.capacity()
            + self.position_stamp.capacity()
            + self.position_groups.capacity()
            + self.position_segments.capacity()
            + self.position_fields.capacity()
            + self.touched_positions.capacity()
            + self.positioned_groups.capacity()
            + self.chosen_postings.capacity()
            + self.hot_candidates.capacity()
            + self.cold_evidence.capacity()
            + self.v2_top_scores.capacity()
            + self.wand_cursors.capacity()
            + self.wand_order.capacity()
            + self.wand_suffix.capacity()
    }

    fn begin_positions(&mut self, capacity: usize) {
        self.position_epoch = self.position_epoch.wrapping_add(1);
        if self.position_epoch == 0 {
            self.position_stamp.fill(0);
            self.position_epoch = 1;
        }
        self.position_stamp.resize(capacity, 0);
        self.position_groups.resize(capacity, GroupMask::default());
        self.position_segments.resize(capacity, 0);
        self.position_fields.resize(capacity, 0);
        self.touched_positions.clear();
        self.positioned_groups.clear();
    }
}

impl Default for SearchScratch {
    fn default() -> Self {
        Self::new()
    }
}

fn consider_literal_candidate(
    heap: &mut BinaryHeap<Reverse<LiteralHeapEntry>>,
    candidates: &mut Vec<LiteralCandidate>,
    free_slots: &mut Vec<u32>,
    choice_buffer: &mut [u32],
    group_count: usize,
    limit: usize,
    ranked: RankedCandidate,
    lexical: f32,
    coverage: f32,
    choices: &[u32],
) {
    if ranked.score <= 0.0 || limit == 0 {
        return;
    }
    if heap.len() >= limit {
        let Some(threshold) = heap.peek() else {
            return;
        };
        if ranked <= threshold.0.ranked {
            return;
        }
        let evicted = heap.pop().expect("heap threshold exists").0;
        free_slots.push(evicted.slot);
    }
    let slot = free_slots.pop().unwrap_or_else(|| {
        let slot = candidates.len() as u32;
        candidates.push(LiteralCandidate::default());
        slot
    });
    let slot_start = slot as usize * group_count;
    choice_buffer[slot_start..slot_start + group_count].copy_from_slice(&choices[..group_count]);
    candidates[slot as usize] = LiteralCandidate {
        document: ranked.document,
        score: ranked.score,
        lexical,
        coverage,
    };
    heap.push(Reverse(LiteralHeapEntry { ranked, slot }));
}

impl QpsIndex {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        config: QpsConfig,
        field_configs: Box<[FieldConfig]>,
        term_ids: HashMap<CompactString, u32>,
        terms: Box<[CompactString]>,
        documents: Box<[DocumentMeta]>,
        field_ranges: Box<[FieldRange]>,
        posting_ranges: Box<[PostingRange]>,
        postings: Box<[PostingRecord]>,
        posting_field_impacts: Box<[f32]>,
        term_rarities: Box<[f32]>,
        posting_position_starts: Box<[u32]>,
        posting_positions: Box<[PostingPosition]>,
        posting_block_ranges: Box<[PostingRange]>,
        posting_block_max: Box<[f32]>,
        posting_block_end: Box<[u32]>,
    ) -> Self {
        Self {
            config,
            field_configs,
            term_ids,
            terms,
            documents,
            field_ranges,
            posting_ranges,
            postings,
            posting_field_impacts,
            term_rarities,
            posting_position_starts,
            posting_positions,
            posting_block_ranges,
            posting_block_max,
            posting_block_end,
        }
    }

    pub fn search_into(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Bounded,
            None,
            false,
        )
    }

    /// V3 serving path. V2 still performs query planning, traversal, bounded
    /// candidate selection, and primitive-evidence production. Constitutional
    /// tiers and this immutable model exclusively own final ordering.
    pub fn search_v3_into(
        &self,
        query: &str,
        top_k: usize,
        model: &LinearRankerV3,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        if !model.is_valid() {
            return Err(QpsError::InvalidV3Ranker);
        }
        self.search_text_with_mode(
            query,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Bounded,
            Some(model),
            true,
        )
    }

    /// Offline receipt path which preserves the serving candidate selection for
    /// `top_k` but returns evidence for every selected candidate. Callers must
    /// pre-size `output` when allocation accounting matters. This never runs on
    /// the serving path.
    pub fn search_evidence_into(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            top_k,
            usize::MAX,
            scratch,
            output,
            SearchMode::Bounded,
            None,
            true,
        )
    }

    /// Offline V3 receipt path returning the complete frozen V2 candidate pool.
    pub fn search_v3_evidence_into(
        &self,
        query: &str,
        top_k: usize,
        model: &LinearRankerV3,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        if !model.is_valid() {
            return Err(QpsError::InvalidV3Ranker);
        }
        self.search_text_with_mode(
            query,
            top_k,
            usize::MAX,
            scratch,
            output,
            SearchMode::Bounded,
            Some(model),
            true,
        )
    }

    /// Exhaustive positional oracle used to prove bounded candidate recall.
    /// This is intentionally not the serving path.
    pub fn search_exhaustive_into(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Exhaustive,
            None,
            false,
        )
    }

    /// Offline exhaustive evidence path. This preserves the complete frozen
    /// candidate universe while also materializing V3 primitive evidence for
    /// authority microscopes and feature diagnostics. It is never a serving
    /// path and must not be used for latency claims.
    pub fn search_exhaustive_evidence_into(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Exhaustive,
            None,
            true,
        )
    }

    /// REDLINE Phase 3C diagnostic-only decomposition for an all-literal
    /// query. It intentionally bypasses the serving result and reports the
    /// cost of merge, flat selection, classic accumulation, heap selection,
    /// and null traversal. No serving state or thresholds are changed.
    pub fn phase3c_diagnostics(
        &self,
        query: &str,
        candidate_top_k: usize,
        scratch: &mut SearchScratch,
    ) -> Result<Phase3cDiagnostics, QpsError> {
        scratch.prepare(self.documents.len(), self.config.maximum_query_groups);
        scratch.begin_query();
        tokenize_into(
            query,
            &mut scratch.query_tokens,
            &mut scratch.query_token_buffer,
        );
        if scratch.query_tokens.is_empty() {
            return Err(QpsError::EmptyQuery);
        }
        if scratch.query_tokens.len() > self.config.maximum_query_groups {
            return Err(QpsError::QueryTooLarge);
        }
        for token in &scratch.query_tokens {
            let start = scratch.query_expansions.len() as u32;
            let Some(term) = self.term_ids.get(token.token.as_str()) else {
                return Err(QpsError::InvalidExpansionTerm);
            };
            scratch.query_expansions.push(ResolvedExpansion {
                term: *term,
                quality: 1.0,
            });
            scratch.query_ranges.push(PostingRange { start, len: 1 });
        }
        let group_count = scratch.query_ranges.len();
        let total_weight = group_count as f32;
        let candidate_limit =
            bounded_candidate_limit(&self.config, group_count, candidate_top_k, None);

        let classic_started = Instant::now();
        let mut posting_rows = 0_u32;
        for group in 0..group_count {
            let expansion = scratch.query_expansions[group];
            posting_rows =
                posting_rows.saturating_add(self.score_exact_group(group, expansion, scratch));
        }
        let mut covered_candidates = 0_u32;
        for &document in &scratch.touched {
            let index = document as usize;
            let coverage = scratch.coverage_weight[index] / total_weight;
            if coverage >= self.config.coverage_floor {
                scratch.candidate_scores[index] = scratch.lexical[index]
                    * coverage_factor(coverage, self.config.coverage_exponent);
                covered_candidates = covered_candidates.saturating_add(1);
            }
        }
        let classic_accum_nanos = elapsed_nanos(classic_started);

        let heap_started = Instant::now();
        let mut heap = BinaryHeap::new();
        let mut heap_selected = Vec::with_capacity(candidate_limit);
        retain_sparse(
            &scratch.candidate_scores,
            &scratch.touched,
            candidate_limit.min(covered_candidates as usize),
            &mut heap,
            &mut heap_selected,
        );
        let classic_heap_nanos = elapsed_nanos(heap_started);

        let mut flat = Vec::with_capacity(covered_candidates as usize);
        let merge_started = Instant::now();
        let documents_finalized = self.merge_literal_flat(scratch, total_weight, &mut flat);
        let merge_flat_nanos = elapsed_nanos(merge_started);
        let flat_select_started = Instant::now();
        let flat_limit = candidate_limit.min(flat.len());
        if flat.len() > flat_limit {
            flat.select_nth_unstable_by(flat_limit, |left, right| right.cmp(left));
            flat.truncate(flat_limit);
        }
        let flat_select_nanos = elapsed_nanos(flat_select_started);

        let merge_null_started = Instant::now();
        let merged_rows = self.merge_literal_null(scratch);
        std::hint::black_box(merged_rows);
        let merge_null_nanos = elapsed_nanos(merge_null_started);

        let classic_null_started = Instant::now();
        let mut null_rows = 0_u32;
        for range in &scratch.query_ranges {
            let expansion = scratch.query_expansions[range.start as usize];
            for row in self.posting_slice(self.posting_ranges[expansion.term as usize]) {
                std::hint::black_box(row.document);
                null_rows = null_rows.saturating_add(1);
            }
        }
        std::hint::black_box(null_rows);
        let classic_null_nanos = elapsed_nanos(classic_null_started);

        Ok(Phase3cDiagnostics {
            posting_rows,
            documents_finalized,
            covered_candidates,
            candidate_limit: candidate_limit as u32,
            merge_flat_nanos,
            flat_select_nanos,
            classic_accum_nanos,
            classic_heap_nanos,
            merge_null_nanos,
            classic_null_nanos,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn search_text_with_mode(
        &self,
        query: &str,
        candidate_top_k: usize,
        output_limit: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        mode: SearchMode,
        v3_ranker: Option<&LinearRankerV3>,
        collect_v3_primitive_evidence: bool,
    ) -> Result<SearchReceipt, QpsError> {
        scratch.prepare(self.documents.len(), self.config.maximum_query_groups);
        scratch.begin_query();
        tokenize_into(
            query,
            &mut scratch.query_tokens,
            &mut scratch.query_token_buffer,
        );
        if scratch.query_tokens.is_empty() {
            return Err(QpsError::EmptyQuery);
        }
        if scratch.query_tokens.len() > self.config.maximum_query_groups {
            return Err(QpsError::QueryTooLarge);
        }
        for token in &scratch.query_tokens {
            let start = scratch.query_expansions.len() as u32;
            if let Some(term) = self.term_ids.get(token.token.as_str()) {
                scratch.query_expansions.push(ResolvedExpansion {
                    term: *term,
                    quality: 1.0,
                });
            }
            scratch.query_ranges.push(PostingRange {
                start,
                len: scratch.query_expansions.len() as u32 - start,
            });
        }
        self.search_resolved(
            scratch,
            output,
            SearchResolvedRequest {
                candidate_top_k,
                output_limit,
                mode,
                v3_ranker,
                collect_v3_primitive_evidence,
            },
        )
    }

    pub fn search_groups_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(
            groups,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Bounded,
            None,
            false,
        )
    }

    pub fn search_groups_v3_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        model: &LinearRankerV3,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        if !model.is_valid() {
            return Err(QpsError::InvalidV3Ranker);
        }
        self.search_groups_with_mode(
            groups,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Bounded,
            Some(model),
            true,
        )
    }

    /// Explicit-group counterpart to [`Self::search_evidence_into`].
    pub fn search_groups_evidence_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(
            groups,
            top_k,
            usize::MAX,
            scratch,
            output,
            SearchMode::Bounded,
            None,
            true,
        )
    }

    pub fn search_groups_v3_evidence_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        model: &LinearRankerV3,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        if !model.is_valid() {
            return Err(QpsError::InvalidV3Ranker);
        }
        self.search_groups_with_mode(
            groups,
            top_k,
            usize::MAX,
            scratch,
            output,
            SearchMode::Bounded,
            Some(model),
            true,
        )
    }

    /// Reconstructs the selected lexical contribution for every query group
    /// and returned candidate from the immediately preceding evidence search.
    ///
    /// This diagnostic path must be called before `scratch` begins another
    /// query. It performs no posting traversal and cannot affect the candidate
    /// pool, V2 score, tier, or final order. Strength is the selected posting's
    /// expansion-quality-weighted BM25F field contribution transformed by
    /// `x / (1 + x)`; unmatched groups remain zero.
    pub fn capture_group_strengths_into(
        &self,
        scratch: &SearchScratch,
        hits: &[SearchHit],
        output: &mut GroupStrengthBatch,
    ) -> Result<(), QpsError> {
        let group_count = scratch.query_ranges.len();
        if group_count == 0 || group_count > self.config.maximum_query_groups {
            return Err(QpsError::EmptyQuery);
        }
        output.prepare(hits.len(), group_count);
        let field_count = self.field_configs.len();
        for (hit_index, hit) in hits.iter().enumerate() {
            let document = hit.document.0 as usize;
            if document >= self.documents.len() {
                return Err(QpsError::InvalidDocument);
            }
            let mut reconstructed_lexical = 0.0_f32;
            for group in 0..group_count {
                let choice = document * self.config.maximum_query_groups + group;
                if scratch.choice_stamp[choice] != scratch.epoch {
                    continue;
                }
                let posting = scratch.choices[choice];
                let expansion = self
                    .chosen_expansion(group, posting, scratch)
                    .ok_or(QpsError::InvalidExpansionTerm)?;
                let field_start = posting as usize * field_count;
                let contribution = self.posting_field_impacts
                    [field_start..field_start + field_count]
                    .iter()
                    .copied()
                    .sum::<f32>()
                    * expansion.quality;
                if !contribution.is_finite() || contribution < 0.0 {
                    return Err(QpsError::InvalidExpansionQuality);
                }
                reconstructed_lexical += contribution;
                output.set(hit_index, group, unit_saturating(contribution));
            }
            debug_assert!(
                (reconstructed_lexical - hit.lexical_score).abs()
                    <= 1.0e-4 * hit.lexical_score.max(1.0),
                "group-strength decomposition must reproduce lexical evidence"
            );
        }
        Ok(())
    }

    /// Exhaustive oracle for explicit expansion groups.
    pub fn search_groups_exhaustive_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(
            groups,
            top_k,
            top_k,
            scratch,
            output,
            SearchMode::Exhaustive,
            None,
            false,
        )
    }

    /// REDLINE Phase 3: test-only classic-accumulation twin of the bounded
    /// serving paths. Production code never constructs `Classic`; the
    /// differential suite uses these to prove block-max equivalence.
    #[cfg(test)]
    pub(crate) fn search_text_classic_into(
        &self,
        query: &str,
        candidate_top_k: usize,
        output_limit: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        collect_v3_primitive_evidence: bool,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            candidate_top_k,
            output_limit,
            scratch,
            output,
            SearchMode::Classic,
            None,
            collect_v3_primitive_evidence,
        )
    }

    /// REDLINE Phase 3B differential twin for single-expansion literal
    /// queries. This is test-only so the production API keeps one bounded
    /// serving contract while the fused stream remains directly comparable
    /// with the classic accumulator.
    #[cfg(test)]
    pub(crate) fn search_text_fused_into(
        &self,
        query: &str,
        candidate_top_k: usize,
        output_limit: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        collect_v3_primitive_evidence: bool,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(
            query,
            candidate_top_k,
            output_limit,
            scratch,
            output,
            SearchMode::Fused,
            None,
            collect_v3_primitive_evidence,
        )
    }

    /// REDLINE Phase 3: test-only classic-accumulation twin for explicit
    /// expansion groups (serving and evidence limits).
    #[cfg(test)]
    pub(crate) fn search_groups_classic_into(
        &self,
        groups: &[QueryGroup<'_>],
        candidate_top_k: usize,
        output_limit: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        collect_v3_primitive_evidence: bool,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(
            groups,
            candidate_top_k,
            output_limit,
            scratch,
            output,
            SearchMode::Classic,
            None,
            collect_v3_primitive_evidence,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn search_groups_with_mode(
        &self,
        groups: &[QueryGroup<'_>],
        candidate_top_k: usize,
        output_limit: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        mode: SearchMode,
        v3_ranker: Option<&LinearRankerV3>,
        collect_v3_primitive_evidence: bool,
    ) -> Result<SearchReceipt, QpsError> {
        if groups.is_empty() {
            return Err(QpsError::EmptyQuery);
        }
        if groups.len() > self.config.maximum_query_groups {
            return Err(QpsError::QueryTooLarge);
        }
        scratch.prepare(self.documents.len(), self.config.maximum_query_groups);
        scratch.begin_query();
        for group in groups {
            if group.expansions.is_empty()
                || group.expansions.len() > self.config.maximum_expansions_per_group
            {
                return Err(QpsError::QueryTooLarge);
            }
            let start = scratch.query_expansions.len() as u32;
            for expansion in group.expansions {
                if !expansion.quality.is_finite()
                    || expansion.quality <= 0.0
                    || expansion.quality > 1.0
                {
                    return Err(QpsError::InvalidExpansionQuality);
                }
                tokenize_into(
                    expansion.term,
                    &mut scratch.query_tokens,
                    &mut scratch.query_token_buffer,
                );
                if scratch.query_tokens.len() != 1 {
                    return Err(QpsError::InvalidExpansionTerm);
                }
                if let Some(term) = self.term_ids.get(scratch.query_tokens[0].token.as_str()) {
                    scratch.query_expansions.push(ResolvedExpansion {
                        term: *term,
                        quality: expansion.quality,
                    });
                }
            }
            scratch.query_ranges.push(PostingRange {
                start,
                len: scratch.query_expansions.len() as u32 - start,
            });
        }
        self.search_resolved(
            scratch,
            output,
            SearchResolvedRequest {
                candidate_top_k,
                output_limit,
                mode,
                v3_ranker,
                collect_v3_primitive_evidence,
            },
        )
    }

    fn search_resolved(
        &self,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        request: SearchResolvedRequest<'_>,
    ) -> Result<SearchReceipt, QpsError> {
        let SearchResolvedRequest {
            candidate_top_k,
            output_limit,
            mode,
            v3_ranker,
            collect_v3_primitive_evidence,
        } = request;
        let total_started = Instant::now();
        let capacity_before = scratch.capacity_fingerprint() + output.capacity();
        output.clear();
        let group_count = scratch.query_ranges.len();
        let has_transport_groups = scratch.query_ranges.iter().any(|range| range.len > 1);
        let collect_transport_diagnostics = has_transport_groups && phase4_diagnostics_requested();
        if collect_transport_diagnostics {
            let expansion_count = scratch.query_expansions.len();
            scratch
                .transport_winning_expansions
                .resize(expansion_count, 0);
            scratch.transport_pool_winners.resize(expansion_count, 0);
            scratch.transport_outside_winners.resize(expansion_count, 0);
        }
        // REDLINE Phase 3/3B triggers. WAND remains a test-only qualification
        // arm; production bounded literals use the fused exact stream.
        let all_single = scratch.query_ranges.iter().all(|range| range.len == 1);
        let mut rows_available = 0_u32;
        if all_single && bounded_literal_mode(mode) {
            for range in scratch.query_ranges.iter() {
                let term = scratch.query_expansions[range.start as usize].term;
                rows_available =
                    rows_available.saturating_add(self.posting_ranges[term as usize].len);
            }
        }
        let use_bmw = BMW_SERVING_ENABLED
            && matches!(mode, SearchMode::Bounded)
            && all_single
            && (rows_available as usize) > BMW_ROW_THRESHOLD;
        let use_literal_fused = !use_bmw
            && all_single
            && (forced_fused_mode(mode)
                || (LITERAL_FUSED_SERVING_ENABLED && bounded_literal_mode(mode)));
        let total_weight = group_count as f32;
        let limit_bound = bounded_candidate_limit(&self.config, group_count, candidate_top_k, None);
        let mut visited = 0_u32;
        let mut covered_candidates = 0_u32;
        let mut maximum_candidate_score = 0.0_f32;
        let mut blocks_skipped = 0_u32;
        let mut coverage_deaths = 0_u32;
        let mut score_deaths = 0_u32;
        let mut limit_new = 0_usize;
        let mut literal_documents_finalized = 0_u32;
        let mut literal_choice_records_materialized = 0_u32;
        let mut literal_scratch_documents_avoided = 0_u32;
        let mut transport_raw_expansion_rows = 0_u32;
        let mut transport_unique_group_documents = 0_u32;
        let mut transport_nonliteral_winner_documents = 0_u32;
        let mut transport_literal_winner_documents = 0_u32;
        let mut transport_winning_expansion_rows = 0_u32;
        let mut transport_rows_never_winner = 0_u32;
        let mut transport_winner_rows_outside_pool = 0_u32;
        let mut transport_touched_uncovered = 0_u32;
        if use_bmw {
            let outcome = self.accumulate_bmw(
                scratch,
                group_count,
                group_count as f32,
                limit_bound,
                rows_available,
            );
            visited = outcome.visited;
            covered_candidates = outcome.covered;
            maximum_candidate_score = outcome.maximum_score;
            blocks_skipped = outcome.blocks_skipped;
            coverage_deaths = outcome.coverage_deaths;
            score_deaths = outcome.score_deaths;
            // No truncation: when pruning engaged, covered_old >= limit_bound
            // so the heap cap already equals the classic limit; otherwise
            // nothing was pruned and the heap holds the classic pool.
            // (An earlier revision truncated here and broke pool equality.)
            limit_new = limit_bound;
        } else if use_literal_fused {
            let outcome = self.accumulate_literal_fused(
                scratch,
                group_count,
                total_weight,
                limit_bound,
                rows_available,
            );
            visited = outcome.visited;
            covered_candidates = outcome.covered;
            maximum_candidate_score = outcome.maximum_score;
            literal_documents_finalized = outcome.documents_finalized;
            rows_available = visited;
            literal_choice_records_materialized =
                self.materialize_literal_survivors(scratch, group_count);
            literal_scratch_documents_avoided = literal_documents_finalized
                .saturating_sub(scratch.selected_candidates.len() as u32);
        } else {
            for group in 0..group_count {
                let range = scratch.query_ranges[group];
                if range.len == 1 {
                    let expansion = scratch.query_expansions[range.start as usize];
                    visited =
                        visited.saturating_add(self.score_exact_group(group, expansion, scratch));
                    continue;
                }

                let rows_before_group = visited;
                scratch.begin_group();
                for expansion_index in range.start..range.start.saturating_add(range.len) {
                    let expansion = scratch.query_expansions[expansion_index as usize];
                    visited = visited.saturating_add(self.score_expansion(
                        expansion_index as usize,
                        expansion,
                        scratch,
                    ));
                }
                for &document in &scratch.group_documents {
                    let index = document as usize;
                    if scratch.document_stamp[index] != scratch.epoch {
                        scratch.document_stamp[index] = scratch.epoch;
                        scratch.lexical[index] = 0.0;
                        scratch.coverage_weight[index] = 0.0;
                        scratch.touched.push(document);
                    }
                    scratch.lexical[index] += scratch.group_best_score[index];
                    scratch.coverage_weight[index] += scratch.group_best_quality[index];
                    let choice = index * self.config.maximum_query_groups + group;
                    scratch.choices[choice] = scratch.group_best_choice[index];
                    scratch.choice_stamp[choice] = scratch.epoch;
                    if collect_transport_diagnostics {
                        let expansion_index = scratch.group_best_expansion[index] as usize;
                        if expansion_index != NO_CHOICE as usize {
                            scratch.transport_winning_expansions[expansion_index] = 1;
                            if expansion_index == range.start as usize {
                                transport_literal_winner_documents =
                                    transport_literal_winner_documents.saturating_add(1);
                            } else {
                                transport_nonliteral_winner_documents =
                                    transport_nonliteral_winner_documents.saturating_add(1);
                            }
                        }
                    }
                }
                if collect_transport_diagnostics {
                    transport_raw_expansion_rows = transport_raw_expansion_rows
                        .saturating_add(visited.saturating_sub(rows_before_group));
                    transport_unique_group_documents = transport_unique_group_documents
                        .saturating_add(scratch.group_documents.len() as u32);
                }
            }

            for &document in &scratch.touched {
                let index = document as usize;
                let coverage = scratch.coverage_weight[index] / total_weight;
                if coverage >= self.config.coverage_floor {
                    scratch.candidate_scores[index] = scratch.lexical[index]
                        * coverage_factor(coverage, self.config.coverage_exponent);
                    maximum_candidate_score =
                        maximum_candidate_score.max(scratch.candidate_scores[index]);
                    covered_candidates = covered_candidates.saturating_add(1);
                }
            }
            if collect_transport_diagnostics {
                transport_touched_uncovered =
                    (scratch.touched.len() as u32).saturating_sub(covered_candidates);
                for (expansion_index, &winning) in
                    scratch.transport_winning_expansions.iter().enumerate()
                {
                    if winning == 0 {
                        continue;
                    }
                    let expansion = scratch.query_expansions[expansion_index];
                    transport_winning_expansion_rows = transport_winning_expansion_rows
                        .saturating_add(self.posting_ranges[expansion.term as usize].len);
                }
                transport_rows_never_winner =
                    transport_raw_expansion_rows.saturating_sub(transport_winning_expansion_rows);
            }
            // Classic path reads every involved row: available equals visited.
            rows_available = visited;
        }
        let accumulation_nanos = elapsed_nanos(total_started);

        let selection_started = Instant::now();
        let candidate_limit = match mode {
            SearchMode::Exhaustive => covered_candidates as usize,
            SearchMode::Bounded | SearchMode::Classic => bounded_candidate_limit(
                &self.config,
                group_count,
                candidate_top_k,
                Some(covered_candidates as usize),
            ),
            #[cfg(test)]
            SearchMode::Fused => bounded_candidate_limit(
                &self.config,
                group_count,
                candidate_top_k,
                Some(covered_candidates as usize),
            ),
        };
        // REDLINE Phase 3: the block-max path arrives with its heap already
        // capped; only the classic path runs the selector. The density label
        // is recomputed (not executed) so the receipt stays regime-honest.
        let selection = if use_bmw || use_literal_fused {
            let density = covered_candidates as f32 / self.documents.len().max(1) as f32;
            if density >= self.config.dense_simd_threshold {
                CandidateSelection::DenseSimd
            } else {
                CandidateSelection::SparseTouched
            }
        } else {
            match mode {
                SearchMode::Exhaustive => {
                    scratch.selected_candidates.clear();
                    scratch.selected_candidates.extend(
                        scratch
                            .touched
                            .iter()
                            .copied()
                            .filter(|document| scratch.candidate_scores[*document as usize] > 0.0),
                    );
                    CandidateSelection::Exhaustive
                }
                SearchMode::Bounded | SearchMode::Classic => {
                    let density = covered_candidates as f32 / self.documents.len().max(1) as f32;
                    if density >= self.config.dense_simd_threshold {
                        retain_dense_simd(
                            &scratch.candidate_scores,
                            candidate_limit,
                            &mut scratch.dense_candidates,
                            &mut scratch.selected_candidates,
                        );
                        CandidateSelection::DenseSimd
                    } else {
                        retain_sparse(
                            &scratch.candidate_scores,
                            &scratch.touched,
                            candidate_limit,
                            &mut scratch.candidate_heap,
                            &mut scratch.selected_candidates,
                        );
                        CandidateSelection::SparseTouched
                    }
                }
                #[cfg(test)]
                SearchMode::Fused => CandidateSelection::SparseTouched,
            }
        };
        sort_pool_by_generation_score(scratch);
        if use_bmw {
            scratch.selected_candidates.truncate(limit_new);
        }
        let selection_nanos = elapsed_nanos(selection_started);

        if collect_transport_diagnostics && !use_bmw && !use_literal_fused {
            for value in &mut scratch.transport_pool_winners {
                *value = 0;
            }
            for value in &mut scratch.transport_outside_winners {
                *value = 0;
            }
            for &document in &scratch.touched {
                let in_pool = scratch.selected_candidates.contains(&document);
                let index = document as usize;
                for group in 0..group_count {
                    let range = scratch.query_ranges[group];
                    if range.len <= 1 {
                        continue;
                    }
                    let choice = index * self.config.maximum_query_groups + group;
                    if scratch.choice_stamp[choice] != scratch.epoch {
                        continue;
                    }
                    if let Some(expansion_index) =
                        self.chosen_expansion_index(group, scratch.choices[choice], scratch)
                    {
                        let winners = if in_pool {
                            &mut scratch.transport_pool_winners
                        } else {
                            &mut scratch.transport_outside_winners
                        };
                        winners[expansion_index] = 1;
                    }
                }
            }
            for expansion_index in 0..scratch.query_expansions.len() {
                if scratch.transport_outside_winners[expansion_index] == 1
                    && scratch.transport_pool_winners[expansion_index] == 0
                {
                    let expansion = scratch.query_expansions[expansion_index];
                    transport_winner_rows_outside_pool = transport_winner_rows_outside_pool
                        .saturating_add(self.posting_ranges[expansion.term as usize].len);
                }
            }
        }

        let coherence_started = Instant::now();
        let need_v2_positional_scoring = self.config.proximity_weight != 0.0
            || self.config.order_weight != 0.0
            || self.config.phrase_weight != 0.0
            || self.config.segment_weight != 0.0
            || self
                .field_configs
                .iter()
                .any(|field| field.exact_match_bonus != 0.0);
        let need_v3_primitive_evidence = v3_ranker.is_some() || collect_v3_primitive_evidence;
        let need_positions = need_v2_positional_scoring || need_v3_primitive_evidence;
        let query_rarity_total = if need_v3_primitive_evidence {
            self.prepare_query_group_rarity(scratch)
        } else {
            0.0
        };
        // REDLINE Phase 2: exact lazy evaluation. Serving paths whose output
        // is truncated (output_limit < pool) may skip positions + full
        // evidence for candidates proven unable to enter top-k. Evidence
        // paths (output_limit >= pool) always evaluate everything.
        let lazy_active = output_limit < scratch.selected_candidates.len();
        // V2 score bound needs only candidate_score: baseline <= cs * MAXMULT
        // with coherence in [0,1] and exact_field <= max bonus (both verified
        // against score.rs constructors + best-field aggregation).
        let max_exact_bonus = self
            .field_configs
            .iter()
            .map(|field| field.exact_match_bonus)
            .fold(0.0_f32, f32::max);
        let score_multiplier_bound = 1.0_f64
            + f64::from(self.config.proximity_weight.max(0.0))
            + f64::from(self.config.order_weight.max(0.0))
            + f64::from(self.config.phrase_weight.max(0.0))
            + f64::from(self.config.segment_weight.max(0.0))
            + f64::from(max_exact_bonus);
        let v1_enabled = self.config.learned_ranker.is_enabled();
        let v1_weights = self.config.learned_ranker.weights();
        let all_single_group = scratch.query_ranges.iter().all(|range| range.len == 1);
        let mut tier_counts = [0_u32; 6];
        scratch.v2_top_scores.clear();
        let mut opened_positions = 0_u32;
        let mut position_values_visited = 0_u32;
        for candidate_index in 0..scratch.selected_candidates.len() {
            let document = scratch.selected_candidates[candidate_index];
            let index = document as usize;
            let coverage = scratch.coverage_weight[index] / total_weight;
            // Hoisted group census (previously inline below): matched + exact
            // counts and single-expansion shape. Values identical to what the
            // evidence path recomputes; used here for exact prune bounds.
            let mut matched_groups = 0_usize;
            let mut exact_groups = 0_usize;
            for group in 0..group_count {
                let choice = index * self.config.maximum_query_groups + group;
                if scratch.choice_stamp[choice] != scratch.epoch {
                    continue;
                }
                matched_groups += 1;
                let posting = scratch.choices[choice];
                let Some(expansion) = self.chosen_expansion(group, posting, scratch) else {
                    debug_assert!(false, "chosen posting must belong to its query group");
                    continue;
                };
                if expansion.quality >= 1.0 - f32::EPSILON {
                    exact_groups += 1;
                }
            }
            let complete = matched_groups == group_count;
            let all_exact_known = complete && exact_groups == group_count && all_single_group;
            if lazy_active {
                if v3_ranker.is_some() {
                    // Tier gate: integer-exact, no floats. Optimistic tier is
                    // the strongest tier this candidate could legally reach
                    // (exact identifier flag assumed achievable).
                    let tier_optimistic: u8 = if complete && all_exact_known {
                        if self.config.v3_exact_identifier_fields != 0 {
                            1
                        } else {
                            2
                        }
                    } else if complete {
                        3
                    } else {
                        4
                    };
                    let stronger: u32 = tier_counts[1..tier_optimistic as usize].iter().sum();
                    if stronger >= output_limit as u32 {
                        continue;
                    }
                } else {
                    // Score gate: U(cs) >= actual final score by construction
                    // (non-negative scores, monotone weights, clamped
                    // features); skip only on strict proof plus ulp slack.
                    let candidate_score = f64::from(scratch.candidate_scores[index]);
                    let upper = if v1_enabled {
                        let token_prior =
                            1.0_f64 / (1.0 + f64::from(self.documents[index].token_count)).sqrt();
                        let expansion_quality = if matched_groups == 0 {
                            0.0
                        } else {
                            f64::from(
                                (scratch.coverage_weight[index] / matched_groups as f32)
                                    .clamp(0.0, 1.0),
                            )
                        };
                        let xmax = [
                            candidate_score * score_multiplier_bound,
                            f64::from(scratch.lexical[index]),
                            f64::from(coverage),
                            1.0,
                            1.0,
                            1.0,
                            1.0,
                            1.0,
                            1.0,
                            1.0,
                            token_prior,
                            expansion_quality,
                        ];
                        v1_weights
                            .into_iter()
                            .zip(xmax)
                            .map(|(w, x)| f64::from(w) * x)
                            .sum()
                    } else {
                        candidate_score * score_multiplier_bound
                    };
                    let bound = scratch.v2_top_scores.len() >= output_limit;
                    if bound {
                        let weakest = f64::from(*scratch.v2_top_scores.last().unwrap_or(&0.0));
                        // Strict proof only; ties and the slack band evaluate.
                        if upper + 1e-6 * (1.0 + upper.abs()) < weakest {
                            continue;
                        }
                    }
                }
            }
            let (coherence, primitive_coherence, opened) = if need_positions {
                self.coherence(document, group_count, scratch, need_v3_primitive_evidence)
            } else {
                (Coherence::default(), PrimitiveCoherence::default(), 0)
            };
            position_values_visited = position_values_visited.saturating_add(opened);
            if need_positions {
                opened_positions += 1;
            }
            let multiplier = 1.0
                + self.config.proximity_weight * coherence.proximity
                + self.config.order_weight * coherence.order
                + self.config.phrase_weight * coherence.phrase
                + self.config.segment_weight * coherence.segment
                + coherence.exact_field;
            let lexical = scratch.lexical[index];
            let baseline_score = scratch.candidate_scores[index] * multiplier;
            // Reuses the hoisted group census (identical formula).
            let expansion_quality = if matched_groups == 0 {
                0.0
            } else {
                (scratch.coverage_weight[index] / matched_groups as f32).clamp(0.0, 1.0)
            };
            let features = rank_features(RankFeatureInputs {
                baseline: baseline_score,
                lexical,
                coverage,
                coherence,
                candidate_score: scratch.candidate_scores[index],
                maximum_candidate_score,
                token_count: self.documents[index].token_count,
                expansion_quality,
            });
            let (rank_evidence_v3, rarity_weighted_group_coverage) = self.rank_evidence_v3(
                CandidateEvidenceContext {
                    document,
                    candidate_rank: candidate_index,
                    candidate_pool_size: scratch.selected_candidates.len(),
                    lexical,
                    coverage,
                    coherence: primitive_coherence,
                },
                scratch,
                query_rarity_total,
            );
            let relevance_tier =
                rank_evidence_v3.relevance_tier(primitive_coherence.exact_identifier_field);
            let final_score = self.config.learned_ranker.score(baseline_score, features);
            // REDLINE Phase 2: feed the exact bound trackers for evaluated
            // candidates only. Pruned candidates never reach top-k by proof.
            if lazy_active {
                if v3_ranker.is_some() {
                    tier_counts[relevance_tier as u8 as usize] += 1;
                } else {
                    let top = &mut scratch.v2_top_scores;
                    let position = top.partition_point(|&candidate| candidate > final_score);
                    top.insert(position, final_score);
                    top.truncate(output_limit);
                }
            }
            // REDLINE Phase 1: shuffle a 24-byte hot record through ordering;
            // cold evidence is indexed, never sorted.
            let evidence = scratch.cold_evidence.len() as u32;
            scratch.cold_evidence.push(ColdEvidence {
                v2_score: baseline_score,
                lexical_score: lexical,
                coverage,
                proximity: coherence.proximity,
                order: coherence.order,
                phrase: coherence.phrase,
                segment: coherence.segment,
                exact_field: coherence.exact_field,
                matched_group_locality: primitive_coherence.matched_group_locality,
                rarity_weighted_group_coverage,
                rank_features: features,
                rank_evidence_v3,
                relevance_tier,
            });
            scratch.hot_candidates.push(HotCandidate {
                external_id: self.documents[index].external_id,
                document,
                evidence,
                score: final_score,
                tier: relevance_tier,
            });
        }
        // REDLINE Phase 2: reranked counts honestly opened position payloads.
        // Without lazy evaluation this equals the selected pool, matching
        // historical behavior.
        let reranked_candidates = opened_positions;
        let coherence_nanos = elapsed_nanos(coherence_started);
        let ordering_started = Instant::now();
        if let Some(model) = v3_ranker {
            rerank_v3_hot(
                model,
                &mut scratch.hot_candidates,
                &scratch.cold_evidence,
                output_limit,
            );
        } else {
            scratch.hot_candidates.sort_unstable_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| left.document.cmp(&right.document))
            });
        }
        // Materialize public hits only for the surviving prefix.
        let survivors = scratch.hot_candidates.len().min(output_limit);
        output.reserve(survivors.saturating_sub(output.len()));
        for candidate in scratch.hot_candidates.iter().take(survivors) {
            let cold = &scratch.cold_evidence[candidate.evidence as usize];
            output.push(SearchHit {
                document: DocumentId(candidate.document),
                external_id: candidate.external_id,
                score: candidate.score,
                v2_score: cold.v2_score,
                lexical_score: cold.lexical_score,
                coverage: cold.coverage,
                proximity: cold.proximity,
                order: cold.order,
                phrase: cold.phrase,
                segment: cold.segment,
                exact_field: cold.exact_field,
                matched_group_locality: cold.matched_group_locality,
                rarity_weighted_group_coverage: cold.rarity_weighted_group_coverage,
                rank_features: cold.rank_features,
                rank_evidence_v3: cold.rank_evidence_v3,
                relevance_tier: cold.relevance_tier,
            });
        }
        let ordering_nanos = elapsed_nanos(ordering_started);
        let capacity_after = scratch.capacity_fingerprint() + output.capacity();
        Ok(SearchReceipt {
            query_groups: group_count as u16,
            candidates: if use_literal_fused {
                literal_documents_finalized
            } else {
                scratch.touched.len() as u32
            },
            covered_candidates,
            reranked_candidates,
            posting_rows_visited: visited,
            position_values_visited,
            selection,
            allocations_grew: capacity_after > capacity_before,
            stages: SearchStageNanos {
                accumulation: accumulation_nanos,
                selection: selection_nanos,
                coherence: coherence_nanos,
                ordering: ordering_nanos,
                total: elapsed_nanos(total_started),
            },
            posting_rows_available: rows_available,
            blocks_skipped,
            coverage_impossible_deaths: coverage_deaths,
            score_bound_deaths: score_deaths,
            literal_documents_finalized,
            literal_choice_records_materialized,
            literal_scratch_documents_avoided,
            transport_raw_expansion_rows,
            transport_unique_group_documents,
            transport_nonliteral_winner_documents,
            transport_literal_winner_documents,
            transport_winning_expansion_rows,
            transport_rows_never_winner,
            transport_winner_rows_outside_pool,
            transport_touched_uncovered,
        })
    }

    fn merge_literal_flat(
        &self,
        scratch: &mut SearchScratch,
        total_weight: f32,
        output: &mut Vec<RankedCandidate>,
    ) -> u32 {
        scratch.literal_cursors.clear();
        output.clear();
        for range in &scratch.query_ranges {
            let expansion = scratch.query_expansions[range.start as usize];
            let posting_range = self.posting_ranges[expansion.term as usize];
            let mut cursor = LiteralCursor {
                quality: expansion.quality,
                pos: posting_range.start,
                end: posting_range.start.saturating_add(posting_range.len),
                curr: u32::MAX,
            };
            if cursor.pos < cursor.end {
                cursor.curr = self.postings[cursor.pos as usize].document;
            }
            scratch.literal_cursors.push(cursor);
        }
        let mut documents_finalized = 0_u32;
        loop {
            let mut document = u32::MAX;
            for cursor in &scratch.literal_cursors {
                document = document.min(cursor.curr);
            }
            if document == u32::MAX {
                break;
            }
            documents_finalized = documents_finalized.saturating_add(1);
            let mut lexical = 0.0_f32;
            let mut coverage_weight = 0.0_f32;
            for cursor in &mut scratch.literal_cursors {
                if cursor.curr != document {
                    continue;
                }
                let row = self.postings[cursor.pos as usize];
                lexical += row.impact * cursor.quality;
                coverage_weight += cursor.quality;
                cursor.pos += 1;
                cursor.curr = if cursor.pos < cursor.end {
                    self.postings[cursor.pos as usize].document
                } else {
                    u32::MAX
                };
            }
            let coverage = coverage_weight / total_weight;
            if coverage >= self.config.coverage_floor {
                output.push(RankedCandidate {
                    score: lexical * coverage_factor(coverage, self.config.coverage_exponent),
                    document,
                });
            }
        }
        documents_finalized
    }

    fn merge_literal_null(&self, scratch: &mut SearchScratch) -> u32 {
        scratch.literal_cursors.clear();
        for range in &scratch.query_ranges {
            let expansion = scratch.query_expansions[range.start as usize];
            let posting_range = self.posting_ranges[expansion.term as usize];
            let mut cursor = LiteralCursor {
                pos: posting_range.start,
                end: posting_range.start.saturating_add(posting_range.len),
                curr: u32::MAX,
                ..LiteralCursor::default()
            };
            if cursor.pos < cursor.end {
                cursor.curr = self.postings[cursor.pos as usize].document;
            }
            scratch.literal_cursors.push(cursor);
        }
        let mut rows = 0_u32;
        loop {
            let mut document = u32::MAX;
            for cursor in &scratch.literal_cursors {
                document = document.min(cursor.curr);
            }
            if document == u32::MAX {
                break;
            }
            for cursor in &mut scratch.literal_cursors {
                if cursor.curr != document {
                    continue;
                }
                std::hint::black_box(self.postings[cursor.pos as usize].document);
                rows = rows.saturating_add(1);
                cursor.pos += 1;
                cursor.curr = if cursor.pos < cursor.end {
                    self.postings[cursor.pos as usize].document
                } else {
                    u32::MAX
                };
            }
        }
        rows
    }

    /// REDLINE Phase 3B: fuse ordered literal posting traversal, scoring, and
    /// bounded selection. Every posting row is read once and every document
    /// is finalized once; only heap survivors retain per-group choices.
    fn accumulate_literal_fused(
        &self,
        scratch: &mut SearchScratch,
        group_count: usize,
        total_weight: f32,
        limit: usize,
        _rows_available: u32,
    ) -> LiteralAccum {
        if group_count == 3 {
            return self.accumulate_literal_fused_three(scratch, total_weight, limit);
        }
        scratch.literal_cursors.clear();
        scratch.literal_candidates.clear();
        scratch.literal_free_slots.clear();
        scratch.literal_heap.clear();
        scratch.literal_choices_work.clear();
        scratch.literal_choices_work.resize(group_count, NO_CHOICE);
        scratch.literal_matched_groups.clear();
        scratch
            .literal_choice_buffer
            .resize(limit.saturating_mul(group_count), NO_CHOICE);

        let mut visited = 0_u32;
        for group in 0..group_count {
            let range = scratch.query_ranges[group];
            debug_assert_eq!(range.len, 1);
            let expansion = scratch.query_expansions[range.start as usize];
            let posting_range = self.posting_ranges[expansion.term as usize];
            let mut cursor = LiteralCursor {
                quality: expansion.quality,
                pos: posting_range.start,
                end: posting_range.start.saturating_add(posting_range.len),
                curr: u32::MAX,
            };
            if cursor.pos < cursor.end {
                cursor.curr = self.postings[cursor.pos as usize].document;
                visited = visited.saturating_add(1);
            }
            scratch.literal_cursors.push(cursor);
        }

        let mut covered = 0_u32;
        let mut maximum_score = 0.0_f32;
        let mut documents_finalized = 0_u32;
        loop {
            let mut document = u32::MAX;
            for cursor in &scratch.literal_cursors {
                document = document.min(cursor.curr);
            }
            if document == u32::MAX {
                break;
            }
            documents_finalized = documents_finalized.saturating_add(1);
            let mut lexical = 0.0_f32;
            let mut coverage_weight = 0.0_f32;
            scratch.literal_matched_groups.clear();

            for (group, cursor) in scratch.literal_cursors.iter_mut().enumerate() {
                if cursor.curr != document {
                    continue;
                }
                let row = self.postings[cursor.pos as usize];
                lexical += row.impact * cursor.quality;
                coverage_weight += cursor.quality;
                scratch.literal_choices_work[group] = cursor.pos;
                scratch.literal_matched_groups.push(group as u32);
                cursor.pos = cursor.pos.saturating_add(1);
                if cursor.pos < cursor.end {
                    cursor.curr = self.postings[cursor.pos as usize].document;
                    visited = visited.saturating_add(1);
                } else {
                    cursor.curr = u32::MAX;
                }
            }

            let coverage = coverage_weight / total_weight;
            if coverage >= self.config.coverage_floor {
                let score = lexical * coverage_factor(coverage, self.config.coverage_exponent);
                maximum_score = maximum_score.max(score);
                covered = covered.saturating_add(1);
                consider_literal_candidate(
                    &mut scratch.literal_heap,
                    &mut scratch.literal_candidates,
                    &mut scratch.literal_free_slots,
                    &mut scratch.literal_choice_buffer,
                    group_count,
                    limit,
                    RankedCandidate { score, document },
                    lexical,
                    coverage,
                    &scratch.literal_choices_work,
                );
            }
            for &group in &scratch.literal_matched_groups {
                scratch.literal_choices_work[group as usize] = NO_CHOICE;
            }
        }
        LiteralAccum {
            visited,
            covered,
            maximum_score,
            documents_finalized,
        }
    }

    /// Three literal groups are the dominant dense workload. Keeping the
    /// cursor state in registers removes the iterator and small-vector traffic
    /// from the generic k-way merge while preserving the same stream order.
    fn accumulate_literal_fused_three(
        &self,
        scratch: &mut SearchScratch,
        total_weight: f32,
        limit: usize,
    ) -> LiteralAccum {
        scratch.literal_candidates.clear();
        scratch.literal_free_slots.clear();
        scratch.literal_heap.clear();
        scratch
            .literal_choice_buffer
            .resize(limit.saturating_mul(3), NO_CHOICE);
        let mut choices = [NO_CHOICE; 3];

        let mut cursors = [LiteralCursor::default(); 3];
        let mut visited = 0_u32;
        for (group, cursor) in cursors.iter_mut().enumerate() {
            let range = scratch.query_ranges[group];
            let expansion = scratch.query_expansions[range.start as usize];
            let posting_range = self.posting_ranges[expansion.term as usize];
            cursor.quality = expansion.quality;
            cursor.pos = posting_range.start;
            cursor.end = posting_range.start.saturating_add(posting_range.len);
            if cursor.pos < cursor.end {
                cursor.curr = self.postings[cursor.pos as usize].document;
                visited = visited.saturating_add(1);
            } else {
                cursor.curr = u32::MAX;
            }
        }

        let mut covered = 0_u32;
        let mut maximum_score = 0.0_f32;
        let mut documents_finalized = 0_u32;
        loop {
            let mut document = cursors[0].curr;
            if cursors[1].curr < document {
                document = cursors[1].curr;
            }
            if cursors[2].curr < document {
                document = cursors[2].curr;
            }
            if document == u32::MAX {
                break;
            }
            documents_finalized = documents_finalized.saturating_add(1);
            let mut lexical = 0.0_f32;
            let mut coverage_weight = 0.0_f32;
            let mut matched0 = false;
            let mut matched1 = false;
            let mut matched2 = false;

            if cursors[0].curr == document {
                let row = self.postings[cursors[0].pos as usize];
                lexical += row.impact * cursors[0].quality;
                coverage_weight += cursors[0].quality;
                choices[0] = cursors[0].pos;
                matched0 = true;
                cursors[0].pos += 1;
                if cursors[0].pos < cursors[0].end {
                    cursors[0].curr = self.postings[cursors[0].pos as usize].document;
                    visited += 1;
                } else {
                    cursors[0].curr = u32::MAX;
                }
            }
            if cursors[1].curr == document {
                let row = self.postings[cursors[1].pos as usize];
                lexical += row.impact * cursors[1].quality;
                coverage_weight += cursors[1].quality;
                choices[1] = cursors[1].pos;
                matched1 = true;
                cursors[1].pos += 1;
                if cursors[1].pos < cursors[1].end {
                    cursors[1].curr = self.postings[cursors[1].pos as usize].document;
                    visited += 1;
                } else {
                    cursors[1].curr = u32::MAX;
                }
            }
            if cursors[2].curr == document {
                let row = self.postings[cursors[2].pos as usize];
                lexical += row.impact * cursors[2].quality;
                coverage_weight += cursors[2].quality;
                choices[2] = cursors[2].pos;
                matched2 = true;
                cursors[2].pos += 1;
                if cursors[2].pos < cursors[2].end {
                    cursors[2].curr = self.postings[cursors[2].pos as usize].document;
                    visited += 1;
                } else {
                    cursors[2].curr = u32::MAX;
                }
            }

            let coverage = coverage_weight / total_weight;
            if coverage >= self.config.coverage_floor {
                let score = lexical * coverage_factor(coverage, self.config.coverage_exponent);
                maximum_score = maximum_score.max(score);
                covered += 1;
                consider_literal_candidate(
                    &mut scratch.literal_heap,
                    &mut scratch.literal_candidates,
                    &mut scratch.literal_free_slots,
                    &mut scratch.literal_choice_buffer,
                    3,
                    limit,
                    RankedCandidate { score, document },
                    lexical,
                    coverage,
                    &choices,
                );
            }
            if matched0 {
                choices[0] = NO_CHOICE;
            }
            if matched1 {
                choices[1] = NO_CHOICE;
            }
            if matched2 {
                choices[2] = NO_CHOICE;
            }
        }
        LiteralAccum {
            visited,
            covered,
            maximum_score,
            documents_finalized,
        }
    }

    /// Copies only the bounded heap survivors into the existing Phase-2
    /// scratch contract. This is the sole point where corpus-indexed arrays
    /// are touched by the fused path.
    fn materialize_literal_survivors(
        &self,
        scratch: &mut SearchScratch,
        group_count: usize,
    ) -> u32 {
        let mut choice_records = 0_u32;
        while let Some(Reverse(entry)) = scratch.literal_heap.pop() {
            let candidate = scratch.literal_candidates[entry.slot as usize];
            let document = candidate.document;
            let index = document as usize;
            scratch.document_stamp[index] = scratch.epoch;
            scratch.lexical[index] = candidate.lexical;
            scratch.coverage_weight[index] = candidate.coverage * group_count as f32;
            scratch.candidate_scores[index] = candidate.score;
            scratch.touched.push(document);
            scratch.selected_candidates.push(document);
            let base = entry.slot as usize * group_count;
            for group in 0..group_count {
                let choice = scratch.literal_choice_buffer[base + group];
                if choice == NO_CHOICE {
                    continue;
                }
                let destination = index * self.config.maximum_query_groups + group;
                scratch.choices[destination] = choice;
                scratch.choice_stamp[destination] = scratch.epoch;
                choice_records = choice_records.saturating_add(1);
            }
        }
        choice_records
    }

    fn score_exact_group(
        &self,
        group: usize,
        expansion: ResolvedExpansion,
        scratch: &mut SearchScratch,
    ) -> u32 {
        let range = self.posting_ranges[expansion.term as usize];
        let rows = self.posting_slice(range);
        for (offset, row) in rows.iter().enumerate() {
            let document = row.document;
            let score = row.impact * expansion.quality;
            let index = document as usize;
            if scratch.document_stamp[index] != scratch.epoch {
                scratch.document_stamp[index] = scratch.epoch;
                scratch.lexical[index] = 0.0;
                scratch.coverage_weight[index] = 0.0;
                scratch.touched.push(document);
            }
            scratch.lexical[index] += score;
            scratch.coverage_weight[index] += expansion.quality;
            let choice = index * self.config.maximum_query_groups + group;
            scratch.choices[choice] = range.start + offset as u32;
            scratch.choice_stamp[choice] = scratch.epoch;
        }
        range.len
    }

    /// REDLINE Phase 3: exact MaxScore/WAND traversal for literal
    /// (single-expansion) groups. Callers guarantee the trigger conditions.
    /// Produces identical per-document lexical/coverage/choice state as the
    /// classic accumulator for every scored document, in identical
    /// accumulation order (group index order), so downstream stages observe
    /// no difference. Pruned documents are proven below the evolving
    /// top-`limit_bound` threshold (strict gap; ties evaluate).
    #[allow(clippy::too_many_arguments)]
    fn accumulate_bmw(
        &self,
        scratch: &mut SearchScratch,
        group_count: usize,
        total_weight: f32,
        limit_bound: usize,
        rows_available: u32,
    ) -> BmwAccum {
        debug_assert!(group_count > 0 && group_count <= 128);
        let mut outcome = BmwAccum::default();
        scratch.wand_cursors.clear();
        scratch.wand_suffix.clear();
        scratch.candidate_heap.clear();
        for group in 0..group_count {
            let range = scratch.query_ranges[group];
            debug_assert_eq!(range.len, 1);
            let expansion = scratch.query_expansions[range.start as usize];
            let posting = self.posting_ranges[expansion.term as usize];
            let blocks = self.posting_block_ranges[expansion.term as usize];
            let suffix_base = scratch.wand_suffix.len() as u32;
            scratch
                .wand_suffix
                .resize(suffix_base as usize + blocks.len as usize, 0.0);
            let mut running = 0.0_f32;
            for block in (0..blocks.len).rev() {
                let m = self.posting_block_max[(blocks.start + block) as usize];
                running = running.max(m);
                scratch.wand_suffix[(suffix_base + block) as usize] = running;
            }
            let end = posting.start.saturating_add(posting.len);
            scratch.wand_cursors.push(WandCursor {
                quality: expansion.quality,
                range_start: posting.start,
                range_end: end,
                pos: posting.start,
                curr: u32::MAX,
                block: 0,
                block_base: blocks.start,
                suffix_base,
            });
        }
        // Initial landing reads.
        for cursor in scratch.wand_cursors.iter_mut() {
            if cursor.pos < cursor.range_end {
                outcome.visited += 1;
                cursor.curr = self.postings[cursor.pos as usize].document;
            }
        }
        let floor = self.config.coverage_floor;
        let exponent = self.config.coverage_exponent;
        let max_groups = self.config.maximum_query_groups;
        loop {
            // Standard WAND pivoting: cursors are ordered by their current
            // document and the first prefix whose optimistic bound reaches
            // theta becomes the pivot. Prefixes below theta can advance to
            // that pivot without scoring any of the skipped documents.
            scratch.wand_order.clear();
            scratch.wand_order.extend(
                scratch
                    .wand_cursors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, cursor)| {
                        (cursor.curr != u32::MAX).then_some(index as u32)
                    }),
            );
            scratch.wand_order.sort_unstable_by(|left, right| {
                scratch.wand_cursors[*left as usize]
                    .curr
                    .cmp(&scratch.wand_cursors[*right as usize].curr)
                    .then_with(|| left.cmp(right))
            });
            let Some(&first_index) = scratch.wand_order.first() else {
                break;
            };
            let theta = scratch
                .candidate_heap
                .peek()
                .map_or(0.0, |entry| entry.0.score);
            if scratch.candidate_heap.len() >= limit_bound {
                // A block-local MaxScore check removes the per-document WAND
                // loop when even the first cursor's current block cannot
                // reach theta. Only cursors whose current document can still
                // occur inside that block contribute to this bound.
                let first_cursor = scratch.wand_cursors[first_index as usize];
                let block_end_doc =
                    self.posting_block_end[(first_cursor.block_base + first_cursor.block) as usize];
                let mut block_bound = 0.0_f32;
                for &index in &scratch.wand_order {
                    let cursor = scratch.wand_cursors[index as usize];
                    if cursor.curr > block_end_doc {
                        break;
                    }
                    block_bound += cursor.quality
                        * self.posting_block_max[(cursor.block_base + cursor.block) as usize];
                }
                if (block_bound as f64) + BMW_SLACK_REL * (1.0 + (block_bound as f64).abs())
                    < f64::from(theta)
                {
                    self.wand_skip_current_block(
                        scratch,
                        first_index as usize,
                        &mut outcome.visited,
                        &mut outcome.blocks_skipped,
                    );
                    continue;
                }
            }
            let pivot_position = if scratch.candidate_heap.len() < limit_bound {
                Some(0)
            } else {
                let mut bound = 0.0_f32;
                scratch.wand_order.iter().position(|&index| {
                    let cursor = scratch.wand_cursors[index as usize];
                    bound += cursor.quality
                        * scratch.wand_suffix[(cursor.suffix_base + cursor.block) as usize];
                    (bound as f64) + BMW_SLACK_REL * (1.0 + (bound as f64).abs())
                        >= f64::from(theta)
                })
            };
            let Some(pivot_position) = pivot_position else {
                // Every remaining document is below theta. Drain by whole
                // blocks; no posting row in the drained suffix is observed.
                self.wand_drain_tail(scratch, &mut outcome.blocks_skipped);
                break;
            };
            let pivot_doc = scratch.wand_cursors[scratch.wand_order[pivot_position] as usize].curr;
            if scratch.wand_cursors[first_index as usize].curr < pivot_doc {
                self.wand_advance_to(
                    scratch,
                    first_index as usize,
                    pivot_doc,
                    &mut outcome.visited,
                    &mut outcome.blocks_skipped,
                );
                continue;
            }
            // Exact pivot bound over groups currently holding the pivot.
            // curr == pivot implies presence (pos points at the pivot doc).
            let mut coverage_bound = 0.0_f32;
            let mut lexical_bound = 0.0_f32;
            for g in 0..group_count {
                let cursor = &scratch.wand_cursors[g];
                if cursor.curr == pivot_doc {
                    coverage_bound += cursor.quality;
                    lexical_bound += self.postings[cursor.pos as usize].impact * cursor.quality;
                }
            }
            let coverage_possible = coverage_bound / total_weight;
            if coverage_possible < floor {
                outcome.coverage_deaths += 1;
                self.wand_advance_past(scratch, pivot_doc, &mut outcome.visited);
                continue;
            }
            // A pivot document cannot receive future rows from a cursor whose
            // current document is already greater than it. The exact current
            // rows therefore form a tighter safe score bound than a suffix
            // block maximum.
            let bound = lexical_bound * coverage_factor(coverage_possible, exponent);
            if scratch.candidate_heap.len() >= limit_bound
                && (bound as f64) + BMW_SLACK_REL * (1.0 + (bound as f64).abs()) < f64::from(theta)
            {
                outcome.score_deaths += 1;
                // Passed the coverage gate above, so this pruned document is
                // covered: count it to keep covered_candidates exact. Only
                // bulk-drained ranges escape covered accounting (documented).
                outcome.covered += 1;
                self.wand_advance_past(scratch, pivot_doc, &mut outcome.visited);
                continue;
            }
            // Full score in group index order: bit-identical accumulation.
            let index = pivot_doc as usize;
            for g in 0..group_count {
                if scratch.wand_cursors[g].curr != pivot_doc {
                    continue;
                }
                let cursor = scratch.wand_cursors[g];
                debug_assert_eq!(
                    self.postings[cursor.pos as usize].document, pivot_doc,
                    "cursor position must address the pivot document"
                );
                let row = &self.postings[cursor.pos as usize];
                if scratch.document_stamp[index] != scratch.epoch {
                    scratch.document_stamp[index] = scratch.epoch;
                    scratch.lexical[index] = 0.0;
                    scratch.coverage_weight[index] = 0.0;
                    scratch.touched.push(pivot_doc);
                }
                scratch.lexical[index] += row.impact * cursor.quality;
                scratch.coverage_weight[index] += cursor.quality;
                let choice = index * max_groups + g;
                scratch.choices[choice] = cursor.pos;
                scratch.choice_stamp[choice] = scratch.epoch;
            }
            let coverage = scratch.coverage_weight[index] / total_weight;
            if coverage >= floor {
                let score = scratch.lexical[index] * coverage_factor(coverage, exponent);
                scratch.candidate_scores[index] = score;
                outcome.maximum_score = outcome.maximum_score.max(score);
                outcome.covered += 1;
                consider(
                    &mut scratch.candidate_heap,
                    limit_bound,
                    RankedCandidate {
                        score,
                        document: pivot_doc,
                    },
                );
            }
            self.wand_advance_past(scratch, pivot_doc, &mut outcome.visited);
        }
        debug_assert!(
            outcome.visited <= rows_available,
            "visited={} available={} cursors={:?}",
            outcome.visited,
            rows_available,
            scratch.wand_cursors
        );
        finish(
            &mut scratch.candidate_heap,
            &mut scratch.selected_candidates,
        );
        outcome
    }

    /// Advance one WAND cursor to its first document at or after `target`.
    /// Whole blocks whose last document is still below the target are skipped;
    /// the landing block uses a bounded lower-bound search and accounts only
    /// for rows whose document value was actually read.
    fn wand_advance_to(
        &self,
        scratch: &mut SearchScratch,
        cursor_index: usize,
        target: u32,
        visited: &mut u32,
        blocks_skipped: &mut u32,
    ) {
        let cursor = &mut scratch.wand_cursors[cursor_index];
        if cursor.curr == u32::MAX || cursor.curr >= target {
            return;
        }
        let mut jumped = false;
        while cursor.pos < cursor.range_end {
            let block_end = (cursor.range_start + (cursor.block + 1) * REDLINE_BLOCK_ROWS)
                .min(cursor.range_end);
            let block_last = self.posting_block_end[(cursor.block_base + cursor.block) as usize];
            if block_last >= target {
                break;
            }
            *blocks_skipped = blocks_skipped.saturating_add(1);
            cursor.block = cursor.block.saturating_add(1);
            cursor.pos = block_end;
            jumped = true;
        }
        if cursor.pos >= cursor.range_end {
            cursor.curr = u32::MAX;
            return;
        }
        let block_end =
            (cursor.range_start + (cursor.block + 1) * REDLINE_BLOCK_ROWS).min(cursor.range_end);
        let mut position = if jumped {
            cursor.pos
        } else {
            cursor.pos.saturating_add(1)
        };
        while position < block_end {
            let document = self.postings[position as usize].document;
            *visited = visited.saturating_add(1);
            if document >= target {
                cursor.pos = position;
                cursor.curr = document;
                return;
            }
            position = position.saturating_add(1);
        }
        // The block metadata says a target row should exist here. If an index
        // is malformed, fail closed by continuing at the next block rather
        // than exposing a stale current document.
        cursor.pos = block_end;
        cursor.block = cursor.block.saturating_add(1);
        cursor.curr = u32::MAX;
    }

    fn wand_drain_tail(&self, scratch: &mut SearchScratch, blocks_skipped: &mut u32) {
        for cursor in scratch.wand_cursors.iter_mut() {
            while cursor.pos < cursor.range_end {
                let next = (cursor.range_start + (cursor.block + 1) * REDLINE_BLOCK_ROWS)
                    .min(cursor.range_end);
                if next > cursor.pos {
                    *blocks_skipped = blocks_skipped.saturating_add(1);
                }
                cursor.pos = next;
                cursor.block = cursor.block.saturating_add(1);
            }
            cursor.curr = u32::MAX;
        }
    }

    fn wand_skip_current_block(
        &self,
        scratch: &mut SearchScratch,
        cursor_index: usize,
        visited: &mut u32,
        blocks_skipped: &mut u32,
    ) {
        let cursor = &mut scratch.wand_cursors[cursor_index];
        let next =
            (cursor.range_start + (cursor.block + 1) * REDLINE_BLOCK_ROWS).min(cursor.range_end);
        if next <= cursor.pos {
            cursor.curr = u32::MAX;
            return;
        }
        *blocks_skipped = blocks_skipped.saturating_add(1);
        cursor.pos = next;
        cursor.block = cursor.block.saturating_add(1);
        if cursor.pos < cursor.range_end {
            *visited = visited.saturating_add(1);
            cursor.curr = self.postings[cursor.pos as usize].document;
        } else {
            cursor.curr = u32::MAX;
        }
    }

    /// Advance every cursor holding `pivot` one row past it. Each call
    /// consumes at least one row position, so traversal always terminates.
    fn wand_advance_past(&self, scratch: &mut SearchScratch, pivot: u32, visited: &mut u32) {
        for cursor in scratch.wand_cursors.iter_mut() {
            if cursor.curr != pivot {
                continue;
            }
            cursor.pos = cursor.pos.saturating_add(1);
            if cursor.pos < cursor.range_end {
                cursor.block = (cursor.pos - cursor.range_start) / REDLINE_BLOCK_ROWS;
                *visited += 1;
                cursor.curr = self.postings[cursor.pos as usize].document;
            } else {
                cursor.curr = u32::MAX;
            }
        }
    }

    fn score_expansion(
        &self,
        expansion_index: usize,
        expansion: ResolvedExpansion,
        scratch: &mut SearchScratch,
    ) -> u32 {
        let range = self.posting_ranges[expansion.term as usize];
        let rows = self.posting_slice(range);
        for (offset, row) in rows.iter().enumerate() {
            let document = row.document;
            let score = row.impact * expansion.quality;
            let posting = range.start + offset as u32;
            let index = document as usize;
            if scratch.group_stamp[index] != scratch.group_epoch {
                scratch.group_stamp[index] = scratch.group_epoch;
                scratch.group_best_score[index] = score;
                scratch.group_best_quality[index] = expansion.quality;
                scratch.group_best_choice[index] = posting;
                scratch.group_best_expansion[index] = expansion_index as u32;
                scratch.group_documents.push(document);
            } else if score > scratch.group_best_score[index] {
                scratch.group_best_score[index] = score;
                scratch.group_best_quality[index] = expansion.quality;
                scratch.group_best_choice[index] = posting;
                scratch.group_best_expansion[index] = expansion_index as u32;
            }
        }
        range.len
    }

    #[inline]
    fn rank_evidence_v3(
        &self,
        context: CandidateEvidenceContext,
        scratch: &SearchScratch,
        query_rarity_total: f32,
    ) -> (RankEvidenceV3, f32) {
        let group_count = scratch.query_ranges.len();
        let field_count = self.field_configs.len();
        let mut field_lexical = [0.0_f32; RANK_EVIDENCE_V3_FIELD_SLOTS];
        let mut field_lexical_overflow = 0.0_f32;
        let mut matched_groups = 0_usize;
        let mut exact_groups = 0_usize;
        let mut quality_sum = 0.0_f32;
        let mut quality_best = 0.0_f32;
        let mut quality_minimum = 1.0_f32;
        let mut rarity_sum = 0.0_f32;
        let mut rarity_maximum = 0.0_f32;
        let mut matched_rarity_mass = 0.0_f32;
        let mut has_expansions = false;
        let mut all_exact_groups = true;

        for group in 0..group_count {
            let query_range = scratch.query_ranges[group];
            if query_range.len != 1 {
                has_expansions = true;
                all_exact_groups = false;
            }
            let choice = context.document as usize * self.config.maximum_query_groups + group;
            if scratch.choice_stamp[choice] != scratch.epoch {
                all_exact_groups = false;
                continue;
            }
            let posting = scratch.choices[choice];
            let Some(expansion) = self.chosen_expansion(group, posting, scratch) else {
                debug_assert!(false, "chosen posting must belong to its query group");
                continue;
            };
            matched_groups += 1;
            if query_rarity_total > 0.0 {
                matched_rarity_mass += scratch.query_group_rarity[group];
            }
            quality_sum += expansion.quality;
            quality_best = quality_best.max(expansion.quality);
            quality_minimum = quality_minimum.min(expansion.quality);
            let rarity = self.term_rarities[expansion.term as usize];
            rarity_sum += rarity;
            rarity_maximum = rarity_maximum.max(rarity);
            if expansion.quality >= 1.0 - f32::EPSILON {
                exact_groups += 1;
            } else {
                has_expansions = true;
                all_exact_groups = false;
            }
            let field_start = posting as usize * field_count;
            for (field, contribution) in self.posting_field_impacts
                [field_start..field_start + field_count]
                .iter()
                .enumerate()
            {
                let contribution = *contribution * expansion.quality;
                if field < RANK_EVIDENCE_V3_FIELD_SLOTS {
                    field_lexical[field] += contribution;
                } else {
                    field_lexical_overflow += contribution;
                }
            }
        }
        if matched_groups == 0 {
            quality_minimum = 0.0;
        }
        let decomposed = field_lexical.iter().sum::<f32>() + field_lexical_overflow;
        debug_assert!(
            (decomposed - context.lexical).abs() <= 1.0e-4 * context.lexical.max(1.0),
            "per-field BM25F decomposition must sum to lexical evidence"
        );
        let covered_fields = field_lexical
            .iter()
            .take(field_count.min(RANK_EVIDENCE_V3_FIELD_SLOTS))
            .filter(|value| **value > 0.0)
            .count()
            + usize::from(field_lexical_overflow > 0.0);
        let evidence = RankEvidenceV3::from_inputs(RankEvidenceInputs {
            lexical: context.lexical,
            field_lexical,
            field_lexical_overflow,
            weighted_coverage: context.coverage,
            query_groups: group_count,
            matched_groups,
            exact_groups,
            minimum_complete_span: context.coherence.minimum_complete_span,
            minimum_ordered_span: context.coherence.minimum_ordered_span,
            ordered_fraction: context.coherence.ordered_fraction,
            exact_phrase: context.coherence.exact_phrase,
            exact_field: context.coherence.exact_field,
            best_expansion_quality: quality_best,
            mean_expansion_quality: quality_sum / matched_groups.max(1) as f32,
            minimum_expansion_quality: quality_minimum,
            rarest_matched_term: rarity_maximum,
            mean_matched_term_rarity: rarity_sum / matched_groups.max(1) as f32,
            document_tokens: self.documents[context.document as usize].token_count,
            candidate_rank: context.candidate_rank,
            candidate_pool_size: context.candidate_pool_size,
            maximum_candidate_pool: self.config.maximum_candidate_pool,
            maximum_query_groups: self.config.maximum_query_groups,
            field_count,
            field_coverage_fraction: covered_fields as f32 / field_count.max(1) as f32,
            has_expansions,
            all_exact_groups,
        });
        let rarity_weighted_group_coverage = if query_rarity_total > 0.0 {
            (matched_rarity_mass / query_rarity_total).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (evidence, rarity_weighted_group_coverage)
    }

    #[inline]
    fn prepare_query_group_rarity(&self, scratch: &mut SearchScratch) -> f32 {
        scratch.query_group_rarity.clear();
        let mut total = 0.0_f32;
        for range in &scratch.query_ranges {
            let start = range.start as usize;
            let end = range.start.saturating_add(range.len) as usize;
            let weight = scratch.query_expansions[start..end]
                .iter()
                .map(|expansion| expansion.quality * self.term_rarities[expansion.term as usize])
                .fold(0.0_f32, f32::max);
            scratch.query_group_rarity.push(weight);
            total += weight;
        }
        total
    }

    #[inline]
    fn chosen_expansion(
        &self,
        group: usize,
        posting: u32,
        scratch: &SearchScratch,
    ) -> Option<ResolvedExpansion> {
        self.chosen_expansion_index(group, posting, scratch)
            .map(|index| scratch.query_expansions[index])
    }

    #[inline]
    fn chosen_expansion_index(
        &self,
        group: usize,
        posting: u32,
        scratch: &SearchScratch,
    ) -> Option<usize> {
        let query_range = scratch.query_ranges[group];
        scratch.query_expansions
            [query_range.start as usize..query_range.start.saturating_add(query_range.len) as usize]
            .iter()
            .enumerate()
            .find_map(|(offset, expansion)| {
                let posting_range = self.posting_ranges[expansion.term as usize];
                (posting >= posting_range.start
                    && posting < posting_range.start.saturating_add(posting_range.len))
                .then_some(query_range.start as usize + offset)
            })
    }

    fn coherence(
        &self,
        document: u32,
        group_count: usize,
        scratch: &mut SearchScratch,
        collect_primitive_evidence: bool,
    ) -> (Coherence, PrimitiveCoherence, u32) {
        let field_count = self.field_configs.len();
        let final_field = self.field_range(document, field_count - 1);
        let position_capacity = final_field.start.saturating_add(final_field.len) as usize;
        scratch.begin_positions(position_capacity);
        scratch.chosen_postings.clear();
        let mut position_values_visited = 0_u32;
        for group in 0..group_count {
            let choice = document as usize * self.config.maximum_query_groups + group;
            let selected =
                (scratch.choice_stamp[choice] == scratch.epoch).then_some(scratch.choices[choice]);
            scratch.chosen_postings.push(selected);
            let Some(posting) = selected else {
                continue;
            };
            for position in self.posting_position_slice(posting) {
                let field = position.field as usize;
                let field_range = self.field_range(document, field);
                let slot = field_range.start.saturating_add(position.position) as usize;
                if scratch.position_stamp[slot] != scratch.position_epoch {
                    scratch.position_stamp[slot] = scratch.position_epoch;
                    scratch.position_groups[slot] = GroupMask::default();
                    scratch.position_segments[slot] = position.segment;
                    scratch.position_fields[slot] = position.field;
                    scratch.touched_positions.push(slot as u32);
                }
                scratch.position_groups[slot].insert(group);
                position_values_visited = position_values_visited.saturating_add(1);
            }
        }
        scratch.touched_positions.sort_unstable();
        for &slot in &scratch.touched_positions {
            let slot = slot as usize;
            let field = scratch.position_fields[slot];
            let field_range = self.field_range(document, field as usize);
            scratch.positioned_groups.push(PositionedGroups {
                position: slot as u32 - field_range.start,
                segment: scratch.position_segments[slot],
                field,
                groups: scratch.position_groups[slot],
            });
        }
        let mut best = Coherence::default();
        let mut primitive = PrimitiveCoherence::default();
        let mut best_total = -1.0_f32;
        let signals = CoherenceSignals {
            proximity: self.config.proximity_weight != 0.0,
            order: self.config.order_weight != 0.0,
            phrase: self.config.phrase_weight != 0.0,
            segment: self.config.segment_weight != 0.0,
        };
        let mut cursor = 0;
        while cursor < scratch.positioned_groups.len() {
            let field = scratch.positioned_groups[cursor].field as usize;
            let start = cursor;
            while cursor < scratch.positioned_groups.len()
                && scratch.positioned_groups[cursor].field as usize == field
            {
                cursor += 1;
            }
            let precomputed_order = signals
                .order
                .then(|| self.ordered_fraction(field as u16, &scratch.chosen_postings));
            debug_assert!(
                precomputed_order.is_none_or(|order| {
                    order.to_bits()
                        == ordered_fraction(
                            &scratch.positioned_groups[start..cursor],
                            &scratch.chosen_postings,
                        )
                        .to_bits()
                }),
                "posting-local order must match the position-mask oracle"
            );
            let (measured, field_evidence) = measure_field_with_evidence(
                &scratch.positioned_groups[start..cursor],
                &scratch.chosen_postings,
                FieldMeasurementOptions {
                    field_len: self.field_range(document, field).len,
                    exact_bonus: self.field_configs[field].exact_match_bonus,
                    proximity_decay: self.config.proximity_decay_tokens,
                    signals,
                    precomputed_order,
                    collect_primitive_evidence,
                },
            );
            let total = self.config.proximity_weight * measured.proximity
                + self.config.order_weight * measured.order
                + self.config.phrase_weight * measured.phrase
                + self.config.segment_weight * measured.segment
                + measured.exact_field;
            if total > best_total {
                best_total = total;
                best = measured;
            }
            primitive.minimum_complete_span = primitive
                .minimum_complete_span
                .min(field_evidence.minimum_complete_span);
            primitive.matched_group_locality = primitive
                .matched_group_locality
                .max(field_evidence.matched_group_locality);
            primitive.minimum_ordered_span = primitive
                .minimum_ordered_span
                .min(field_evidence.minimum_ordered_span);
            primitive.ordered_fraction = primitive
                .ordered_fraction
                .max(field_evidence.ordered_fraction);
            primitive.exact_phrase |= field_evidence.exact_phrase;
            primitive.exact_field |= field_evidence.exact_field;
            primitive.exact_identifier_field |= field < 64
                && self.config.v3_exact_identifier_fields & (1_u64 << field) != 0
                && field_evidence.exact_field;
        }
        (best, primitive, position_values_visited)
    }

    fn posting_slice(&self, range: PostingRange) -> &[PostingRecord] {
        &self.postings[range.start as usize..(range.start + range.len) as usize]
    }

    fn posting_position_slice(&self, posting: u32) -> &[PostingPosition] {
        let index = posting as usize;
        let start = self.posting_position_starts[index] as usize;
        let end = self
            .posting_position_starts
            .get(index + 1)
            .map_or(self.posting_positions.len(), |next| *next as usize);
        &self.posting_positions[start..end]
    }

    fn ordered_fraction(&self, field: u16, chosen: &[Option<u32>]) -> f32 {
        let expected = chosen.iter().filter(|posting| posting.is_some()).count();
        if expected <= 1 {
            return 1.0;
        }
        let mut position_floor = 0_u32;
        let mut ordered = 0;
        for posting in chosen.iter().flatten() {
            let positions = self.posting_position_slice(*posting);
            let offset = positions.partition_point(|position| {
                position.field < field
                    || (position.field == field && position.position < position_floor)
            });
            let Some(position) = positions.get(offset) else {
                continue;
            };
            if position.field == field {
                position_floor = position.position.saturating_add(1);
                ordered += 1;
            }
        }
        ordered as f32 / expected as f32
    }

    fn field_range(&self, document: u32, field: usize) -> FieldRange {
        self.field_ranges[document as usize * self.field_configs.len() + field]
    }

    pub fn stats(&self) -> IndexStats {
        let string_bytes = self.terms.iter().map(CompactString::len).sum::<usize>();
        IndexStats {
            documents: self.documents.len(),
            terms: self.terms.len(),
            posting_rows: self.postings.len(),
            positions: self.posting_positions.len(),
            estimated_bytes: string_bytes
                + self.documents.len() * size_of::<DocumentMeta>()
                + self.field_ranges.len() * size_of::<FieldRange>()
                + self.posting_ranges.len() * size_of::<PostingRange>()
                + self.postings.len() * size_of::<PostingRecord>()
                + self.posting_field_impacts.len() * size_of::<f32>()
                + self.term_rarities.len() * size_of::<f32>()
                + self.posting_position_starts.len() * size_of::<u32>()
                + self.posting_positions.len() * size_of::<PostingPosition>()
                + self.posting_block_ranges.len() * size_of::<PostingRange>()
                + self.posting_block_max.len() * size_of::<f32>()
                + self.posting_block_end.len() * size_of::<u32>(),
        }
    }
}

#[inline]
fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{ColdEvidence, HotCandidate};
    use std::mem::size_of;

    #[test]
    fn hot_candidate_respects_the_sort_record_budget() {
        // REDLINE Phase 1 gate: ordering shuffles HotCandidate, never SearchHit.
        assert!(
            size_of::<HotCandidate>() <= 32,
            "hot record is {} bytes",
            size_of::<HotCandidate>()
        );
        // Cold evidence is never sorted; layout is informational only.
        assert_eq!(size_of::<ColdEvidence>() % 4, 0);
    }

    /// REDLINE Phase 3A gate: block-max accumulation reproduces the classic
    /// bounded candidate set and final top-k exactly, while visiting a
    /// strict subset of posting rows. V3 paths share accumulation, so V2
    /// equality implies V3 equality (same pool, scores, evidence inputs).
    #[test]
    fn block_max_matches_classic_candidate_set_and_top_k() {
        use crate::{
            DocumentInput, Expansion, FieldConfig, QpsBuilder, QpsConfig, QueryGroup, SearchScratch,
        };
        use std::collections::HashSet;

        fn corpus(seed: u64, documents: usize) -> Vec<String> {
            let vocabulary = (0..512)
                .map(|index| format!("term{index}"))
                .collect::<Vec<_>>();
            (0..documents)
                .map(|document| {
                    let mut state = (document as u64 + 1)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(seed.wrapping_mul(0xBF58_476D_1CE4_E5B9));
                    let mut text = String::with_capacity(768);
                    for token in 0..96 {
                        state ^= state >> 12;
                        state ^= state << 25;
                        state ^= state >> 27;
                        let word = &vocabulary[(state as usize) & (vocabulary.len() - 1)];
                        text.push_str(word);
                        text.push(if token % 24 == 23 { '.' } else { ' ' });
                    }
                    if document % 97 == 0 {
                        text.push_str(" graph memory retrieval.");
                    }
                    text
                })
                .collect()
        }

        fn build(documents: &[(u64, &str)], config: QpsConfig) -> super::QpsIndex {
            let fields = [FieldConfig::new("body", 1.0, 0.75, 0.0)];
            let mut builder =
                QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).unwrap();
            for (id, text) in documents {
                let values = [*text];
                builder
                    .insert(DocumentInput {
                        external_id: *id,
                        fields: &values,
                    })
                    .unwrap();
            }
            builder.build().unwrap()
        }

        let mut total_available = 0_u64;
        let mut total_visited = 0_u64;
        for seed in [11_u64, 23, 47] {
            let texts = corpus(seed, 10_000);
            let owned: Vec<(u64, &str)> = texts
                .iter()
                .enumerate()
                .map(|(i, t)| (i as u64, t.as_str()))
                .collect();
            for (config_name, config) in [
                ("default", QpsConfig::default()),
                (
                    "strict-coverage",
                    QpsConfig {
                        coverage_floor: 0.5,
                        coverage_exponent: 1.0,
                        ..QpsConfig::default()
                    },
                ),
            ] {
                let index = build(&owned, config);
                // Randomized literal queries: 1-5 groups over the vocabulary.
                let mut state = seed.wrapping_mul(0xD1B5_4D95_AA46_9C1F);
                let mut queries = vec![
                    "graph memory retrieval".to_string(),
                    "term17".to_string(),
                    "term17 term203 term411".to_string(),
                    "term3 term3 term44".to_string(),
                    "neverindexedtoken".to_string(),
                ];
                for _ in 0..40 {
                    let groups = 1 + (state % 5) as usize;
                    let mut query = String::new();
                    for _ in 0..groups {
                        state ^= state >> 12;
                        state ^= state << 25;
                        state ^= state >> 27;
                        query.push_str(&format!("term{} ", (state % 512) as usize));
                    }
                    queries.push(query);
                }
                for query in &queries {
                    for top_k in [1_usize, 10, 50] {
                        let mut classic_scratch = SearchScratch::new();
                        let mut bmw_scratch = SearchScratch::new();
                        let mut classic_hits = Vec::new();
                        let mut bmw_hits = Vec::new();
                        let mut classic_evidence = Vec::new();
                        let mut bmw_evidence = Vec::new();
                        // Serving limits.
                        let classic_receipt = index
                            .search_text_classic_into(
                                query,
                                top_k,
                                top_k,
                                &mut classic_scratch,
                                &mut classic_hits,
                                false,
                            )
                            .unwrap();
                        let bmw_receipt = index
                            .search_into(query, top_k, &mut bmw_scratch, &mut bmw_hits)
                            .unwrap();
                        assert_eq!(
                            classic_hits, bmw_hits,
                            "top-k mismatch {config_name}/seed{seed}/{query}/k{top_k}"
                        );
                        // Evidence limits (full pools must match as sets+order).
                        let classic_evidence_receipt = index
                            .search_text_classic_into(
                                query,
                                top_k,
                                usize::MAX,
                                &mut classic_scratch,
                                &mut classic_evidence,
                                true,
                            )
                            .unwrap();
                        let bmw_evidence_receipt = index
                            .search_evidence_into(query, top_k, &mut bmw_scratch, &mut bmw_evidence)
                            .unwrap();
                        if classic_evidence != bmw_evidence {
                            use std::collections::HashSet;
                            let a: HashSet<u64> =
                                classic_evidence.iter().map(|hit| hit.external_id).collect();
                            let b: HashSet<u64> =
                                bmw_evidence.iter().map(|hit| hit.external_id).collect();
                            let mut ac: Vec<u64> = a.difference(&b).copied().collect();
                            let mut bc: Vec<u64> = b.difference(&a).copied().collect();
                            ac.sort_unstable();
                            bc.sort_unstable();
                            ac.truncate(8);
                            bc.truncate(8);
                            eprintln!("VEC DIFF {config_name}/seed{seed}/{query}/k{top_k}: classic_n={} bmw_n={} only_classic={ac:?} only_bmw={bc:?}",
                                classic_evidence.len(), bmw_evidence.len());
                        }
                        assert_eq!(
                            classic_evidence, bmw_evidence,
                            "pool mismatch {config_name}/seed{seed}/{query}/k{top_k}"
                        );
                        // Work accounting: available is calibrated (equals
                        // classic reads), BMW never reads more.
                        assert_eq!(
                            bmw_receipt.posting_rows_available,
                            classic_receipt.posting_rows_visited,
                            "available calibration {config_name}/seed{seed}/{query}"
                        );
                        assert!(
                            bmw_receipt.posting_rows_visited <= bmw_receipt.posting_rows_available,
                            "skip invariant {config_name}/seed{seed}/{query}"
                        );
                        assert_eq!(
                            classic_evidence_receipt.posting_rows_visited,
                            bmw_evidence_receipt.posting_rows_available,
                            "evidence available {config_name}/seed{seed}/{query}"
                        );
                        total_available += u64::from(bmw_receipt.posting_rows_available);
                        total_visited += u64::from(bmw_receipt.posting_rows_visited);
                        // `covered_candidates` is a discovered-count receipt
                        // on the WAND path: prefix advances can skip a covered
                        // document without materializing it. Pool identity and
                        // final top-k equality remain the semantic gates.
                        // Pool identity as sets (order normalized downstream).
                        let classic_pool: HashSet<u64> =
                            classic_evidence.iter().map(|hit| hit.external_id).collect();
                        let bmw_pool: HashSet<u64> =
                            bmw_evidence.iter().map(|hit| hit.external_id).collect();
                        if classic_pool != bmw_pool {
                            let mut only_classic: Vec<u64> =
                                classic_pool.difference(&bmw_pool).copied().collect();
                            let mut only_bmw: Vec<u64> =
                                bmw_pool.difference(&classic_pool).copied().collect();
                            only_classic.sort_unstable();
                            only_bmw.sort_unstable();
                            eprintln!(
                                "POOL DIFF {config_name}/seed{seed}/{query}/k{top_k}: classic_n={} bmw_n={} only_classic={only_classic:?} only_bmw={only_bmw:?} bmw_rows={}/{}",
                                classic_evidence.len(),
                                bmw_evidence.len(),
                                bmw_evidence_receipt.posting_rows_visited,
                                bmw_evidence_receipt.posting_rows_available,
                            );
                        }
                        assert_eq!(
                            classic_pool, bmw_pool,
                            "pool set {config_name}/seed{seed}/{query}"
                        );
                    }
                }
                // Multi-expansion groups take the classic fallback in both
                // modes (transport stays out of Phase 3 by design).
                let multi = [
                    Expansion {
                        term: "graph",
                        quality: 1.0,
                    },
                    Expansion {
                        term: "term17",
                        quality: 0.6,
                    },
                ];
                let single = [Expansion {
                    term: "memory",
                    quality: 1.0,
                }];
                let groups = [
                    QueryGroup { expansions: &multi },
                    QueryGroup {
                        expansions: &single,
                    },
                ];
                let mut classic_scratch = SearchScratch::new();
                let mut bmw_scratch = SearchScratch::new();
                let mut classic_hits = Vec::new();
                let mut bmw_hits = Vec::new();
                let classic_receipt = index
                    .search_groups_classic_into(
                        &groups,
                        10,
                        10,
                        &mut classic_scratch,
                        &mut classic_hits,
                        false,
                    )
                    .unwrap();
                let bmw_receipt = index
                    .search_groups_into(&groups, 10, &mut bmw_scratch, &mut bmw_hits)
                    .unwrap();
                assert_eq!(classic_hits, bmw_hits, "transport fallback top-k");
                assert_eq!(
                    classic_receipt.posting_rows_visited, bmw_receipt.posting_rows_visited,
                    "transport fallback reads (no skipping by design)"
                );
                let expected_transport_rows = index.posting_ranges
                    [*index.term_ids.get("graph").expect("graph term") as usize]
                    .len
                    .saturating_add(
                        index.posting_ranges
                            [*index.term_ids.get("term17").expect("term17 term") as usize]
                            .len,
                    );
                assert_eq!(
                    classic_receipt.transport_raw_expansion_rows, expected_transport_rows,
                    "transport A counts every expansion row"
                );
                assert!(
                    classic_receipt.transport_unique_group_documents > 0,
                    "transport U must observe group documents"
                );
                assert!(
                    classic_receipt.transport_raw_expansion_rows
                        >= classic_receipt.transport_unique_group_documents,
                    "transport A must dominate unique group documents"
                );
                assert!(
                    classic_receipt.transport_winning_expansion_rows
                        <= classic_receipt.transport_raw_expansion_rows,
                    "winning rows cannot exceed raw rows"
                );
                assert_eq!(
                    classic_receipt.transport_raw_expansion_rows,
                    bmw_receipt.transport_raw_expansion_rows,
                    "transport metrics must be mode invariant"
                );
            }
        }
        let ratio = total_visited as f64 / total_available.max(1) as f64;
        eprintln!("REDLINE Phase 3A: rows visited/available = {total_visited}/{total_available} ({ratio:.3})");
    }

    #[test]
    fn fused_literal_matches_classic_pool_and_receipts() {
        use crate::{DocumentInput, FieldConfig, QpsBuilder, QpsConfig, SearchScratch};

        let fields = [FieldConfig::new("body", 1.0, 0.75, 0.0)];
        let mut builder =
            QpsBuilder::new(Vec::from(fields).into_boxed_slice(), QpsConfig::default()).unwrap();
        let texts = [
            "graph memory retrieval",
            "graph memory",
            "memory retrieval",
            "graph only",
            "retrieval only",
            "graph memory retrieval graph",
            "unrelated text",
            "graph memory retrieval",
            "graph",
            "memory",
            "retrieval",
            "graph memory retrieval",
        ];
        for (external_id, text) in texts.iter().enumerate() {
            let values = [*text];
            builder
                .insert(DocumentInput {
                    external_id: external_id as u64,
                    fields: &values,
                })
                .unwrap();
        }
        let index = builder.build().unwrap();
        for top_k in [1_usize, 3, 8] {
            let mut classic_scratch = SearchScratch::new();
            let mut fused_scratch = SearchScratch::new();
            let mut classic = Vec::new();
            let mut fused = Vec::new();
            let classic_receipt = index
                .search_text_classic_into(
                    "graph memory retrieval",
                    top_k,
                    usize::MAX,
                    &mut classic_scratch,
                    &mut classic,
                    true,
                )
                .unwrap();
            let fused_receipt = index
                .search_text_fused_into(
                    "graph memory retrieval",
                    top_k,
                    usize::MAX,
                    &mut fused_scratch,
                    &mut fused,
                    true,
                )
                .unwrap();
            assert_eq!(fused, classic, "fused pool mismatch for k={top_k}");
            assert_eq!(
                fused_receipt.covered_candidates,
                classic_receipt.covered_candidates
            );
            assert_eq!(
                fused_receipt.posting_rows_visited,
                classic_receipt.posting_rows_visited
            );
            assert_eq!(
                fused_receipt.posting_rows_available,
                classic_receipt.posting_rows_visited
            );
            assert!(fused_receipt.literal_documents_finalized > 0);
            assert!(fused_receipt.literal_choice_records_materialized > 0);
            assert!(
                fused_receipt.literal_scratch_documents_avoided
                    <= fused_receipt.literal_documents_finalized
            );
        }
    }
}
