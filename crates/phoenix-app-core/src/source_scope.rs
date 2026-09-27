//! Source-local scope over stored, verified provenance.
//!
//! A scope is derived only from explicit bindings in the verified V2
//! generation: chunk passages carry exact source spans, and each evidence
//! record names its chunk, its entity, and its exact span. Nothing enters a
//! scope through similarity, proximity, chapter structure, shared entities, or
//! graph neighborhood. Results are display state and are never written back.

use phoenix_graph_generation_v2::{
    ChunkRecord, DocumentRecord, EvidenceRecord, PageKind, VerifiedGraphGenerationV2,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Exact source coordinates for one stored binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceBinding {
    pub document_id: u64,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceAnchorKind {
    /// A chunk passage selected directly; scope is that exact passage.
    Passage,
    /// An evidence node; scope is the passage its record names.
    Evidence,
    /// An entity node; scope is the passages named by its evidence records.
    Entity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceScope {
    pub anchor: u64,
    pub kind: SourceAnchorKind,
    /// Sorted, de-duplicated graph node ids in scope, including the anchor.
    pub members: Arc<[u64]>,
    /// Passage spans that define the scope, in source order.
    pub passages: Arc<[SourceBinding]>,
    /// Exact spans that "Open source" navigates to, in source order.
    pub targets: Arc<[SourceBinding]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceUnavailable {
    /// Nothing is selected; source-local mode waits for an anchor.
    NoSelection,
    /// No verified source generation is installed for the current scene.
    NoVerifiedSource,
    /// The selected object has no stored passage binding.
    Unbound(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceScopeResolution {
    Scoped(SourceScope),
    Unavailable(SourceUnavailable),
}

#[derive(Clone, Copy, Debug)]
struct EvidenceBinding {
    id: u64,
    chunk: u64,
    entity: u64,
    start: u32,
    end: u32,
}

/// Immutable lookup tables over one verified generation.
#[derive(Debug)]
pub struct SourceScopeIndex {
    generation_hash: [u8; 32],
    document_id: u64,
    document_revision: u64,
    content_hash: [u8; 32],
    chunks: HashMap<u64, (u32, u32)>,
    evidence: HashMap<u64, EvidenceBinding>,
    evidence_by_chunk: HashMap<u64, Vec<u64>>,
    chunks_by_entity: HashMap<u64, BTreeSet<u64>>,
    evidence_by_entity: HashMap<u64, Vec<u64>>,
}

impl SourceScopeIndex {
    /// Builds the index from verified pages. A page that cannot be read
    /// yields `None`; the caller reports that as unavailable, never guessed.
    #[must_use]
    pub fn build(generation: &VerifiedGraphGenerationV2) -> Option<Self> {
        let header = generation.header();
        // Chunks name the generation's structural document record, whose id
        // is distinct from the workspace entry id the editor lease carries.
        let [document] = generation
            .typed_page::<DocumentRecord>(PageKind::Documents)
            .ok()?
        else {
            return None;
        };
        let chunks = generation.typed_page::<ChunkRecord>(PageKind::Chunks).ok()?;
        let evidence = generation
            .typed_page::<EvidenceRecord>(PageKind::Evidence)
            .ok()?;
        Some(Self::from_records(
            header.generation_hash,
            document.id,
            header.native_document_id,
            header.document_revision,
            header.content_hash,
            chunks,
            evidence,
        ))
    }

    #[must_use]
    pub fn from_records(
        generation_hash: [u8; 32],
        structural_document_id: u64,
        document_id: u64,
        document_revision: u64,
        content_hash: [u8; 32],
        chunk_records: &[ChunkRecord],
        evidence_records: &[EvidenceRecord],
    ) -> Self {
        let chunks: HashMap<u64, (u32, u32)> = chunk_records
            .iter()
            .filter(|chunk| chunk.document_id == structural_document_id && chunk.start < chunk.end)
            .map(|chunk| (chunk.id, (chunk.start, chunk.end)))
            .collect();
        let mut evidence = HashMap::with_capacity(evidence_records.len());
        let mut evidence_by_chunk: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut chunks_by_entity: HashMap<u64, BTreeSet<u64>> = HashMap::new();
        let mut evidence_by_entity: HashMap<u64, Vec<u64>> = HashMap::new();
        for record in evidence_records {
            // A binding counts only when its named passage exists and fully
            // contains its span. Anything else fails closed.
            let Some(&(chunk_start, chunk_end)) = chunks.get(&record.chunk_id) else {
                continue;
            };
            if record.start >= record.end || record.start < chunk_start || record.end > chunk_end
            {
                continue;
            }
            evidence.insert(
                record.id,
                EvidenceBinding {
                    id: record.id,
                    chunk: record.chunk_id,
                    entity: record.entity_id,
                    start: record.start,
                    end: record.end,
                },
            );
            evidence_by_chunk
                .entry(record.chunk_id)
                .or_default()
                .push(record.id);
            chunks_by_entity
                .entry(record.entity_id)
                .or_default()
                .insert(record.chunk_id);
            evidence_by_entity
                .entry(record.entity_id)
                .or_default()
                .push(record.id);
        }
        for ids in evidence_by_chunk.values_mut() {
            ids.sort_unstable();
        }
        for ids in evidence_by_entity.values_mut() {
            ids.sort_unstable();
        }
        Self {
            generation_hash,
            document_id,
            document_revision,
            content_hash,
            chunks,
            evidence,
            evidence_by_chunk,
            chunks_by_entity,
            evidence_by_entity,
        }
    }

    #[must_use]
    pub fn generation_hash(&self) -> [u8; 32] {
        self.generation_hash
    }

    /// Resolves the source-local scope for one selected graph node.
    #[must_use]
    pub fn resolve(&self, anchor: u64) -> SourceScopeResolution {
        let (kind, passages, targets): (_, BTreeSet<u64>, Vec<(u32, u32)>) =
            if let Some(&(start, end)) = self.chunks.get(&anchor) {
                (
                    SourceAnchorKind::Passage,
                    BTreeSet::from([anchor]),
                    vec![(start, end)],
                )
            } else if let Some(binding) = self.evidence.get(&anchor) {
                (
                    SourceAnchorKind::Evidence,
                    BTreeSet::from([binding.chunk]),
                    vec![(binding.start, binding.end)],
                )
            } else if let Some(chunks) = self.chunks_by_entity.get(&anchor) {
                let targets = self
                    .evidence_by_entity
                    .get(&anchor)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| self.evidence.get(id))
                    .map(|binding| (binding.start, binding.end))
                    .collect();
                (SourceAnchorKind::Entity, chunks.clone(), targets)
            } else {
                return SourceScopeResolution::Unavailable(SourceUnavailable::Unbound(anchor));
            };

        let mut members = BTreeSet::from([anchor]);
        for &chunk in &passages {
            members.insert(chunk);
            for id in self.evidence_by_chunk.get(&chunk).into_iter().flatten() {
                if let Some(binding) = self.evidence.get(id) {
                    members.insert(binding.id);
                    members.insert(binding.entity);
                }
            }
        }
        let passage_spans: BTreeMap<(u32, u32), ()> = passages
            .iter()
            .filter_map(|chunk| self.chunks.get(chunk))
            .map(|&span| (span, ()))
            .collect();
        let mut targets = targets;
        targets.sort_unstable();
        targets.dedup();
        SourceScopeResolution::Scoped(SourceScope {
            anchor,
            kind,
            members: members.into_iter().collect(),
            passages: passage_spans
                .into_keys()
                .map(|(start, end)| self.binding(start, end))
                .collect(),
            targets: targets
                .into_iter()
                .map(|(start, end)| self.binding(start, end))
                .collect(),
        })
    }

    /// Objects whose stored spans overlap any of `ranges` (half-open byte
    /// ranges in this generation's document revision): the passages that
    /// overlap, evidence whose exact span overlaps, and those evidence
    /// records' entities. Nothing is inferred from proximity or structure.
    #[must_use]
    pub fn bound_to_ranges(&self, ranges: &[(u32, u32)]) -> RangeBinding {
        let overlaps = |start: u32, end: u32| {
            ranges
                .iter()
                .any(|&(from, to)| from < to && start < to && from < end)
        };
        let mut passages: Vec<u64> = self
            .chunks
            .iter()
            .filter(|(_, &(start, end))| overlaps(start, end))
            .map(|(&id, _)| id)
            .collect();
        passages.sort_unstable();
        let mut members = BTreeSet::new();
        members.extend(passages.iter().copied());
        for binding in self.evidence.values() {
            if overlaps(binding.start, binding.end) {
                members.insert(binding.id);
                members.insert(binding.entity);
            }
        }
        RangeBinding {
            passages: passages.into(),
            members: members.into_iter().collect(),
        }
    }

    fn binding(&self, start: u32, end: u32) -> SourceBinding {
        SourceBinding {
            document_id: self.document_id,
            document_revision: self.document_revision,
            content_hash: self.content_hash,
            start,
            end,
        }
    }
}

/// Graph objects bound to a set of byte ranges; see
/// [`SourceScopeIndex::bound_to_ranges`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RangeBinding {
    /// Sorted passage (chunk) node ids that overlap the ranges.
    pub passages: Arc<[u64]>,
    /// Sorted node ids: passages, overlapping evidence, and their entities.
    pub members: Arc<[u64]>,
}

/// Caches one index per verified generation so repeated selection changes
/// reuse the same lookup tables.
#[derive(Debug, Default)]
pub struct SourceScopeCache {
    index: Option<Arc<SourceScopeIndex>>,
}

impl SourceScopeCache {
    /// The index for `generation`, rebuilt only when the generation changes.
    pub fn index(
        &mut self,
        generation: Option<&VerifiedGraphGenerationV2>,
    ) -> Option<Arc<SourceScopeIndex>> {
        let generation = generation?;
        let hash = generation.header().generation_hash;
        if self
            .index
            .as_ref()
            .is_none_or(|index| index.generation_hash() != hash)
        {
            self.index = SourceScopeIndex::build(generation).map(Arc::new);
        }
        self.index.clone()
    }

    /// Resolves the current selection. `None` for the generation means the
    /// scene has no verified source authority.
    pub fn resolve(
        &mut self,
        generation: Option<&VerifiedGraphGenerationV2>,
        selected_node: Option<u64>,
    ) -> SourceScopeResolution {
        let Some(anchor) = selected_node else {
            return SourceScopeResolution::Unavailable(SourceUnavailable::NoSelection);
        };
        let Some(generation) = generation else {
            return SourceScopeResolution::Unavailable(SourceUnavailable::NoVerifiedSource);
        };
        let hash = generation.header().generation_hash;
        if self
            .index
            .as_ref()
            .is_none_or(|index| index.generation_hash() != hash)
        {
            self.index = SourceScopeIndex::build(generation).map(Arc::new);
        }
        match self.index.as_ref() {
            Some(index) => index.resolve(anchor),
            None => SourceScopeResolution::Unavailable(SourceUnavailable::NoVerifiedSource),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    /// Structural document record id; deliberately different from the
    /// workspace entry id bindings carry, as in real generations.
    const DOC: u64 = 7;
    const ENTRY: u64 = 3;

    fn chunk(id: u64, start: u32, end: u32) -> ChunkRecord {
        let mut record = ChunkRecord::zeroed();
        record.id = id;
        record.document_id = DOC;
        record.start = start;
        record.end = end;
        record
    }

    fn evidence(id: u64, entity: u64, chunk: u64, start: u32, end: u32) -> EvidenceRecord {
        let mut record = EvidenceRecord::zeroed();
        record.id = id;
        record.entity_id = entity;
        record.chunk_id = chunk;
        record.start = start;
        record.end = end;
        record
    }

    fn index() -> SourceScopeIndex {
        // Two passages. Entity 100 appears in both; 200 only in the first;
        // 300 only in the second. Evidence 14 names a missing passage and 15
        // lies outside its passage, so neither is a verified binding.
        SourceScopeIndex::from_records(
            [1; 32],
            DOC,
            ENTRY,
            3,
            [9; 32],
            &[chunk(1, 0, 100), chunk(2, 100, 200)],
            &[
                evidence(10, 100, 1, 5, 10),
                evidence(11, 200, 1, 20, 25),
                evidence(12, 100, 2, 110, 115),
                evidence(13, 300, 2, 150, 160),
                evidence(14, 400, 99, 5, 10),
                evidence(15, 500, 1, 95, 120),
            ],
        )
    }

    fn scoped(resolution: SourceScopeResolution) -> SourceScope {
        match resolution {
            SourceScopeResolution::Scoped(scope) => scope,
            other => panic!("expected scope, got {other:?}"),
        }
    }

    #[test]
    fn passage_scope_is_exactly_the_objects_bound_to_that_passage() {
        let scope = scoped(index().resolve(1));
        assert_eq!(scope.kind, SourceAnchorKind::Passage);
        assert_eq!(&*scope.members, &[1, 10, 11, 100, 200]);
        assert_eq!(scope.targets.len(), 1);
        assert_eq!((scope.targets[0].start, scope.targets[0].end), (0, 100));
        assert_eq!(scope.targets[0].document_revision, 3);
        assert_eq!(scope.targets[0].document_id, ENTRY);
    }

    #[test]
    fn entity_scope_unions_only_passages_named_by_its_evidence() {
        let scope = scoped(index().resolve(100));
        assert_eq!(scope.kind, SourceAnchorKind::Entity);
        assert_eq!(&*scope.members, &[1, 2, 10, 11, 12, 13, 100, 200, 300]);
        assert_eq!(scope.passages.len(), 2);
        let spans: Vec<_> = scope.targets.iter().map(|t| (t.start, t.end)).collect();
        assert_eq!(spans, vec![(5, 10), (110, 115)]);
    }

    #[test]
    fn evidence_scope_uses_its_named_passage_and_targets_its_exact_span() {
        let scope = scoped(index().resolve(13));
        assert_eq!(scope.kind, SourceAnchorKind::Evidence);
        assert_eq!(&*scope.members, &[2, 12, 13, 100, 300]);
        assert_eq!((scope.targets[0].start, scope.targets[0].end), (150, 160));
    }

    #[test]
    fn invalid_or_missing_bindings_are_unavailable_not_guessed() {
        let index = index();
        for unbound in [14, 15, 400, 500, 999] {
            assert_eq!(
                index.resolve(unbound),
                SourceScopeResolution::Unavailable(SourceUnavailable::Unbound(unbound))
            );
        }
    }

    #[test]
    fn same_anchor_always_resolves_to_the_same_scope_without_leaking() {
        let index = index();
        let first = index.resolve(200);
        let _other = index.resolve(300);
        assert_eq!(index.resolve(200), first);
        let scope = scoped(first);
        assert!(!scope.members.contains(&300));
        assert!(!scope.members.contains(&2));
    }

    #[test]
    fn spoken_ranges_bind_only_overlapping_passages_evidence_and_entities() {
        let index = index();
        // A sentence inside the first passage covering evidence 10 only.
        let bound = index.bound_to_ranges(&[(4, 12)]);
        assert_eq!(&*bound.passages, &[1]);
        assert_eq!(&*bound.members, &[1, 10, 100]);
        // Touching a boundary is not an overlap; empty ranges bind nothing.
        assert!(index.bound_to_ranges(&[(10, 11)]).members.iter().all(|id| *id != 10));
        assert!(index.bound_to_ranges(&[(50, 50)]).members.is_empty());
        // The same ranges always bind the same objects.
        assert_eq!(index.bound_to_ranges(&[(4, 12)]), bound);
        // Ranges spanning two passages bind both and their evidence.
        let wide = index.bound_to_ranges(&[(90, 112)]);
        assert_eq!(&*wide.passages, &[1, 2]);
        assert!(wide.members.contains(&12) && wide.members.contains(&100));
    }

    #[test]
    fn cache_reports_missing_selection_and_missing_authority() {
        let mut cache = SourceScopeCache::default();
        assert_eq!(
            cache.resolve(None, None),
            SourceScopeResolution::Unavailable(SourceUnavailable::NoSelection)
        );
        assert_eq!(
            cache.resolve(None, Some(1)),
            SourceScopeResolution::Unavailable(SourceUnavailable::NoVerifiedSource)
        );
    }
}
