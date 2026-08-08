use phoenix_lexical_qps::SearchReceipt;
use phoenix_memory_contract::{
    CandidateStatus, ParticipantRole, SourceId, SourceKind, TemporalPrecisionV1,
};
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision};
use std::sync::Arc;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum MemoryScope {
    #[default]
    Workspace,
    Document(u64),
    Conversation(Arc<[u8]>),
    DocumentSet(Arc<[u64]>),
    Compare {
        documents: Arc<[u64]>,
        conversations: Arc<[Arc<[u8]>]>,
    },
}

impl MemoryScope {
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        match self {
            Self::Workspace => {
                hasher.update(b"workspace");
            }
            Self::Document(entry_id) => {
                hasher.update(b"document");
                hasher.update(&entry_id.to_le_bytes());
            }
            Self::Conversation(external_id) => {
                hasher.update(b"conversation");
                hasher.update(external_id);
            }
            Self::DocumentSet(documents) => {
                hasher.update(b"document-set");
                for entry_id in documents.iter() {
                    hasher.update(&entry_id.to_le_bytes());
                }
            }
            Self::Compare {
                documents,
                conversations,
            } => {
                hasher.update(b"compare");
                for entry_id in documents.iter() {
                    hasher.update(&entry_id.to_le_bytes());
                }
                for external_id in conversations.iter() {
                    hasher.update(&(external_id.len() as u64).to_le_bytes());
                    hasher.update(external_id);
                }
            }
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestionOrigin {
    Workspace,
    ExternalConversation,
    LongMemEvalHistory,
    LongMemEvalQuery,
    LongMemEvalGoldAnswer,
    LongMemEvalGoldSession,
}

impl IngestionOrigin {
    pub(crate) const fn is_forbidden_gold(self) -> bool {
        matches!(
            self,
            Self::LongMemEvalGoldAnswer | Self::LongMemEvalGoldSession
        )
    }
}

#[derive(Clone, Debug)]
pub struct IngestDocumentRevision {
    pub lease: Arc<DocumentLease>,
    pub revision: DocumentRevision,
    pub hash: ContentHash,
    pub origin: IngestionOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationKey {
    pub external_id: Arc<[u8]>,
    pub started_at_millis: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingTurn {
    pub external_id: Arc<[u8]>,
    pub ordinal: u32,
    pub role: ParticipantRole,
    pub event_time_millis: i64,
    pub reply_to_ordinal: Option<u32>,
    pub content: Arc<str>,
    pub origin: IngestionOrigin,
}

#[derive(Clone, Debug)]
pub struct RecallTurn {
    pub conversation: ConversationKey,
    pub pending_turn: PendingTurn,
    pub scope: MemoryScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedTurn {
    pub external_id: Arc<[u8]>,
    pub ordinal: u32,
    pub role: ParticipantRole,
    pub event_time_millis: i64,
    pub reply_to_ordinal: Option<u32>,
    pub actor_entity_id: u64,
    pub model_identity_index: Option<u32>,
    pub content: Arc<str>,
    pub origin: IngestionOrigin,
}

#[derive(Clone, Debug)]
pub struct IngestTurn {
    pub conversation: ConversationKey,
    pub committed_turn: CommittedTurn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextItem {
    pub source_id: SourceId,
    pub source_kind: SourceKind,
    /// Typed product navigation. Consumers must never recover source identity
    /// from labels, excerpts, or positional coincidence.
    pub locator: MemorySourceLocator,
    /// Exact chunk ID for documents or exact turn ID for conversations.
    pub content_id: u64,
    pub ordinal: u32,
    pub role: Option<ParticipantRole>,
    pub event_time_millis: Option<i64>,
    pub source_start: u32,
    pub source_end: u32,
    pub content_hash: [u8; 32],
    pub content: Arc<str>,
    /// Stable fixed-point form of the lexical score (score * 1_000_000).
    pub score_micros: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemorySourceLocator {
    Document {
        entry_id: u64,
        revision: DocumentRevision,
    },
    ConversationTurn {
        conversation_external_id: Arc<[u8]>,
        turn_ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextEvidenceExcerpt {
    pub evidence_id: u64,
    pub source_id: SourceId,
    pub start: u32,
    pub end: u32,
    pub content: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextCandidateItem {
    pub candidate_id: [u8; 32],
    pub vocabulary_pack_id: u64,
    pub relation_kind: Arc<str>,
    pub value: Arc<str>,
    pub endpoint_ids: Arc<[u64]>,
    pub evidence: Arc<[ContextEvidenceExcerpt]>,
    pub producer_identity_hash: [u8; 32],
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub temporal_envelopes: Arc<[ContextTemporalEnvelopeV1]>,
    pub status: CandidateStatus,
    pub score: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextTemporalEnvelopeV1 {
    pub id: [u8; 32],
    pub source_time_millis: i64,
    pub asserted_at_millis: i64,
    pub occurred_from_millis: i64,
    pub occurred_to_millis: i64,
    pub observed_at_millis: i64,
    pub valid_time_from_millis: i64,
    pub valid_time_to_millis: i64,
    pub original_text: Arc<str>,
    pub evidence_ids: Arc<[u64]>,
    pub timezone_offset_minutes: i32,
    pub confidence_bits: u32,
    pub precision: TemporalPrecisionV1,
    pub flags: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPacket {
    pub conversation_hash: [u8; 32],
    pub pending_turn_hash: [u8; 32],
    pub scope_hash: [u8; 32],
    pub resident_generation_hash: Option<[u8; 32]>,
    pub committed_history_count: u32,
    pub returned_bytes: u32,
    pub items: Arc<[ContextItem]>,
    pub proposed_candidates: Arc<[ContextCandidateItem]>,
    /// Authoritative bounded lexical retrieval receipt.
    pub lexical: LexicalRecallReceipt,
    /// Optional non-authoritative comparison evidence. This never changes
    /// `items` and is disabled by default.
    pub qps_shadow: QpsShadowReceipt,
}

use crate::QpsShadowReceipt;

pub const LEXICAL_RECALL_PATH: &str = "phoenix.lexical.positional/v1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LexicalRecallPathId {
    #[default]
    PositionalV1,
}

impl LexicalRecallPathId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PositionalV1 => LEXICAL_RECALL_PATH,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LexicalRecallStatus {
    #[default]
    EmptyCorpus,
    Ready,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LexicalRecallReceipt {
    pub path_id: LexicalRecallPathId,
    pub status: LexicalRecallStatus,
    pub query_hash: [u8; 32],
    pub corpus_hash: [u8; 32],
    pub generation_hash: Option<[u8; 32]>,
    pub indexed_items: u32,
    pub returned_items: u16,
    pub returned_candidates: u16,
    pub returned_bytes: u32,
    pub search: SearchReceipt,
}
