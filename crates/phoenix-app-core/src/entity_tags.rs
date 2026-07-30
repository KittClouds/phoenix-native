use super::*;
use hashbrown::HashMap;
use phoenix_scene_contract::{AnchorCandidate, AnchorSource};
use phoenix_workspace::{commit_document, EntityRegistry, EntityTag};

pub(super) fn registry_anchors(
    registry: &EntityRegistry,
    lease: Option<&DocumentLease>,
) -> Result<Option<Arc<VerifiedDocumentAnchors>>, KernelError> {
    registry_anchors_with_base(registry, lease, None)
}

fn registry_anchors_with_base(
    registry: &EntityRegistry,
    lease: Option<&DocumentLease>,
    base: Option<&VerifiedDocumentAnchors>,
) -> Result<Option<Arc<VerifiedDocumentAnchors>>, KernelError> {
    let Some(lease) = lease else {
        return Ok(None);
    };
    let entity_slots = registry
        .entities()
        .iter()
        .enumerate()
        .map(|(slot, entity)| {
            let slot = u32::try_from(slot).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
            Ok((entity.id, (slot, entity.kind.family())))
        })
        .collect::<Result<HashMap<_, _>, KernelError>>()?;
    let manual_candidates = registry
        .active_mentions_for(lease)
        .map(|(mention, entity)| {
            Ok(AnchorCandidate {
                start: mention.start,
                end: mention.end,
                node_id: entity.id,
                entity_slot: entity_slots
                    .get(&entity.id)
                    .map(|(slot, _)| *slot)
                    .ok_or(KernelError::AnalysisAuthorityMismatch)?,
                family: entity.kind.family(),
                surface: mention.surface.clone(),
            })
        })
        .collect::<Result<Vec<_>, KernelError>>()?;
    let base = base.filter(|anchors| {
        anchors.document() == DocumentId(lease.entry_id.0)
            && anchors.document_revision() == lease.revision.0
            && anchors.content_hash() == lease.content_hash.0
    });
    let capacity = base
        .map_or(0, |anchors| anchors.anchors().len())
        .checked_add(manual_candidates.len())
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let mut candidates = Vec::with_capacity(capacity);
    if let Some(base) = base {
        for anchor in base.anchors() {
            if manual_candidates
                .iter()
                .any(|manual| ranges_overlap(anchor.start, anchor.end, manual.start, manual.end))
            {
                continue;
            }
            let Some((slot, family)) = entity_slots.get(&anchor.node_id).copied() else {
                continue;
            };
            let start = usize::try_from(anchor.start)
                .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
            let end =
                usize::try_from(anchor.end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
            let surface = lease
                .content
                .get(start..end)
                .ok_or(KernelError::AnalysisAuthorityMismatch)?;
            candidates.push(AnchorCandidate {
                start: anchor.start,
                end: anchor.end,
                node_id: anchor.node_id,
                entity_slot: slot,
                family,
                surface: surface.to_owned(),
            });
        }
    }
    let preserved_base = !candidates.is_empty();
    candidates.extend(manual_candidates);
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        None,
        if preserved_base {
            AnchorSource::CanonicalRegistry
        } else {
            AnchorSource::ManualRegistry
        },
        &lease.content,
        candidates,
    )?)))
}

const fn ranges_overlap(left_start: u32, left_end: u32, right_start: u32, right_end: u32) -> bool {
    left_start < right_end && right_start < left_end
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
    let (workspace, active_lease, mut registry, previous_anchors) = {
        let state = read_state(shared)?;
        (
            Arc::clone(&state.workspace),
            state.active_document_lease.as_ref().map(Arc::clone),
            (*state.entity_registry).clone(),
            state.document_anchors.as_ref().map(Arc::clone),
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
    let anchors = registry_anchors_with_base(
        &registry,
        Some(&committed),
        if content_changed {
            None
        } else {
            previous_anchors.as_deref()
        },
    )?;
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
    state.document_analysis = None;
    state.structural_analysis = None;
    state.nli_analysis = None;
    state.producer_coordinator = None;
    state.graph_generation_v2 = None;
    state.review_catalog_v2 = None;
    state.analysis_publication = None;
    if let Some((registry, atlas)) = registry {
        state.atlas_registry = atlas;
        state.entity_registry = registry;
    }
    let scene_publication = published
        .map(|published| scene_publication::install_published_scene_state(&mut state, published))
        .transpose()?;
    state.document_anchors = anchors;
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
