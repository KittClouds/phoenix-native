use super::*;

pub(super) fn set_manifold(
    shared: &KernelShared,
    sequence: u64,
    manifold: Manifold,
) -> Result<CommandReceipt, KernelError> {
    let mut state = write_state(shared)?;
    state.graph_view.manifold = manifold;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::ManifoldChanged(manifold),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

pub(super) fn set_graph_view(
    shared: &KernelShared,
    sequence: u64,
    view: GraphViewState,
) -> Result<CommandReceipt, KernelError> {
    let mut state = write_state(shared)?;
    if view.authority != state.graph_view.authority {
        return Err(KernelError::StaleGraphViewAuthority);
    }
    if state.scene_product_index.is_none() && !view.is_unfiltered() {
        return Err(KernelError::ProductIndexRequiredForFilteredView);
    }
    state.graph_view = view;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::GraphViewChanged(view),
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

pub(super) fn set_style(
    shared: &KernelShared,
    sequence: u64,
    style: StyleState,
) -> Result<CommandReceipt, KernelError> {
    if !style.node_scale.is_finite() || style.node_scale <= 0.0 || style.revision == 0 {
        return Err(KernelError::InvalidStyle);
    }
    let mut state = write_state(shared)?;
    state.style = style;
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::StyleChanged {
                revision: style.revision,
            },
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}
