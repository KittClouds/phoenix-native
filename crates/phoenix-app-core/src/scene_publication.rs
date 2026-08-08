use super::*;
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{EntityFamily, SceneSource};
use phoenix_scene_contract::{FamilyMask, GraphSurface, GraphViewState};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use phoenix_scene_publisher::{
    NativeScenePublication, PublishedScene, SceneEdgeProduct, SceneNodeProduct,
    ScenePublicationKind, ScenePublicationReceipt, ScenePublicationStore,
};

const REGISTRY_SCOPE: u64 = 1;
const REVIEW_ACCEPTED: u32 = 1;
const NO_REFERENCE: u32 = u32::MAX;

pub(super) fn configure_restored_graph_view(
    view: &mut GraphViewState,
    receipt: Option<ScenePublicationReceipt>,
) {
    if receipt.is_some_and(|receipt| receipt.kind == ScenePublicationKind::Full) {
        view.surface = GraphSurface::Atlas;
        view.families = FamilyMask::ALL;
    }
}

fn reconcile_published_graph_view(
    previous: GraphViewState,
    previous_was_full: bool,
    mut published: GraphViewState,
) -> GraphViewState {
    published.manifold = previous.manifold;
    published.canvas = previous.canvas;
    if previous_was_full {
        published.surface = previous.surface;
        published.families = previous.families;
        published.entity_families = previous.entity_families;
        published.topology_families = previous.topology_families;
        published.lens = previous.lens;
        published.scope = previous.scope;
        published.reviews = previous.reviews;
        published.relations = previous.relations;
    } else {
        published.surface = GraphSurface::Atlas;
        published.families = FamilyMask::ALL;
    }
    published
}

pub(super) fn initial_production_scene(
    publisher: &ScenePublicationStore,
    atlas: &AtlasRegistry,
    palette: HighlightPalette,
) -> Result<PublishedScene, KernelError> {
    if let Some(current) = publisher.open_current()? {
        if current.receipt.kind == ScenePublicationKind::Full {
            match scene_compiler_authority_v3::verify(publisher, current.receipt) {
                Ok(visual) => {
                    let legacy_source_hash =
                        scene_compiler_authority::verify(publisher, current.receipt)?;
                    if legacy_source_hash != visual.source_generation_hash {
                        return Err(KernelError::InvalidV3VisualAuthority);
                    }
                }
                // Historical full generations predate the V3 visual receipt.
                // They remain readable, but the next production publication
                // must write the V3 authority before it can become current.
                Err(KernelError::MissingV3VisualAuthority) => {
                    scene_compiler_authority::verify(publisher, current.receipt)?;
                }
                Err(error) => return Err(error),
            }
            return Ok(current);
        }
        if current.receipt.registry_revision == atlas.registry_revision {
            return Ok(current);
        }
        if current.receipt.registry_revision > atlas.registry_revision {
            return Err(KernelError::PublishedRegistryAhead {
                published: current.receipt.registry_revision,
                workspace: atlas.registry_revision,
            });
        }
    }
    publish_registry_scene(publisher, atlas, palette)
}

pub(super) fn refresh_registry_scene(
    shared: &KernelShared,
    atlas: &AtlasRegistry,
    palette: HighlightPalette,
) -> Result<Option<PublishedScene>, KernelError> {
    let publisher = match shared.publisher.as_ref() {
        Some(publisher) => publisher,
        None => return Ok(None),
    };
    // A registry refresh is a useful bootstrap only while no full graph has
    // ever been published. The resident scene may be deliberately withdrawn
    // when its restored compiler contract is stale, while the durable full
    // generation remains the rollback authority. In that state, attempting
    // to publish a registry-only generation would correctly be rejected by
    // the store and would abort the analysis pipeline before the replacement
    // full generation can be compiled.
    if publisher
        .open_current()?
        .is_some_and(|current| current.receipt.kind == ScenePublicationKind::Full)
    {
        return Ok(None);
    }
    if read_state(shared)?
        .resident_scene
        .as_ref()
        .is_some_and(|scene| scene.source() == SceneSource::Backend)
    {
        return Ok(None);
    }
    publish_registry_scene(publisher, atlas, palette).map(Some)
}

fn publish_registry_scene(
    publisher: &ScenePublicationStore,
    atlas: &AtlasRegistry,
    palette: HighlightPalette,
) -> Result<PublishedScene, KernelError> {
    let generation = publisher.next_generation()?;
    let publication = registry_publication(generation, atlas, palette)?;
    publisher.publish(publication).map_err(KernelError::from)
}

pub(super) fn publish_full_scene(
    shared: &KernelShared,
    sequence: u64,
    command: NativeScenePublishCommand,
) -> Result<CommandReceipt, KernelError> {
    validate_compiled_metadata(&command)?;
    let compile_receipt = command.compile_receipt;
    let visual_receipt = command.visual_receipt;
    let (publication_receipt, revision) = publish_and_install(
        shared,
        command.publication,
        command.anchors,
        command.source_generation_v2,
        command.review_catalog_v2,
        visual_receipt,
    )?;
    let (event, outcome) = if let Some(compile) = compile_receipt {
        let run_id = command
            .run_id
            .ok_or(KernelError::CompiledPublicationMismatch)?;
        let receipt = GraphRebuildReceipt {
            run_id,
            compile,
            publication: publication_receipt,
        };
        (
            KernelEventKind::GraphRebuilt { receipt },
            KernelOutcome::GraphRebuilt(receipt),
        )
    } else {
        (
            KernelEventKind::SceneGenerationPublished {
                receipt: publication_receipt,
            },
            KernelOutcome::SceneGenerationPublished(publication_receipt),
        )
    };
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: event,
        },
    )?;
    Ok(receipt(sequence, revision, outcome))
}

fn validate_compiled_metadata(command: &NativeScenePublishCommand) -> Result<(), KernelError> {
    let Some(compile) = command.compile_receipt else {
        #[cfg(not(test))]
        {
            return Err(KernelError::MissingV2CompilerAuthority);
        }
        #[cfg(test)]
        {
            if command.anchors.is_some()
                || command.visual_receipt.is_some()
                || command.run_id.is_some()
                || command.source_generation_v2.is_some()
                || command.review_catalog_v2.is_some()
            {
                return Err(KernelError::CompiledPublicationMismatch);
            }
            return Ok(());
        }
    };
    let generation = command.publication.generation_id;
    let visual = command
        .visual_receipt
        .as_ref()
        .ok_or(KernelError::MissingV3VisualAuthority)?;
    if visual.scene_generation_id != generation
        || visual.node_count != command.publication.identities.len() as u64
        || visual.edge_count != command.publication.edges.len() as u64
    {
        return Err(KernelError::CompiledPublicationMismatch);
    }
    if command.run_id.is_none_or(|run_id| run_id == 0) {
        return Err(KernelError::CompiledPublicationMismatch);
    }
    let anchors = command
        .anchors
        .as_ref()
        .ok_or(KernelError::CompiledPublicationMismatch)?;
    if compile.scene_generation_id != generation
        || compile.registry_revision != command.publication.registry_revision
        || command.publication.document_id != Some(compile.document_id)
        || anchors.document() != DocumentId(compile.document_id)
        || anchors.document_revision() != compile.document_revision
        || anchors.content_hash() != compile.content_hash
        || anchors.graph_generation() != Some(GraphGeneration(generation))
    {
        return Err(KernelError::CompiledPublicationMismatch);
    }
    let source = command
        .source_generation_v2
        .as_ref()
        .ok_or(KernelError::CompiledPublicationMismatch)?;
    let catalog = command
        .review_catalog_v2
        .as_ref()
        .ok_or(KernelError::CompiledPublicationMismatch)?;
    let catalog_authority = catalog.authority();
    if source.header().generation_hash != compile.source_generation_hash
        || source.header().native_document_id != compile.document_id
        || source.header().document_revision != compile.document_revision
        || source.header().content_hash != compile.content_hash
        || source.header().registry_revision != compile.registry_revision
        || catalog_authority.document_hash != compile.content_hash
        || catalog_authority.native_document_id != compile.document_id
        || catalog_authority.document_revision != compile.document_revision
        || catalog_authority.registry_revision != compile.registry_revision
        || catalog_authority.producer_generation != source.header().producer_generation
    {
        return Err(KernelError::CompiledPublicationMismatch);
    }
    Ok(())
}

pub(super) fn install_published_scene_state(
    state: &mut KernelState,
    published: PublishedScene,
) -> Result<ScenePublicationReceipt, KernelError> {
    let previous_view = state.graph_view;
    let previous_was_full = state
        .scene_publication
        .is_some_and(|receipt| receipt.kind == ScenePublicationKind::Full);
    let published_view = published
        .scene
        .graph_view_state(Some(&published.product_index))?;
    let graph_view =
        reconcile_published_graph_view(previous_view, previous_was_full, published_view);
    let receipt = published.receipt;
    state.resident_scene = Some(published.scene);
    state.scene_product_index = Some(published.product_index);
    state.scene_publication = Some(receipt);
    state.graph_view = graph_view;
    state.graph_selection = GraphSelectionState {
        revision: state.graph_selection.revision.saturating_add(1),
        ..GraphSelectionState::default()
    };
    state.document_anchors = None;
    Ok(receipt)
}

fn publish_and_install(
    shared: &KernelShared,
    publication: NativeScenePublication,
    anchors: Option<Arc<VerifiedDocumentAnchors>>,
    source_generation_v2: Option<Arc<VerifiedGraphGenerationV2>>,
    review_catalog_v2: Option<Arc<ReviewCatalog>>,
    visual_receipt: Option<VisualContractDraftV3>,
) -> Result<(ScenePublicationReceipt, u64), KernelError> {
    if publication.kind != ScenePublicationKind::Full {
        return Err(KernelError::BackendPublicationMustBeFull);
    }
    let (publisher, registry_revision) = {
        let state = read_state(shared)?;
        if let Some(anchors) = anchors.as_ref() {
            let active_document = state
                .active_document
                .ok_or(KernelError::DocumentAnchorsNotActive)?;
            let active_lease = state
                .active_document_lease
                .as_ref()
                .ok_or(KernelError::DocumentAnchorsNotActive)?;
            if anchors.document() != active_document
                || anchors.document_revision() != active_lease.revision.0
                || anchors.content_hash() != active_lease.content_hash.0
            {
                return Err(KernelError::DocumentAnchorsNotActive);
            }
        }
        let publisher = shared
            .publisher
            .as_ref()
            .ok_or(KernelError::ProductionPublisherUnavailable)?;
        (
            Arc::clone(publisher),
            state.atlas_registry.registry_revision,
        )
    };
    if publication.registry_revision != registry_revision {
        return Err(KernelError::PublicationRegistryMismatch {
            publication: publication.registry_revision,
            current: registry_revision,
        });
    }
    let published = publisher.publish(publication)?;
    if let Some(source) = source_generation_v2.as_ref() {
        scene_compiler_authority::write_new(
            &publisher,
            published.receipt,
            source.header().generation_hash,
        )?;
        let visual = visual_receipt
            .ok_or(KernelError::MissingV3VisualAuthority)?
            .bind_publication(published.receipt, source.header().generation_hash)
            .map_err(|_| KernelError::CompiledPublicationMismatch)?;
        scene_compiler_authority_v3::write_new(&publisher, visual)?;
    } else if visual_receipt.is_some() {
        return Err(KernelError::CompiledPublicationMismatch);
    }
    let mut state = write_state(shared)?;
    if let Some(current) = state.scene_publication {
        if published.receipt.generation_id <= current.generation_id {
            return Err(KernelError::StaleGeneration {
                current: GraphGeneration(current.generation_id),
                incoming: GraphGeneration(published.receipt.generation_id),
            });
        }
    }
    let publication_receipt = install_published_scene_state(&mut state, published)?;
    state.document_anchors = anchors;
    if let (Some(generation), Some(catalog)) = (source_generation_v2, review_catalog_v2) {
        state.graph_generation_v2 = Some(generation);
        state.review_catalog_v2 = Some(catalog);
    }
    {
        let ledger = shared
            .atlas_review
            .lock()
            .map_err(|_| KernelError::Poisoned("Atlas review ledger"))?;
        atlas_review::refresh_review_overlay(&mut state, &ledger)?;
    }
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    Ok((publication_receipt, revision))
}

fn registry_publication(
    generation_id: u64,
    atlas: &AtlasRegistry,
    palette: HighlightPalette,
) -> Result<NativeScenePublication, KernelError> {
    palette.validate()?;
    let mut entities = atlas.entities.iter().collect::<Vec<_>>();
    entities.sort_unstable_by_key(|entity| entity.stable_id);
    let mut identities = Vec::with_capacity(entities.len());
    let mut styles = Vec::with_capacity(entities.len());
    let mut node_products = Vec::with_capacity(entities.len());
    let mut mappings = Vec::with_capacity(entities.len());
    let mut positions: [Vec<PositionRecord>; 6] =
        std::array::from_fn(|_| Vec::with_capacity(entities.len()));

    let entity_count = entities.len();
    for (ordinal, entity) in entities.into_iter().enumerate() {
        if entity.stable_id == 0 {
            return Err(KernelError::InvalidAtlasEntityIdentity);
        }
        let family = entity.kind.family();
        let node_id = entity.stable_id;
        identities.push(NodeIdentityRecord { id: node_id });
        styles.push(NodeStyleRecord {
            color: palette.for_family(family).primary,
            radius: 4.0,
            kind: entity.kind as u16,
            flags: source_flags(entity.sources),
        });
        node_products.push(SceneNodeProduct {
            node_id,
            family_mask: family_mask(family),
            scope_mask: REGISTRY_SCOPE,
            review_mask: REVIEW_ACCEPTED,
            label: Arc::clone(&entity.label),
            inspector_ref: NO_REFERENCE,
            provenance_ref: NO_REFERENCE,
        });
        mappings.push(EntityNodeMappingRecord {
            entity_id: entity.stable_id,
            node_id,
        });
        let manifold_positions = stable_positions(entity.stable_id, ordinal, entity_count);
        for (target, position) in positions.iter_mut().zip(manifold_positions) {
            target.push(PositionRecord { position });
        }
    }

    Ok(NativeScenePublication {
        generation_id,
        kind: ScenePublicationKind::RegistryOnly,
        registry_revision: atlas.registry_revision,
        document_id: None,
        identities,
        styles,
        topology: Vec::<TopologyRecord>::new(),
        edges: Vec::<EdgeRecord>::new(),
        positions,
        caps_guides: Vec::new(),
        node_products,
        edge_products: Vec::<SceneEdgeProduct>::new(),
        entity_mappings: mappings,
        references: Vec::<ProductReferenceRecord>::new(),
    })
}

fn source_flags(sources: EntitySourceMask) -> u16 {
    u16::from(sources.ner) | (u16::from(sources.user_tagged) << 1)
}

const fn family_mask(family: EntityFamily) -> u64 {
    FamilyMask::ENTITIES.0 | FamilyMask::entity_lane(family).0
}

fn stable_positions(stable_id: u64, ordinal: usize, count: usize) -> [[f32; 3]; 6] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-native-registry-position-v1");
    hasher.update(&stable_id.to_le_bytes());
    let digest = hasher.finalize();
    let bytes = digest.as_bytes();
    let x = unit_coordinate(&bytes[0..4]);
    let y = unit_coordinate(&bytes[4..8]);
    let z = unit_coordinate(&bytes[8..12]);
    let scale = 18.0;
    let base = [x * scale, y * scale, z * scale];
    let fiber_count = phoenix_hopf_space::fiber_count(count).max(1);
    let hopf = phoenix_hopf_space::fiber_point(
        phoenix_hopf_space::base_direction(ordinal % fiber_count, fiber_count),
        std::f32::consts::TAU * ((z + 1.0) * 0.5),
    );
    [
        base,
        [base[0] - base[2] * 0.25, base[1], base[2] + base[0] * 0.25],
        [base[0], base[1] - base[2] * 0.2, base[2] + base[1] * 0.2],
        [base[0] + base[1] * 0.15, base[1] - base[0] * 0.15, base[2]],
        [base[0] * 0.9, base[1] * 0.9, base[2] * 1.2],
        hopf,
    ]
}

fn unit_coordinate(bytes: &[u8]) -> f32 {
    let mut encoded = [0_u8; 4];
    encoded.copy_from_slice(bytes);
    let raw = u32::from_le_bytes(encoded);
    (raw as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}

#[cfg(test)]
mod graph_view_contract_tests {
    use super::*;
    use phoenix_scene_contract::{GraphCanvas, GraphScope, Manifold, RelationMask, ReviewMask};

    #[test]
    fn registry_to_full_publication_opens_the_structural_atlas() {
        let previous = GraphViewState {
            manifold: Manifold::Caps,
            canvas: GraphCanvas::Grid,
            ..GraphViewState::default()
        };

        let reconciled = reconcile_published_graph_view(previous, false, GraphViewState::default());

        assert_eq!(reconciled.surface, GraphSurface::Atlas);
        assert_eq!(reconciled.families, FamilyMask::ALL);
        assert_eq!(reconciled.manifold, Manifold::Caps);
        assert_eq!(reconciled.canvas, GraphCanvas::Grid);
    }

    #[test]
    fn full_republication_preserves_the_active_visual_contract() {
        let previous = GraphViewState {
            surface: GraphSurface::Atlas,
            families: FamilyMask::FACTS,
            scope: GraphScope::Note,
            reviews: ReviewMask::PROPOSED,
            relations: RelationMask(0x24),
            manifold: Manifold::Siegel,
            ..GraphViewState::default()
        };

        let reconciled = reconcile_published_graph_view(previous, true, GraphViewState::default());

        assert_eq!(reconciled.surface, previous.surface);
        assert_eq!(reconciled.families, previous.families);
        assert_eq!(reconciled.scope, previous.scope);
        assert_eq!(reconciled.reviews, previous.reviews);
        assert_eq!(reconciled.relations, previous.relations);
        assert_eq!(reconciled.manifold, previous.manifold);
    }
}
