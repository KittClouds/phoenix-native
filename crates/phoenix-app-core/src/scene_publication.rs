use super::*;
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::{EntityFamily, SceneSource};
use phoenix_scene_product_index::{EntityNodeMappingRecord, ProductReferenceRecord};
use phoenix_scene_publisher::{
    NativeScenePublication, PublishedScene, SceneEdgeProduct, SceneNodeProduct,
    ScenePublicationKind, ScenePublicationReceipt, ScenePublicationStore,
};

const REGISTRY_SCOPE: u64 = 1;
const REVIEW_ACCEPTED: u32 = 1;
const NO_REFERENCE: u32 = u32::MAX;

pub(super) fn initial_production_scene(
    publisher: &ScenePublicationStore,
    atlas: &AtlasRegistry,
    palette: HighlightPalette,
) -> Result<PublishedScene, KernelError> {
    if let Some(current) = publisher.open_current()? {
        if current.receipt.kind == ScenePublicationKind::Full {
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
    publication: NativeScenePublication,
) -> Result<CommandReceipt, KernelError> {
    if publication.kind != ScenePublicationKind::Full {
        return Err(KernelError::BackendPublicationMustBeFull);
    }
    let (publisher, registry_revision) = {
        let state = read_state(shared)?;
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
    install_published_scene(shared, sequence, published)
}

pub(super) fn install_published_scene_state(
    state: &mut KernelState,
    published: PublishedScene,
) -> Result<ScenePublicationReceipt, KernelError> {
    let previous_view = state.graph_view;
    let mut graph_view = published
        .scene
        .graph_view_state(Some(&published.product_index))?;
    graph_view.manifold = previous_view.manifold;
    let receipt = published.receipt;
    state.resident_scene = Some(published.scene);
    state.scene_product_index = Some(published.product_index);
    state.scene_publication = Some(receipt);
    state.graph_view = graph_view;
    state.document_anchors = None;
    Ok(receipt)
}

fn install_published_scene(
    shared: &KernelShared,
    sequence: u64,
    published: PublishedScene,
) -> Result<CommandReceipt, KernelError> {
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
    state.revision = checked_revision(state.revision)?;
    let revision = state.revision;
    drop(state);
    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision: revision,
            kind: KernelEventKind::SceneGenerationPublished {
                receipt: publication_receipt,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        revision,
        KernelOutcome::SceneGenerationPublished(publication_receipt),
    ))
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
    let mut positions: [Vec<PositionRecord>; 5] =
        std::array::from_fn(|_| Vec::with_capacity(entities.len()));

    for entity in entities {
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
        let manifold_positions = stable_positions(entity.stable_id);
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
    let slot = match family {
        EntityFamily::Character => 0,
        EntityFamily::Location => 1,
        EntityFamily::Organization => 2,
        EntityFamily::Item => 3,
        EntityFamily::Concept => 4,
        EntityFamily::Event => 5,
        EntityFamily::Structure => 6,
        EntityFamily::Other => 7,
    };
    1_u64 << slot
}

fn stable_positions(stable_id: u64) -> [[f32; 3]; 5] {
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
    [
        base,
        [base[0] - base[2] * 0.25, base[1], base[2] + base[0] * 0.25],
        [base[0], base[1] - base[2] * 0.2, base[2] + base[1] * 0.2],
        [base[0] + base[1] * 0.15, base[1] - base[0] * 0.15, base[2]],
        [base[0] * 0.9, base[1] * 0.9, base[2] * 1.2],
    ]
}

fn unit_coordinate(bytes: &[u8]) -> f32 {
    let mut encoded = [0_u8; 4];
    encoded.copy_from_slice(bytes);
    let raw = u32::from_le_bytes(encoded);
    (raw as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}
