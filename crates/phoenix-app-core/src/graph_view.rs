use super::*;
use phoenix_scene_contract::SceneAuthority;
use phoenix_workspace::save_highlight_palette_atomic;

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
    if !view.is_valid() {
        return Err(KernelError::InvalidGraphView);
    }
    if state.scene_product_index.is_none()
        && view != state.graph_view
        && !matches!(view.authority, SceneAuthority::Unavailable)
    {
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

pub(super) fn set_highlight_palette(
    shared: &KernelShared,
    sequence: u64,
    palette: HighlightPalette,
) -> Result<CommandReceipt, KernelError> {
    palette.validate()?;
    save_highlight_palette_atomic(&shared.workspace_path, palette)?;
    let mut state = write_state(shared)?;
    state.highlight_palette = Arc::new(palette);
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::HighlightPaletteChanged,
        },
    )?;
    Ok(receipt(sequence, revision, KernelOutcome::StateChanged))
}

pub(super) fn dispatch_graph_action(
    shared: &KernelShared,
    sequence: u64,
    action: GraphAction,
) -> Result<CommandReceipt, KernelError> {
    let revision = read_state(shared)?.revision;
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::GraphActionRequested(action),
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::GraphActionQueued(action),
    ))
}

pub(super) fn request_graph_provenance(
    shared: &KernelShared,
    sequence: u64,
) -> Result<CommandReceipt, KernelError> {
    let state = read_state(shared)?;
    let revision = state.revision;
    let provenance = state.resident_scene.as_ref().map(|scene| {
        let inventory = scene.inventory();
        GraphProvenanceReceipt {
            source: scene.source(),
            generation_id: scene.generation().0,
            registry_revision: state.atlas_registry.registry_revision,
            node_count: inventory.node_count as u64,
            edge_count: inventory.edge_count as u64,
            cohort_hash: scene.archive_identity().cohort_hash,
            product_index_hash: state
                .scene_product_index
                .as_ref()
                .map(|index| index.header().index_hash),
        }
    });
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::GraphProvenanceRequested { provenance },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::GraphProvenance(provenance),
    ))
}
