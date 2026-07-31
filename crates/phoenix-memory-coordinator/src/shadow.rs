use crate::StoredConversation;
use hashbrown::HashMap;
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchHit, SearchReceipt,
    SearchScratch,
};
use std::sync::Arc;
use std::time::Instant;

pub const QPS_V2_01_SHADOW_PATH: &str = "qps/v2.01/shadow";
const TURN_FIELD: FieldConfig = FieldConfig::new("turn", 1.0, 0.75, 0.0);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QpsShadowPathId {
    #[default]
    V2_01Shadow,
}

impl QpsShadowPathId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V2_01Shadow => QPS_V2_01_SHADOW_PATH,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QpsShadowStatus {
    #[default]
    Disabled,
    EmptyCorpus,
    Ready,
    BuildFailed,
    QueryRejected,
    OversizedCorpus,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QpsShadowQueryShape {
    #[default]
    Empty,
    SingleToken,
    MultiToken,
    PhraseLike,
    HighFrequencyDense,
    NoResult,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QpsShadowConfig {
    pub enabled: bool,
    pub top_k: usize,
    pub maximum_documents: usize,
    pub maximum_candidate_pool: usize,
}

impl Default for QpsShadowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            top_k: 24,
            maximum_documents: 100_000,
            maximum_candidate_pool: 160,
        }
    }
}

impl QpsShadowConfig {
    pub fn v2_01_shadow() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QpsShadowReceipt {
    pub path_id: QpsShadowPathId,
    pub status: QpsShadowStatus,
    pub authority_unchanged: bool,
    pub query_hash: [u8; 32],
    pub corpus_hash: [u8; 32],
    pub query_shape: QpsShadowQueryShape,
    pub documents: u32,
    pub authority_items: u16,
    pub shadow_items: u16,
    pub exact_top_overlap: u16,
    pub index_build_nanos: u64,
    pub search: SearchReceipt,
    pub shadow_ordinals: Arc<[u32]>,
}

impl Default for QpsShadowReceipt {
    fn default() -> Self {
        Self {
            path_id: QpsShadowPathId::V2_01Shadow,
            status: QpsShadowStatus::Disabled,
            authority_unchanged: true,
            query_hash: [0; 32],
            corpus_hash: [0; 32],
            query_shape: QpsShadowQueryShape::Empty,
            documents: 0,
            authority_items: 0,
            shadow_items: 0,
            exact_top_overlap: 0,
            index_build_nanos: 0,
            search: SearchReceipt::default(),
            shadow_ordinals: Arc::from([]),
        }
    }
}

pub(crate) struct QpsShadowState {
    config: QpsShadowConfig,
    indexes: HashMap<Vec<u8>, ShadowConversationIndex>,
    failed: HashMap<Vec<u8>, QpsShadowStatus>,
}

struct ShadowConversationIndex {
    index: QpsIndex,
    scratch: SearchScratch,
    hits: Vec<SearchHit>,
    corpus_hash: [u8; 32],
    build_nanos: u64,
}

impl QpsShadowState {
    pub fn new(config: QpsShadowConfig) -> Self {
        Self {
            config,
            indexes: HashMap::new(),
            failed: HashMap::new(),
        }
    }

    pub fn rebuild(&mut self, key: &[u8], conversation: &StoredConversation) {
        if !self.config.enabled {
            return;
        }
        if conversation.turns.len() > self.config.maximum_documents {
            self.indexes.remove(key);
            self.failed
                .insert(key.to_vec(), QpsShadowStatus::OversizedCorpus);
            return;
        }
        let started = Instant::now();
        match build_index(conversation, self.config.maximum_candidate_pool) {
            Ok((index, corpus_hash)) => {
                let documents = index.stats().documents;
                self.indexes.insert(
                    key.to_vec(),
                    ShadowConversationIndex {
                        index,
                        scratch: SearchScratch::with_document_capacity(documents, 32),
                        hits: Vec::with_capacity(self.config.top_k),
                        corpus_hash,
                        build_nanos: elapsed_nanos(started),
                    },
                );
                self.failed.remove(key);
            }
            Err(()) => {
                self.indexes.remove(key);
                self.failed
                    .insert(key.to_vec(), QpsShadowStatus::BuildFailed);
            }
        }
    }

    pub fn evaluate(
        &mut self,
        key: &[u8],
        query: &str,
        authority_ordinals: &[u32],
    ) -> QpsShadowReceipt {
        let query_hash = *blake3::hash(query.as_bytes()).as_bytes();
        if !self.config.enabled {
            return QpsShadowReceipt {
                query_hash,
                ..Default::default()
            };
        }
        let Some(indexed) = self.indexes.get_mut(key) else {
            return QpsShadowReceipt {
                status: self
                    .failed
                    .get(key)
                    .copied()
                    .unwrap_or(QpsShadowStatus::EmptyCorpus),
                query_hash,
                authority_items: saturating_u16(authority_ordinals.len()),
                ..Default::default()
            };
        };
        let result = indexed.index.search_into(
            query,
            self.config.top_k,
            &mut indexed.scratch,
            &mut indexed.hits,
        );
        let Ok(search) = result else {
            return QpsShadowReceipt {
                status: QpsShadowStatus::QueryRejected,
                query_hash,
                corpus_hash: indexed.corpus_hash,
                documents: saturating_u32(indexed.index.stats().documents),
                authority_items: saturating_u16(authority_ordinals.len()),
                index_build_nanos: indexed.build_nanos,
                ..Default::default()
            };
        };
        let shadow_ordinals = indexed
            .hits
            .iter()
            .map(|hit| hit.external_id as u32)
            .collect::<Vec<_>>();
        let overlap = shadow_ordinals
            .iter()
            .filter(|ordinal| authority_ordinals.contains(ordinal))
            .count();
        QpsShadowReceipt {
            path_id: QpsShadowPathId::V2_01Shadow,
            status: QpsShadowStatus::Ready,
            authority_unchanged: true,
            query_hash,
            corpus_hash: indexed.corpus_hash,
            query_shape: classify_query(query, search),
            documents: saturating_u32(indexed.index.stats().documents),
            authority_items: saturating_u16(authority_ordinals.len()),
            shadow_items: saturating_u16(shadow_ordinals.len()),
            exact_top_overlap: saturating_u16(overlap),
            index_build_nanos: indexed.build_nanos,
            search,
            shadow_ordinals: shadow_ordinals.into(),
        }
    }
}

fn build_index(
    conversation: &StoredConversation,
    maximum_candidate_pool: usize,
) -> Result<(QpsIndex, [u8; 32]), ()> {
    let config = QpsConfig {
        maximum_candidate_pool,
        ..QpsConfig::default()
    };
    let mut builder =
        QpsBuilder::new(Vec::from([TURN_FIELD]).into_boxed_slice(), config).map_err(|_| ())?;
    let mut hasher = blake3::Hasher::new();
    for stored in &conversation.turns {
        hasher.update(&stored.turn.ordinal.to_le_bytes());
        hasher.update(stored.turn.content.as_bytes());
        let fields = [stored.turn.content.as_ref()];
        builder
            .insert(DocumentInput {
                external_id: u64::from(stored.turn.ordinal),
                fields: &fields,
            })
            .map_err(|_| ())?;
    }
    let corpus_hash = *hasher.finalize().as_bytes();
    builder
        .build()
        .map(|index| (index, corpus_hash))
        .map_err(|_| ())
}

fn classify_query(query: &str, search: SearchReceipt) -> QpsShadowQueryShape {
    if search.candidates == 0 {
        return QpsShadowQueryShape::NoResult;
    }
    if matches!(
        search.selection,
        phoenix_lexical_qps::CandidateSelection::DenseSimd
    ) {
        return QpsShadowQueryShape::HighFrequencyDense;
    }
    let tokens = query.split_ascii_whitespace().take(3).count();
    match tokens {
        0 => QpsShadowQueryShape::Empty,
        1 => QpsShadowQueryShape::SingleToken,
        _ if query.contains('"') => QpsShadowQueryShape::PhraseLike,
        _ => QpsShadowQueryShape::MultiToken,
    }
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
