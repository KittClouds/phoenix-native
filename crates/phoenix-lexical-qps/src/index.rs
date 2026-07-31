use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::mem::size_of;
use std::time::Instant;

use compact_str::CompactString;
use hashbrown::HashMap;

use crate::score::{
    measure_field, ordered_fraction, Coherence, CoherenceSignals, GroupMask, PositionedGroups,
};
use crate::selection::{retain_dense_simd, retain_sparse, RankedCandidate};
use crate::tokenize::{tokenize_into, TokenOccurrence};
use crate::types::{
    CandidateSelection, DocumentId, FieldConfig, QpsConfig, QpsError, QueryGroup, SearchHit,
    SearchReceipt, SearchStageNanos,
};

const NO_CHOICE: u32 = u32::MAX;

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
    /// Cold random-access offsets are split from hot accumulation rows. The
    /// next start (or the position-array end) closes each posting range.
    posting_position_starts: Box<[u32]>,
    posting_positions: Box<[PostingPosition]>,
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
        self.query_tokens.reserve(maximum_query_groups);
        self.query_token_buffer.reserve(64);
        self.chosen_postings.reserve(maximum_query_groups);
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
            + self.query_tokens.capacity()
            + self.query_token_buffer.capacity()
            + self.position_stamp.capacity()
            + self.position_groups.capacity()
            + self.position_segments.capacity()
            + self.position_fields.capacity()
            + self.touched_positions.capacity()
            + self.positioned_groups.capacity()
            + self.chosen_postings.capacity()
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
        posting_position_starts: Box<[u32]>,
        posting_positions: Box<[PostingPosition]>,
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
            posting_position_starts,
            posting_positions,
        }
    }

    pub fn search_into(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_text_with_mode(query, top_k, scratch, output, SearchMode::Bounded)
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
        self.search_text_with_mode(query, top_k, scratch, output, SearchMode::Exhaustive)
    }

    fn search_text_with_mode(
        &self,
        query: &str,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        mode: SearchMode,
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
        self.search_resolved(top_k, scratch, output, mode)
    }

    pub fn search_groups_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(groups, top_k, scratch, output, SearchMode::Bounded)
    }

    /// Exhaustive oracle for explicit expansion groups.
    pub fn search_groups_exhaustive_into(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt, QpsError> {
        self.search_groups_with_mode(groups, top_k, scratch, output, SearchMode::Exhaustive)
    }

    fn search_groups_with_mode(
        &self,
        groups: &[QueryGroup<'_>],
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        mode: SearchMode,
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
        self.search_resolved(top_k, scratch, output, mode)
    }

    fn search_resolved(
        &self,
        top_k: usize,
        scratch: &mut SearchScratch,
        output: &mut Vec<SearchHit>,
        mode: SearchMode,
    ) -> Result<SearchReceipt, QpsError> {
        let total_started = Instant::now();
        let capacity_before = scratch.capacity_fingerprint() + output.capacity();
        output.clear();
        let mut visited = 0_u32;
        let group_count = scratch.query_ranges.len();
        for group in 0..group_count {
            let range = scratch.query_ranges[group];
            if range.len == 1 {
                let expansion = scratch.query_expansions[range.start as usize];
                visited = visited.saturating_add(self.score_exact_group(group, expansion, scratch));
                continue;
            }

            scratch.begin_group();
            for expansion_index in range.start..range.start.saturating_add(range.len) {
                let expansion = scratch.query_expansions[expansion_index as usize];
                visited = visited.saturating_add(self.score_expansion(expansion, scratch));
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
            }
        }

        let total_weight = group_count as f32;
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
        let accumulation_nanos = elapsed_nanos(total_started);

        let selection_started = Instant::now();
        let candidate_limit = match mode {
            SearchMode::Exhaustive => covered_candidates as usize,
            SearchMode::Bounded => {
                let requested = if group_count == 1 {
                    // A single group has no inter-term proximity, order, or
                    // phrase signal. Retain room for field-exact reranking
                    // without paying the multi-term ambiguity budget.
                    top_k.saturating_mul(4).max(top_k)
                } else {
                    self.config
                        .minimum_candidate_pool
                        .max(top_k.saturating_mul(self.config.candidate_pool_multiplier))
                };
                requested
                    .min(self.config.maximum_candidate_pool)
                    .min(covered_candidates as usize)
            }
        };
        let selection = match mode {
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
            SearchMode::Bounded => {
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
        };
        let selection_nanos = elapsed_nanos(selection_started);

        let coherence_started = Instant::now();
        let positional_enabled = self.config.proximity_weight != 0.0
            || self.config.order_weight != 0.0
            || self.config.phrase_weight != 0.0
            || self.config.segment_weight != 0.0
            || self
                .field_configs
                .iter()
                .any(|field| field.exact_match_bonus != 0.0);
        let reranked_candidates = if positional_enabled {
            scratch.selected_candidates.len() as u32
        } else {
            0
        };
        let mut position_values_visited = 0_u32;
        for candidate_index in 0..scratch.selected_candidates.len() {
            let document = scratch.selected_candidates[candidate_index];
            let index = document as usize;
            let coverage = scratch.coverage_weight[index] / total_weight;
            let (coherence, opened_positions) = if positional_enabled {
                self.coherence(document, group_count, scratch)
            } else {
                (Coherence::default(), 0)
            };
            position_values_visited = position_values_visited.saturating_add(opened_positions);
            let multiplier = 1.0
                + self.config.proximity_weight * coherence.proximity
                + self.config.order_weight * coherence.order
                + self.config.phrase_weight * coherence.phrase
                + self.config.segment_weight * coherence.segment
                + coherence.exact_field;
            let lexical = scratch.lexical[index];
            output.push(SearchHit {
                document: DocumentId(document),
                external_id: self.documents[index].external_id,
                score: scratch.candidate_scores[index] * multiplier,
                lexical_score: lexical,
                coverage,
                proximity: coherence.proximity,
                order: coherence.order,
                phrase: coherence.phrase,
                segment: coherence.segment,
                exact_field: coherence.exact_field,
            });
        }
        let coherence_nanos = elapsed_nanos(coherence_started);
        let ordering_started = Instant::now();
        output.sort_unstable_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.document.cmp(&right.document))
        });
        output.truncate(top_k);
        let ordering_nanos = elapsed_nanos(ordering_started);
        let capacity_after = scratch.capacity_fingerprint() + output.capacity();
        Ok(SearchReceipt {
            query_groups: group_count as u16,
            candidates: scratch.touched.len() as u32,
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
        })
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

    fn score_expansion(&self, expansion: ResolvedExpansion, scratch: &mut SearchScratch) -> u32 {
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
                scratch.group_documents.push(document);
            } else if score > scratch.group_best_score[index] {
                scratch.group_best_score[index] = score;
                scratch.group_best_quality[index] = expansion.quality;
                scratch.group_best_choice[index] = posting;
            }
        }
        range.len
    }

    fn coherence(
        &self,
        document: u32,
        group_count: usize,
        scratch: &mut SearchScratch,
    ) -> (Coherence, u32) {
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
            let measured = measure_field(
                &scratch.positioned_groups[start..cursor],
                &scratch.chosen_postings,
                self.field_range(document, field).len,
                self.field_configs[field].exact_match_bonus,
                self.config.proximity_decay_tokens,
                signals,
                precomputed_order,
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
        }
        (best, position_values_visited)
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
                + self.posting_position_starts.len() * size_of::<u32>()
                + self.posting_positions.len() * size_of::<PostingPosition>(),
        }
    }
}

#[inline]
fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
