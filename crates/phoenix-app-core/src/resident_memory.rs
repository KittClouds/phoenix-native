use crate::{
    checked_revision, push_event, receipt, write_state, CommandReceipt, KernelEvent,
    KernelEventKind, KernelOutcome, KernelShared, MemoryContextSelection, MemoryGenerationReceipt,
    MemoryRecallReceipt,
};
use phoenix_analysis_contract::{
    NliCandidateKind, PhoenixDocumentAnalysisV1, PhoenixStructuralSubstrateV1,
};
use phoenix_memory_contract::{
    CandidateEndpointRoleV3, CandidateStatus, ProducerProductV3, SemanticCandidateFamilyV3,
    VerifiedGraphGenerationV3, VocabularyPackKindV3,
};
use phoenix_memory_coordinator::{
    CancellationProbe, CandidateEndpointDraft, CanonicalBindingDraft, CommonProducts,
    ContextPacket, CoordinatorConfig, CoordinatorError, CoordinatorMetrics, DocumentProduction,
    DualFaceIngestionCoordinator, DualFaceProducer, GenerationPublication, IngestDocumentRevision,
    IngestTurn, MemoryScope, ProducerRegistrationV3, RegistrationSupport, SemanticCandidateDraft,
    TurnProduction, VocabularyPackDraft,
};
use phoenix_workspace::DocumentLease;
use std::path::Path;
use std::sync::{Arc, RwLock};
use thiserror::Error;

const NER_SOURCE_MASK: u16 = 1;
const IDENTITY_PACK_ID: u64 = 0x5048_5849_4445_4e54;
const CONTEXT_PACK_ID: u64 = 0x5048_5843_4f4e_5458;

#[derive(Clone, Debug)]
pub struct VerifiedMemoryPublication {
    pub receipt: GenerationPublication,
    pub graph: Arc<VerifiedGraphGenerationV3>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResidentRecallIndexes {
    pub path_id: &'static str,
    pub generation_hash: Option<[u8; 32]>,
    pub corpus_hash: [u8; 32],
    pub indexed_items: u32,
}

#[derive(Clone, Debug)]
pub struct ResidentMemorySnapshot {
    pub publication: Option<Arc<VerifiedMemoryPublication>>,
    pub active_scope: MemoryScope,
    pub recall_indexes: ResidentRecallIndexes,
    pub last_context: Option<Arc<ContextPacket>>,
    pub pending_document_count: u32,
    pub commands: CoordinatorMetrics,
}

#[derive(Debug, Error)]
pub enum ResidentMemoryError {
    #[error(transparent)]
    Coordinator(#[from] CoordinatorError),
    #[error("resident memory lock is poisoned: {0}")]
    Poisoned(&'static str),
    #[error("no exact structural product is registered for this document revision")]
    StructuralProductUnavailable,
    #[error("memory context item is absent from the current verified recall packet")]
    ContextItemUnavailable,
    #[error("memory context packet does not match the resident generation")]
    StaleContextPacket,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DocumentProductKey {
    entry_id: u64,
    revision: u64,
    content_hash: [u8; 32],
}

#[derive(Default)]
struct KernelMemoryProducer {
    documents: RwLock<hashbrown::HashMap<DocumentProductKey, DocumentProduction>>,
}

impl KernelMemoryProducer {
    fn register(
        &self,
        lease: &DocumentLease,
        structural: &PhoenixStructuralSubstrateV1,
        analysis: &PhoenixDocumentAnalysisV1,
    ) -> Result<(), ResidentMemoryError> {
        let key = document_key(lease);
        let production = DocumentProduction {
            structural: structural.clone(),
            common: common_products(analysis),
        };
        self.documents
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("document products"))?
            .insert(key, production);
        Ok(())
    }
}

impl DualFaceProducer for KernelMemoryProducer {
    fn analyze_document(
        &self,
        request: &IngestDocumentRevision,
        cancellation: &CancellationProbe,
    ) -> Result<DocumentProduction, CoordinatorError> {
        cancellation.check()?;
        self.documents
            .read()
            .map_err(|_| CoordinatorError::ProducerAuthority("document product lock poisoned"))?
            .get(&document_key(&request.lease))
            .cloned()
            .ok_or(CoordinatorError::ProducerAuthority(
                "exact dynamic chunks have not been produced for this revision",
            ))
    }

    fn analyze_turn(
        &self,
        _conversation_external_id: &[u8],
        _turn: &phoenix_memory_coordinator::CommittedTurn,
        cancellation: &CancellationProbe,
    ) -> Result<TurnProduction, CoordinatorError> {
        cancellation.check()?;
        Ok(TurnProduction::default())
    }
}

pub struct ResidentMemory {
    namespace_hash: [u8; 32],
    publication: RwLock<Option<Arc<VerifiedMemoryPublication>>>,
    recall_indexes: RwLock<ResidentRecallIndexes>,
    active_scope: RwLock<MemoryScope>,
    last_context: RwLock<Option<Arc<ContextPacket>>>,
    stale_sources: RwLock<hashbrown::HashSet<phoenix_memory_contract::SourceId>>,
    producer: Arc<KernelMemoryProducer>,
    bounded_commands: DualFaceIngestionCoordinator<KernelMemoryProducer>,
}

impl ResidentMemory {
    pub fn open(
        workspace_path: &Path,
        registry_revision: u64,
    ) -> Result<Self, ResidentMemoryError> {
        Self::open_with_context_limit(workspace_path, registry_revision, 24)
    }

    pub fn open_with_context_limit(
        workspace_path: &Path,
        registry_revision: u64,
        maximum_context_items: usize,
    ) -> Result<Self, ResidentMemoryError> {
        let producer = Arc::new(KernelMemoryProducer::default());
        let root = workspace_path
            .parent()
            .ok_or(ResidentMemoryError::StructuralProductUnavailable)?;
        let namespace = workspace_path.as_os_str().to_string_lossy().into_owned();
        let namespace_hash = *blake3::hash(namespace.as_bytes()).as_bytes();
        let mut config = CoordinatorConfig::new(
            namespace.as_bytes(),
            root.join("memory-authority-v3"),
            registrations(),
            Arc::from([]),
        );
        config.registry_revision = registry_revision;
        config.max_context_items = maximum_context_items;
        config.lexical_recall.top_k = maximum_context_items;
        config.lexical_recall.maximum_candidate_pool = config
            .lexical_recall
            .maximum_candidate_pool
            .max(maximum_context_items);
        let bounded_commands = DualFaceIngestionCoordinator::new(config, Arc::clone(&producer))?;
        Ok(Self {
            namespace_hash,
            publication: RwLock::new(None),
            recall_indexes: RwLock::new(ResidentRecallIndexes {
                path_id: phoenix_memory_coordinator::LEXICAL_RECALL_PATH,
                ..ResidentRecallIndexes::default()
            }),
            active_scope: RwLock::new(MemoryScope::Workspace),
            last_context: RwLock::new(None),
            stale_sources: RwLock::new(hashbrown::HashSet::new()),
            producer,
            bounded_commands,
        })
    }

    pub fn publish_document(
        &self,
        lease: Arc<DocumentLease>,
        structural: &PhoenixStructuralSubstrateV1,
        analysis: &PhoenixDocumentAnalysisV1,
    ) -> Result<Arc<VerifiedMemoryPublication>, ResidentMemoryError> {
        self.producer.register(&lease, structural, analysis)?;
        let source_id = self.document_source_id(lease.entry_id.0);
        let request = IngestDocumentRevision {
            revision: lease.revision,
            hash: lease.content_hash,
            lease,
            origin: phoenix_memory_coordinator::IngestionOrigin::Workspace,
        };
        let receipt = self.bounded_commands.try_ingest_document(request)?.wait()?;
        let publication = self.install_publication(receipt)?;
        self.stale_sources
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("stale sources"))?
            .remove(&source_id);
        Ok(publication)
    }

    pub fn ingest_turn(
        &self,
        request: IngestTurn,
    ) -> Result<Arc<VerifiedMemoryPublication>, ResidentMemoryError> {
        let receipt = self.bounded_commands.try_ingest_turn(request)?.wait()?;
        self.install_publication(receipt)
    }

    pub fn recall(
        &self,
        mut request: phoenix_memory_coordinator::RecallTurn,
    ) -> Result<Arc<ContextPacket>, ResidentMemoryError> {
        request.scope = self
            .active_scope
            .read()
            .map_err(|_| ResidentMemoryError::Poisoned("active scope"))?
            .clone();
        let mut packet = self.bounded_commands.try_recall(request)?.wait()?;
        self.filter_stale_sources(&mut packet)?;
        let packet = Arc::new(packet);
        *self
            .recall_indexes
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("recall indexes"))? =
            ResidentRecallIndexes {
                path_id: packet.lexical.path_id.as_str(),
                generation_hash: packet.resident_generation_hash,
                corpus_hash: packet.lexical.corpus_hash,
                indexed_items: packet.lexical.indexed_items,
            };
        *self
            .last_context
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("last context"))? =
            Some(Arc::clone(&packet));
        Ok(packet)
    }

    pub fn set_scope(&self, scope: MemoryScope) -> Result<(), ResidentMemoryError> {
        *self
            .active_scope
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("active scope"))? = normalize_scope(scope);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<ResidentMemorySnapshot, ResidentMemoryError> {
        Ok(ResidentMemorySnapshot {
            publication: self
                .publication
                .read()
                .map_err(|_| ResidentMemoryError::Poisoned("publication"))?
                .clone(),
            active_scope: self
                .active_scope
                .read()
                .map_err(|_| ResidentMemoryError::Poisoned("active scope"))?
                .clone(),
            recall_indexes: *self
                .recall_indexes
                .read()
                .map_err(|_| ResidentMemoryError::Poisoned("recall indexes"))?,
            last_context: self
                .last_context
                .read()
                .map_err(|_| ResidentMemoryError::Poisoned("last context"))?
                .clone(),
            pending_document_count: u32::try_from(
                self.stale_sources
                    .read()
                    .map_err(|_| ResidentMemoryError::Poisoned("stale sources"))?
                    .len(),
            )
            .unwrap_or(u32::MAX),
            commands: self.bounded_commands.metrics(),
        })
    }

    pub fn cancel(&self) -> u64 {
        self.bounded_commands.cancel()
    }

    pub fn conversation_source_id(&self, external_id: &[u8]) -> phoenix_memory_contract::SourceId {
        phoenix_memory_contract::SourceId(phoenix_memory_contract::deterministic_id(
            b"source/conversation",
            &[&self.namespace_hash, external_id],
        ))
    }

    pub fn mark_document_pending(&self, entry_id: u64) -> Result<(), ResidentMemoryError> {
        self.stale_sources
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("stale sources"))?
            .insert(self.document_source_id(entry_id));
        Ok(())
    }

    fn document_source_id(&self, entry_id: u64) -> phoenix_memory_contract::SourceId {
        phoenix_memory_contract::SourceId(phoenix_memory_contract::deterministic_id(
            b"source/document",
            &[&self.namespace_hash, &entry_id.to_le_bytes()],
        ))
    }

    fn filter_stale_sources(&self, packet: &mut ContextPacket) -> Result<(), ResidentMemoryError> {
        let stale = self
            .stale_sources
            .read()
            .map_err(|_| ResidentMemoryError::Poisoned("stale sources"))?;
        if stale.is_empty() {
            return Ok(());
        }
        let mut items = packet.items.to_vec();
        items.retain(|item| !stale.contains(&item.source_id));
        let mut candidates = packet.proposed_candidates.to_vec();
        candidates.retain(|candidate| {
            candidate
                .evidence
                .iter()
                .all(|evidence| !stale.contains(&evidence.source_id))
        });
        let bytes = items
            .iter()
            .map(|item| item.content.len())
            .chain(candidates.iter().flat_map(|candidate| {
                candidate
                    .evidence
                    .iter()
                    .map(|evidence| evidence.content.len())
            }))
            .fold(0_usize, usize::saturating_add);
        packet.items = items.into();
        packet.proposed_candidates = candidates.into();
        packet.returned_bytes = u32::try_from(bytes).unwrap_or(u32::MAX);
        packet.lexical.returned_items = u16::try_from(packet.items.len()).unwrap_or(u16::MAX);
        packet.lexical.returned_candidates =
            u16::try_from(packet.proposed_candidates.len()).unwrap_or(u16::MAX);
        packet.lexical.returned_bytes = packet.returned_bytes;
        Ok(())
    }

    fn install_publication(
        &self,
        receipt: GenerationPublication,
    ) -> Result<Arc<VerifiedMemoryPublication>, ResidentMemoryError> {
        let graph = Arc::new(
            VerifiedGraphGenerationV3::open(&receipt.path).map_err(CoordinatorError::from)?,
        );
        let publication = Arc::new(VerifiedMemoryPublication { receipt, graph });
        *self
            .publication
            .write()
            .map_err(|_| ResidentMemoryError::Poisoned("publication"))? =
            Some(Arc::clone(&publication));
        Ok(publication)
    }
}

pub(super) fn set_memory_scope(
    shared: &KernelShared,
    sequence: u64,
    scope: MemoryScope,
) -> Result<CommandReceipt, crate::KernelError> {
    shared.resident_memory.set_scope(scope)?;
    let scope_hash = shared
        .resident_memory
        .snapshot()?
        .active_scope
        .fingerprint();
    let revision = bump_kernel_revision(shared)?;
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::MemoryScopeChanged { scope_hash },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::MemoryScopeChanged,
    ))
}

pub(super) fn recall_memory(
    shared: &KernelShared,
    sequence: u64,
    request: phoenix_memory_coordinator::RecallTurn,
) -> Result<CommandReceipt, crate::KernelError> {
    let packet = shared.resident_memory.recall(request)?;
    let recall = MemoryRecallReceipt {
        pending_turn_hash: packet.pending_turn_hash,
        scope_hash: packet.scope_hash,
        generation_hash: packet.resident_generation_hash,
        returned_items: packet.lexical.returned_items,
        returned_candidates: packet.lexical.returned_candidates,
        returned_bytes: packet.returned_bytes,
    };
    let revision = bump_kernel_revision(shared)?;
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::MemoryRecalled { receipt: recall },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::MemoryRecalled(recall),
    ))
}

pub(super) fn ingest_memory_turn(
    shared: &KernelShared,
    sequence: u64,
    request: IngestTurn,
) -> Result<CommandReceipt, crate::KernelError> {
    let publication = shared.resident_memory.ingest_turn(request)?;
    let memory = MemoryGenerationReceipt {
        generation_hash: publication.receipt.generation_hash,
        source_count: publication.receipt.source_count,
        document_count: publication.receipt.document_count,
        conversation_count: publication.receipt.conversation_count,
        turn_count: publication.receipt.turn_count,
    };
    let revision = bump_kernel_revision(shared)?;
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::MemoryGenerationPublished {
                generation_hash: memory.generation_hash,
                source_count: memory.source_count,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::MemoryGenerationPublished(memory),
    ))
}

pub(super) fn select_memory_context(
    shared: &KernelShared,
    sequence: u64,
    source_id: u64,
    content_id: u64,
) -> Result<CommandReceipt, crate::KernelError> {
    let memory = shared.resident_memory.snapshot()?;
    let publication = memory
        .publication
        .ok_or(ResidentMemoryError::ContextItemUnavailable)?;
    let packet = memory
        .last_context
        .ok_or(ResidentMemoryError::ContextItemUnavailable)?;
    if packet.resident_generation_hash != Some(publication.receipt.generation_hash) {
        return Err(ResidentMemoryError::StaleContextPacket.into());
    }
    let item = packet
        .items
        .iter()
        .find(|item| item.source_id.0 == source_id && item.content_id == content_id)
        .ok_or(ResidentMemoryError::ContextItemUnavailable)?;
    let selection = MemoryContextSelection {
        resident_generation_hash: publication.receipt.generation_hash,
        source_id,
        content_id,
        locator: item.locator.clone(),
        source_start: item.source_start,
        source_end: item.source_end,
        content_hash: item.content_hash,
    };
    let mut state = write_state(shared)?;
    state.memory_context_selection = Some(selection.clone());
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::MemoryContextSelected(selection),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

fn bump_kernel_revision(shared: &KernelShared) -> Result<u64, crate::KernelError> {
    let mut state = write_state(shared)?;
    state.revision = checked_revision(state.revision)?;
    Ok(state.revision)
}

fn document_key(lease: &DocumentLease) -> DocumentProductKey {
    DocumentProductKey {
        entry_id: lease.entry_id.0,
        revision: lease.revision.0,
        content_hash: lease.content_hash.0,
    }
}

fn normalize_scope(scope: MemoryScope) -> MemoryScope {
    match scope {
        MemoryScope::DocumentSet(documents) => {
            let mut documents = documents.to_vec();
            documents.sort_unstable();
            documents.dedup();
            MemoryScope::DocumentSet(documents.into())
        }
        MemoryScope::Compare {
            documents,
            conversations,
        } => {
            let mut documents = documents.to_vec();
            documents.sort_unstable();
            documents.dedup();
            let mut conversations = conversations.to_vec();
            conversations.sort_unstable_by(|left, right| left.as_ref().cmp(right.as_ref()));
            conversations.dedup_by(|left, right| left.as_ref() == right.as_ref());
            MemoryScope::Compare {
                documents: documents.into(),
                conversations: conversations.into(),
            }
        }
        other => other,
    }
}

fn registrations() -> Arc<[ProducerRegistrationV3]> {
    let binary_hash = *blake3::hash(b"phoenix-app-core/resident-memory-v1").as_bytes();
    let config_hash = *blake3::hash(b"candidate-only;exact-structural-products").as_bytes();
    ProducerProductV3::ALL
        .iter()
        .copied()
        .map(|product| ProducerRegistrationV3 {
            product,
            producer: Arc::from("phoenix-app-core/resident-memory-v1"),
            producer_binary_hash: binary_hash,
            config_hash,
            model_identity_index: None,
            support: if matches!(
                product,
                ProducerProductV3::SourceStructure
                    | ProducerProductV3::ContentUnitsAndChunks
                    | ProducerProductV3::MentionsAndEvidence
                    | ProducerProductV3::CanonicalEntityBindings
                    | ProducerProductV3::IdentityCoreference
                    | ProducerProductV3::ContextualEvidence
            ) {
                RegistrationSupport::Supported
            } else {
                RegistrationSupport::Unsupported
            },
        })
        .collect::<Vec<_>>()
        .into()
}

fn common_products(analysis: &PhoenixDocumentAnalysisV1) -> CommonProducts {
    let entities = analysis
        .ner
        .entities
        .iter()
        .map(|entity| phoenix_memory_coordinator::EntityDraft {
            stable_id: entity.stable_id,
            label: Arc::from(entity.label.as_str()),
            custom_kind: entity.custom_kind.as_deref().map(Arc::from),
            mention_count: entity.mention_count,
            kind: entity.kind as u16,
            source_mask: NER_SOURCE_MASK,
        })
        .collect();
    let mentions = analysis
        .ner
        .mentions
        .iter()
        .map(|mention| phoenix_memory_coordinator::MentionDraft {
            stable_id: mention.mention_id,
            entity_id: mention.entity_id,
            evidence_id: mention.mention_id,
            start: mention.start,
            end: mention.end,
            confidence: mention.confidence,
            flags: u32::from(mention.accepted),
        })
        .collect::<Vec<_>>();
    let canonical_bindings = analysis
        .ner
        .entities
        .iter()
        .map(|entity| CanonicalBindingDraft {
            source_entity_id: entity.stable_id,
            canonical_entity_id: entity.stable_id,
            decision_id: 0,
            source_mask: NER_SOURCE_MASK,
            kind: phoenix_memory_contract::CanonicalBindingKind::Direct,
        })
        .collect();
    let vocabulary_packs = vec![
        vocabulary_pack(
            IDENTITY_PACK_ID,
            "phoenix.memory.identity",
            VocabularyPackKindV3::Core,
        ),
        vocabulary_pack(
            CONTEXT_PACK_ID,
            "phoenix.memory.context",
            VocabularyPackKindV3::Core,
        ),
    ];
    let candidates = analysis
        .nli
        .nli_candidates
        .iter()
        .zip(&analysis.nli.nli_adjudications)
        .filter_map(|(candidate, adjudication)| {
            let evidence_ids = mentions
                .iter()
                .filter(|mention| {
                    mention.start < candidate.premise_end && mention.end > candidate.premise_start
                })
                .map(|mention| mention.evidence_id)
                .take(phoenix_memory_coordinator::MAX_EVIDENCE_PER_CANDIDATE)
                .collect::<Vec<_>>();
            if evidence_ids.is_empty() {
                return None;
            }
            let (family, pack, relation) = match candidate.kind {
                NliCandidateKind::SameSurface => (
                    SemanticCandidateFamilyV3::Identity,
                    IDENTITY_PACK_ID,
                    "identity.same-surface",
                ),
                NliCandidateKind::Alias => (
                    SemanticCandidateFamilyV3::Identity,
                    IDENTITY_PACK_ID,
                    "identity.alias",
                ),
                NliCandidateKind::Coreference => (
                    SemanticCandidateFamilyV3::Coreference,
                    IDENTITY_PACK_ID,
                    "identity.coreference",
                ),
                NliCandidateKind::Related => (
                    SemanticCandidateFamilyV3::ContextualEvidence,
                    CONTEXT_PACK_ID,
                    "context.related",
                ),
            };
            Some(SemanticCandidateDraft {
                candidate_id: candidate.candidate_id,
                vocabulary_pack_id: pack,
                relation_kind: Arc::from(relation),
                value: Arc::from(candidate.hypothesis.as_str()),
                endpoints: Arc::from([
                    CandidateEndpointDraft {
                        endpoint_id: candidate.left_entity_id,
                        role: CandidateEndpointRoleV3::Subject,
                        flags: 0,
                    },
                    CandidateEndpointDraft {
                        endpoint_id: candidate.right_entity_id,
                        role: CandidateEndpointRoleV3::Object,
                        flags: 0,
                    },
                ]),
                evidence_ids: evidence_ids.into(),
                valid_time_from_millis: i64::MIN,
                valid_time_to_millis: i64::MAX,
                family,
                confidence: adjudication.confidence_millis as f32 / 1_000.0,
                model_identity_index: None,
                producer_identity_hash: analysis.ner.binding.producer_binary_hash,
                status: CandidateStatus::Proposed,
                flags: 0,
            })
        })
        .collect();
    CommonProducts {
        entities,
        mentions,
        canonical_bindings,
        vocabulary_packs,
        candidates,
    }
}

fn vocabulary_pack(id: u64, name: &'static str, kind: VocabularyPackKindV3) -> VocabularyPackDraft {
    VocabularyPackDraft {
        id,
        name: Arc::from(name),
        version: Arc::from("1"),
        schema_hash: *blake3::hash(name.as_bytes()).as_bytes(),
        producer_identity_hash: *blake3::hash(b"phoenix-app-core/resident-memory-v1").as_bytes(),
        kind,
        flags: 0,
    }
}
