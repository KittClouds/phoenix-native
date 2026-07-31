use crate::{
    assemble::prepare_generation,
    coordinator::validate_registrations,
    product::{candidate_product, role_is_source, validate_common_products},
    retrieval::LexicalRecallIndex,
    CancellationProbe, CommittedTurn, ContextPacket, CoordinatorConfig, CoordinatorError,
    DocumentProduction, DualFaceProducer, GenerationPublication, IngestDocumentRevision,
    IngestTurn, RecallTurn, TurnProduction,
};
use hashbrown::HashMap;
use phoenix_memory_contract::VerifiedGraphGenerationV3;
use std::sync::Arc;

pub(crate) struct StoredDocument {
    pub request: IngestDocumentRevision,
    pub production: DocumentProduction,
}

pub(crate) struct StoredTurn {
    pub turn: CommittedTurn,
    pub production: TurnProduction,
}

pub(crate) struct StoredConversation {
    pub external_id: Arc<[u8]>,
    pub started_at_millis: i64,
    pub turns: Vec<StoredTurn>,
}

pub(crate) struct CoordinatorState<P> {
    pub config: CoordinatorConfig,
    producer: Arc<P>,
    pub namespace_hash: [u8; 32],
    pub documents: HashMap<u64, StoredDocument>,
    pub conversations: HashMap<Vec<u8>, StoredConversation>,
    pub published_generation: u64,
    pub current: Option<GenerationPublication>,
    lexical_recall: LexicalRecallIndex,
    qps_shadow: crate::shadow::QpsShadowState,
}

impl<P: DualFaceProducer> CoordinatorState<P> {
    pub fn new(config: CoordinatorConfig, producer: Arc<P>) -> Result<Self, CoordinatorError> {
        validate_registrations(&config)?;
        let namespace_hash = *blake3::hash(&config.namespace_external_identity).as_bytes();
        let lexical_recall = LexicalRecallIndex::empty(config.lexical_recall);
        let qps_shadow = crate::shadow::QpsShadowState::new(config.qps_shadow);
        Ok(Self {
            config,
            producer,
            namespace_hash,
            documents: HashMap::new(),
            conversations: HashMap::new(),
            published_generation: 0,
            current: None,
            lexical_recall,
            qps_shadow,
        })
    }

    pub fn ingest_document(
        &mut self,
        request: IngestDocumentRevision,
        cancellation: &CancellationProbe,
    ) -> Result<GenerationPublication, CoordinatorError> {
        validate_document_command(&request)?;
        cancellation.check()?;
        if let Some(stored) = self.documents.get(&request.lease.entry_id.0) {
            if same_document(&stored.request, &request) {
                return self
                    .current
                    .clone()
                    .ok_or(CoordinatorError::ProducerAuthority(
                        "idempotent document has no published generation",
                    ));
            }
        }
        let production = self.producer.analyze_document(&request, cancellation)?;
        cancellation.check()?;
        validate_document_production(&request, &production)?;
        validate_candidate_capabilities(&self.config, &production.common)?;
        let entry_id = request.lease.entry_id.0;

        let previous = self.documents.insert(
            entry_id,
            StoredDocument {
                request,
                production,
            },
        );
        let prepared_recall = match LexicalRecallIndex::build(
            self.config.lexical_recall,
            self.namespace_hash,
            &self.documents,
            &self.conversations,
        ) {
            Ok(index) => index,
            Err(error) => {
                if let Some(previous) = previous {
                    self.documents
                        .insert(previous.request.lease.entry_id.0, previous);
                } else {
                    self.documents.remove(&entry_id);
                }
                return Err(error);
            }
        };
        match self.publish(cancellation) {
            Ok(receipt) => {
                self.lexical_recall = prepared_recall;
                Ok(receipt)
            }
            Err(error) => {
                if let Some(previous) = previous {
                    self.documents
                        .insert(previous.request.lease.entry_id.0, previous);
                } else {
                    self.documents.remove(&entry_id);
                }
                Err(error)
            }
        }
    }

    pub fn recall(&mut self, request: RecallTurn) -> Result<ContextPacket, CoordinatorError> {
        if request.pending_turn.origin.is_forbidden_gold() {
            return Err(CoordinatorError::GoldDataRejected);
        }
        if self.conversations.values().any(|conversation| {
            conversation
                .turns
                .iter()
                .any(|stored| stored.turn.external_id == request.pending_turn.external_id)
        }) {
            return Err(CoordinatorError::PendingTurnCommitted);
        }
        let mut packet = self.lexical_recall.recall(
            &request,
            &self.documents,
            &self.conversations,
            self.current.as_ref().map(|receipt| receipt.generation_hash),
            self.config.max_context_items,
            self.config.max_context_bytes,
        )?;
        let mut authority_ordinals = packet
            .items
            .iter()
            .filter(|item| {
                item.source_kind == phoenix_memory_contract::SourceKind::Conversation
                    && item.source_id.0
                        == phoenix_memory_contract::deterministic_id(
                            b"source/conversation",
                            &[
                                &self.namespace_hash,
                                request.conversation.external_id.as_ref(),
                            ],
                        )
            })
            .map(|item| (item.score_micros, item.ordinal))
            .collect::<Vec<_>>();
        authority_ordinals.sort_unstable_by(|left, right| {
            right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1))
        });
        let authority_ordinals = authority_ordinals
            .into_iter()
            .map(|(_, ordinal)| ordinal)
            .collect::<Vec<_>>();
        packet.qps_shadow = self.qps_shadow.evaluate(
            request.conversation.external_id.as_ref(),
            request.pending_turn.content.as_ref(),
            &authority_ordinals,
        );
        Ok(packet)
    }

    pub fn ingest_turn(
        &mut self,
        request: IngestTurn,
        cancellation: &CancellationProbe,
    ) -> Result<GenerationPublication, CoordinatorError> {
        validate_turn_command(&request)?;
        cancellation.check()?;
        let key = request.conversation.external_id.to_vec();
        if let Some(conversation) = self.conversations.get(&key) {
            if let Some(stored) = conversation
                .turns
                .iter()
                .find(|stored| stored.turn.external_id == request.committed_turn.external_id)
            {
                if stored.turn == request.committed_turn {
                    return self
                        .current
                        .clone()
                        .ok_or(CoordinatorError::ProducerAuthority(
                            "idempotent turn has no published generation",
                        ));
                }
                return Err(CoordinatorError::ConflictingTurn);
            }
            if request.committed_turn.ordinal != conversation.turns.len() as u32 {
                return Err(CoordinatorError::NonContiguousTurn);
            }
        } else if request.committed_turn.ordinal != 0 {
            return Err(CoordinatorError::NonContiguousTurn);
        }

        let production = self.producer.analyze_turn(
            &request.conversation.external_id,
            &request.committed_turn,
            cancellation,
        )?;
        cancellation.check()?;
        validate_common_products(
            &production.common,
            u32::try_from(request.committed_turn.content.len())
                .map_err(|_| CoordinatorError::Oversized)?,
        )?;
        validate_candidate_capabilities(&self.config, &production.common)?;

        let conversation =
            self.conversations
                .entry(key.clone())
                .or_insert_with(|| StoredConversation {
                    external_id: request.conversation.external_id.clone(),
                    started_at_millis: request.conversation.started_at_millis,
                    turns: Vec::new(),
                });
        if conversation.started_at_millis != request.conversation.started_at_millis {
            return Err(CoordinatorError::ConflictingTurn);
        }
        conversation.turns.push(StoredTurn {
            turn: request.committed_turn,
            production,
        });
        let prepared_recall = match LexicalRecallIndex::build(
            self.config.lexical_recall,
            self.namespace_hash,
            &self.documents,
            &self.conversations,
        ) {
            Ok(index) => index,
            Err(error) => {
                rollback_turn(&mut self.conversations, &key);
                return Err(error);
            }
        };
        match self.publish(cancellation) {
            Ok(receipt) => {
                self.lexical_recall = prepared_recall;
                if let Some(conversation) = self.conversations.get(&key) {
                    self.qps_shadow.rebuild(&key, conversation);
                }
                Ok(receipt)
            }
            Err(error) => {
                rollback_turn(&mut self.conversations, &key);
                Err(error)
            }
        }
    }

    fn publish(
        &mut self,
        cancellation: &CancellationProbe,
    ) -> Result<GenerationPublication, CoordinatorError> {
        cancellation.check()?;
        let next_generation = self
            .published_generation
            .checked_add(1)
            .ok_or(CoordinatorError::Oversized)?;
        let (state_hash, prepared) = prepare_generation(self, next_generation)?;
        cancellation.check()?;
        let receipt = crate::publish_or_reuse(
            &self.config.artifact_dir,
            state_hash,
            self.namespace_hash,
            prepared,
        )?;
        self.published_generation = receipt.published_generation;
        self.current = Some(receipt.clone());
        Ok(receipt)
    }

    pub fn verify_current(&self) -> Result<Option<[u8; 32]>, CoordinatorError> {
        let Some(receipt) = &self.current else {
            return Ok(None);
        };
        let generation = VerifiedGraphGenerationV3::open(&receipt.path)?;
        Ok(Some(generation.header().generation_hash))
    }
}

fn rollback_turn(conversations: &mut HashMap<Vec<u8>, StoredConversation>, key: &[u8]) {
    let mut remove_conversation = false;
    if let Some(conversation) = conversations.get_mut(key) {
        conversation.turns.pop();
        remove_conversation = conversation.turns.is_empty();
    }
    if remove_conversation {
        conversations.remove(key);
    }
}

fn validate_document_command(request: &IngestDocumentRevision) -> Result<(), CoordinatorError> {
    if request.origin.is_forbidden_gold() {
        return Err(CoordinatorError::GoldDataRejected);
    }
    if request.lease.revision != request.revision || request.lease.content_hash != request.hash {
        return Err(CoordinatorError::LeaseMismatch);
    }
    Ok(())
}

fn validate_document_production(
    request: &IngestDocumentRevision,
    production: &DocumentProduction,
) -> Result<(), CoordinatorError> {
    production
        .structural
        .validate()
        .map_err(CoordinatorError::ProducerAuthority)?;
    let binding = &production.structural.binding;
    if binding.native_document_id != request.lease.entry_id.0
        || binding.document_revision != request.revision.0
        || binding.content_hash != request.hash.0
        || production.structural.source_len as usize != request.lease.content.len()
    {
        return Err(CoordinatorError::ProducerAuthority(
            "document structural binding drifted from the lease",
        ));
    }
    validate_common_products(&production.common, production.structural.source_len)
}

fn validate_turn_command(request: &IngestTurn) -> Result<(), CoordinatorError> {
    if request.committed_turn.origin.is_forbidden_gold() {
        return Err(CoordinatorError::GoldDataRejected);
    }
    if request.conversation.external_id.is_empty()
        || request.committed_turn.external_id.is_empty()
        || request.committed_turn.content.is_empty()
        || !role_is_source(request.committed_turn.role)
        || request
            .committed_turn
            .reply_to_ordinal
            .is_some_and(|reply| reply >= request.committed_turn.ordinal)
    {
        return Err(CoordinatorError::ProducerAuthority(
            "committed turn authority is invalid",
        ));
    }
    Ok(())
}

fn validate_candidate_capabilities(
    config: &CoordinatorConfig,
    products: &crate::CommonProducts,
) -> Result<(), CoordinatorError> {
    for candidate in &products.candidates {
        let product = candidate_product(candidate.family);
        let registration = &config.registrations[(product as usize) - 1];
        if registration.support != crate::RegistrationSupport::Supported {
            return Err(CoordinatorError::ProducerAuthority(
                "producer emitted a product registered as unsupported",
            ));
        }
    }
    Ok(())
}

fn same_document(left: &IngestDocumentRevision, right: &IngestDocumentRevision) -> bool {
    left.lease.entry_id == right.lease.entry_id
        && left.revision == right.revision
        && left.hash == right.hash
        && left.lease.content.as_bytes() == right.lease.content.as_bytes()
}
