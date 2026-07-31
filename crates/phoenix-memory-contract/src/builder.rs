use crate::{
    ContentUnitKind, ContentUnitRecord, ConversationRecord, DocumentRevisionRecord,
    GenerationPagesV3, GenerationWriteAuthorityV3, MemoryContractError, ParticipantRole,
    SourceKind, SourceRecord, StringRef, TurnRecord, VerifiedGraphGenerationV3,
    SOURCE_FLAG_COMPLETE, TIME_UNBOUNDED,
};
use hashbrown::{HashMap, HashSet};
use phoenix_graph_generation_v2::ChunkRecord;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChunkInput {
    pub start: u32,
    pub end: u32,
    pub sentence_start: u32,
    pub sentence_end: u32,
    pub paragraph_start: u32,
    pub paragraph_end: u32,
    pub chapter_index: u32,
    pub token_count: u32,
    pub flags: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentInput {
    pub external_id: Vec<u8>,
    pub revision: u64,
    pub path: String,
    pub text: String,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub system_generation_from: u64,
    pub system_generation_to: u64,
    pub chunks: Vec<DocumentChunkInput>,
}

impl DocumentInput {
    pub fn current(
        external_id: impl Into<Vec<u8>>,
        revision: u64,
        path: impl Into<String>,
        text: impl Into<String>,
        chunks: Vec<DocumentChunkInput>,
        system_generation: u64,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            revision,
            path: path.into(),
            text: text.into(),
            valid_time_from_millis: i64::MIN,
            valid_time_to_millis: TIME_UNBOUNDED,
            system_generation_from: system_generation,
            system_generation_to: u64::MAX,
            chunks,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnInput {
    pub external_id: Vec<u8>,
    pub ordinal: u32,
    pub role: ParticipantRole,
    pub event_time_millis: i64,
    pub reply_to_ordinal: Option<u32>,
    pub actor_entity_id: u64,
    pub model_identity_index: Option<u32>,
    pub content: String,
    pub flags: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationInput {
    pub external_id: Vec<u8>,
    pub started_at_millis: i64,
    pub ended_at_millis: i64,
    pub turns: Vec<TurnInput>,
}

#[derive(Debug)]
pub struct PreparedMixedSource {
    pub authority: GenerationWriteAuthorityV3,
    pub pages: GenerationPagesV3,
}

impl PreparedMixedSource {
    pub fn write(
        self,
        path: impl AsRef<Path>,
    ) -> Result<VerifiedGraphGenerationV3, MemoryContractError> {
        crate::write_generation_new(path, self.authority, self.pages)
    }
}

#[derive(Debug)]
pub struct MixedSourceBuilder {
    namespace_hash: [u8; 32],
    cohort_hash: [u8; 32],
    registry_revision: u64,
    producer_generation: u64,
    published_generation: u64,
    documents: Vec<DocumentInput>,
    conversations: Vec<ConversationInput>,
}

impl MixedSourceBuilder {
    pub fn new(namespace_external_identity: &[u8]) -> Self {
        let namespace_hash = *blake3::hash(namespace_external_identity).as_bytes();
        Self::from_namespace_hash(namespace_hash)
    }

    pub fn from_namespace_hash(namespace_hash: [u8; 32]) -> Self {
        let mut cohort = blake3::Hasher::new();
        cohort.update(b"phoenix.graph-generation/v3/default-cohort\0");
        cohort.update(&namespace_hash);
        Self {
            namespace_hash,
            cohort_hash: *cohort.finalize().as_bytes(),
            registry_revision: 0,
            producer_generation: 0,
            published_generation: 0,
            documents: Vec::new(),
            conversations: Vec::new(),
        }
    }

    pub fn cohort_hash(mut self, cohort_hash: [u8; 32]) -> Self {
        self.cohort_hash = cohort_hash;
        self
    }

    pub fn generations(
        mut self,
        registry_revision: u64,
        producer_generation: u64,
        published_generation: u64,
    ) -> Self {
        self.registry_revision = registry_revision;
        self.producer_generation = producer_generation;
        self.published_generation = published_generation;
        self
    }

    pub fn add_document(mut self, document: DocumentInput) -> Self {
        self.documents.push(document);
        self
    }

    pub fn add_conversation(mut self, conversation: ConversationInput) -> Self {
        self.conversations.push(conversation);
        self
    }

    pub fn prepare(mut self) -> Result<PreparedMixedSource, MemoryContractError> {
        canonicalize_source_inputs(
            self.namespace_hash,
            &mut self.documents,
            &mut self.conversations,
        )?;
        let namespace_id = deterministic_id(b"namespace", &[&self.namespace_hash]);
        let mut pages = GenerationPagesV3::default();
        pages.sources.reserve(
            self.documents
                .len()
                .saturating_add(self.conversations.len()),
        );

        for document in self.documents {
            append_document(self.namespace_hash, namespace_id, document, &mut pages)?;
        }
        for conversation in self.conversations {
            append_conversation(self.namespace_hash, namespace_id, conversation, &mut pages)?;
        }
        pages.canonicalize();

        Ok(PreparedMixedSource {
            authority: GenerationWriteAuthorityV3 {
                namespace_hash: self.namespace_hash,
                cohort_hash: self.cohort_hash,
                registry_revision: self.registry_revision,
                producer_generation: self.producer_generation,
                published_generation: self.published_generation,
            },
            pages,
        })
    }
}

pub fn deterministic_id(domain: &[u8], parts: &[&[u8]]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix/stable-id/v1\0");
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain);
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(bytes).max(1)
}

fn canonicalize_source_inputs(
    namespace_hash: [u8; 32],
    documents: &mut [DocumentInput],
    conversations: &mut [ConversationInput],
) -> Result<(), MemoryContractError> {
    documents.sort_unstable_by_key(|document| {
        source_id(
            namespace_hash,
            SourceKind::WorkspaceDocument,
            &document.external_id,
        )
    });
    conversations.sort_unstable_by_key(|conversation| {
        let source_id = source_id(
            namespace_hash,
            SourceKind::Conversation,
            &conversation.external_id,
        );
        deterministic_id(
            b"conversation",
            &[&namespace_hash, &source_id.to_le_bytes()],
        )
    });
    reject_duplicate_source_ids(documents.iter().map(|document| {
        source_id(
            namespace_hash,
            SourceKind::WorkspaceDocument,
            &document.external_id,
        )
    }))?;
    reject_duplicate_source_ids(conversations.iter().map(|conversation| {
        source_id(
            namespace_hash,
            SourceKind::Conversation,
            &conversation.external_id,
        )
    }))?;
    Ok(())
}

fn reject_duplicate_source_ids(
    ids: impl IntoIterator<Item = u64>,
) -> Result<(), MemoryContractError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(MemoryContractError::DuplicateSourceIdentity);
        }
    }
    Ok(())
}

fn append_document(
    namespace_hash: [u8; 32],
    namespace_id: u64,
    document: DocumentInput,
    pages: &mut GenerationPagesV3,
) -> Result<(), MemoryContractError> {
    let source_id = source_id(
        namespace_hash,
        SourceKind::WorkspaceDocument,
        &document.external_id,
    );
    let document_id = deterministic_id(b"document", &[&namespace_hash, &source_id.to_le_bytes()]);
    let content_hash = *blake3::hash(document.text.as_bytes()).as_bytes();
    let source_len =
        u32::try_from(document.text.len()).map_err(|_| MemoryContractError::SourceTextOversized)?;
    let path = append_ref(&mut pages.strings, document.path.as_bytes())?;
    let content = append_ref(&mut pages.source_text, document.text.as_bytes())?;
    let chunk_start =
        u32::try_from(pages.chunks.len()).map_err(|_| MemoryContractError::CountOverflow)?;
    let chunk_count =
        u32::try_from(document.chunks.len()).map_err(|_| MemoryContractError::CountOverflow)?;
    let token_count = document.chunks.iter().try_fold(0_u32, |total, chunk| {
        total
            .checked_add(chunk.token_count)
            .ok_or(MemoryContractError::CountOverflow)
    })?;

    let document_unit_id = deterministic_id(
        b"content-unit/document",
        &[
            &namespace_hash,
            &source_id.to_le_bytes(),
            &document.revision.to_le_bytes(),
            &content_hash,
        ],
    );
    pages.content_units.push(ContentUnitRecord {
        id: document_unit_id,
        source_id,
        owner_id: document_id,
        parent_id: 0,
        content_hash,
        start: 0,
        end: source_len,
        ordinal: 0,
        token_count,
        kind: ContentUnitKind::Document as u16,
        flags: SOURCE_FLAG_COMPLETE,
        reserved: 0,
    });

    for (ordinal, chunk) in document.chunks.iter().enumerate() {
        if chunk.start > chunk.end || chunk.end > source_len {
            return Err(MemoryContractError::InvalidChunkRange);
        }
        let chunk_text = document
            .text
            .as_bytes()
            .get(chunk.start as usize..chunk.end as usize)
            .ok_or(MemoryContractError::InvalidChunkRange)?;
        let chunk_hash = *blake3::hash(chunk_text).as_bytes();
        let chunk_id = deterministic_id(
            b"content-unit/dynamic-chunk",
            &[
                &namespace_hash,
                &source_id.to_le_bytes(),
                &document.revision.to_le_bytes(),
                &chunk.start.to_le_bytes(),
                &chunk.end.to_le_bytes(),
                &chunk_hash,
            ],
        );
        pages.chunks.push(ChunkRecord {
            id: chunk_id,
            document_id,
            content_hash: hash_prefix_u64(chunk_hash),
            start: chunk.start,
            end: chunk.end,
            sentence_start: chunk.sentence_start,
            sentence_end: chunk.sentence_end,
            paragraph_start: chunk.paragraph_start,
            paragraph_end: chunk.paragraph_end,
            chapter_index: chunk.chapter_index,
            token_count: chunk.token_count,
            flags: chunk.flags,
            reserved: 0,
        });
        pages.content_units.push(ContentUnitRecord {
            id: chunk_id,
            source_id,
            owner_id: document_id,
            parent_id: document_unit_id,
            content_hash: chunk_hash,
            start: chunk.start,
            end: chunk.end,
            ordinal: u32::try_from(ordinal).map_err(|_| MemoryContractError::CountOverflow)?,
            token_count: chunk.token_count,
            kind: ContentUnitKind::DynamicChunk as u16,
            flags: SOURCE_FLAG_COMPLETE,
            reserved: 0,
        });
    }

    pages.sources.push(SourceRecord {
        id: source_id,
        namespace_id,
        external_identity_hash: *blake3::hash(&document.external_id).as_bytes(),
        content_hash,
        kind: SourceKind::WorkspaceDocument as u16,
        flags: SOURCE_FLAG_COMPLETE,
        reserved_u32: 0,
        reserved: [0; 2],
    });
    pages.document_revisions.push(DocumentRevisionRecord {
        source_id,
        document_id,
        revision: document.revision,
        path,
        content,
        content_hash,
        valid_time_from_millis: document.valid_time_from_millis,
        valid_time_to_millis: document.valid_time_to_millis,
        system_generation_from: document.system_generation_from,
        system_generation_to: document.system_generation_to,
        chunk_start,
        chunk_count,
        flags: u32::from(SOURCE_FLAG_COMPLETE),
        reserved: 0,
    });
    Ok(())
}

fn append_conversation(
    namespace_hash: [u8; 32],
    namespace_id: u64,
    mut conversation: ConversationInput,
    pages: &mut GenerationPagesV3,
) -> Result<(), MemoryContractError> {
    let source_id = source_id(
        namespace_hash,
        SourceKind::Conversation,
        &conversation.external_id,
    );
    let conversation_id = deterministic_id(
        b"conversation",
        &[&namespace_hash, &source_id.to_le_bytes()],
    );
    conversation.turns.sort_unstable_by_key(|turn| turn.ordinal);
    let mut ordinal_to_id = HashMap::with_capacity(conversation.turns.len());
    let mut turn_ids = HashSet::with_capacity(conversation.turns.len());
    for turn in &conversation.turns {
        let id = turn_id(namespace_hash, source_id, conversation_id, turn);
        if ordinal_to_id.insert(turn.ordinal, id).is_some() || !turn_ids.insert(id) {
            return Err(MemoryContractError::DuplicateTurnOrdinal);
        }
    }

    let turn_start =
        u32::try_from(pages.turns.len()).map_err(|_| MemoryContractError::CountOverflow)?;
    let turn_count =
        u32::try_from(conversation.turns.len()).map_err(|_| MemoryContractError::CountOverflow)?;
    let mut conversation_hasher = blake3::Hasher::new();
    conversation_hasher.update(b"phoenix/conversation-content/v1\0");
    for turn in &conversation.turns {
        let id = ordinal_to_id[&turn.ordinal];
        let reply_to_turn_id = match turn.reply_to_ordinal {
            Some(reply_ordinal) if reply_ordinal < turn.ordinal => {
                *ordinal_to_id.get(&reply_ordinal).ok_or(
                    MemoryContractError::InvalidSourceModel("reply target ordinal does not exist"),
                )?
            }
            Some(_) => {
                return Err(MemoryContractError::InvalidSourceModel(
                    "reply target must precede the turn",
                ))
            }
            None => 0,
        };
        let source_len = u32::try_from(turn.content.len())
            .map_err(|_| MemoryContractError::SourceTextOversized)?;
        let content_hash = *blake3::hash(turn.content.as_bytes()).as_bytes();
        let content = append_ref(&mut pages.source_text, turn.content.as_bytes())?;
        conversation_hasher.update(&turn.ordinal.to_le_bytes());
        conversation_hasher.update(&(turn.role as u16).to_le_bytes());
        conversation_hasher.update(&turn.event_time_millis.to_le_bytes());
        conversation_hasher.update(&content_hash);

        pages.turns.push(TurnRecord {
            id,
            source_id,
            conversation_id,
            reply_to_turn_id,
            actor_entity_id: turn.actor_entity_id,
            content,
            content_hash,
            event_time_millis: turn.event_time_millis,
            ordinal: turn.ordinal,
            model_identity_index: turn.model_identity_index.unwrap_or(u32::MAX),
            role: turn.role as u16,
            flags: turn.flags | SOURCE_FLAG_COMPLETE,
            reserved: 0,
        });
        pages.content_units.push(ContentUnitRecord {
            id: deterministic_id(
                b"content-unit/turn",
                &[&namespace_hash, &source_id.to_le_bytes(), &id.to_le_bytes()],
            ),
            source_id,
            owner_id: id,
            parent_id: 0,
            content_hash,
            start: 0,
            end: source_len,
            ordinal: turn.ordinal,
            token_count: 0,
            kind: ContentUnitKind::Turn as u16,
            flags: SOURCE_FLAG_COMPLETE,
            reserved: 0,
        });
    }
    let content_hash = *conversation_hasher.finalize().as_bytes();
    pages.sources.push(SourceRecord {
        id: source_id,
        namespace_id,
        external_identity_hash: *blake3::hash(&conversation.external_id).as_bytes(),
        content_hash,
        kind: SourceKind::Conversation as u16,
        flags: SOURCE_FLAG_COMPLETE,
        reserved_u32: 0,
        reserved: [0; 2],
    });
    pages.conversations.push(ConversationRecord {
        source_id,
        conversation_id,
        started_at_millis: conversation.started_at_millis,
        ended_at_millis: conversation.ended_at_millis,
        turn_start,
        turn_count,
        flags: u32::from(SOURCE_FLAG_COMPLETE),
        reserved: 0,
        content_hash,
    });
    Ok(())
}

fn source_id(namespace_hash: [u8; 32], kind: SourceKind, external_id: &[u8]) -> u64 {
    deterministic_id(
        match kind {
            SourceKind::WorkspaceDocument => b"source/document",
            SourceKind::Conversation => b"source/conversation",
        },
        &[&namespace_hash, external_id],
    )
}

fn turn_id(
    namespace_hash: [u8; 32],
    source_id: u64,
    conversation_id: u64,
    turn: &TurnInput,
) -> u64 {
    deterministic_id(
        b"turn",
        &[
            &namespace_hash,
            &source_id.to_le_bytes(),
            &conversation_id.to_le_bytes(),
            &turn.external_id,
        ],
    )
}

fn append_ref(slab: &mut Vec<u8>, bytes: &[u8]) -> Result<StringRef, MemoryContractError> {
    let offset = slab.len() as u64;
    let length =
        u32::try_from(bytes.len()).map_err(|_| MemoryContractError::SourceTextOversized)?;
    slab.extend_from_slice(bytes);
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}

fn hash_prefix_u64(hash: [u8; 32]) -> u64 {
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(bytes)
}
