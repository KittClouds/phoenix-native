use crate::{
    compute_source_set_hash, deterministic_id, validate::validate_directory,
    validate::validate_header, AuthoritySubjectKind, CandidateEndpointBindingRecordV3,
    CandidateEndpointRoleV3, ContentUnitKind, ContentUnitRecord, ConversationRecord,
    DocumentRevisionRecord, EvidenceRecordV3, GenerationHeaderV3, MemoryContractError,
    MentionRecordV3, PageDescriptorV3, PageKindV3, ParticipantRole, ProducerCapabilityRecordV3,
    ProducerProductV3, ProducerStateV3, SemanticCandidateFamilyV3, SemanticCandidateRecordV3,
    SourceKind, SourceRecord, StringRef, SupersessionRecord, TurnRecord, ValidityIntervalRecord,
    VocabularyPackKindV3, VocabularyPackRecordV3, SOURCE_FLAG_COMPLETE,
};
use bytemuck::Pod;
use hashbrown::{HashMap, HashSet};
use memmap2::{Mmap, MmapOptions};
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateStatus, ChunkRecord, ModelIdentityRecord,
};
use std::fs::File;
use std::mem::size_of;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenExpectation {
    pub namespace_hash: Option<[u8; 32]>,
    pub source_set_hash: Option<[u8; 32]>,
    pub minimum_published_generation: Option<u64>,
}

pub struct VerifiedGraphGenerationV3 {
    mmap: Mmap,
    header: GenerationHeaderV3,
    directory: [PageDescriptorV3; 39],
}

impl std::fmt::Debug for VerifiedGraphGenerationV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedGraphGenerationV3")
            .field("generation_hash", &self.header.generation_hash)
            .field("source_set_hash", &self.header.source_set_hash)
            .field("source_count", &self.header.source_count)
            .field(
                "document_revision_count",
                &self.header.document_revision_count,
            )
            .field("conversation_count", &self.header.conversation_count)
            .field("turn_count", &self.header.turn_count)
            .finish()
    }
}

impl VerifiedGraphGenerationV3 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MemoryContractError> {
        Self::open_expected(path, OpenExpectation::default())
    }

    pub fn open_expected(
        path: impl AsRef<Path>,
        expectation: OpenExpectation,
    ) -> Result<Self, MemoryContractError> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?;
        let actual_len = file
            .metadata()
            .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?
            .len();

        // SAFETY: the map is read-only and retained by this owner. Every page
        // is bounds-, alignment-, schema-, count-, and hash-verified before a
        // typed `Pod` view can be returned.
        let mmap = unsafe {
            MmapOptions::new()
                .map(&file)
                .map_err(|source| MemoryContractError::io(path.to_path_buf(), source))?
        };
        let header_bytes =
            mmap.get(..size_of::<GenerationHeaderV3>())
                .ok_or(MemoryContractError::TooSmall {
                    actual: actual_len,
                    minimum: size_of::<GenerationHeaderV3>() as u64,
                })?;
        let header =
            *bytemuck::try_from_bytes::<GenerationHeaderV3>(header_bytes).map_err(|_| {
                MemoryContractError::InvalidRecordLayout {
                    page: PageKindV3::Sources,
                }
            })?;
        validate_header(&header, actual_len)?;

        let directory_start = usize::try_from(header.directory_offset)
            .map_err(|_| MemoryContractError::DirectoryOutOfBounds)?;
        let directory_end = header
            .directory_offset
            .checked_add(header.directory_len)
            .and_then(|end| usize::try_from(end).ok())
            .ok_or(MemoryContractError::DirectoryOutOfBounds)?;
        let directory_bytes = mmap
            .get(directory_start..directory_end)
            .ok_or(MemoryContractError::DirectoryOutOfBounds)?;
        let directory: [PageDescriptorV3; 39] =
            bytemuck::try_cast_slice::<u8, PageDescriptorV3>(directory_bytes)
                .map_err(|_| MemoryContractError::DirectoryOutOfBounds)?
                .try_into()
                .map_err(|_| MemoryContractError::DirectoryOutOfBounds)?;
        validate_directory(&header, &directory, &mmap)?;
        validate_source_set_hash(&header, &directory)?;
        validate_expectation(&header, expectation)?;

        let generation = Self {
            mmap,
            header,
            directory,
        };
        generation.validate_source_model()?;
        Ok(generation)
    }

    pub fn header(&self) -> &GenerationHeaderV3 {
        &self.header
    }

    pub fn directory(&self) -> &[PageDescriptorV3] {
        &self.directory
    }

    pub fn descriptor(&self, kind: PageKindV3) -> &PageDescriptorV3 {
        &self.directory[(kind as usize) - 1]
    }

    pub fn page_bytes(&self, kind: PageKindV3) -> &[u8] {
        let descriptor = self.descriptor(kind);
        let start = descriptor.offset as usize;
        let end = start + descriptor.length as usize;
        &self.mmap[start..end]
    }

    pub fn typed_page<T: Pod>(&self, kind: PageKindV3) -> Result<&[T], MemoryContractError> {
        let descriptor = self.descriptor(kind);
        if descriptor.record_size as usize != size_of::<T>() {
            return Err(MemoryContractError::InvalidRecordLayout { page: kind });
        }
        bytemuck::try_cast_slice(self.page_bytes(kind))
            .map_err(|_| MemoryContractError::InvalidRecordLayout { page: kind })
    }

    pub fn resolve_string(&self, reference: StringRef) -> Result<&str, MemoryContractError> {
        self.resolve_ref(PageKindV3::Strings, reference)
    }

    pub fn resolve_source_text(&self, reference: StringRef) -> Result<&str, MemoryContractError> {
        self.resolve_ref(PageKindV3::SourceText, reference)
    }

    fn resolve_ref(
        &self,
        kind: PageKindV3,
        reference: StringRef,
    ) -> Result<&str, MemoryContractError> {
        let bytes = self.page_bytes(kind);
        let start =
            usize::try_from(reference.offset).map_err(|_| MemoryContractError::InvalidStringRef)?;
        let end = start
            .checked_add(reference.length as usize)
            .ok_or(MemoryContractError::InvalidStringRef)?;
        std::str::from_utf8(
            bytes
                .get(start..end)
                .ok_or(MemoryContractError::InvalidStringRef)?,
        )
        .map_err(|_| MemoryContractError::InvalidStringRef)
    }

    fn validate_source_model(&self) -> Result<(), MemoryContractError> {
        let sources = self.typed_page::<SourceRecord>(PageKindV3::Sources)?;
        let documents = self.typed_page::<DocumentRevisionRecord>(PageKindV3::DocumentRevisions)?;
        let conversations = self.typed_page::<ConversationRecord>(PageKindV3::Conversations)?;
        let turns = self.typed_page::<TurnRecord>(PageKindV3::Turns)?;
        let units = self.typed_page::<ContentUnitRecord>(PageKindV3::ContentUnits)?;
        let chunks = self.typed_page::<ChunkRecord>(PageKindV3::Chunks)?;
        let mentions = self.typed_page::<MentionRecordV3>(PageKindV3::Mentions)?;
        let evidence = self.typed_page::<EvidenceRecordV3>(PageKindV3::Evidence)?;
        let validity = self.typed_page::<ValidityIntervalRecord>(PageKindV3::ValidityIntervals)?;
        let supersessions = self.typed_page::<SupersessionRecord>(PageKindV3::Supersessions)?;
        let semantic_candidates =
            self.typed_page::<SemanticCandidateRecordV3>(PageKindV3::SemanticCandidates)?;
        let candidate_evidence = self
            .typed_page::<CandidateEvidenceBindingRecord>(PageKindV3::CandidateEvidenceBindings)?;
        let producer_capabilities =
            self.typed_page::<ProducerCapabilityRecordV3>(PageKindV3::ProducerCapabilitiesV3)?;
        let vocabulary_packs =
            self.typed_page::<VocabularyPackRecordV3>(PageKindV3::VocabularyPacks)?;
        let candidate_endpoints = self.typed_page::<CandidateEndpointBindingRecordV3>(
            PageKindV3::CandidateEndpointBindings,
        )?;
        let model_identities =
            self.typed_page::<ModelIdentityRecord>(PageKindV3::ModelIdentities)?;

        if sources.is_empty()
            || self.header.source_count != sources.len() as u64
            || self.header.document_revision_count != documents.len() as u64
            || self.header.conversation_count != conversations.len() as u64
            || self.header.turn_count != turns.len() as u64
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "header counts do not match source pages",
            ));
        }
        ensure_strictly_increasing(sources.iter().map(|record| record.id))?;

        let namespace_id = deterministic_id(b"namespace", &[&self.header.namespace_hash]);
        let mut source_kinds = HashMap::with_capacity(sources.len());
        let mut source_hashes = HashMap::with_capacity(sources.len());
        for source in sources {
            let kind = SourceKind::from_raw(source.kind).ok_or(
                MemoryContractError::InvalidSourceModel("source kind is invalid"),
            )?;
            if source.namespace_id != namespace_id || source.flags & SOURCE_FLAG_COMPLETE == 0 {
                return Err(MemoryContractError::InvalidSourceModel(
                    "source namespace or completeness is invalid",
                ));
            }
            source_kinds.insert(source.id, kind);
            source_hashes.insert(source.id, source.content_hash);
        }

        ensure_strictly_increasing(
            documents
                .iter()
                .map(|record| (record.source_id, record.revision)),
        )?;
        let mut owner_lengths = HashMap::with_capacity(documents.len() + turns.len());
        let mut document_ids = HashSet::with_capacity(documents.len());
        for document in documents {
            require_source_kind(
                &source_kinds,
                document.source_id,
                SourceKind::WorkspaceDocument,
            )?;
            let path = self.resolve_string(document.path)?;
            let content = self.resolve_source_text(document.content)?;
            if path.is_empty()
                || document.content_hash != *blake3::hash(content.as_bytes()).as_bytes()
                || source_hashes.get(&document.source_id) != Some(&document.content_hash)
                || document.valid_time_from_millis > document.valid_time_to_millis
                || document.system_generation_from > document.system_generation_to
                || !document_ids.insert(document.document_id)
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "document revision binding is invalid",
                ));
            }
            owner_lengths.insert(document.document_id, content.len() as u32);
            validate_index_range(
                document.chunk_start,
                document.chunk_count,
                chunks.len(),
                "document chunk range is invalid",
            )?;
            for chunk in &chunks[document.chunk_start as usize
                ..(document.chunk_start + document.chunk_count) as usize]
            {
                let chunk_bytes = content
                    .as_bytes()
                    .get(chunk.start as usize..chunk.end as usize)
                    .ok_or(MemoryContractError::InvalidSourceModel(
                        "dynamic chunk range is invalid",
                    ))?;
                if chunk.document_id != document.document_id
                    || chunk.start > chunk.end
                    || chunk.end > content.len() as u32
                    || chunk.content_hash != hash_prefix_u64(*blake3::hash(chunk_bytes).as_bytes())
                {
                    return Err(MemoryContractError::InvalidSourceModel(
                        "dynamic chunk does not bind to its document revision",
                    ));
                }
            }
        }

        ensure_strictly_increasing(conversations.iter().map(|record| record.conversation_id))?;
        let mut conversation_sources = HashMap::with_capacity(conversations.len());
        for conversation in conversations {
            require_source_kind(
                &source_kinds,
                conversation.source_id,
                SourceKind::Conversation,
            )?;
            if conversation.started_at_millis > conversation.ended_at_millis {
                return Err(MemoryContractError::InvalidSourceModel(
                    "conversation time interval is invalid",
                ));
            }
            validate_index_range(
                conversation.turn_start,
                conversation.turn_count,
                turns.len(),
                "conversation turn range is invalid",
            )?;
            let range = &turns[conversation.turn_start as usize
                ..(conversation.turn_start + conversation.turn_count) as usize];
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"phoenix/conversation-content/v1\0");
            for turn in range {
                hasher.update(&turn.ordinal.to_le_bytes());
                hasher.update(&turn.role.to_le_bytes());
                hasher.update(&turn.event_time_millis.to_le_bytes());
                hasher.update(&turn.content_hash);
            }
            if hasher.finalize().as_bytes() != &conversation.content_hash
                || source_hashes.get(&conversation.source_id) != Some(&conversation.content_hash)
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "conversation aggregate hash is invalid",
                ));
            }
            conversation_sources.insert(conversation.conversation_id, conversation.source_id);
        }

        let mut turn_ids = HashSet::with_capacity(turns.len());
        let mut prior_turns = HashSet::with_capacity(turns.len());
        let mut last_key = None;
        for turn in turns {
            let key = (turn.conversation_id, turn.ordinal, turn.id);
            if last_key.is_some_and(|previous| previous >= key) {
                return Err(MemoryContractError::NonCanonicalOrder);
            }
            last_key = Some(key);
            let expected_source = conversation_sources.get(&turn.conversation_id).ok_or(
                MemoryContractError::InvalidSourceModel("turn conversation does not resolve"),
            )?;
            let content = self.resolve_source_text(turn.content)?;
            if turn.source_id != *expected_source
                || ParticipantRole::from_raw(turn.role).is_none()
                || turn.content_hash != *blake3::hash(content.as_bytes()).as_bytes()
                || !turn_ids.insert(turn.id)
                || (turn.reply_to_turn_id != 0 && !prior_turns.contains(&turn.reply_to_turn_id))
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "turn identity, content, role, or reply binding is invalid",
                ));
            }
            prior_turns.insert(turn.id);
            owner_lengths.insert(turn.id, content.len() as u32);
        }

        ensure_strictly_increasing(
            units
                .iter()
                .map(|record| (record.source_id, record.kind, record.ordinal, record.id)),
        )?;
        let mut unit_ranges = HashMap::with_capacity(units.len());
        for unit in units {
            if !source_kinds.contains_key(&unit.source_id)
                || ContentUnitKind::from_raw(unit.kind).is_none()
                || unit.start > unit.end
                || unit.end > *owner_lengths.get(&unit.owner_id).unwrap_or(&0)
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "content unit source, kind, or range is invalid",
                ));
            }
            unit_ranges.insert(unit.id, (unit.source_id, unit.start, unit.end));
        }

        validate_evidence_ranges(mentions, evidence, &source_kinds, &unit_ranges)?;
        validate_semantic_candidates(
            self,
            semantic_candidates,
            candidate_endpoints,
            candidate_evidence,
            producer_capabilities,
            vocabulary_packs,
            model_identities.len(),
            &source_kinds,
            evidence,
        )?;
        validate_authority_time(validity, supersessions)
    }
}

fn validate_source_set_hash(
    header: &GenerationHeaderV3,
    directory: &[PageDescriptorV3; 39],
) -> Result<(), MemoryContractError> {
    let source_text_hash = directory[(PageKindV3::SourceText as usize) - 1].hash;
    let source_hashes = PageKindV3::ALL
        [(PageKindV3::Sources as usize) - 1..=(PageKindV3::Spans as usize) - 1]
        .iter()
        .map(|kind| directory[(*kind as usize) - 1].hash)
        .collect::<Vec<_>>();
    if compute_source_set_hash(&source_text_hash, &source_hashes) != header.source_set_hash {
        return Err(MemoryContractError::SourceSetHashMismatch);
    }
    Ok(())
}

fn validate_expectation(
    header: &GenerationHeaderV3,
    expectation: OpenExpectation,
) -> Result<(), MemoryContractError> {
    if expectation
        .namespace_hash
        .is_some_and(|expected| expected != header.namespace_hash)
    {
        return Err(MemoryContractError::NamespaceMismatch);
    }
    if expectation
        .source_set_hash
        .is_some_and(|expected| expected != header.source_set_hash)
    {
        return Err(MemoryContractError::ExpectedSourceSetMismatch);
    }
    if expectation
        .minimum_published_generation
        .is_some_and(|minimum| header.published_generation < minimum)
    {
        return Err(MemoryContractError::StaleGeneration {
            actual: header.published_generation,
            minimum: expectation.minimum_published_generation.unwrap_or_default(),
        });
    }
    Ok(())
}

fn require_source_kind(
    source_kinds: &HashMap<u64, SourceKind>,
    source_id: u64,
    expected: SourceKind,
) -> Result<(), MemoryContractError> {
    if source_kinds.get(&source_id) != Some(&expected) {
        return Err(MemoryContractError::InvalidSourceModel(
            "record source does not resolve to the required source kind",
        ));
    }
    Ok(())
}

fn validate_index_range(
    start: u32,
    count: u32,
    total: usize,
    message: &'static str,
) -> Result<(), MemoryContractError> {
    let end = start
        .checked_add(count)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(MemoryContractError::InvalidSourceModel(message))?;
    if end > total {
        return Err(MemoryContractError::InvalidSourceModel(message));
    }
    Ok(())
}

fn validate_evidence_ranges(
    mentions: &[MentionRecordV3],
    evidence: &[EvidenceRecordV3],
    sources: &HashMap<u64, SourceKind>,
    units: &HashMap<u64, (u64, u32, u32)>,
) -> Result<(), MemoryContractError> {
    ensure_strictly_increasing(mentions.iter().map(|record| record.id))?;
    ensure_strictly_increasing(evidence.iter().map(|record| record.id))?;
    let mention_ids = mentions
        .iter()
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    for mention in mentions {
        let unit =
            units
                .get(&mention.content_unit_id)
                .ok_or(MemoryContractError::InvalidSourceModel(
                    "mention content unit does not resolve",
                ))?;
        if !sources.contains_key(&mention.source_id)
            || unit.0 != mention.source_id
            || mention.start < unit.1
            || mention.end > unit.2
            || mention.start > mention.end
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "mention source range is invalid",
            ));
        }
    }
    for anchor in evidence {
        let unit =
            units
                .get(&anchor.content_unit_id)
                .ok_or(MemoryContractError::InvalidSourceModel(
                    "evidence content unit does not resolve",
                ))?;
        if !sources.contains_key(&anchor.source_id)
            || unit.0 != anchor.source_id
            || anchor.start < unit.1
            || anchor.end > unit.2
            || anchor.start > anchor.end
            || (anchor.mention_id != 0 && !mention_ids.contains(&anchor.mention_id))
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "evidence source range is invalid",
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_semantic_candidates(
    generation: &VerifiedGraphGenerationV3,
    candidates: &[SemanticCandidateRecordV3],
    endpoints: &[CandidateEndpointBindingRecordV3],
    bindings: &[CandidateEvidenceBindingRecord],
    capabilities: &[ProducerCapabilityRecordV3],
    vocabulary_packs: &[VocabularyPackRecordV3],
    model_count: usize,
    sources: &HashMap<u64, SourceKind>,
    evidence: &[EvidenceRecordV3],
) -> Result<(), MemoryContractError> {
    ensure_strictly_increasing(candidates.iter().map(|record| record.candidate_id))?;
    let evidence_ids = evidence
        .iter()
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    ensure_strictly_increasing(vocabulary_packs.iter().map(|record| record.id))?;
    let mut pack_ids = HashSet::with_capacity(vocabulary_packs.len());
    for pack in vocabulary_packs {
        if pack.id == 0
            || pack.schema_hash == [0; 32]
            || pack.producer_identity_hash == [0; 32]
            || VocabularyPackKindV3::from_raw(pack.kind).is_none()
            || generation.resolve_string(pack.name)?.is_empty()
            || generation.resolve_string(pack.version)?.is_empty()
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "vocabulary pack authority is invalid",
            ));
        }
        pack_ids.insert(pack.id);
    }
    let mut bound_binding_count = 0_usize;
    let mut bound_endpoint_count = 0_usize;
    for candidate in candidates {
        validate_index_range(
            candidate.endpoint_start,
            candidate.endpoint_count,
            endpoints.len(),
            "semantic candidate endpoint range is invalid",
        )?;
        validate_index_range(
            candidate.evidence_start,
            candidate.evidence_count,
            bindings.len(),
            "semantic candidate evidence range is invalid",
        )?;
        let range = &bindings[candidate.evidence_start as usize
            ..(candidate.evidence_start + candidate.evidence_count) as usize];
        let endpoint_range = &endpoints[candidate.endpoint_start as usize
            ..(candidate.endpoint_start + candidate.endpoint_count) as usize];
        if candidate.candidate_id.iter().all(|byte| *byte == 0)
            || candidate.endpoint_count == 0
            || candidate.evidence_count == 0
            || !sources.contains_key(&candidate.source_id)
            || !pack_ids.contains(&candidate.vocabulary_pack_id)
            || candidate.producer_identity_hash == [0; 32]
            || SemanticCandidateFamilyV3::from_raw(candidate.family).is_none()
            || candidate.status != CandidateStatus::Proposed as u16
            || candidate.valid_time_from_millis > candidate.valid_time_to_millis
            || candidate.system_generation_from > candidate.system_generation_to
            || (candidate.model_identity_index != u32::MAX
                && candidate.model_identity_index as usize >= model_count)
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "semantic candidate authority is invalid",
            ));
        }
        generation.resolve_string(candidate.relation_kind)?;
        generation.resolve_string(candidate.value)?;
        for (ordinal, endpoint) in endpoint_range.iter().enumerate() {
            if endpoint.candidate_id != candidate.candidate_id
                || endpoint.ordinal as usize != ordinal
                || endpoint.endpoint_id == 0
                || CandidateEndpointRoleV3::from_raw(endpoint.role).is_none()
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "semantic candidate endpoint binding is invalid",
                ));
            }
        }
        for (ordinal, binding) in range.iter().enumerate() {
            if binding.candidate_id.0 != candidate.candidate_id
                || binding.ordinal as usize != ordinal
                || !evidence_ids.contains(&binding.evidence_id)
            {
                return Err(MemoryContractError::InvalidSourceModel(
                    "semantic candidate evidence binding is invalid",
                ));
            }
        }
        bound_endpoint_count = bound_endpoint_count.saturating_add(endpoint_range.len());
        bound_binding_count = bound_binding_count.saturating_add(range.len());
    }
    if bound_endpoint_count != endpoints.len() {
        return Err(MemoryContractError::InvalidSourceModel(
            "semantic candidate endpoints contain orphan rows",
        ));
    }
    if bound_binding_count != bindings.len() {
        return Err(MemoryContractError::InvalidSourceModel(
            "semantic candidate evidence bindings contain orphan rows",
        ));
    }

    if capabilities.is_empty() {
        return Ok(());
    }
    ensure_strictly_increasing(capabilities.iter().map(|record| record.product))?;
    if capabilities.len() != ProducerProductV3::ALL.len() {
        return Err(MemoryContractError::InvalidSourceModel(
            "producer capability matrix is incomplete",
        ));
    }
    for (record, expected) in capabilities.iter().zip(ProducerProductV3::ALL) {
        let state = ProducerStateV3::from_raw(record.state).ok_or(
            MemoryContractError::InvalidSourceModel("producer capability state is invalid"),
        )?;
        if record.product != expected as u16
            || record.producer_binary_hash == [0; 32]
            || record.config_hash == [0; 32]
            || (record.model_identity_index != u32::MAX
                && record.model_identity_index as usize >= model_count)
            || (state == ProducerStateV3::Unsupported && record.output_count != 0)
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "producer capability authority is invalid",
            ));
        }
        let producer = generation.resolve_string(record.producer)?;
        if producer.is_empty() {
            return Err(MemoryContractError::InvalidSourceModel(
                "producer identity is empty",
            ));
        }
    }
    Ok(())
}

fn ensure_strictly_increasing<T: Ord>(
    values: impl IntoIterator<Item = T>,
) -> Result<(), MemoryContractError> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|item| item >= &value) {
            return Err(MemoryContractError::NonCanonicalOrder);
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_authority_time(
    intervals: &[ValidityIntervalRecord],
    supersessions: &[SupersessionRecord],
) -> Result<(), MemoryContractError> {
    ensure_strictly_increasing(intervals.iter().map(|record| {
        (
            record.subject_id,
            record.subject_kind,
            record.system_generation_from,
        )
    }))?;
    for interval in intervals {
        if interval.subject_id.iter().all(|byte| *byte == 0)
            || AuthoritySubjectKind::from_raw(interval.subject_kind).is_none()
            || interval.valid_time_from_millis > interval.valid_time_to_millis
            || interval.system_generation_from > interval.system_generation_to
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "authority validity interval is invalid",
            ));
        }
    }
    ensure_strictly_increasing(
        supersessions
            .iter()
            .map(|record| (record.subject_id, record.replacement_id)),
    )?;
    for supersession in supersessions {
        if supersession.subject_id.iter().all(|byte| *byte == 0)
            || supersession.replacement_id.iter().all(|byte| *byte == 0)
            || supersession.subject_id == supersession.replacement_id
            || supersession.decision_id == 0
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "supersession is not decision-bound",
            ));
        }
    }
    Ok(())
}

fn hash_prefix_u64(hash: [u8; 32]) -> u64 {
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(bytes)
}
