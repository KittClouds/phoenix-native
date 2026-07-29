use crate::error::GraphGenerationError;
use crate::format::*;
use bytemuck::{try_cast_slice, try_from_bytes, Pod};
use memmap2::Mmap;
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct VerifiedGraphGeneration {
    mmap: Arc<Mmap>,
    path: PathBuf,
    header: GenerationHeader,
    sections: Vec<SectionDescriptor>,
}

impl std::fmt::Debug for VerifiedGraphGeneration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedGraphGeneration")
            .field("path", &self.path)
            .field("generation_hash", &self.header.generation_hash)
            .field("native_document_id", &self.header.native_document_id)
            .field("document_revision", &self.header.document_revision)
            .field("analysis_generation", &self.header.analysis_generation)
            .finish()
    }
}

impl VerifiedGraphGeneration {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GraphGenerationError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| GraphGenerationError::io(path, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| GraphGenerationError::io(path, source))?;
        validate_file_len(metadata.len())?;
        // SAFETY: this is a private read-only mapping. Every byte range is checked
        // before a typed view is exposed, and the file handle remains independent.
        let mmap =
            unsafe { Mmap::map(&file) }.map_err(|source| GraphGenerationError::io(path, source))?;
        Self::from_mmap(path.to_path_buf(), Arc::new(mmap))
    }

    fn from_mmap(path: PathBuf, mmap: Arc<Mmap>) -> Result<Self, GraphGenerationError> {
        let header_len = std::mem::size_of::<GenerationHeader>();
        let header = *try_from_bytes::<GenerationHeader>(&mmap[..header_len])
            .map_err(|_| GraphGenerationError::Corrupt("misaligned header"))?;
        validate_header(&header, mmap.len())?;
        let directory_len = (header.section_count as usize)
            .checked_mul(std::mem::size_of::<SectionDescriptor>())
            .ok_or(GraphGenerationError::Oversized("section directory"))?;
        let directory_end = header_len
            .checked_add(directory_len)
            .ok_or(GraphGenerationError::Oversized("section directory"))?;
        let directory_bytes = mmap
            .get(header_len..directory_end)
            .ok_or(GraphGenerationError::Corrupt("truncated section directory"))?;
        let sections = try_cast_slice::<u8, SectionDescriptor>(directory_bytes)
            .map_err(|_| GraphGenerationError::Corrupt("misaligned section directory"))?
            .to_vec();
        validate_sections(&mmap, directory_end, &sections)?;
        verify_generation_hash(&mmap, &header)?;
        let opened = Self {
            mmap,
            path,
            header,
            sections,
        };
        opened.validate_records()?;
        Ok(opened)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &GenerationHeader {
        &self.header
    }

    pub fn generation_hash(&self) -> [u8; 32] {
        self.header.generation_hash
    }

    pub fn verify_binding(
        &self,
        native_document_id: u64,
        document_revision: u64,
        content_hash: [u8; 32],
        registry_revision: u64,
    ) -> Result<(), GraphGenerationError> {
        if self.header.native_document_id != native_document_id
            || self.header.document_revision != document_revision
            || self.header.content_hash != content_hash
            || self.header.registry_revision != registry_revision
        {
            return Err(GraphGenerationError::Binding(
                "opened generation does not match requested authority",
            ));
        }
        Ok(())
    }

    pub fn document(&self) -> &DocumentRecord {
        &self.documents()[0]
    }

    pub fn documents(&self) -> &[DocumentRecord] {
        self.typed(SectionKind::Document)
    }

    pub fn chunks(&self) -> &[ChunkRecord] {
        self.typed(SectionKind::Chunks)
    }

    pub fn sentences(&self) -> &[SentenceRecord] {
        self.typed(SectionKind::Sentences)
    }

    pub fn spans(&self) -> &[SpanRecord] {
        self.typed(SectionKind::Spans)
    }

    pub fn entities(&self) -> &[EntityRecord] {
        self.typed(SectionKind::Entities)
    }

    pub fn mentions(&self) -> &[MentionRecord] {
        self.typed(SectionKind::Mentions)
    }

    pub fn evidence(&self) -> &[EvidenceRecord] {
        self.typed(SectionKind::Evidence)
    }

    pub fn accepted_edges(&self) -> &[AcceptedEdgeRecord] {
        self.typed(SectionKind::AcceptedEdges)
    }

    pub fn candidate_edges(&self) -> &[CandidateEdgeRecord] {
        self.typed(SectionKind::CandidateEdges)
    }

    pub fn adjudications(&self) -> &[AdjudicationRecord] {
        self.typed(SectionKind::Adjudications)
    }

    pub fn decisions(&self) -> &[DecisionRecord] {
        self.typed(SectionKind::Decisions)
    }

    pub fn capabilities(&self) -> &[CapabilityRecord] {
        self.typed(SectionKind::Capabilities)
    }

    pub fn identities(&self) -> &[IdentityRecord] {
        self.typed(SectionKind::Identities)
    }

    pub fn stage_receipts(&self) -> &[StageReceiptRecord] {
        self.typed(SectionKind::StageReceipts)
    }

    pub fn string(&self, reference: StringRef) -> Result<&str, GraphGenerationError> {
        let slab = self.section_bytes(SectionKind::Strings);
        let start = reference.offset as usize;
        let end = start
            .checked_add(reference.length as usize)
            .ok_or(GraphGenerationError::Corrupt("string reference overflow"))?;
        std::str::from_utf8(slab.get(start..end).ok_or(GraphGenerationError::Corrupt(
            "string reference is out of bounds",
        ))?)
        .map_err(|_| GraphGenerationError::Corrupt("string reference is not UTF-8"))
    }

    fn typed<T: Pod>(&self, kind: SectionKind) -> &[T] {
        try_cast_slice(self.section_bytes(kind))
            .expect("verified generation typed section invariant")
    }

    fn section_bytes(&self, kind: SectionKind) -> &[u8] {
        let descriptor = self
            .sections
            .iter()
            .find(|descriptor| descriptor.kind == kind as u16)
            .expect("verified generation required section invariant");
        let start = descriptor.offset as usize;
        &self.mmap[start..start + descriptor.length as usize]
    }

    fn validate_records(&self) -> Result<(), GraphGenerationError> {
        let document = self
            .documents()
            .first()
            .ok_or(GraphGenerationError::Corrupt("missing document record"))?;
        if self.documents().len() != 1
            || document.chunk_count as usize != self.chunks().len()
            || document.sentence_count as usize != self.sentences().len()
            || document.span_count as usize != self.spans().len()
            || document.entity_count as usize != self.entities().len()
            || document.mention_count as usize != self.mentions().len()
            || document.accepted_edge_count as usize != self.accepted_edges().len()
            || document.candidate_edge_count as usize != self.candidate_edges().len()
        {
            return Err(GraphGenerationError::Corrupt(
                "document record counts do not match sections",
            ));
        }
        self.string(document.source_id)?;
        for span in self.spans() {
            self.string(span.label)?;
        }
        for entity in self.entities() {
            self.string(entity.label)?;
            self.string(entity.custom_kind)?;
        }
        for decision in self.decisions() {
            self.string(decision.reason)?;
        }
        for capability in self.capabilities() {
            self.string(capability.name)?;
            self.string(capability.producer)?;
        }
        for identity in self.identities() {
            self.string(identity.name)?;
            self.string(identity.runtime)?;
        }
        for stage in self.stage_receipts() {
            self.string(stage.name)?;
        }
        validate_references(self)
    }
}

fn validate_file_len(length: u64) -> Result<(), GraphGenerationError> {
    if length > MAX_GENERATION_BYTES {
        return Err(GraphGenerationError::Oversized("generation file"));
    }
    if length < std::mem::size_of::<GenerationHeader>() as u64 {
        return Err(GraphGenerationError::Corrupt("truncated header"));
    }
    Ok(())
}

fn validate_header(
    header: &GenerationHeader,
    actual_len: usize,
) -> Result<(), GraphGenerationError> {
    if header.magic != GRAPH_GENERATION_MAGIC {
        return Err(GraphGenerationError::Corrupt("invalid magic"));
    }
    if header.version != GRAPH_GENERATION_VERSION {
        return Err(GraphGenerationError::Unsupported(header.version as u16));
    }
    if header.header_size as usize != std::mem::size_of::<GenerationHeader>()
        || header.section_count as usize != SectionKind::ALL.len()
        || header.section_count as usize > MAX_SECTION_COUNT
        || header.total_len as usize != actual_len
        || header.native_document_id == 0
        || header.document_revision == 0
        || header.analysis_generation == 0
        || header.content_hash == [0; 32]
        || header.generation_hash == [0; 32]
    {
        return Err(GraphGenerationError::Corrupt("invalid header fields"));
    }
    Ok(())
}

fn validate_sections(
    bytes: &[u8],
    directory_end: usize,
    sections: &[SectionDescriptor],
) -> Result<(), GraphGenerationError> {
    let mut seen = [false; 16];
    let mut previous_end = align8(directory_end);
    for descriptor in sections {
        let kind = SectionKind::from_raw(descriptor.kind)
            .ok_or(GraphGenerationError::Unsupported(descriptor.kind))?;
        if seen[kind as usize] {
            return Err(GraphGenerationError::Corrupt("duplicate section"));
        }
        seen[kind as usize] = true;
        if descriptor.record_size != expected_record_size(kind)
            || descriptor.count > MAX_RECORDS_PER_SECTION
            || descriptor.offset % 8 != 0
        {
            return Err(GraphGenerationError::Corrupt("invalid section descriptor"));
        }
        let expected_len = descriptor
            .count
            .checked_mul(u64::from(descriptor.record_size))
            .ok_or(GraphGenerationError::Oversized("section length"))?;
        if expected_len != descriptor.length {
            return Err(GraphGenerationError::Corrupt(
                "section count and length differ",
            ));
        }
        let start = descriptor.offset as usize;
        let end = start
            .checked_add(descriptor.length as usize)
            .ok_or(GraphGenerationError::Oversized("section range"))?;
        if start < previous_end || end > bytes.len() {
            return Err(GraphGenerationError::Corrupt(
                "overlapping or out-of-bounds section",
            ));
        }
        if *blake3::hash(&bytes[start..end]).as_bytes() != descriptor.hash {
            return Err(GraphGenerationError::Corrupt("section hash mismatch"));
        }
        previous_end = end;
    }
    if SectionKind::ALL.iter().any(|kind| !seen[*kind as usize]) {
        return Err(GraphGenerationError::Corrupt("missing required section"));
    }
    Ok(())
}

fn verify_generation_hash(
    bytes: &[u8],
    header: &GenerationHeader,
) -> Result<(), GraphGenerationError> {
    let field = std::mem::offset_of!(GenerationHeader, generation_hash);
    let end = field + 32;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&bytes[..field]);
    hasher.update(&[0; 32]);
    hasher.update(&bytes[end..]);
    if *hasher.finalize().as_bytes() != header.generation_hash {
        return Err(GraphGenerationError::Corrupt("generation hash mismatch"));
    }
    Ok(())
}

fn validate_references(generation: &VerifiedGraphGeneration) -> Result<(), GraphGenerationError> {
    let document = generation.document();
    let chunks = generation
        .chunks()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let sentences = generation
        .sentences()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let spans = generation
        .spans()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let entities = generation
        .entities()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let mentions = generation
        .mentions()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let evidence = generation
        .evidence()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let accepted = generation
        .accepted_edges()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    let candidates = generation
        .candidate_edges()
        .iter()
        .map(|record| record.candidate_id)
        .collect::<std::collections::BTreeSet<_>>();
    let decisions = generation
        .decisions()
        .iter()
        .map(|record| record.id)
        .collect::<std::collections::BTreeSet<_>>();
    if document.id == 0
        || unique_count_mismatch(chunks.len(), generation.chunks().len())
        || unique_count_mismatch(sentences.len(), generation.sentences().len())
        || unique_count_mismatch(spans.len(), generation.spans().len())
        || unique_count_mismatch(entities.len(), generation.entities().len())
        || unique_count_mismatch(mentions.len(), generation.mentions().len())
        || unique_count_mismatch(evidence.len(), generation.evidence().len())
        || unique_count_mismatch(accepted.len(), generation.accepted_edges().len())
        || unique_count_mismatch(candidates.len(), generation.candidate_edges().len())
        || unique_count_mismatch(decisions.len(), generation.decisions().len())
        || chunks.contains(&0)
        || sentences.contains(&0)
        || spans.contains(&0)
        || entities.contains(&0)
        || mentions.contains(&0)
        || evidence.contains(&0)
        || accepted.contains(&0)
        || candidates.contains(&[0; 32])
        || decisions.contains(&0)
        || generation
            .entities()
            .iter()
            .any(|record| record.source_mask == 0 || record.mention_count == 0)
    {
        return Err(GraphGenerationError::Corrupt(
            "generation IDs are zero or duplicated",
        ));
    }
    let source_len = document.source_len;
    if generation.chunks().iter().any(|record| {
        record.start >= record.end
            || record.end > source_len
            || record.sentence_start > record.sentence_end
            || record.paragraph_start > record.paragraph_end
    }) || generation
        .sentences()
        .iter()
        .any(|record| record.start >= record.end || record.end > source_len)
        || generation
            .spans()
            .iter()
            .any(|record| record.start >= record.end || record.end > source_len)
    {
        return Err(GraphGenerationError::Corrupt(
            "structural record ranges are invalid",
        ));
    }
    if generation.mentions().iter().any(|record| {
        !chunks.contains(&record.chunk_id)
            || !entities.contains(&record.entity_id)
            || !evidence.contains(&record.evidence_id)
            || record.start >= record.end
    }) {
        return Err(GraphGenerationError::Corrupt(
            "mention references are invalid",
        ));
    }
    if generation.evidence().iter().any(|record| {
        !chunks.contains(&record.chunk_id)
            || !entities.contains(&record.entity_id)
            || record.start >= record.end
    }) {
        return Err(GraphGenerationError::Corrupt(
            "evidence references are invalid",
        ));
    }
    let mut accepted_nodes = chunks.clone();
    accepted_nodes.extend(entities.iter().copied());
    accepted_nodes.extend(evidence.iter().copied());
    accepted_nodes.extend(sentences.iter().copied());
    accepted_nodes.extend(spans.iter().copied());
    accepted_nodes.insert(document.id);
    let expected_node_ids = 1usize
        .checked_add(chunks.len())
        .and_then(|count| count.checked_add(sentences.len()))
        .and_then(|count| count.checked_add(spans.len()))
        .and_then(|count| count.checked_add(entities.len()))
        .and_then(|count| count.checked_add(evidence.len()))
        .ok_or(GraphGenerationError::Oversized("node identity count"))?;
    if accepted_nodes.len() != expected_node_ids {
        return Err(GraphGenerationError::Corrupt("generation node IDs collide"));
    }
    if generation.accepted_edges().iter().any(|record| {
        !accepted_nodes.contains(&record.source_id)
            || !accepted_nodes.contains(&record.target_id)
            || record.source_id == record.target_id
            || (record.evidence_id != 0 && !evidence.contains(&record.evidence_id))
            || !f32::from_bits(record.weight_bits).is_finite()
    }) {
        return Err(GraphGenerationError::Corrupt(
            "accepted edge authority is invalid",
        ));
    }
    if generation.candidate_edges().len() != generation.adjudications().len()
        || generation
            .candidate_edges()
            .iter()
            .zip(generation.adjudications())
            .any(|(edge, adjudication)| {
                edge.candidate_id != adjudication.candidate_id
                    || !entities.contains(&edge.source_id)
                    || !entities.contains(&edge.target_id)
            })
    {
        return Err(GraphGenerationError::Corrupt(
            "candidate edge authority is invalid",
        ));
    }
    if generation
        .decisions()
        .iter()
        .any(|record| !candidates.contains(&record.candidate_id))
    {
        return Err(GraphGenerationError::Corrupt(
            "durable decision references an unknown candidate",
        ));
    }
    validate_review_promotion(generation)?;
    Ok(())
}

fn validate_review_promotion(
    generation: &VerifiedGraphGeneration,
) -> Result<(), GraphGenerationError> {
    let candidate_records = generation
        .candidate_edges()
        .iter()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut decided = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    for decision in generation.decisions() {
        if !decided.insert(decision.candidate_id)
            || !matches!(
                decision.status,
                crate::DECISION_STATUS_ACCEPTED
                    | crate::DECISION_STATUS_REJECTED
                    | crate::DECISION_STATUS_DEFERRED
            )
            || decision.flags & crate::DECISION_FLAG_DURABLE_RECEIPT == 0
        {
            return Err(GraphGenerationError::Corrupt(
                "effective decision authority is invalid",
            ));
        }
        if decision.status == crate::DECISION_STATUS_ACCEPTED {
            accepted.insert(decision.candidate_id);
        }
    }
    let mut promoted = BTreeSet::new();
    for edge in generation
        .accepted_edges()
        .iter()
        .filter(|edge| edge.flags & crate::ACCEPTED_EDGE_FLAG_PROMOTED != 0)
    {
        let matching = candidate_records
            .iter()
            .find_map(|(candidate_id, candidate)| {
                (crate::promoted_edge_id(*candidate_id) == edge.id
                    && candidate.source_id == edge.source_id
                    && candidate.target_id == edge.target_id
                    && candidate.relation == edge.relation)
                    .then_some(*candidate_id)
            });
        let Some(candidate_id) = matching else {
            return Err(GraphGenerationError::Corrupt(
                "promoted edge does not match a semantic candidate",
            ));
        };
        if !accepted.contains(&candidate_id) || !promoted.insert(candidate_id) {
            return Err(GraphGenerationError::Corrupt(
                "promoted edge lacks a unique accepted decision",
            ));
        }
    }
    if promoted != accepted {
        return Err(GraphGenerationError::Corrupt(
            "accepted decision is missing its promoted edge",
        ));
    }
    Ok(())
}

fn unique_count_mismatch(unique: usize, actual: usize) -> bool {
    unique != actual
}

fn align8(value: usize) -> usize {
    (value + 7) & !7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_file_length_fails_before_mapping() {
        assert!(matches!(
            validate_file_len(MAX_GENERATION_BYTES + 1),
            Err(GraphGenerationError::Oversized("generation file"))
        ));
    }
}
