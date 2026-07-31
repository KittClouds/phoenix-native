use super::*;
use hashbrown::HashMap;
use memchr::memmem;
use phoenix_scene_contract::{AnchorCandidate, AnchorSource};
use phoenix_workspace::{commit_document, EntityRegistry, EntityTag};

const UNMAPPED_SOURCE_OFFSET: u32 = u32::MAX;
const SOURCE_ALIGNMENT_LOOKAHEAD: usize = 16 * 1024;
const SOURCE_ALIGNMENT_ANCHOR: usize = 16;

pub(super) fn registry_anchors_with_base(
    index: &entity_highlights::EntityHighlightIndex,
    registry: &EntityRegistry,
    lease: Option<&DocumentLease>,
    base: Option<&VerifiedDocumentAnchors>,
) -> Result<Option<Arc<VerifiedDocumentAnchors>>, KernelError> {
    let Some(lease) = lease else {
        return Ok(None);
    };
    if index.registry_revision() != registry.revision() {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
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
    let mut reserved = candidates
        .iter()
        .map(|candidate| (candidate.start, candidate.end))
        .collect::<Vec<_>>();
    reserved.sort_unstable();
    let before_registry_projection = candidates.len();
    index.append_matches(&lease.content, &reserved, &mut candidates)?;
    let projected_registry = candidates.len() > before_registry_projection;
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(lease.entry_id.0),
        lease.revision.0,
        lease.content_hash.0,
        None,
        if preserved_base || projected_registry {
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

fn rebind_verified_anchors(
    anchors: Option<&VerifiedDocumentAnchors>,
    previous: &DocumentLease,
    committed: &DocumentLease,
) -> Result<Option<VerifiedDocumentAnchors>, KernelError> {
    let Some(anchors) = anchors.filter(|anchors| {
        anchors.document() == DocumentId(previous.entry_id.0)
            && anchors.document_revision() == previous.revision.0
            && anchors.content_hash() == previous.content_hash.0
            && previous.entry_id == committed.entry_id
    }) else {
        return Ok(None);
    };
    let alignment = align_source_offsets(previous.content.as_bytes(), committed.content.as_bytes());
    let mut candidates = Vec::with_capacity(anchors.anchors().len());
    for anchor in anchors.anchors() {
        let old_start =
            usize::try_from(anchor.start).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let old_end =
            usize::try_from(anchor.end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
        let surface = previous
            .content
            .get(old_start..old_end)
            .ok_or(KernelError::AnalysisAuthorityMismatch)?;
        let Some((start, end)) = aligned_range(old_start, old_end, &alignment) else {
            continue;
        };
        if committed.content.get(start..end) != Some(surface) {
            continue;
        }
        candidates.push(AnchorCandidate {
            start: u32::try_from(start).map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
            end: u32::try_from(end).map_err(|_| KernelError::AnalysisAuthorityMismatch)?,
            node_id: anchor.node_id,
            entity_slot: anchor.entity_slot,
            family: anchor.family,
            surface: surface.to_owned(),
        });
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(VerifiedDocumentAnchors::verify(
        DocumentId(committed.entry_id.0),
        committed.revision.0,
        committed.content_hash.0,
        anchors.graph_generation(),
        AnchorSource::CanonicalRegistry,
        &committed.content,
        candidates,
    )?))
}

fn aligned_range(start: usize, end: usize, alignment: &[u32]) -> Option<(usize, usize)> {
    let start = *alignment.get(start)?;
    let end = *alignment.get(end)?;
    if start == UNMAPPED_SOURCE_OFFSET || end == UNMAPPED_SOURCE_OFFSET {
        return None;
    }
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    (start < end).then_some((start, end))
}

fn align_source_offsets(authoritative: &[u8], edited: &[u8]) -> Vec<u32> {
    let mut offsets = vec![UNMAPPED_SOURCE_OFFSET; authoritative.len().saturating_add(1)];
    let mut source_index = 0usize;
    let mut edited_index = 0usize;
    offsets[0] = 0;
    while source_index < authoritative.len() && edited_index < edited.len() {
        if authoritative[source_index] == edited[edited_index] {
            source_index += 1;
            edited_index += 1;
            offsets[source_index] = u32::try_from(edited_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            continue;
        }
        let edited_skip =
            find_alignment_anchor(&edited[edited_index..], &authoritative[source_index..]);
        let source_skip =
            find_alignment_anchor(&authoritative[source_index..], &edited[edited_index..]);
        match (edited_skip, source_skip) {
            (Some(inserted), Some(deleted)) if inserted <= deleted => {
                edited_index += inserted;
                offsets[source_index] =
                    u32::try_from(edited_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (Some(inserted), None) => {
                edited_index += inserted;
                offsets[source_index] =
                    u32::try_from(edited_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (_, Some(deleted)) => {
                source_index += deleted;
                offsets[source_index] =
                    u32::try_from(edited_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (None, None) => {
                source_index += 1;
                edited_index += 1;
                offsets[source_index] =
                    u32::try_from(edited_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
        }
    }
    if source_index == authoritative.len() {
        offsets[source_index] = u32::try_from(edited.len()).unwrap_or(UNMAPPED_SOURCE_OFFSET);
    }
    offsets
}

fn find_alignment_anchor(haystack: &[u8], authority: &[u8]) -> Option<usize> {
    let anchor_len = SOURCE_ALIGNMENT_ANCHOR.min(authority.len());
    if anchor_len == 0 {
        return None;
    }
    let search_len = haystack
        .len()
        .min(SOURCE_ALIGNMENT_LOOKAHEAD.saturating_add(anchor_len));
    memmem::find(&haystack[..search_len], &authority[..anchor_len]).filter(|offset| *offset > 0)
}

pub(super) fn save_document(
    shared: &KernelShared,
    sequence: u64,
    lease: DocumentLeaseToken,
    content: Arc<str>,
) -> Result<CommandReceipt, KernelError> {
    let (workspace, active_lease, mut registry, highlight_index, previous_anchors) = {
        let state = read_state(shared)?;
        (
            Arc::clone(&state.workspace),
            state.active_document_lease.as_ref().map(Arc::clone),
            (*state.entity_registry).clone(),
            Arc::clone(&state.entity_highlights),
            state.document_anchors.as_ref().map(Arc::clone),
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
    let highlight_index = if registry_changed {
        entity_highlights::EntityHighlightIndex::build(&registry)?
    } else {
        highlight_index
    };
    let rebound = rebind_verified_anchors(previous_anchors.as_deref(), &active_lease, &committed)?;
    let anchors = registry_anchors_with_base(
        &highlight_index,
        &registry,
        Some(&committed),
        rebound.as_ref(),
    )?;
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
        Some((registry, atlas, highlight_index)),
        anchors,
        published,
    )?;
    shared
        .resident_memory
        .mark_document_pending(committed.entry_id.0)?;
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
        Arc::clone(&active_lease)
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
    let highlight_index = entity_highlights::EntityHighlightIndex::build(&registry)?;
    let rebound = if content_changed {
        rebind_verified_anchors(previous_anchors.as_deref(), &active_lease, &committed)?
    } else {
        previous_anchors.as_deref().cloned()
    };
    let anchors = registry_anchors_with_base(
        &highlight_index,
        &registry,
        Some(&committed),
        rebound.as_ref(),
    )?;
    let atlas = Arc::new(AtlasRegistry::from_registry(&registry));
    let palette = *read_state(shared)?.highlight_palette;
    let published = scene_publication::refresh_registry_scene(shared, &atlas, palette)?;
    let (revision, scene_publication) = install_committed_document(
        shared,
        Arc::clone(&committed),
        Some((registry, atlas, highlight_index)),
        anchors,
        published,
    )?;
    if content_changed {
        shared
            .resident_memory
            .mark_document_pending(committed.entry_id.0)?;
    }
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
    registry: Option<(
        Arc<EntityRegistry>,
        Arc<AtlasRegistry>,
        Arc<entity_highlights::EntityHighlightIndex>,
    )>,
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
    if let Some((registry, atlas, highlight_index)) = registry {
        state.atlas_registry = atlas;
        state.entity_registry = registry;
        state.entity_highlights = highlight_index;
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
