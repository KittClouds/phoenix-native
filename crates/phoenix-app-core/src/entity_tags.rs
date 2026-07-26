use super::*;
use phoenix_scene_contract::{AnchorCandidate, AnchorSource};
use phoenix_workspace::{commit_document, EntityRegistry, EntityTag};

pub(super) fn registry_anchors(
    registry: &EntityRegistry,
    lease: Option<&DocumentLease>,
) -> Result<Option<Arc<VerifiedDocumentAnchors>>, KernelError> {
    let Some(lease) = lease else {
        return Ok(None);
    };
    let candidates = registry
        .active_mentions_for(lease)
        .map(|(mention, entity)| AnchorCandidate {
            start: mention.start,
            end: mention.end,
            node_id: entity.id,
            entity_slot: entity.id as u32,
            family: entity.kind.family(),
            surface: mention.surface.clone(),
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        None,
        AnchorSource::ManualRegistry,
        &lease.content,
        candidates,
    )?)))
}

pub(super) fn save_document(
    shared: &KernelShared,
    sequence: u64,
    lease: DocumentLeaseToken,
    content: Arc<str>,
) -> Result<CommandReceipt, KernelError> {
    let (workspace, active_lease, mut registry) = {
        let state = read_state(shared)?;
        (
            Arc::clone(&state.workspace),
            state.active_document_lease.as_ref().map(Arc::clone),
            (*state.entity_registry).clone(),
        )
    };
    let active_lease = active_lease.ok_or(KernelError::DocumentLeaseNotActive)?;
    if active_lease.token() != lease {
        return Err(KernelError::DocumentLeaseNotActive);
    }
    let committed = Arc::new(commit_document(
        &shared.workspace_path,
        &workspace,
        lease,
        &content,
    )?);
    let registry_changed = registry.reanchor_document(&committed)?;
    if registry_changed {
        if let Err(error) = registry.save_atomic(&shared.workspace_path) {
            install_committed_document(shared, Arc::clone(&committed), None, None, None)?;
            return Err(error.into());
        }
    }
    let registry = Arc::new(registry);
    let anchors = registry_anchors(&registry, Some(&committed))?;
    let atlas = Arc::new(AtlasRegistry::from_registry(&registry));
    let palette = *read_state(shared)?.highlight_palette;
    let published = if registry_changed {
        scene_publication::refresh_registry_scene(shared, &atlas, palette)?
    } else {
        None
    };
    let (revision, scene_publication) = install_committed_document(
        shared,
        Arc::clone(&committed),
        Some((registry, atlas)),
        anchors,
        published,
    )?;
    let document = DocumentId(committed.entry_id.0);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::DocumentCommitted {
                document,
                revision: committed.revision,
                content_hash: committed.content_hash,
                scene_publication,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::DocumentSaved(committed.revision),
    ))
}

pub(super) fn tag_selection(
    shared: &KernelShared,
    sequence: u64,
    lease: DocumentLeaseToken,
    content: Arc<str>,
    tag: EntityTag,
) -> Result<CommandReceipt, KernelError> {
    let (workspace, active_lease, mut registry) = {
        let state = read_state(shared)?;
        (
            Arc::clone(&state.workspace),
            state.active_document_lease.as_ref().map(Arc::clone),
            (*state.entity_registry).clone(),
        )
    };
    let active_lease = active_lease.ok_or(KernelError::DocumentLeaseNotActive)?;
    if active_lease.token() != lease {
        return Err(KernelError::DocumentLeaseNotActive);
    }

    let content_changed = active_lease.content.as_ref() != content.as_ref();
    let prospective = if content_changed {
        DocumentLease {
            entry_id: lease.entry_id,
            revision: DocumentRevision(
                lease
                    .revision
                    .0
                    .checked_add(1)
                    .ok_or(WorkspaceError::EntityRegistryRevisionExhausted)?,
            ),
            content_hash: ContentHash::of(content.as_bytes()),
            content: Arc::clone(&content),
        }
    } else {
        (*active_lease).clone()
    };
    registry.reanchor_document(&prospective)?;
    let result = registry.tag(&prospective, tag)?;

    let committed = if content_changed {
        Arc::new(commit_document(
            &shared.workspace_path,
            &workspace,
            lease,
            &content,
        )?)
    } else {
        active_lease
    };
    if committed.token() != prospective.token() {
        install_committed_document(shared, Arc::clone(&committed), None, None, None)?;
        return Err(KernelError::DocumentLeaseNotActive);
    }
    if let Err(error) = registry.save_atomic(&shared.workspace_path) {
        install_committed_document(shared, Arc::clone(&committed), None, None, None)?;
        return Err(error.into());
    }

    let registry = Arc::new(registry);
    let anchors = registry_anchors(&registry, Some(&committed))?;
    let atlas = Arc::new(AtlasRegistry::from_registry(&registry));
    let palette = *read_state(shared)?.highlight_palette;
    let published = scene_publication::refresh_registry_scene(shared, &atlas, palette)?;
    let (revision, scene_publication) = install_committed_document(
        shared,
        Arc::clone(&committed),
        Some((registry, atlas)),
        anchors,
        published,
    )?;
    let document = DocumentId(committed.entry_id.0);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::EntityRegistryCommitted {
                document,
                entity_id: result.entity_id,
                registry_revision: result.registry_revision,
                scene_publication,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::EntityTagged(result),
    ))
}

fn install_committed_document(
    shared: &KernelShared,
    committed: Arc<DocumentLease>,
    registry: Option<(Arc<EntityRegistry>, Arc<AtlasRegistry>)>,
    anchors: Option<Arc<VerifiedDocumentAnchors>>,
    published: Option<phoenix_scene_publisher::PublishedScene>,
) -> Result<(u64, Option<ScenePublicationReceipt>), KernelError> {
    let mut state = write_state(shared)?;
    state.active_document_lease = Some(committed);
    if let Some((registry, atlas)) = registry {
        state.atlas_registry = atlas;
        state.entity_registry = registry;
        state.document_anchors = anchors;
    } else {
        state.document_anchors = None;
    }
    let scene_publication = published
        .map(|published| scene_publication::install_published_scene_state(&mut state, published))
        .transpose()?;
    state.revision = checked_revision(state.revision)?;
    Ok((state.revision, scene_publication))
}

#[cfg(test)]
mod tests {
    use phoenix_scene_contract::EntityKind;

    #[test]
    fn builtin_kinds_keep_the_old_toolbar_semantics() {
        assert_eq!(
            EntityKind::Npc.family(),
            phoenix_scene_contract::EntityFamily::Character
        );
        assert_eq!(
            EntityKind::Faction.family(),
            phoenix_scene_contract::EntityFamily::Organization
        );
    }
}
