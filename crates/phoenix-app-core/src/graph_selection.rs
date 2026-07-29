use super::*;
use phoenix_scene_product_index::{EntityId, NodeId};

pub(super) fn set_selection(
    shared: &KernelShared,
    sequence: u64,
    command: GraphSelectionCommand,
) -> Result<CommandReceipt, KernelError> {
    let mut state = write_state(shared)?;
    let (node_id, entity_id, origin) = match command {
        GraphSelectionCommand::AtlasEntity(entity_id) => {
            let index = state
                .scene_product_index
                .as_ref()
                .ok_or(KernelError::AtlasEntityNotMapped(entity_id))?;
            let node = index
                .node_for_entity(EntityId(entity_id))
                .ok_or(KernelError::AtlasEntityNotMapped(entity_id))?;
            (Some(node.0), Some(entity_id), GraphSelectionOrigin::Atlas)
        }
        GraphSelectionCommand::GraphNode(node_id) => {
            let index = state
                .scene_product_index
                .as_ref()
                .ok_or(KernelError::GraphNodeNotFound(node_id))?;
            if !index.nodes().iter().any(|node| node.node_id == node_id) {
                return Err(KernelError::GraphNodeNotFound(node_id));
            }
            (
                Some(node_id),
                index
                    .entity_for_node(NodeId(node_id))
                    .map(|entity| entity.0),
                GraphSelectionOrigin::Renderer,
            )
        }
        GraphSelectionCommand::Clear => (None, None, GraphSelectionOrigin::None),
    };
    let selection = GraphSelectionState {
        revision: state.graph_selection.revision.saturating_add(1),
        node_id,
        secondary_node_id: None,
        entity_id,
        candidate_id: None,
        evidence: None,
        origin,
    };
    state.graph_selection = selection;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::GraphSelectionChanged(selection),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

pub(super) fn select_candidate(
    shared: &KernelShared,
    sequence: u64,
    candidate_id: AtlasCandidateId,
) -> Result<CommandReceipt, KernelError> {
    let mut state = write_state(shared)?;
    let binding = atlas_review::current_candidate_binding(&state, candidate_id)?;
    let index = state
        .scene_product_index
        .as_ref()
        .ok_or(KernelError::ProductIndexWithoutScene)?;
    let left = index
        .node_for_entity(EntityId(binding.left_entity_id))
        .ok_or(KernelError::AtlasEntityNotMapped(binding.left_entity_id))?;
    let right = index
        .node_for_entity(EntityId(binding.right_entity_id))
        .ok_or(KernelError::AtlasEntityNotMapped(binding.right_entity_id))?;
    let lease = state
        .active_document_lease
        .as_deref()
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let start = usize::try_from(binding.premise_start)
        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    let end =
        usize::try_from(binding.premise_end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    if start >= end
        || lease.content.get(start..end).is_none()
        || lease.entry_id.0 != binding.authority.document_id
        || lease.revision.0 != binding.authority.document_revision
        || lease.content_hash.0 != binding.authority.document_hash
    {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let selection = GraphSelectionState {
        revision: state.graph_selection.revision.saturating_add(1),
        node_id: Some(left.0),
        secondary_node_id: Some(right.0),
        entity_id: None,
        candidate_id: Some(candidate_id),
        evidence: Some(EditorEvidenceSelection {
            document_id: binding.authority.document_id,
            document_revision: binding.authority.document_revision,
            content_hash: binding.authority.document_hash,
            start: binding.premise_start,
            end: binding.premise_end,
            evidence_hash: binding.evidence_hash,
        }),
        origin: GraphSelectionOrigin::AtlasCandidate,
    };
    state.graph_selection = selection;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::GraphSelectionChanged(selection),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}
