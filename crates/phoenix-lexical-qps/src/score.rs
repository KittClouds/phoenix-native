use crate::types::MAXIMUM_QUERY_GROUPS;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Coherence {
    pub proximity: f32,
    pub order: f32,
    pub phrase: f32,
    pub segment: f32,
    pub exact_field: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PositionedGroups {
    pub position: u32,
    pub segment: u16,
    pub field: u16,
    pub groups: GroupMask,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CoherenceSignals {
    pub proximity: bool,
    pub order: bool,
    pub phrase: bool,
    pub segment: bool,
}

pub(crate) fn measure_field(
    positions: &[PositionedGroups],
    chosen_postings: &[Option<u32>],
    field_len: u32,
    exact_bonus: f32,
    proximity_decay: f32,
    signals: CoherenceSignals,
    precomputed_order: Option<f32>,
) -> Coherence {
    if positions.is_empty() {
        return Coherence::default();
    }
    debug_assert!(positions.windows(2).all(|pair| {
        pair[0].field < pair[1].field
            || (pair[0].field == pair[1].field && pair[0].position <= pair[1].position)
    }));
    let matched_total = chosen_postings
        .iter()
        .filter(|posting| posting.is_some())
        .count()
        .max(1);
    let needs_field_coverage = signals.proximity || signals.order;
    let (field_mask, field_groups, field_coverage) = if needs_field_coverage {
        let mut mask = GroupMask::default();
        for position in positions {
            mask.union(position.groups);
        }
        let groups = mask.count_ones();
        (mask, groups, groups as f32 / matched_total as f32)
    } else {
        (GroupMask::default(), 0, 0.0)
    };
    let proximity = if signals.proximity {
        let span = minimum_covering_span(positions, field_mask);
        let excess = span.saturating_sub(field_groups as u32) as f32;
        field_coverage / (1.0 + excess / proximity_decay.max(1.0))
    } else {
        0.0
    };
    let order = if signals.order {
        precomputed_order.unwrap_or_else(|| ordered_fraction(positions, chosen_postings))
            * field_coverage
    } else {
        0.0
    };
    let phrase_match =
        (signals.phrase || exact_bonus != 0.0) && exact_phrase(positions, chosen_postings);
    let phrase = if signals.phrase && phrase_match {
        1.0
    } else {
        0.0
    };
    let segment = if signals.segment {
        segment_concentration(positions, matched_total)
    } else {
        0.0
    };
    let exact_field = if field_len as usize == chosen_postings.len() && phrase_match {
        exact_bonus
    } else {
        0.0
    };
    Coherence {
        proximity,
        order,
        phrase,
        segment,
        exact_field,
    }
}

fn minimum_covering_span(positions: &[PositionedGroups], required_mask: GroupMask) -> u32 {
    let required = required_mask.count_ones() as u32;
    if required <= 1 {
        return 1;
    }
    let mut counts = [0_u16; MAXIMUM_QUERY_GROUPS];
    let mut present = 0_u32;
    let mut left = 0;
    let mut best = u32::MAX;
    for right in 0..positions.len() {
        for group in positions[right].groups.ones() {
            if counts[group] == 0 {
                present += 1;
            }
            counts[group] = counts[group].saturating_add(1);
        }
        while present == required {
            best = best.min(
                positions[right]
                    .position
                    .saturating_sub(positions[left].position)
                    .saturating_add(1),
            );
            for group in positions[left].groups.ones() {
                counts[group] -= 1;
                if counts[group] == 0 {
                    present -= 1;
                }
            }
            left += 1;
        }
    }
    best
}

pub(crate) fn ordered_fraction(positions: &[PositionedGroups], chosen: &[Option<u32>]) -> f32 {
    let expected = chosen.iter().filter(|posting| posting.is_some()).count();
    if expected <= 1 {
        return 1.0;
    }
    let mut cursor = 0;
    let mut ordered = 0;
    for (group, posting) in chosen.iter().enumerate() {
        if posting.is_none() {
            continue;
        }
        if let Some(offset) = positions[cursor..]
            .iter()
            .position(|position| position.groups.contains(group))
        {
            cursor += offset + 1;
            ordered += 1;
        }
    }
    ordered as f32 / expected as f32
}

fn exact_phrase(positions: &[PositionedGroups], chosen: &[Option<u32>]) -> bool {
    if chosen.is_empty() || chosen.iter().any(Option::is_none) {
        return false;
    }
    let mut previous_position: Option<u32> = None;
    let mut matched = 0;
    for position in positions {
        if matched > 0
            && previous_position
                .is_none_or(|previous| previous.saturating_add(1) != position.position)
        {
            matched = 0;
        }
        if position.groups.contains(matched) {
            matched += 1;
            if matched == chosen.len() {
                return true;
            }
        } else if position.groups.contains(0) {
            matched = 1;
        } else {
            matched = 0;
        }
        previous_position = Some(position.position);
    }
    false
}

fn segment_concentration(positions: &[PositionedGroups], matched_total: usize) -> f32 {
    let mut best = 0_u32;
    let mut cursor = 0;
    while cursor < positions.len() {
        let segment = positions[cursor].segment;
        let mut mask = GroupMask::default();
        while cursor < positions.len() && positions[cursor].segment == segment {
            mask.union(positions[cursor].groups);
            cursor += 1;
        }
        best = best.max(mask.count_ones() as u32);
    }
    best as f32 / matched_total.max(1) as f32
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GroupMask([u64; 2]);

impl GroupMask {
    #[inline]
    pub(crate) fn insert(&mut self, group: usize) {
        debug_assert!(group < MAXIMUM_QUERY_GROUPS);
        self.0[group >> 6] |= 1_u64 << (group & 63);
    }

    #[inline]
    fn union(&mut self, other: Self) {
        self.0[0] |= other.0[0];
        self.0[1] |= other.0[1];
    }

    #[inline]
    fn count_ones(self) -> usize {
        self.0.iter().map(|lane| lane.count_ones() as usize).sum()
    }

    #[inline]
    fn contains(self, group: usize) -> bool {
        group < MAXIMUM_QUERY_GROUPS && self.0[group >> 6] & (1_u64 << (group & 63)) != 0
    }

    fn ones(self) -> impl Iterator<Item = usize> {
        self.0
            .into_iter()
            .enumerate()
            .flat_map(|(lane, bits)| Ones {
                bits,
                base: lane * 64,
            })
    }
}

struct Ones {
    bits: u64,
    base: usize,
}

impl Iterator for Ones {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            return None;
        }
        let bit = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        Some(self.base + bit)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        measure_field, Coherence, CoherenceSignals, GroupMask, PositionedGroups,
        MAXIMUM_QUERY_GROUPS,
    };

    const ALL_SIGNALS: CoherenceSignals = CoherenceSignals {
        proximity: true,
        order: true,
        phrase: true,
        segment: true,
    };

    #[test]
    fn exact_phrase_beats_scattered_terms() {
        let chosen = [Some(1), Some(2), Some(3)];
        let phrase = vec![
            PositionedGroups {
                position: 4,
                segment: 0,
                field: 0,
                groups: mask(0),
            },
            PositionedGroups {
                position: 5,
                segment: 0,
                field: 0,
                groups: mask(1),
            },
            PositionedGroups {
                position: 6,
                segment: 0,
                field: 0,
                groups: mask(2),
            },
        ];
        let measured = measure_field(&phrase, &chosen, 3, 0.4, 12.0, ALL_SIGNALS, None);
        assert_eq!(measured.phrase, 1.0);
        assert_eq!(measured.proximity, 1.0);
        assert_eq!(measured.exact_field, 0.4);
    }

    #[test]
    fn coherence_supports_groups_beyond_the_original_u32_mask() {
        let chosen = (0..40).map(Some).collect::<Vec<_>>();
        let phrase = (0..40)
            .map(|group| PositionedGroups {
                position: group,
                segment: 0,
                field: 0,
                groups: mask(group as usize),
            })
            .collect::<Vec<_>>();
        let measured = measure_field(&phrase, &chosen, 40, 0.4, 12.0, ALL_SIGNALS, None);
        assert_eq!(measured.phrase, 1.0);
        assert_eq!(measured.proximity, 1.0);
        assert_eq!(measured.segment, 1.0);
    }

    #[test]
    fn posting_local_kernel_matches_document_scan_semantics() {
        let mut state = 0x9e37_79b9_u32;
        for case in 0..512 {
            let field_len = 8 + next(&mut state) as usize % 160;
            let group_count = 1 + next(&mut state) as usize % 24;
            let mut terms = Vec::with_capacity(field_len);
            let mut segments = Vec::with_capacity(field_len);
            for position in 0..field_len {
                terms.push(next(&mut state) % 32);
                segments.push((position / 11) as u16);
            }
            let chosen = (0..group_count)
                .map(|_| {
                    let term = next(&mut state) % 36;
                    (term < 32).then_some(term)
                })
                .collect::<Vec<_>>();
            let positions = position_masks(&terms, &segments, &chosen);
            let measured = measure_field(
                &positions,
                &chosen,
                field_len as u32,
                0.4,
                12.0,
                ALL_SIGNALS,
                None,
            );
            let expected = document_scan_reference(&terms, &segments, &chosen, 0.4, 12.0);
            assert_close(case, &terms, &chosen, measured, expected);
        }
    }

    fn mask(group: usize) -> GroupMask {
        let mut mask = GroupMask::default();
        mask.insert(group);
        mask
    }

    fn next(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *state
    }

    fn position_masks(
        terms: &[u32],
        segments: &[u16],
        chosen: &[Option<u32>],
    ) -> Vec<PositionedGroups> {
        let mut positions = Vec::new();
        for (position, (&term, &segment)) in terms.iter().zip(segments).enumerate() {
            let mut groups = GroupMask::default();
            for (group, selected) in chosen.iter().enumerate() {
                if *selected == Some(term) {
                    groups.insert(group);
                }
            }
            if groups.count_ones() != 0 {
                positions.push(PositionedGroups {
                    position: position as u32,
                    segment,
                    field: 0,
                    groups,
                });
            }
        }
        positions
    }

    fn document_scan_reference(
        terms: &[u32],
        segments: &[u16],
        chosen: &[Option<u32>],
        exact_bonus: f32,
        proximity_decay: f32,
    ) -> Coherence {
        let mut occurrences = Vec::new();
        for (position, (&term, &segment)) in terms.iter().zip(segments).enumerate() {
            for (group, selected) in chosen.iter().enumerate().rev() {
                if *selected == Some(term) {
                    occurrences.push((position as u32, segment, group));
                }
            }
        }
        if occurrences.is_empty() {
            return Coherence::default();
        }
        let matched_total = chosen.iter().filter(|term| term.is_some()).count().max(1);
        let mut field_mask = GroupMask::default();
        for occurrence in &occurrences {
            field_mask.insert(occurrence.2);
        }
        let field_groups = field_mask.count_ones();
        let field_coverage = field_groups as f32 / matched_total as f32;
        let required = field_groups as u32;
        let mut counts = [0_u16; MAXIMUM_QUERY_GROUPS];
        let mut present = 0_u32;
        let mut left = 0;
        let mut best = u32::MAX;
        for right in 0..occurrences.len() {
            let group = occurrences[right].2;
            if counts[group] == 0 {
                present += 1;
            }
            counts[group] = counts[group].saturating_add(1);
            while present == required {
                best = best.min(
                    occurrences[right]
                        .0
                        .saturating_sub(occurrences[left].0)
                        .saturating_add(1),
                );
                let group = occurrences[left].2;
                counts[group] -= 1;
                if counts[group] == 0 {
                    present -= 1;
                }
                left += 1;
            }
        }
        let excess = best.saturating_sub(field_groups as u32) as f32;
        let proximity = field_coverage / (1.0 + excess / proximity_decay.max(1.0));
        let expected = chosen.iter().filter(|term| term.is_some()).count();
        let mut cursor = 0;
        let mut ordered = 0;
        for term in chosen.iter().flatten() {
            if let Some(offset) = terms[cursor..]
                .iter()
                .position(|field_term| field_term == term)
            {
                cursor += offset + 1;
                ordered += 1;
            }
        }
        let order = ordered as f32 / expected.max(1) as f32 * field_coverage;
        let query = chosen.iter().copied().collect::<Option<Vec<_>>>();
        let phrase = query
            .as_deref()
            .is_some_and(|query| terms.windows(query.len()).any(|window| window == query))
            as u8 as f32;
        let mut best_segment = 0;
        let mut occurrence = 0;
        while occurrence < occurrences.len() {
            let segment = occurrences[occurrence].1;
            let mut mask = GroupMask::default();
            while occurrence < occurrences.len() && occurrences[occurrence].1 == segment {
                mask.insert(occurrences[occurrence].2);
                occurrence += 1;
            }
            best_segment = best_segment.max(mask.count_ones());
        }
        let segment = best_segment as f32 / matched_total as f32;
        let exact_field = if query.as_deref() == Some(terms) {
            exact_bonus
        } else {
            0.0
        };
        Coherence {
            proximity,
            order,
            phrase,
            segment,
            exact_field,
        }
    }

    fn assert_close(
        case: usize,
        terms: &[u32],
        chosen: &[Option<u32>],
        actual: Coherence,
        expected: Coherence,
    ) {
        for (name, actual, expected) in [
            ("proximity", actual.proximity, expected.proximity),
            ("order", actual.order, expected.order),
            ("phrase", actual.phrase, expected.phrase),
            ("segment", actual.segment, expected.segment),
            ("exact_field", actual.exact_field, expected.exact_field),
        ] {
            assert!(
                (actual - expected).abs() <= f32::EPSILON,
                "case {case} {name}: actual {actual}, expected {expected}; \
                 terms={terms:?}, chosen={chosen:?}"
            );
        }
    }
}
