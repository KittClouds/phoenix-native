use compact_str::CompactString;
use hashbrown::HashMap;
use smallvec::SmallVec;

use crate::index::{
    DocumentMeta, FieldRange, PostingPosition, PostingRange, PostingRecord, QpsIndex,
    StoredDocument, StoredField,
};
use crate::tokenize::tokenize;
use crate::types::{
    DocumentId, DocumentInput, FieldConfig, QpsConfig, QpsError, MAXIMUM_QUERY_GROUPS,
};

pub struct QpsBuilder {
    field_configs: Box<[FieldConfig]>,
    config: QpsConfig,
    term_ids: HashMap<CompactString, u32>,
    terms: Vec<CompactString>,
    external_ids: HashMap<u64, u32>,
    documents: Vec<StoredDocument>,
}

impl QpsBuilder {
    pub fn new(
        field_configs: impl Into<Box<[FieldConfig]>>,
        config: QpsConfig,
    ) -> Result<Self, QpsError> {
        let field_configs = field_configs.into();
        validate(&field_configs, config)?;
        Ok(Self {
            field_configs,
            config,
            term_ids: HashMap::new(),
            terms: Vec::new(),
            external_ids: HashMap::new(),
            documents: Vec::new(),
        })
    }

    pub fn insert(&mut self, input: DocumentInput<'_>) -> Result<DocumentId, QpsError> {
        if input.fields.len() != self.field_configs.len() {
            return Err(QpsError::FieldCountMismatch);
        }
        if self.external_ids.contains_key(&input.external_id) {
            return Err(QpsError::DuplicateExternalId(input.external_id));
        }
        let document =
            u32::try_from(self.documents.len()).map_err(|_| QpsError::DocumentIdOverflow)?;
        let mut fields = Vec::with_capacity(input.fields.len());
        for text in input.fields {
            let occurrences = tokenize(text);
            let mut terms = Vec::with_capacity(occurrences.len());
            let mut segments = Vec::with_capacity(occurrences.len());
            for occurrence in occurrences {
                debug_assert_eq!(occurrence.position, terms.len() as u32);
                let term = self.intern(occurrence.token);
                terms.push(term);
                segments.push(
                    u16::try_from(occurrence.segment).map_err(|_| QpsError::SegmentIdOverflow)?,
                );
            }
            fields.push(StoredField {
                terms: terms.into_boxed_slice(),
                segments: segments.into_boxed_slice(),
            });
        }
        self.external_ids.insert(input.external_id, document);
        self.documents.push(StoredDocument {
            external_id: input.external_id,
            active: true,
            fields: fields.into_boxed_slice(),
        });
        Ok(DocumentId(document))
    }

    /// Tombstones a document using its stored token manifest. Source text is
    /// deliberately not retained or re-tokenized for deletion.
    pub fn remove(&mut self, external_id: u64) -> bool {
        let Some(document) = self.external_ids.remove(&external_id) else {
            return false;
        };
        let Some(stored) = self.documents.get_mut(document as usize) else {
            return false;
        };
        stored.active = false;
        true
    }

    pub fn build(self) -> Result<QpsIndex, QpsError> {
        let field_count = self.field_configs.len();
        let active_count = self
            .documents
            .iter()
            .filter(|document| document.active)
            .count();
        let mut documents = Vec::with_capacity(active_count);
        let mut field_ranges = Vec::with_capacity(active_count * field_count);
        let mut unpacked = Vec::<UnpackedPosting>::new();
        let mut total_field_lengths = vec![0_u64; field_count];

        for source in self.documents.iter().filter(|document| document.active) {
            let document =
                u32::try_from(documents.len()).map_err(|_| QpsError::DocumentIdOverflow)?;
            documents.push(DocumentMeta {
                external_id: source.external_id,
            });
            let mut document_position_start = 0_u32;
            for (field, stored) in source.fields.iter().enumerate() {
                let field_len = checked_u32(stored.terms.len())?;
                field_ranges.push(FieldRange {
                    start: document_position_start,
                    len: field_len,
                });
                document_position_start = document_position_start
                    .checked_add(field_len)
                    .ok_or(QpsError::IndexAddressOverflow)?;
                total_field_lengths[field] += stored.terms.len() as u64;
                let field = u16::try_from(field).map_err(|_| QpsError::FieldIdOverflow)?;
                collect_postings(document, field, stored, &mut unpacked);
            }
        }

        unpacked.sort_unstable_by_key(|posting| (posting.term, posting.document, posting.field));
        let denominator = active_count.max(1) as f32;
        let average_field_lengths = total_field_lengths
            .into_iter()
            .map(|length| (length as f32 / denominator).max(1.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut document_frequency = vec![0_u32; self.terms.len()];
        let mut prior = None;
        for posting in &unpacked {
            let identity = (posting.term, posting.document);
            if prior != Some(identity) {
                document_frequency[posting.term as usize] =
                    document_frequency[posting.term as usize].saturating_add(1);
                prior = Some(identity);
            }
        }
        let mut postings = Vec::with_capacity(unpacked.len());
        let mut posting_position_starts = Vec::with_capacity(unpacked.len());
        let mut posting_positions = Vec::new();
        let mut posting_ranges = vec![PostingRange::default(); self.terms.len()];
        let mut cursor = 0;
        while cursor < unpacked.len() {
            let term = unpacked[cursor].term;
            let posting_start = checked_u32(postings.len())?;
            while cursor < unpacked.len() && unpacked[cursor].term == term {
                let document = unpacked[cursor].document;
                let mut weighted_tf = 0.0_f32;
                let position_start = checked_u32(posting_positions.len())?;
                while cursor < unpacked.len()
                    && unpacked[cursor].term == term
                    && unpacked[cursor].document == document
                {
                    let source = &unpacked[cursor];
                    let field = source.field as usize;
                    let length = field_ranges[document as usize * field_count + field].len as f32;
                    let field_config = self.field_configs[field];
                    let normalization = 1.0 - field_config.length_normalization
                        + field_config.length_normalization * length / average_field_lengths[field];
                    weighted_tf += field_config.weight * source.positions.len() as f32
                        / normalization.max(0.01);
                    posting_positions.extend(source.positions.iter().map(
                        |&(position, segment)| PostingPosition {
                            position,
                            segment,
                            field: source.field,
                        },
                    ));
                    cursor += 1;
                }
                let idf = inverse_document_frequency(
                    active_count,
                    document_frequency[term as usize] as usize,
                );
                let impact =
                    idf * (self.config.k1 + 1.0) * weighted_tf / (self.config.k1 + weighted_tf);
                postings.push(PostingRecord { document, impact });
                posting_position_starts.push(position_start);
            }
            posting_ranges[term as usize] = PostingRange {
                start: posting_start,
                len: checked_u32(postings.len())?.saturating_sub(posting_start),
            };
        }

        Ok(QpsIndex::from_parts(
            self.config,
            self.field_configs,
            self.term_ids,
            self.terms.into_boxed_slice(),
            documents.into_boxed_slice(),
            field_ranges.into_boxed_slice(),
            posting_ranges.into_boxed_slice(),
            postings.into_boxed_slice(),
            posting_position_starts.into_boxed_slice(),
            posting_positions.into_boxed_slice(),
        ))
    }

    fn intern(&mut self, token: CompactString) -> u32 {
        if let Some(term) = self.term_ids.get(token.as_str()) {
            return *term;
        }
        let term = self.terms.len() as u32;
        self.terms.push(token.clone());
        self.term_ids.insert(token, term);
        term
    }
}

fn validate(fields: &[FieldConfig], config: QpsConfig) -> Result<(), QpsError> {
    if fields.is_empty() {
        return Err(QpsError::MissingFields);
    }
    for field in fields {
        if !field.weight.is_finite()
            || field.weight <= 0.0
            || !(0.0..=1.0).contains(&field.length_normalization)
            || !field.exact_match_bonus.is_finite()
            || field.exact_match_bonus < 0.0
        {
            return Err(QpsError::InvalidField { field: field.name });
        }
    }
    if !config.k1.is_finite() || config.k1 <= 0.0 {
        return Err(QpsError::InvalidConfig("k1"));
    }
    if !(0.0..=1.0).contains(&config.coverage_floor)
        || !config.coverage_exponent.is_finite()
        || config.coverage_exponent < 0.0
    {
        return Err(QpsError::InvalidConfig("coverage"));
    }
    if config.maximum_query_groups == 0
        || config.maximum_query_groups > MAXIMUM_QUERY_GROUPS
        || config.maximum_expansions_per_group == 0
    {
        return Err(QpsError::InvalidConfig("query bounds"));
    }
    if config.minimum_candidate_pool == 0
        || config.candidate_pool_multiplier == 0
        || config.maximum_candidate_pool < config.minimum_candidate_pool
        || !config.dense_simd_threshold.is_finite()
        || !(0.0..=1.0).contains(&config.dense_simd_threshold)
    {
        return Err(QpsError::InvalidConfig("candidate selection"));
    }
    Ok(())
}

fn inverse_document_frequency(documents: usize, frequency: usize) -> f32 {
    let documents = documents as f32;
    let frequency = frequency as f32;
    (1.0 + (documents - frequency + 0.5) / (frequency + 0.5)).ln()
}

fn checked_u32(value: usize) -> Result<u32, QpsError> {
    u32::try_from(value).map_err(|_| QpsError::IndexAddressOverflow)
}

#[derive(Debug)]
struct UnpackedPosting {
    term: u32,
    document: u32,
    field: u16,
    positions: SmallVec<[(u32, u16); 4]>,
}

fn collect_postings(
    document: u32,
    field: u16,
    stored: &StoredField,
    output: &mut Vec<UnpackedPosting>,
) {
    let mut occurrences = stored
        .terms
        .iter()
        .copied()
        .zip(stored.segments.iter().copied())
        .enumerate()
        .map(|(position, (term, segment))| (term, position as u32, segment))
        .collect::<Vec<_>>();
    // Posting payloads are opened directly by the serving kernel. Keep each
    // term's positions monotonic so order scoring can advance with binary
    // search instead of materializing and sorting a second position stream.
    occurrences.sort_unstable_by_key(|occurrence| (occurrence.0, occurrence.1));
    let mut cursor = 0;
    while cursor < occurrences.len() {
        let term = occurrences[cursor].0;
        let mut positions = SmallVec::new();
        while cursor < occurrences.len() && occurrences[cursor].0 == term {
            positions.push((occurrences[cursor].1, occurrences[cursor].2));
            cursor += 1;
        }
        output.push(UnpackedPosting {
            term,
            document,
            field,
            positions,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::{
        collect_postings, FieldRange, PostingPosition, PostingRange, PostingRecord, StoredField,
    };

    #[test]
    fn serving_records_keep_their_compact_layout() {
        assert_eq!(size_of::<FieldRange>(), 8);
        assert_eq!(size_of::<PostingRange>(), 8);
        assert_eq!(size_of::<PostingRecord>(), 8);
        assert_eq!(size_of::<PostingPosition>(), 8);
    }

    #[test]
    fn posting_positions_are_monotonic_within_each_term() {
        let field = StoredField {
            terms: vec![4, 1, 4, 2, 4, 1, 4].into_boxed_slice(),
            segments: vec![0, 0, 0, 0, 1, 1, 1].into_boxed_slice(),
        };
        let mut postings = Vec::new();
        collect_postings(0, 0, &field, &mut postings);

        for posting in postings {
            assert!(posting
                .positions
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0));
        }
    }
}
