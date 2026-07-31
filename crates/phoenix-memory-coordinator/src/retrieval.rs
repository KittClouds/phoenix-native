use crate::{
    CommonProducts, ContextCandidateItem, ContextEvidenceExcerpt, ContextItem, ContextPacket,
    CoordinatorError, LexicalRecallConfig, LexicalRecallReceipt, LexicalRecallStatus, MemoryScope,
    MemorySourceLocator, RecallTurn, StoredConversation, StoredDocument,
};
use hashbrown::{HashMap, HashSet};
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchHit, SearchScratch,
};
use phoenix_memory_contract::{deterministic_id, SourceId, SourceKind};
use std::sync::Arc;

const CONTENT_FIELD: FieldConfig = FieldConfig::new("content", 1.0, 0.75, 0.0);

#[derive(Clone, Debug)]
enum CorpusLocation {
    Document {
        entry_id: u64,
        chunk_index: u32,
    },
    Turn {
        conversation_key: Arc<[u8]>,
        turn_index: u32,
    },
}

#[derive(Clone, Debug)]
struct CorpusRow {
    source_id: SourceId,
    content_id: u64,
    location: CorpusLocation,
}

pub(crate) struct LexicalRecallIndex {
    config: LexicalRecallConfig,
    index: Option<QpsIndex>,
    rows: Box<[CorpusRow]>,
    corpus_hash: [u8; 32],
    scratch: SearchScratch,
    hits: Vec<SearchHit>,
}

impl LexicalRecallIndex {
    pub fn empty(config: LexicalRecallConfig) -> Self {
        Self {
            config,
            index: None,
            rows: Box::new([]),
            corpus_hash: [0; 32],
            scratch: SearchScratch::with_document_capacity(0, 32),
            hits: Vec::with_capacity(config.top_k),
        }
    }

    pub fn build(
        config: LexicalRecallConfig,
        namespace_hash: [u8; 32],
        documents: &HashMap<u64, StoredDocument>,
        conversations: &HashMap<Vec<u8>, StoredConversation>,
    ) -> Result<Self, CoordinatorError> {
        let item_count = documents
            .values()
            .map(|document| document.production.structural.chunks.len())
            .sum::<usize>()
            .saturating_add(
                conversations
                    .values()
                    .map(|conversation| conversation.turns.len())
                    .sum::<usize>(),
            );
        if item_count > config.maximum_items {
            return Err(CoordinatorError::Oversized);
        }
        if item_count == 0 {
            return Ok(Self::empty(config));
        }

        let qps_config = QpsConfig {
            maximum_candidate_pool: config.maximum_candidate_pool,
            ..QpsConfig::default()
        };
        let mut builder =
            QpsBuilder::new(Vec::from([CONTENT_FIELD]).into_boxed_slice(), qps_config)
                .map_err(lexical_error)?;
        let mut rows = Vec::with_capacity(item_count);
        let mut corpus_hasher = blake3::Hasher::new();

        let mut document_keys = documents.keys().copied().collect::<Vec<_>>();
        document_keys.sort_unstable();
        for entry_id in document_keys {
            let stored = documents
                .get(&entry_id)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "document recall row disappeared",
                ))?;
            let source_id = source_id(
                namespace_hash,
                SourceKind::WorkspaceDocument,
                &entry_id.to_le_bytes(),
            );
            for (chunk_index, chunk) in stored.production.structural.chunks.iter().enumerate() {
                let text = stored
                    .request
                    .lease
                    .content
                    .get(chunk.start as usize..chunk.end as usize)
                    .ok_or(CoordinatorError::ProducerAuthority(
                        "document recall chunk is not valid UTF-8",
                    ))?;
                let content_id =
                    structural_id(b"chunk", source_id.0, chunk_index, chunk.content_hash);
                insert_row(
                    &mut builder,
                    &mut rows,
                    &mut corpus_hasher,
                    source_id,
                    content_id,
                    text,
                    CorpusLocation::Document {
                        entry_id,
                        chunk_index: u32::try_from(chunk_index)
                            .map_err(|_| CoordinatorError::Oversized)?,
                    },
                )?;
            }
        }

        let mut conversation_keys = conversations.keys().cloned().collect::<Vec<_>>();
        conversation_keys.sort_unstable();
        for key in conversation_keys {
            let conversation =
                conversations
                    .get(&key)
                    .ok_or(CoordinatorError::ProducerAuthority(
                        "conversation recall row disappeared",
                    ))?;
            let source_id = source_id(namespace_hash, SourceKind::Conversation, &key);
            let conversation_id = deterministic_id(
                b"conversation",
                &[&namespace_hash, &source_id.0.to_le_bytes()],
            );
            for (turn_index, stored) in conversation.turns.iter().enumerate() {
                let content_id = deterministic_id(
                    b"turn",
                    &[
                        &namespace_hash,
                        &source_id.0.to_le_bytes(),
                        &conversation_id.to_le_bytes(),
                        &stored.turn.external_id,
                    ],
                );
                insert_row(
                    &mut builder,
                    &mut rows,
                    &mut corpus_hasher,
                    source_id,
                    content_id,
                    &stored.turn.content,
                    CorpusLocation::Turn {
                        conversation_key: conversation.external_id.clone(),
                        turn_index: u32::try_from(turn_index)
                            .map_err(|_| CoordinatorError::Oversized)?,
                    },
                )?;
            }
        }

        let index = builder.build().map_err(lexical_error)?;
        Ok(Self {
            config,
            scratch: SearchScratch::with_document_capacity(index.stats().documents, 32),
            hits: Vec::with_capacity(config.top_k),
            index: Some(index),
            rows: rows.into_boxed_slice(),
            corpus_hash: *corpus_hasher.finalize().as_bytes(),
        })
    }

    pub fn recall(
        &mut self,
        request: &RecallTurn,
        documents: &HashMap<u64, StoredDocument>,
        conversations: &HashMap<Vec<u8>, StoredConversation>,
        generation_hash: Option<[u8; 32]>,
        max_items: usize,
        max_bytes: usize,
    ) -> Result<ContextPacket, CoordinatorError> {
        let query_hash = *blake3::hash(request.pending_turn.content.as_bytes()).as_bytes();
        let scope_hash = request.scope.fingerprint();
        let conversation_hash = *blake3::hash(&request.conversation.external_id).as_bytes();
        let committed_history_count = conversations
            .values()
            .map(|conversation| conversation.turns.len())
            .sum::<usize>();
        let Some(index) = self.index.as_ref() else {
            return Ok(ContextPacket {
                conversation_hash,
                pending_turn_hash: query_hash,
                scope_hash,
                resident_generation_hash: generation_hash,
                committed_history_count: saturating_u32(committed_history_count),
                returned_bytes: 0,
                items: Arc::from([]),
                proposed_candidates: Arc::from([]),
                lexical: LexicalRecallReceipt {
                    status: LexicalRecallStatus::EmptyCorpus,
                    query_hash,
                    generation_hash,
                    ..LexicalRecallReceipt::default()
                },
                qps_shadow: Default::default(),
            });
        };

        let top_k = self.config.top_k.min(max_items);
        let search_limit = if request.scope == MemoryScope::Workspace {
            top_k
        } else {
            self.config.maximum_candidate_pool.min(self.rows.len())
        };
        let search = index
            .search_into(
                &request.pending_turn.content,
                search_limit,
                &mut self.scratch,
                &mut self.hits,
            )
            .map_err(lexical_error)?;
        let mut returned_bytes = 0_usize;
        let mut items = Vec::with_capacity(self.hits.len());
        let mut selected_rows = Vec::with_capacity(self.hits.len());
        for hit in &self.hits {
            let row_index =
                usize::try_from(hit.external_id).map_err(|_| CoordinatorError::Oversized)?;
            let row = self.rows.get(row_index).ok_or_else(|| {
                CoordinatorError::LexicalRecall("search result is outside the corpus".into())
            })?;
            if !row_in_scope(row, &request.scope) {
                continue;
            }
            let item = materialize_item(row, hit, documents, conversations)?;
            let next_bytes = returned_bytes.saturating_add(item.content.len());
            if next_bytes > max_bytes {
                continue;
            }
            returned_bytes = next_bytes;
            selected_rows.push(row_index);
            items.push(item);
            if items.len() == top_k {
                break;
            }
        }

        let mut candidates = Vec::new();
        let mut seen_candidates = HashSet::new();
        for row_index in selected_rows {
            let row = &self.rows[row_index];
            append_row_candidates(
                row,
                documents,
                conversations,
                max_items,
                max_bytes,
                &mut returned_bytes,
                &mut seen_candidates,
                &mut candidates,
            )?;
        }
        items.sort_unstable_by(|left, right| {
            left.source_id
                .0
                .cmp(&right.source_id.0)
                .then_with(|| left.ordinal.cmp(&right.ordinal))
        });
        let receipt = LexicalRecallReceipt {
            path_id: Default::default(),
            status: LexicalRecallStatus::Ready,
            query_hash,
            corpus_hash: self.corpus_hash,
            generation_hash,
            indexed_items: saturating_u32(self.rows.len()),
            returned_items: saturating_u16(items.len()),
            returned_candidates: saturating_u16(candidates.len()),
            returned_bytes: saturating_u32(returned_bytes),
            search,
        };
        Ok(ContextPacket {
            conversation_hash,
            pending_turn_hash: query_hash,
            scope_hash,
            resident_generation_hash: generation_hash,
            committed_history_count: saturating_u32(committed_history_count),
            returned_bytes: receipt.returned_bytes,
            items: items.into(),
            proposed_candidates: candidates.into(),
            lexical: receipt,
            qps_shadow: Default::default(),
        })
    }
}

fn row_in_scope(row: &CorpusRow, scope: &MemoryScope) -> bool {
    match (scope, &row.location) {
        (MemoryScope::Workspace, _) => true,
        (MemoryScope::Document(expected), CorpusLocation::Document { entry_id, .. }) => {
            expected == entry_id
        }
        (
            MemoryScope::Conversation(expected),
            CorpusLocation::Turn {
                conversation_key, ..
            },
        ) => expected.as_ref() == conversation_key.as_ref(),
        (MemoryScope::DocumentSet(expected), CorpusLocation::Document { entry_id, .. }) => {
            expected.binary_search(entry_id).is_ok()
        }
        (MemoryScope::Compare { documents, .. }, CorpusLocation::Document { entry_id, .. }) => {
            documents.binary_search(entry_id).is_ok()
        }
        (
            MemoryScope::Compare { conversations, .. },
            CorpusLocation::Turn {
                conversation_key, ..
            },
        ) => conversations
            .binary_search_by(|candidate| candidate.as_ref().cmp(conversation_key.as_ref()))
            .is_ok(),
        _ => false,
    }
}

fn insert_row(
    builder: &mut QpsBuilder,
    rows: &mut Vec<CorpusRow>,
    hasher: &mut blake3::Hasher,
    source_id: SourceId,
    content_id: u64,
    text: &str,
    location: CorpusLocation,
) -> Result<(), CoordinatorError> {
    let external_id = u64::try_from(rows.len()).map_err(|_| CoordinatorError::Oversized)?;
    let fields = [text];
    builder
        .insert(DocumentInput {
            external_id,
            fields: &fields,
        })
        .map_err(lexical_error)?;
    let content_hash = blake3::hash(text.as_bytes());
    hasher.update(&source_id.0.to_le_bytes());
    hasher.update(&content_id.to_le_bytes());
    hasher.update(content_hash.as_bytes());
    rows.push(CorpusRow {
        source_id,
        content_id,
        location,
    });
    Ok(())
}

fn materialize_item(
    row: &CorpusRow,
    hit: &SearchHit,
    documents: &HashMap<u64, StoredDocument>,
    conversations: &HashMap<Vec<u8>, StoredConversation>,
) -> Result<ContextItem, CoordinatorError> {
    match &row.location {
        CorpusLocation::Document {
            entry_id,
            chunk_index,
        } => {
            let stored = documents
                .get(entry_id)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "retrieved document no longer exists",
                ))?;
            let chunk = stored
                .production
                .structural
                .chunks
                .get(*chunk_index as usize)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "retrieved document chunk no longer exists",
                ))?;
            let text = stored
                .request
                .lease
                .content
                .get(chunk.start as usize..chunk.end as usize)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "retrieved document chunk is invalid UTF-8",
                ))?;
            Ok(ContextItem {
                source_id: row.source_id,
                source_kind: SourceKind::WorkspaceDocument,
                locator: MemorySourceLocator::Document {
                    entry_id: *entry_id,
                    revision: stored.request.revision,
                },
                content_id: row.content_id,
                ordinal: *chunk_index,
                role: None,
                event_time_millis: None,
                source_start: chunk.start,
                source_end: chunk.end,
                content_hash: *blake3::hash(text.as_bytes()).as_bytes(),
                content: Arc::from(text),
                score_micros: score_micros(hit.score),
            })
        }
        CorpusLocation::Turn {
            conversation_key,
            turn_index,
        } => {
            let stored = conversations
                .get(conversation_key.as_ref())
                .and_then(|conversation| conversation.turns.get(*turn_index as usize))
                .ok_or(CoordinatorError::ProducerAuthority(
                    "retrieved conversation turn no longer exists",
                ))?;
            let source_end = u32::try_from(stored.turn.content.len())
                .map_err(|_| CoordinatorError::Oversized)?;
            Ok(ContextItem {
                source_id: row.source_id,
                source_kind: SourceKind::Conversation,
                locator: MemorySourceLocator::ConversationTurn {
                    conversation_external_id: conversation_key.clone(),
                    turn_ordinal: stored.turn.ordinal,
                },
                content_id: row.content_id,
                ordinal: stored.turn.ordinal,
                role: Some(stored.turn.role),
                event_time_millis: Some(stored.turn.event_time_millis),
                source_start: 0,
                source_end,
                content_hash: *blake3::hash(stored.turn.content.as_bytes()).as_bytes(),
                content: stored.turn.content.clone(),
                score_micros: score_micros(hit.score),
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn append_row_candidates(
    row: &CorpusRow,
    documents: &HashMap<u64, StoredDocument>,
    conversations: &HashMap<Vec<u8>, StoredConversation>,
    max_items: usize,
    max_bytes: usize,
    returned_bytes: &mut usize,
    seen: &mut HashSet<[u8; 32]>,
    output: &mut Vec<ContextCandidateItem>,
) -> Result<(), CoordinatorError> {
    let (products, source, selected_range) = match &row.location {
        CorpusLocation::Document {
            entry_id,
            chunk_index,
        } => {
            let stored = documents
                .get(entry_id)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "candidate document no longer exists",
                ))?;
            let chunk = stored
                .production
                .structural
                .chunks
                .get(*chunk_index as usize)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "candidate document chunk no longer exists",
                ))?;
            (
                &stored.production.common,
                stored.request.lease.content.as_ref(),
                (chunk.start, chunk.end),
            )
        }
        CorpusLocation::Turn {
            conversation_key,
            turn_index,
        } => {
            let stored = conversations
                .get(conversation_key.as_ref())
                .and_then(|conversation| conversation.turns.get(*turn_index as usize))
                .ok_or(CoordinatorError::ProducerAuthority(
                    "candidate turn no longer exists",
                ))?;
            (
                &stored.production.common,
                stored.turn.content.as_ref(),
                (0, stored.turn.content.len() as u32),
            )
        }
    };
    append_candidates(
        row.source_id,
        products,
        source,
        selected_range,
        max_items,
        max_bytes,
        returned_bytes,
        seen,
        output,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_candidates(
    source_id: SourceId,
    products: &CommonProducts,
    source: &str,
    selected_range: (u32, u32),
    max_items: usize,
    max_bytes: usize,
    returned_bytes: &mut usize,
    seen: &mut HashSet<[u8; 32]>,
    output: &mut Vec<ContextCandidateItem>,
) -> Result<(), CoordinatorError> {
    for candidate in &products.candidates {
        if output.len() == max_items || seen.contains(&candidate.candidate_id) {
            continue;
        }
        let mentions = candidate
            .evidence_ids
            .iter()
            .filter_map(|evidence_id| {
                products
                    .mentions
                    .iter()
                    .find(|mention| mention.evidence_id == *evidence_id)
            })
            .collect::<Vec<_>>();
        if mentions.len() != candidate.evidence_ids.len()
            || !mentions
                .iter()
                .any(|mention| mention.start < selected_range.1 && mention.end > selected_range.0)
        {
            continue;
        }
        let mut candidate_bytes = candidate
            .relation_kind
            .len()
            .saturating_add(candidate.value.len());
        let mut evidence = Vec::with_capacity(mentions.len());
        for mention in mentions {
            let excerpt = source
                .get(mention.start as usize..mention.end as usize)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "candidate evidence is invalid UTF-8",
                ))?;
            candidate_bytes = candidate_bytes.saturating_add(excerpt.len());
            evidence.push(ContextEvidenceExcerpt {
                evidence_id: mention.evidence_id,
                source_id,
                start: mention.start,
                end: mention.end,
                content: Arc::from(excerpt),
            });
        }
        if returned_bytes.saturating_add(candidate_bytes) > max_bytes {
            continue;
        }
        *returned_bytes = returned_bytes.saturating_add(candidate_bytes);
        seen.insert(candidate.candidate_id);
        output.push(ContextCandidateItem {
            candidate_id: candidate.candidate_id,
            vocabulary_pack_id: candidate.vocabulary_pack_id,
            relation_kind: candidate.relation_kind.clone(),
            value: candidate.value.clone(),
            endpoint_ids: candidate
                .endpoints
                .iter()
                .map(|endpoint| endpoint.endpoint_id)
                .collect::<Vec<_>>()
                .into(),
            evidence: evidence.into(),
            producer_identity_hash: candidate.producer_identity_hash,
            status: candidate.status,
            score: 0,
        });
    }
    Ok(())
}

fn source_id(namespace_hash: [u8; 32], kind: SourceKind, external_id: &[u8]) -> SourceId {
    SourceId(deterministic_id(
        match kind {
            SourceKind::WorkspaceDocument => b"source/document",
            SourceKind::Conversation => b"source/conversation",
        },
        &[&namespace_hash, external_id],
    ))
}

fn structural_id(domain: &[u8], source_id: u64, ordinal: usize, content_hash: u64) -> u64 {
    deterministic_id(
        domain,
        &[
            &source_id.to_le_bytes(),
            &(ordinal as u64).to_le_bytes(),
            &content_hash.to_le_bytes(),
        ],
    )
}

fn score_micros(score: f32) -> u32 {
    if !score.is_finite() || score <= 0.0 {
        return 0;
    }
    (score * 1_000_000.0).min(u32::MAX as f32) as u32
}

fn lexical_error(error: impl std::fmt::Display) -> CoordinatorError {
    CoordinatorError::LexicalRecall(error.to_string())
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
