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
        entity_id,
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
