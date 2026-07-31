use super::*;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use phoenix_scene_contract::{AnchorCandidate, EntityFamily, MAX_DOCUMENT_ANCHORS};

#[derive(Debug)]
pub(super) struct EntityHighlightIndex {
    registry_revision: u64,
    matcher: Option<AhoCorasick>,
    entities: Box<[PaintEntity]>,
}

#[derive(Debug)]
struct PaintEntity {
    stable_id: u64,
    entity_slot: u32,
    family: EntityFamily,
    label: Arc<str>,
}

impl EntityHighlightIndex {
    pub(super) fn build(registry: &EntityRegistry) -> Result<Arc<Self>, KernelError> {
        let mut ordered = registry
            .entities()
            .iter()
            .enumerate()
            .map(|(slot, entity)| (slot, entity, entity.label.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        ordered.sort_unstable_by(
            |(left_slot, left, left_key), (right_slot, right, right_key)| {
                left_key
                    .cmp(right_key)
                    .then_with(|| left.id.cmp(&right.id))
                    .then_with(|| left_slot.cmp(right_slot))
            },
        );

        let mut entities = Vec::with_capacity(ordered.len());
        let mut cursor = 0usize;
        while cursor < ordered.len() {
            let key = ordered[cursor].2.as_str();
            let mut end = cursor + 1;
            while end < ordered.len() && ordered[end].2 == key {
                end += 1;
            }
            // Duplicate labels are identity-ambiguous. Paint must never invent
            // an identity decision merely because two records share text.
            if end == cursor + 1 {
                let (slot, entity, _) = ordered[cursor];
                entities.push(PaintEntity {
                    stable_id: entity.id,
                    entity_slot: u32::try_from(slot)
                        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
                    family: entity.kind.family(),
                    label: Arc::from(entity.label.as_str()),
                });
            }
            cursor = end;
        }

        let matcher = if entities.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .match_kind(MatchKind::LeftmostLongest)
                    .ascii_case_insensitive(true)
                    .build(entities.iter().map(|entity| entity.label.as_ref()))
                    .map_err(|error| KernelError::EntityHighlightIndex(error.to_string()))?,
            )
        };
        Ok(Arc::new(Self {
            registry_revision: registry.revision(),
            matcher,
            entities: entities.into_boxed_slice(),
        }))
    }

    pub(super) fn registry_revision(&self) -> u64 {
        self.registry_revision
    }

    pub(super) fn append_matches(
        &self,
        content: &str,
        reserved: &[(u32, u32)],
        candidates: &mut Vec<AnchorCandidate>,
    ) -> Result<(), KernelError> {
        let Some(matcher) = self.matcher.as_ref() else {
            return Ok(());
        };
        let mut reserved_cursor = 0usize;
        for matched in matcher.find_iter(content.as_bytes()) {
            let start = matched.start();
            let end = matched.end();
            let entity = self
                .entities
                .get(matched.pattern().as_usize())
                .ok_or(KernelError::AnalysisAuthorityMismatch)?;
            if !has_token_boundaries(content, start, end, &entity.label) {
                continue;
            }
            let start_u32 =
                u32::try_from(start).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
            let end_u32 = u32::try_from(end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
            while reserved_cursor < reserved.len() && reserved[reserved_cursor].1 <= start_u32 {
                reserved_cursor += 1;
            }
            if reserved
                .get(reserved_cursor)
                .is_some_and(|(reserved_start, reserved_end)| {
                    *reserved_start < end_u32 && start_u32 < *reserved_end
                })
            {
                continue;
            }
            let next_count = candidates
                .len()
                .checked_add(1)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?;
            if next_count > MAX_DOCUMENT_ANCHORS {
                return Err(phoenix_scene_contract::HighlightContractError::Oversized {
                    actual: next_count,
                    maximum: MAX_DOCUMENT_ANCHORS,
                }
                .into());
            }
            candidates.push(AnchorCandidate {
                start: start_u32,
                end: end_u32,
                node_id: entity.stable_id,
                entity_slot: entity.entity_slot,
                family: entity.family,
                surface: content[start..end].to_owned(),
            });
        }
        Ok(())
    }
}

fn has_token_boundaries(content: &str, start: usize, end: usize, label: &str) -> bool {
    let label_starts_word = label.chars().next().is_some_and(is_word_character);
    let label_ends_word = label.chars().next_back().is_some_and(is_word_character);
    let left_is_word = content[..start]
        .chars()
        .next_back()
        .is_some_and(is_word_character);
    let right_is_word = content[end..].chars().next().is_some_and(is_word_character);
    (!label_starts_word || !left_is_word) && (!label_ends_word || !right_is_word)
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_contract::EntityKind;
    use phoenix_workspace::{EntitySourceMask, NerEntityRecord};

    #[test]
    fn canonical_matches_are_leftmost_longest_and_token_bounded(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = EntityRegistry::empty();
        registry.publish_ner(
            1,
            &[
                record(1, "Ryan", EntityKind::Character),
                record(2, "New Rome", EntityKind::Location),
                record(3, "Rome", EntityKind::Location),
            ],
        )?;
        let index = EntityHighlightIndex::build(&registry)?;
        let mut candidates = Vec::new();
        index.append_matches("Bryan met ryan in new Rome.", &[], &mut candidates)?;
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.surface.as_str())
                .collect::<Vec<_>>(),
            ["ryan", "new Rome"]
        );
        Ok(())
    }

    #[test]
    fn duplicate_labels_do_not_invent_identity() -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = EntityRegistry::empty();
        registry.publish_ner(
            1,
            &[
                record(1, "Alex", EntityKind::Character),
                record(2, "alex", EntityKind::Npc),
            ],
        )?;
        let index = EntityHighlightIndex::build(&registry)?;
        let mut candidates = Vec::new();
        index.append_matches("Alex arrived.", &[], &mut candidates)?;
        assert!(candidates.is_empty());
        Ok(())
    }

    fn record(id: u64, label: &str, kind: EntityKind) -> NerEntityRecord {
        NerEntityRecord {
            stable_id: id,
            label: label.into(),
            kind,
            custom_kind: None,
            mention_count: 1,
        }
    }

    #[test]
    fn source_mask_remains_registry_authority() {
        assert!(!EntitySourceMask::NER.is_empty());
    }
}
