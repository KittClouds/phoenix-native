use hashbrown::HashMap;
use memmap2::Mmap;
use phoenix_analysis_contract::{NliCandidateKind, PhoenixDocumentAnalysisV1};
use phoenix_graph_generation::{
    promoted_edge_id, write_graph_generation_new, AcceptedEdgeInput, CanonicalEntityInput,
    DurableDecisionInput, GraphGenerationInput, ProducerCapabilityInput, VerifiedGraphGeneration,
    ACCEPTED_EDGE_FLAG_PROMOTED, DECISION_FLAG_DURABLE_RECEIPT, DECISION_STATUS_ACCEPTED,
    DECISION_STATUS_DEFERRED, DECISION_STATUS_REJECTED, GRAPH_GENERATION_EXTENSION,
};
use phoenix_scene_compiler::{
    compile_graph_generation, proposed_nli_edge_id, NativeSceneCompilerInput,
};
use phoenix_scene_contract::{
    AnchorSource, DocumentId, GraphGeneration, GraphReviewOverride, ReviewMask,
    VerifiedDocumentAnchors,
};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

use super::{
    checked_revision, push_event, read_state, receipt, scene_publication, write_state,
    AtlasDecisionCommandReceipt, CommandReceipt, GraphReviewOverlay, KernelError, KernelEvent,
    KernelEventKind, KernelOutcome, KernelShared, NativeScenePublishCommand,
};

pub const ATLAS_DECISION_RECEIPT_CONTRACT: &str = "phoenix.native.atlas-decision-receipt/v1";
const AUTHORITY_DIRECTORY: &str = "atlas-decision-authority-v1";
const RECEIPT_EXTENSION: &str = "phxdr";
const MAGIC: [u8; 8] = *b"PHXADR01";
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_RECEIPTS: usize = 1_000_000;
const MAX_REASON_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct AtlasCandidateId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DecisionBindingKey {
    candidate_id: AtlasCandidateId,
    candidate_hash: [u8; 32],
    evidence_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CurrentCandidateBinding {
    pub authority: AtlasDecisionAuthority,
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub left_entity_id: u64,
    pub right_entity_id: u64,
    pub premise_start: u32,
    pub premise_end: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AtlasDecisionAction {
    Accept,
    Reject,
    Defer,
    Undo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AtlasDecisionStatus {
    Accepted,
    Rejected,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasDecisionAuthority {
    pub document_id: u64,
    pub document_revision: u64,
    pub document_hash: [u8; 32],
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub producer_graph_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasDecisionCommand {
    pub candidate_id: AtlasCandidateId,
    pub action: AtlasDecisionAction,
    pub expected_receipt_id: Option<[u8; 32]>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AtlasDecisionReceiptV1 {
    pub contract: String,
    pub receipt_id: [u8; 32],
    pub sequence: u64,
    pub candidate_id: AtlasCandidateId,
    pub action: AtlasDecisionAction,
    pub prior_status: Option<AtlasDecisionStatus>,
    pub resulting_status: Option<AtlasDecisionStatus>,
    pub previous_receipt_id: Option<[u8; 32]>,
    pub authority: AtlasDecisionAuthority,
    pub candidate_hash: [u8; 32],
    pub evidence_hash: [u8; 32],
    pub reason: String,
}

impl AtlasDecisionReceiptV1 {
    fn validate(&self) -> Result<(), AtlasReviewError> {
        if self.contract != ATLAS_DECISION_RECEIPT_CONTRACT
            || self.receipt_id == [0; 32]
            || self.sequence == 0
            || self.candidate_id.0 == [0; 32]
            || self.authority.document_id == 0
            || self.authority.document_revision == 0
            || self.authority.document_hash == [0; 32]
            || self.authority.registry_revision == 0
            || self.authority.producer_generation == 0
            || self.authority.producer_graph_hash == [0; 32]
            || self.candidate_hash == [0; 32]
            || self.evidence_hash == [0; 32]
            || self.reason.len() > MAX_REASON_BYTES
        {
            return Err(AtlasReviewError::InvalidReceipt);
        }
        match self.action {
            AtlasDecisionAction::Accept
                if self.resulting_status != Some(AtlasDecisionStatus::Accepted) =>
            {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            AtlasDecisionAction::Reject
                if self.resulting_status != Some(AtlasDecisionStatus::Rejected) =>
            {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            AtlasDecisionAction::Defer
                if self.resulting_status != Some(AtlasDecisionStatus::Deferred) =>
            {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            AtlasDecisionAction::Undo if self.previous_receipt_id.is_none() => {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            _ => {}
        }
        if receipt_id(self)? != self.receipt_id {
            return Err(AtlasReviewError::HashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum AtlasReviewError {
    #[error("Atlas review authority path has no parent")]
    MissingAuthorityRoot,
    #[error("Atlas decision candidate is unknown")]
    UnknownCandidate,
    #[error("Atlas decision candidate or document authority is stale")]
    StaleAuthority,
    #[error("Atlas decision expected receipt does not match the durable head")]
    StaleDecisionHead,
    #[error("Atlas decision undo requires a durable prior decision")]
    NothingToUndo,
    #[error("Atlas decision reason is oversized")]
    OversizedReason,
    #[error("Atlas decision ledger exceeds its v1 receipt bound")]
    OversizedLedger,
    #[error("Atlas decision receipt is invalid")]
    InvalidReceipt,
    #[error("Atlas decision receipt header is invalid")]
    InvalidHeader,
    #[error("Atlas decision receipt version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("Atlas decision receipt hash mismatch")]
    HashMismatch,
    #[error("Atlas decision receipt codec failed: {0}")]
    Codec(String),
    #[error("Atlas decision receipt I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(super) struct AtlasReviewLedger {
    directory: PathBuf,
    receipts: Vec<AtlasDecisionReceiptV1>,
    heads: HashMap<DecisionBindingKey, usize>,
}

impl AtlasReviewLedger {
    pub(super) fn open(workspace_path: &Path) -> Result<Self, AtlasReviewError> {
        let directory = workspace_path
            .parent()
            .ok_or(AtlasReviewError::MissingAuthorityRoot)?
            .join(AUTHORITY_DIRECTORY);
        let mut paths = match std::fs::read_dir(&directory) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == RECEIPT_EXTENSION))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(source) => {
                return Err(AtlasReviewError::Io {
                    path: directory,
                    source,
                });
            }
        };
        if paths.len() > MAX_RECEIPTS {
            return Err(AtlasReviewError::OversizedLedger);
        }
        paths.sort_unstable();
        let mut receipts: Vec<AtlasDecisionReceiptV1> = Vec::with_capacity(paths.len());
        let mut heads: HashMap<DecisionBindingKey, usize> = HashMap::with_capacity(paths.len());
        for path in paths {
            let receipt = open_receipt(&path)?;
            let key = DecisionBindingKey {
                candidate_id: receipt.candidate_id,
                candidate_hash: receipt.candidate_hash,
                evidence_hash: receipt.evidence_hash,
            };
            let expected_sequence = receipts.len() as u64 + 1;
            if receipt.sequence != expected_sequence {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            let previous = match (heads.get(&key), receipt.previous_receipt_id) {
                (None, None) => None,
                (Some(index), Some(previous)) if receipts[*index].receipt_id == previous => {
                    Some(&receipts[*index])
                }
                _ => return Err(AtlasReviewError::InvalidReceipt),
            };
            if receipt.action == AtlasDecisionAction::Undo
                && previous.is_none_or(|previous| {
                    receipt.prior_status != previous.resulting_status
                        || receipt.resulting_status != previous.prior_status
                })
            {
                return Err(AtlasReviewError::InvalidReceipt);
            }
            let index = receipts.len();
            heads.insert(key, index);
            receipts.push(receipt);
        }
        Ok(Self {
            directory,
            receipts,
            heads,
        })
    }

    pub(super) fn decide(
        &mut self,
        command: &AtlasDecisionCommand,
        authority: AtlasDecisionAuthority,
        candidate_hash: [u8; 32],
        evidence_hash: [u8; 32],
    ) -> Result<AtlasDecisionReceiptV1, AtlasReviewError> {
        if command.reason.len() > MAX_REASON_BYTES {
            return Err(AtlasReviewError::OversizedReason);
        }
        let key = DecisionBindingKey {
            candidate_id: command.candidate_id,
            candidate_hash,
            evidence_hash,
        };
        let current = self.heads.get(&key).map(|index| &self.receipts[*index]);
        if current.is_some_and(|receipt| !authority_can_preserve(receipt.authority, authority)) {
            return Err(AtlasReviewError::StaleAuthority);
        }
        if let Some(existing) = self.receipts.iter().find(|receipt| {
            receipt.candidate_id == command.candidate_id
                && receipt.authority == authority
                && receipt.action == command.action
                && receipt.previous_receipt_id == command.expected_receipt_id
                && receipt.reason == command.reason
        }) {
            if current.is_some_and(|current| current.receipt_id == existing.receipt_id) {
                return Ok(existing.clone());
            }
            return Err(AtlasReviewError::StaleDecisionHead);
        }
        if current.map(|receipt| receipt.receipt_id) != command.expected_receipt_id {
            return Err(AtlasReviewError::StaleDecisionHead);
        }
        let prior_status = current.and_then(|receipt| receipt.resulting_status);
        let resulting_status = match command.action {
            AtlasDecisionAction::Accept => Some(AtlasDecisionStatus::Accepted),
            AtlasDecisionAction::Reject => Some(AtlasDecisionStatus::Rejected),
            AtlasDecisionAction::Defer => Some(AtlasDecisionStatus::Deferred),
            AtlasDecisionAction::Undo => {
                let current = current.ok_or(AtlasReviewError::NothingToUndo)?;
                current.prior_status
            }
        };
        let sequence = u64::try_from(self.receipts.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(AtlasReviewError::OversizedLedger)?;
        let mut receipt = AtlasDecisionReceiptV1 {
            contract: ATLAS_DECISION_RECEIPT_CONTRACT.to_owned(),
            receipt_id: [0; 32],
            sequence,
            candidate_id: command.candidate_id,
            action: command.action,
            prior_status,
            resulting_status,
            previous_receipt_id: command.expected_receipt_id,
            authority,
            candidate_hash,
            evidence_hash,
            reason: command.reason.clone(),
        };
        receipt.receipt_id = receipt_id(&receipt)?;
        persist_receipt(&self.directory, &receipt)?;
        let index = self.receipts.len();
        self.heads.insert(key, index);
        self.receipts.push(receipt.clone());
        Ok(receipt)
    }

    #[cfg(test)]
    pub(super) fn effective(
        &self,
        authority: AtlasDecisionAuthority,
    ) -> Vec<&AtlasDecisionReceiptV1> {
        let mut receipts = self
            .heads
            .values()
            .filter_map(|index| self.receipts.get(*index))
            .filter(|receipt| receipt.authority == authority && receipt.resulting_status.is_some())
            .collect::<Vec<_>>();
        receipts.sort_unstable_by_key(|receipt| receipt.candidate_id);
        receipts
    }

    pub(super) fn receipts(&self) -> &[AtlasDecisionReceiptV1] {
        &self.receipts
    }

    pub(super) fn head_for_binding(
        &self,
        candidate_id: AtlasCandidateId,
        candidate_hash: [u8; 32],
        evidence_hash: [u8; 32],
    ) -> Option<&AtlasDecisionReceiptV1> {
        self.heads
            .get(&DecisionBindingKey {
                candidate_id,
                candidate_hash,
                evidence_hash,
            })
            .and_then(|index| self.receipts.get(*index))
    }

    pub(super) fn has_other_binding(&self, candidate_id: AtlasCandidateId) -> bool {
        self.heads
            .keys()
            .any(|binding| binding.candidate_id == candidate_id)
    }
}

fn authority_can_preserve(
    previous: AtlasDecisionAuthority,
    current: AtlasDecisionAuthority,
) -> bool {
    previous.document_id == current.document_id && previous.document_hash == current.document_hash
}

pub(super) fn review_candidate(
    shared: &KernelShared,
    sequence: u64,
    command: AtlasDecisionCommand,
) -> Result<CommandReceipt, KernelError> {
    let (authority, candidate_hash, evidence_hash) =
        decision_binding(shared, command.candidate_id)?;
    let decision = shared
        .atlas_review
        .lock()
        .map_err(|_| KernelError::Poisoned("Atlas review ledger"))?
        .decide(&command, authority, candidate_hash, evidence_hash)?;
    let mut state = write_state(shared)?;
    {
        let ledger = shared
            .atlas_review
            .lock()
            .map_err(|_| KernelError::Poisoned("Atlas review ledger"))?;
        refresh_review_overlay(&mut state, &ledger)?;
    }
    state.revision = checked_revision(state.revision)?;
    let kernel_revision = state.revision;
    drop(state);
    let outcome = AtlasDecisionCommandReceipt {
        receipt_id: decision.receipt_id,
        candidate_id: decision.candidate_id.0,
        status: decision.resulting_status,
    };
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision,
            kind: KernelEventKind::AtlasCandidateReviewed { receipt: decision },
        },
    )?;
    Ok(receipt(
        sequence,
        kernel_revision,
        KernelOutcome::AtlasCandidateReviewed(outcome),
    ))
}

pub(super) fn publish_reviewed_decisions(
    shared: &KernelShared,
    sequence: u64,
) -> Result<CommandReceipt, KernelError> {
    publish_reviewed_decisions_inner(shared, sequence, sequence)
}

fn publish_reviewed_decisions_inner(
    shared: &KernelShared,
    sequence: u64,
    run_id: u64,
) -> Result<CommandReceipt, KernelError> {
    let (
        authority,
        lease,
        registry,
        analysis,
        structural,
        coordinator,
        source_generation,
        nli,
        anchors,
        palette,
        publisher,
    ) = {
        let state = read_state(shared)?;
        let authority = current_authority(&state)?;
        (
            authority,
            state
                .active_document_lease
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            Arc::clone(&state.entity_registry),
            state
                .document_analysis
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            state
                .structural_analysis
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            state
                .producer_coordinator
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            state
                .graph_generation
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            state
                .nli_analysis
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?,
            state
                .document_anchors
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::DocumentAnchorsNotActive)?,
            *state.highlight_palette,
            shared
                .publisher
                .as_ref()
                .map(Arc::clone)
                .ok_or(KernelError::ProductionPublisherUnavailable)?,
        )
    };
    coordinator
        .validate_final(&analysis, &structural)
        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let effective = {
        let ledger = shared
            .atlas_review
            .lock()
            .map_err(|_| KernelError::Poisoned("Atlas review ledger"))?;
        effective_bound_receipts(&ledger, authority, &nli, &coordinator)?
    };
    let reviewed_generation = materialize_reviewed_generation(
        &lease,
        &registry,
        &analysis,
        &structural,
        &source_generation,
        &effective,
    )?;
    if source_generation.generation_hash() == reviewed_generation.generation_hash() {
        let state = read_state(shared)?;
        let publication = state
            .scene_publication
            .ok_or(KernelError::ProductionPublisherUnavailable)?;
        return Ok(receipt(
            sequence,
            state.revision,
            KernelOutcome::SceneGenerationPublished(publication),
        ));
    }
    let generation_id = publisher.next_generation()?;
    let compiled = compile_graph_generation(
        NativeSceneCompilerInput {
            generation_id,
            registry_revision: authority.registry_revision,
            document: &lease,
            registry: &registry,
            verified_anchors: Some(&anchors),
            nli: Some(&nli),
            palette,
        },
        &reviewed_generation,
    )?;
    let anchors = Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        Some(GraphGeneration(generation_id)),
        AnchorSource::ResidentGraph,
        &lease.content,
        compiled.anchors,
    )?);
    scene_publication::publish_full_scene(
        shared,
        sequence,
        NativeScenePublishCommand::reviewed(
            compiled.publication,
            anchors,
            compiled.receipt,
            run_id,
            reviewed_generation,
        ),
    )
}

fn decision_binding(
    shared: &KernelShared,
    candidate_id: AtlasCandidateId,
) -> Result<(AtlasDecisionAuthority, [u8; 32], [u8; 32]), KernelError> {
    let state = read_state(shared)?;
    let binding = current_candidate_binding(&state, candidate_id)?;
    Ok((
        binding.authority,
        binding.candidate_hash,
        binding.evidence_hash,
    ))
}

pub(super) fn current_candidate_binding(
    state: &super::KernelState,
    candidate_id: AtlasCandidateId,
) -> Result<CurrentCandidateBinding, KernelError> {
    let authority = current_authority(state)?;
    let nli = state
        .nli_analysis
        .as_deref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let coordinator = state
        .producer_coordinator
        .as_deref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let candidate = nli
        .nli_candidates
        .iter()
        .find(|candidate| candidate.candidate_id == candidate_id.0)
        .ok_or(AtlasReviewError::UnknownCandidate)?;
    let adjudication = nli
        .nli_adjudications
        .iter()
        .find(|adjudication| adjudication.candidate_id == candidate_id.0)
        .ok_or(AtlasReviewError::UnknownCandidate)?;
    let evidence = coordinator
        .evidence_bindings
        .iter()
        .find(|binding| binding.candidate_id == candidate_id.0)
        .ok_or(AtlasReviewError::UnknownCandidate)?;
    let candidate_hash = hash_postcard(&(candidate, adjudication))?;
    let evidence_hash = hash_postcard(evidence)?;
    Ok(CurrentCandidateBinding {
        authority,
        candidate_hash,
        evidence_hash,
        left_entity_id: candidate.left_entity_id,
        right_entity_id: candidate.right_entity_id,
        premise_start: evidence.premise_start,
        premise_end: evidence.premise_end,
    })
}

pub(super) fn current_authority(
    state: &super::KernelState,
) -> Result<AtlasDecisionAuthority, KernelError> {
    let lease = state
        .active_document_lease
        .as_deref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let receipt = state
        .analysis_publication
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let generation = state
        .graph_generation
        .as_deref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    generation.verify_binding(
        lease.entry_id.0,
        lease.revision.0,
        lease.content_hash.0,
        state.entity_registry.revision(),
    )?;
    if receipt.native_document_id != lease.entry_id.0
        || receipt.document_revision != lease.revision.0
        || receipt.registry_revision != state.entity_registry.revision()
        || receipt.analysis_generation != generation.header().analysis_generation
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    Ok(AtlasDecisionAuthority {
        document_id: lease.entry_id.0,
        document_revision: lease.revision.0,
        document_hash: lease.content_hash.0,
        registry_revision: receipt.registry_revision,
        producer_generation: receipt.analysis_generation,
        // The producer artifact remains stable while reviewed graph products advance.
        producer_graph_hash: receipt.analysis_artifact_hash,
    })
}

pub(super) fn refresh_review_overlay(
    state: &mut super::KernelState,
    ledger: &AtlasReviewLedger,
) -> Result<(), KernelError> {
    let revision = state.graph_review_overlay.revision.saturating_add(1);
    let generation_id = state
        .scene_publication
        .map(|publication| publication.generation_id);
    let Some(nli) = state.nli_analysis.as_deref() else {
        state.graph_review_overlay = GraphReviewOverlay {
            revision,
            generation_id,
            entries: Arc::from([]),
        };
        return Ok(());
    };
    let Some(coordinator) = state.producer_coordinator.as_deref() else {
        state.graph_review_overlay = GraphReviewOverlay {
            revision,
            generation_id,
            entries: Arc::from([]),
        };
        return Ok(());
    };
    let Some(index) = state.scene_product_index.as_deref() else {
        state.graph_review_overlay = GraphReviewOverlay {
            revision,
            generation_id,
            entries: Arc::from([]),
        };
        return Ok(());
    };
    let Ok(authority) = current_authority(state) else {
        state.graph_review_overlay = GraphReviewOverlay {
            revision,
            generation_id,
            entries: Arc::from([]),
        };
        return Ok(());
    };
    let effective = effective_bound_receipts(ledger, authority, nli, coordinator)?;
    let mut entries = Vec::with_capacity(effective.len());
    for receipt in effective {
        let promoted = state.graph_generation.as_deref().is_some_and(|generation| {
            generation.decisions().iter().any(|decision| {
                decision.candidate_id == receipt.candidate_id.0
                    && decision.status == DECISION_STATUS_ACCEPTED
            })
        });
        let edge_id = if promoted {
            promoted_edge_id(receipt.candidate_id.0)
        } else {
            proposed_nli_edge_id(authority.document_id, &receipt.candidate_id.0)
        };
        if !index.edges().iter().any(|edge| edge.edge_id == edge_id) {
            continue;
        }
        let review_mask = match receipt.resulting_status {
            Some(AtlasDecisionStatus::Accepted) => ReviewMask::ACCEPTED.0,
            Some(AtlasDecisionStatus::Rejected) => ReviewMask::REJECTED.0,
            Some(AtlasDecisionStatus::Deferred) => ReviewMask::PROPOSED.0,
            None => continue,
        };
        entries.push(GraphReviewOverride {
            edge_id,
            review_mask,
        });
    }
    entries.sort_unstable_by_key(|entry| entry.edge_id);
    state.graph_review_overlay = GraphReviewOverlay {
        revision,
        generation_id,
        entries: entries.into(),
    };
    Ok(())
}

fn effective_bound_receipts(
    ledger: &AtlasReviewLedger,
    authority: AtlasDecisionAuthority,
    nli: &phoenix_analysis_contract::PhoenixNliArtifactV1,
    coordinator: &phoenix_analysis_contract::PhoenixProducerCoordinatorV1,
) -> Result<Vec<AtlasDecisionReceiptV1>, KernelError> {
    let mut effective = Vec::with_capacity(nli.nli_candidates.len());
    for candidate in &nli.nli_candidates {
        let Some(adjudication) = nli
            .nli_adjudications
            .iter()
            .find(|item| item.candidate_id == candidate.candidate_id)
        else {
            continue;
        };
        let Some(evidence) = coordinator
            .evidence_bindings
            .iter()
            .find(|item| item.candidate_id == candidate.candidate_id)
        else {
            continue;
        };
        let candidate_hash = hash_postcard(&(candidate, adjudication))?;
        let evidence_hash = hash_postcard(evidence)?;
        let candidate_id = AtlasCandidateId(candidate.candidate_id);
        let Some(receipt) = ledger.head_for_binding(candidate_id, candidate_hash, evidence_hash)
        else {
            continue;
        };
        if receipt.resulting_status.is_some()
            && authority_can_preserve(receipt.authority, authority)
        {
            effective.push(receipt.clone());
        }
    }
    effective.sort_unstable_by_key(|receipt| receipt.candidate_id);
    Ok(effective)
}

fn materialize_reviewed_generation(
    lease: &phoenix_workspace::DocumentLease,
    registry: &phoenix_workspace::EntityRegistry,
    analysis: &PhoenixDocumentAnalysisV1,
    structural: &phoenix_analysis_contract::PhoenixStructuralSubstrateV1,
    source: &VerifiedGraphGeneration,
    effective: &[AtlasDecisionReceiptV1],
) -> Result<Arc<VerifiedGraphGeneration>, KernelError> {
    let canonical_entities = registry
        .entities()
        .iter()
        .map(|entity| {
            let manual_mentions = registry
                .mentions()
                .iter()
                .filter(|mention| mention.active && mention.entity_id == entity.id)
                .count()
                .min(u32::MAX as usize) as u32;
            CanonicalEntityInput {
                id: entity.id,
                label: &entity.label,
                custom_kind: entity.custom_kind.as_deref(),
                mention_count: entity.ner_mention_count.saturating_add(manual_mentions),
                kind: entity.kind as u16,
                source_mask: u16::from(entity.sources.ner)
                    | (u16::from(entity.sources.user_tagged) << 1),
            }
        })
        .collect::<Vec<_>>();
    let decisions = effective
        .iter()
        .map(|receipt| DurableDecisionInput {
            id: stable_u64(b"decision", &receipt.receipt_id),
            candidate_id: receipt.candidate_id.0,
            reason: &receipt.reason,
            decided_at_revision: receipt.authority.document_revision,
            status: decision_status(receipt.resulting_status),
            flags: DECISION_FLAG_DURABLE_RECEIPT,
        })
        .collect::<Vec<_>>();
    let candidates = analysis
        .nli
        .nli_candidates
        .iter()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect::<HashMap<_, _>>();
    let adjudications = analysis
        .nli
        .nli_adjudications
        .iter()
        .map(|adjudication| (adjudication.candidate_id, adjudication))
        .collect::<HashMap<_, _>>();
    let accepted_edges = effective
        .iter()
        .filter(|receipt| receipt.resulting_status == Some(AtlasDecisionStatus::Accepted))
        .map(|receipt| {
            let candidate = candidates
                .get(&receipt.candidate_id.0)
                .ok_or(AtlasReviewError::UnknownCandidate)?;
            let adjudication = adjudications
                .get(&receipt.candidate_id.0)
                .ok_or(AtlasReviewError::UnknownCandidate)?;
            Ok(AcceptedEdgeInput {
                id: promoted_edge_id(receipt.candidate_id.0),
                source_id: candidate.left_entity_id,
                target_id: candidate.right_entity_id,
                evidence_id: 0,
                weight: (adjudication.confidence_millis as f32 / 1000.0).clamp(0.56, 1.0),
                relation: candidate_relation(candidate.kind),
                flags: ACCEPTED_EDGE_FLAG_PROMOTED,
            })
        })
        .collect::<Result<Vec<_>, AtlasReviewError>>()?;
    let owned_capabilities = source
        .capabilities()
        .iter()
        .map(|capability| {
            Ok((
                source.string(capability.name)?.to_owned(),
                source.string(capability.producer)?.to_owned(),
                capability.supported != 0,
                capability.emitted != 0,
                capability.flags,
            ))
        })
        .collect::<Result<Vec<_>, phoenix_graph_generation::GraphGenerationError>>()?;
    let capabilities = owned_capabilities
        .iter()
        .map(
            |(name, producer, supported, emitted, flags)| ProducerCapabilityInput {
                name,
                producer,
                supported: *supported,
                emitted: *emitted,
                flags: *flags,
            },
        )
        .collect::<Vec<_>>();
    let review_hash = hash_postcard(
        &effective
            .iter()
            .map(|receipt| receipt.receipt_id)
            .collect::<Vec<_>>(),
    )?;
    let path = source
        .path()
        .parent()
        .ok_or(AtlasReviewError::MissingAuthorityRoot)?
        .join(format!(
            "review-{}-{}.{}",
            source.header().analysis_generation,
            short_hash(review_hash),
            GRAPH_GENERATION_EXTENSION
        ));
    if !path.exists() {
        write_graph_generation_new(
            &path,
            &GraphGenerationInput {
                text: &lease.content,
                analysis,
                structural,
                canonical_entities: &canonical_entities,
                accepted_edges: &accepted_edges,
                decisions: &decisions,
                capabilities: &capabilities,
            },
        )?;
    }
    Ok(Arc::new(VerifiedGraphGeneration::open(path)?))
}

fn decision_status(status: Option<AtlasDecisionStatus>) -> u16 {
    match status {
        Some(AtlasDecisionStatus::Accepted) => DECISION_STATUS_ACCEPTED,
        Some(AtlasDecisionStatus::Rejected) => DECISION_STATUS_REJECTED,
        Some(AtlasDecisionStatus::Deferred) => DECISION_STATUS_DEFERRED,
        None => unreachable!("effective review list excludes undone decisions"),
    }
}

const fn candidate_relation(kind: NliCandidateKind) -> u16 {
    match kind {
        NliCandidateKind::SameSurface => 1,
        NliCandidateKind::Alias => 2,
        NliCandidateKind::Coreference => 3,
        NliCandidateKind::Related => 4,
    }
}

fn stable_u64(domain: &[u8], value: &[u8]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.native.review-authority/v1\0");
    hasher.update(domain);
    hasher.update(&[0]);
    hasher.update(value);
    let mut raw = [0; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

fn hash_postcard<T: Serialize>(value: &T) -> Result<[u8; 32], AtlasReviewError> {
    let bytes =
        postcard::to_allocvec(value).map_err(|error| AtlasReviewError::Codec(error.to_string()))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn receipt_id(receipt: &AtlasDecisionReceiptV1) -> Result<[u8; 32], AtlasReviewError> {
    let mut canonical = receipt.clone();
    canonical.receipt_id = [0; 32];
    let bytes = postcard::to_allocvec(&canonical)
        .map_err(|error| AtlasReviewError::Codec(error.to_string()))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn persist_receipt(
    directory: &Path,
    receipt: &AtlasDecisionReceiptV1,
) -> Result<(), AtlasReviewError> {
    receipt.validate()?;
    std::fs::create_dir_all(directory).map_err(|source| AtlasReviewError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    let final_path = directory.join(format!(
        "decision-{:020}-{}.{}",
        receipt.sequence,
        short_hash(receipt.receipt_id),
        RECEIPT_EXTENSION
    ));
    let payload = postcard::to_allocvec(receipt)
        .map_err(|error| AtlasReviewError::Codec(error.to_string()))?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(AtlasReviewError::OversizedReason);
    }
    let mut header = [0_u8; HEADER_LEN];
    header[..8].copy_from_slice(&MAGIC);
    header[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    header[24..56].copy_from_slice(blake3::hash(&payload).as_bytes());
    let pending = directory.join(format!(
        ".pending-{}-{}",
        std::process::id(),
        receipt.sequence
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|source| AtlasReviewError::Io {
            path: pending.clone(),
            source,
        })?;
    file.write_all(&header)
        .and_then(|_| file.write_all(&payload))
        .and_then(|_| file.sync_all())
        .map_err(|source| AtlasReviewError::Io {
            path: pending.clone(),
            source,
        })?;
    std::fs::rename(&pending, &final_path).map_err(|source| AtlasReviewError::Io {
        path: final_path,
        source,
    })
}

fn open_receipt(path: &Path) -> Result<AtlasDecisionReceiptV1, AtlasReviewError> {
    let file = File::open(path).map_err(|source| AtlasReviewError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // SAFETY: decision receipts are immutable after their atomic rename.
    let mmap = unsafe { Mmap::map(&file) }.map_err(|source| AtlasReviewError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if mmap.len() < HEADER_LEN || mmap[..8] != MAGIC {
        return Err(AtlasReviewError::InvalidHeader);
    }
    let version = read_u32(&mmap[8..12]);
    if version != FORMAT_VERSION {
        return Err(AtlasReviewError::UnsupportedVersion(version));
    }
    let payload_len =
        usize::try_from(read_u64(&mmap[16..24])).map_err(|_| AtlasReviewError::InvalidHeader)?;
    if read_u32(&mmap[12..16]) as usize != HEADER_LEN
        || payload_len > MAX_PAYLOAD_BYTES
        || mmap.len() != HEADER_LEN + payload_len
        || mmap[56..HEADER_LEN].iter().any(|byte| *byte != 0)
    {
        return Err(AtlasReviewError::InvalidHeader);
    }
    let payload = &mmap[HEADER_LEN..];
    if mmap[24..56] != *blake3::hash(payload).as_bytes() {
        return Err(AtlasReviewError::HashMismatch);
    }
    let receipt: AtlasDecisionReceiptV1 = postcard::from_bytes(payload)
        .map_err(|error| AtlasReviewError::Codec(error.to_string()))?;
    receipt.validate()?;
    Ok(receipt)
}

fn short_hash(hash: [u8; 32]) -> String {
    hash[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(bytes);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn decisions_are_idempotent_and_survive_restart() {
        let root = test_root("restart");
        let workspace = root.join("workspace.json");
        let authority = authority(7);
        let candidate_id = AtlasCandidateId([9; 32]);
        let command = AtlasDecisionCommand {
            candidate_id,
            action: AtlasDecisionAction::Accept,
            expected_receipt_id: None,
            reason: "evidence confirms this relation".into(),
        };
        let receipt = {
            let mut ledger = AtlasReviewLedger::open(&workspace).unwrap();
            let first = ledger
                .decide(&command, authority, [3; 32], [4; 32])
                .unwrap();
            let repeated = ledger
                .decide(&command, authority, [3; 32], [4; 32])
                .unwrap();
            assert_eq!(first, repeated);
            assert_eq!(ledger.receipts.len(), 1);
            first
        };
        let restored = AtlasReviewLedger::open(&workspace).unwrap();
        assert_eq!(restored.receipts, vec![receipt]);
        assert_eq!(
            restored.effective(authority)[0].resulting_status,
            Some(AtlasDecisionStatus::Accepted)
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn undo_restores_the_prior_durable_status() {
        let root = test_root("undo");
        let workspace = root.join("workspace.json");
        let authority = authority(7);
        let candidate_id = AtlasCandidateId([9; 32]);
        let mut ledger = AtlasReviewLedger::open(&workspace).unwrap();
        let accepted = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Accept,
                    expected_receipt_id: None,
                    reason: String::new(),
                },
                authority,
                [3; 32],
                [4; 32],
            )
            .unwrap();
        let deferred = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Defer,
                    expected_receipt_id: Some(accepted.receipt_id),
                    reason: "wait for more evidence".into(),
                },
                authority,
                [3; 32],
                [4; 32],
            )
            .unwrap();
        let undone = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Undo,
                    expected_receipt_id: Some(deferred.receipt_id),
                    reason: "restore previous decision".into(),
                },
                authority,
                [3; 32],
                [4; 32],
            )
            .unwrap();
        assert_eq!(undone.resulting_status, Some(AtlasDecisionStatus::Accepted));
        assert!(matches!(
            ledger.decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Reject,
                    expected_receipt_id: Some(accepted.receipt_id),
                    reason: String::new(),
                },
                authority,
                [3; 32],
                [4; 32],
            ),
            Err(AtlasReviewError::StaleDecisionHead)
        ));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn changed_document_hash_rejects_an_existing_candidate_head() {
        let root = test_root("stale");
        let workspace = root.join("workspace.json");
        let candidate_id = AtlasCandidateId([9; 32]);
        let mut ledger = AtlasReviewLedger::open(&workspace).unwrap();
        let first = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Defer,
                    expected_receipt_id: None,
                    reason: String::new(),
                },
                authority(7),
                [3; 32],
                [4; 32],
            )
            .unwrap();
        assert!(matches!(
            ledger.decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Accept,
                    expected_receipt_id: Some(first.receipt_id),
                    reason: String::new(),
                },
                AtlasDecisionAuthority {
                    document_revision: 8,
                    document_hash: [8; 32],
                    ..authority(7)
                },
                [3; 32],
                [4; 32],
            ),
            Err(AtlasReviewError::StaleAuthority)
        ));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unchanged_evidence_binding_survives_a_revision_only_rerun() {
        let root = test_root("preserved");
        let workspace = root.join("workspace.json");
        let candidate_id = AtlasCandidateId([9; 32]);
        let mut ledger = AtlasReviewLedger::open(&workspace).unwrap();
        let accepted = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Accept,
                    expected_receipt_id: None,
                    reason: String::new(),
                },
                authority(7),
                [3; 32],
                [4; 32],
            )
            .unwrap();
        let deferred = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Defer,
                    expected_receipt_id: Some(accepted.receipt_id),
                    reason: String::new(),
                },
                authority(8),
                [3; 32],
                [4; 32],
            )
            .unwrap();
        assert_eq!(deferred.previous_receipt_id, Some(accepted.receipt_id));
        assert_eq!(
            deferred.resulting_status,
            Some(AtlasDecisionStatus::Deferred)
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn changed_evidence_starts_a_superseding_binding_chain() {
        let root = test_root("superseded");
        let workspace = root.join("workspace.json");
        let candidate_id = AtlasCandidateId([9; 32]);
        let mut ledger = AtlasReviewLedger::open(&workspace).unwrap();
        ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Accept,
                    expected_receipt_id: None,
                    reason: String::new(),
                },
                authority(7),
                [3; 32],
                [4; 32],
            )
            .unwrap();
        let replacement = ledger
            .decide(
                &AtlasDecisionCommand {
                    candidate_id,
                    action: AtlasDecisionAction::Reject,
                    expected_receipt_id: None,
                    reason: String::new(),
                },
                authority(8),
                [3; 32],
                [5; 32],
            )
            .unwrap();
        assert!(ledger.has_other_binding(candidate_id));
        assert_eq!(
            ledger
                .head_for_binding(candidate_id, [3; 32], [5; 32])
                .map(|receipt| receipt.receipt_id),
            Some(replacement.receipt_id)
        );
        std::fs::remove_dir_all(root).ok();
    }

    fn authority(document_revision: u64) -> AtlasDecisionAuthority {
        AtlasDecisionAuthority {
            document_id: 3,
            document_revision,
            document_hash: [1; 32],
            registry_revision: 5,
            producer_generation: 11,
            producer_graph_hash: [2; 32],
        }
    }

    fn test_root(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "phoenix-atlas-review-{label}-{}-{sequence}",
            std::process::id()
        ))
    }
}
