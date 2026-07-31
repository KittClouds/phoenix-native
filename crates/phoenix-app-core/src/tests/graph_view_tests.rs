use super::*;
use phoenix_scene_contract::FamilyMask;

#[test]
fn graph_view_is_kernel_owned_and_preserves_resident_arrays(
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let scene = scene(71)?;
    let index = product_index(&scene)?;
    let kernel = PhoenixKernel::start_with_product_index(
        path.clone(),
        Some(Arc::clone(&scene)),
        Some(Arc::clone(&index)),
    )?;
    let initial = kernel.snapshot()?;
    assert!(Arc::ptr_eq(
        initial.resident_scene.as_ref().ok_or("scene missing")?,
        &scene
    ));
    assert!(Arc::ptr_eq(
        initial
            .scene_product_index
            .as_ref()
            .ok_or("product index missing")?,
        &index
    ));
    let mut view = initial.graph_view;
    view.surface = GraphSurface::Atlas;
    view.families = FamilyMask::FACTS;
    view.scope = GraphScope::Compare;
    view.reviews = ReviewMask::ACCEPTED;
    view.relations = RelationFamily::Causal.mask();
    kernel.execute(KernelCommand::SetGraphView(Box::new(view)))?;
    let updated = kernel.snapshot()?;
    assert_eq!(updated.graph_view, view);
    assert!(Arc::ptr_eq(
        updated.resident_scene.as_ref().ok_or("scene missing")?,
        &scene
    ));
    assert!(Arc::ptr_eq(
        updated
            .scene_product_index
            .as_ref()
            .ok_or("product index missing")?,
        &index
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn graph_view_rejects_missing_or_stale_product_authority() -> Result<(), Box<dyn std::error::Error>>
{
    let path = path();
    let scene = scene(72)?;
    let kernel = PhoenixKernel::start(path.clone(), Some(scene))?;
    let mut filtered = kernel.snapshot()?.graph_view;
    filtered.surface = GraphSurface::Atlas;
    filtered.families = FamilyMask::STRUCTURE;
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(filtered))),
        Err(KernelError::ProductIndexRequiredForFilteredView)
    ));
    let mut stale = kernel.snapshot()?.graph_view;
    stale.authority = SceneAuthority::Archive {
        generation: GraphGeneration(999),
        cohort_hash: [0; 32],
        product_index_hash: None,
    };
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(stale))),
        Err(KernelError::StaleGraphViewAuthority)
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn graph_controls_are_bounded_sequenced_kernel_commands() -> Result<(), Box<dyn std::error::Error>>
{
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let fit = kernel.execute(KernelCommand::DispatchGraphAction(GraphAction::Fit))?;
    let provenance = kernel.execute(KernelCommand::RequestGraphProvenance)?;
    assert!(fit.sequence < provenance.sequence);
    assert_eq!(
        fit.outcome,
        KernelOutcome::GraphActionQueued(GraphAction::Fit)
    );
    assert_eq!(provenance.outcome, KernelOutcome::GraphProvenance(None));
    let events = kernel.drain_events()?;
    assert!(events.iter().any(|event| {
        event.sequence == fit.sequence
            && event.kind == KernelEventKind::GraphActionRequested(GraphAction::Fit)
    }));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn graph_view_rejects_empty_visible_control_sets() -> Result<(), Box<dyn std::error::Error>> {
    let path = path();
    let kernel = PhoenixKernel::start(path.clone(), None)?;
    let mut view = kernel.snapshot()?.graph_view;
    view.families = FamilyMask(0);
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(view))),
        Err(KernelError::InvalidGraphView)
    ));
    let mut view = kernel.snapshot()?.graph_view;
    view.reviews = ReviewMask(0);
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(view))),
        Err(KernelError::InvalidGraphView)
    ));
    let mut view = kernel.snapshot()?.graph_view;
    view.relations = phoenix_scene_contract::RelationMask(0);
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphView(Box::new(view))),
        Err(KernelError::InvalidGraphView)
    ));
    kernel.shutdown()?;
    let parent = path.parent().ok_or("test path has no parent")?;
    let _ = std::fs::remove_dir_all(parent);
    Ok(())
}
