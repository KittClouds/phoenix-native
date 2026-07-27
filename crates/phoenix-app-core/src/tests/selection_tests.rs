use super::*;

#[test]
fn palette_command_is_atomic_and_restart_durable() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = path();
    let kernel = PhoenixKernel::start(workspace.clone(), None)?;
    let mut palette = HighlightPalette::default();
    palette.character.primary = [0.11, 0.42, 0.31, 1.0];
    kernel.execute(KernelCommand::SetHighlightPalette(Box::new(palette)))?;
    kernel.shutdown()?;
    drop(kernel);

    let reopened = PhoenixKernel::start(workspace.clone(), None)?;
    assert_eq!(*reopened.snapshot()?.highlight_palette, palette);
    reopened.shutdown()?;
    let _ = std::fs::remove_dir_all(workspace.parent().ok_or("missing workspace parent")?);
    Ok(())
}

#[test]
fn atlas_and_renderer_selection_use_only_verified_mapping() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = path();
    let kernel = PhoenixKernel::start_production(workspace.clone())?;
    let snapshot = kernel.snapshot()?;
    let generation = snapshot
        .scene_publication
        .map(|receipt| receipt.generation_id + 1)
        .unwrap_or(1);
    kernel.execute(KernelCommand::PublishNativeScene(Box::new(
        NativeScenePublishCommand::backend(full_publication(
            generation,
            snapshot.atlas_registry.registry_revision,
        )),
    )))?;

    kernel.execute(KernelCommand::SetGraphSelection(
        GraphSelectionCommand::AtlasEntity(9001),
    ))?;
    let selection = kernel.snapshot()?.graph_selection;
    assert_eq!(selection.node_id, Some(501));
    assert_eq!(selection.entity_id, Some(9001));
    assert_eq!(selection.origin, GraphSelectionOrigin::Atlas);

    kernel.execute(KernelCommand::SetGraphSelection(
        GraphSelectionCommand::GraphNode(501),
    ))?;
    assert_eq!(
        kernel.snapshot()?.graph_selection.origin,
        GraphSelectionOrigin::Renderer
    );
    assert!(matches!(
        kernel.execute(KernelCommand::SetGraphSelection(
            GraphSelectionCommand::AtlasEntity(77)
        )),
        Err(KernelError::AtlasEntityNotMapped(77))
    ));
    kernel.shutdown()?;
    let _ = std::fs::remove_dir_all(workspace.parent().ok_or("missing workspace parent")?);
    Ok(())
}
