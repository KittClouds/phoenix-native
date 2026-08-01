use super::*;
use hashbrown::HashMap;
use phoenix_analysis_contract::{AnalysisEntity, PhoenixNerArtifactV1, VerifiedAnalysisArtifact};
use phoenix_scene_contract::EntityKind;
use phoenix_workspace::{EntityRegistry, EntitySourceMask, NerEntityRecord};

#[derive(Clone, Debug)]
pub struct NerEntityBatch {
    pub(crate) artifact_hash: [u8; 32],
    pub(crate) artifact: Arc<PhoenixNerArtifactV1>,
}

impl NerEntityBatch {
    pub fn from_verified(verified: &VerifiedAnalysisArtifact) -> Self {
        Self {
            artifact_hash: verified.artifact_hash(),
            artifact: Arc::new(verified.analysis().ner.clone()),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(artifact: PhoenixNerArtifactV1) -> Self {
        Self {
            artifact_hash: [0xA5; 32],
            artifact: Arc::new(artifact),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasEntity {
    pub stable_id: u64,
    pub label: Arc<str>,
    pub kind: EntityKind,
    pub custom_kind: Option<Arc<str>>,
    pub sources: EntitySourceMask,
    pub mention_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasRegistry {
    pub registry_revision: u64,
    pub ner_revision: u64,
    pub entities: Arc<[AtlasEntity]>,
    pub ner_source_count: usize,
    pub user_tagged_source_count: usize,
}

impl AtlasRegistry {
    pub fn from_registry(registry: &EntityRegistry) -> Self {
        let mut user_mentions = HashMap::<u64, u64>::with_capacity(registry.entities().len());
        for mention in registry.mentions().iter().filter(|mention| mention.active) {
            *user_mentions.entry(mention.entity_id).or_default() += 1;
        }

        let mut entities = registry
            .entities()
            .iter()
            .map(|entity| AtlasEntity {
                stable_id: entity.id,
                label: Arc::from(entity.label.as_str()),
                kind: entity.kind,
                custom_kind: entity.custom_kind.as_deref().map(Arc::from),
                sources: entity.sources,
                mention_count: u64::from(entity.ner_mention_count)
                    + user_mentions.get(&entity.id).copied().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        entities.sort_unstable_by(|left, right| {
            left.kind
                .label()
                .cmp(right.kind.label())
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.stable_id.cmp(&right.stable_id))
        });
        let ner_source_count = entities.iter().filter(|entity| entity.sources.ner).count();
        let user_tagged_source_count = entities
            .iter()
            .filter(|entity| entity.sources.user_tagged)
            .count();
        Self {
            registry_revision: registry.revision(),
            ner_revision: registry.ner_revision(),
            entities: entities.into(),
            ner_source_count,
            user_tagged_source_count,
        }
    }
}

pub(super) fn publish_ner_batch(
    shared: &KernelShared,
    sequence: u64,
    batch: NerEntityBatch,
) -> Result<CommandReceipt, KernelError> {
    batch
        .artifact
        .validate()
        .map_err(|_| KernelError::AnalysisAuthorityMismatch)?;
    if batch.artifact_hash == [0; 32] {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    let current = read_state(shared)?;
    analysis::validate_binding(
        &current,
        &batch.artifact.binding,
        batch.artifact.binding.source_registry_revision,
    )?;
    let records = batch
        .artifact
        .entities
        .iter()
        .map(ner_entity_record)
        .collect::<Vec<_>>();
    let ner_revision = batch.artifact.ner_revision;
    drop(current);
    let mut registry = (*read_state(shared)?.entity_registry).clone();
    let result = registry.publish_document_ner(
        phoenix_workspace::EntryId(batch.artifact.binding.native_document_id),
        ner_revision,
        &records,
    )?;
    if result.registry_revision != batch.artifact.binding.target_registry_revision {
        return Err(KernelError::AnalysisAuthorityMismatch);
    }
    registry.save_atomic(&shared.workspace_path)?;
    let registry = Arc::new(registry);
    let atlas = Arc::new(AtlasRegistry::from_registry(&registry));
    let highlight_index = entity_highlights::EntityHighlightIndex::build(&registry)?;
    let canonical_entities = atlas.entities.len();
    let (palette, active_lease, base_anchors) = {
        let state = read_state(shared)?;
        (
            *state.highlight_palette,
            state.active_document_lease.as_ref().map(Arc::clone),
            state.document_anchors.as_ref().map(Arc::clone),
        )
    };
    let document_anchors = entity_tags::registry_anchors_with_base(
        &highlight_index,
        &registry,
        active_lease.as_deref(),
        base_anchors.as_deref(),
    )?;
    let published = scene_publication::refresh_registry_scene(shared, &atlas, palette)?;

    let mut state = write_state(shared)?;
    state.entity_registry = registry;
    state.atlas_registry = atlas;
    state.entity_highlights = highlight_index;
    state.document_anchors = document_anchors;
    let scene_publication = published
        .map(|published| scene_publication::install_published_scene_state(&mut state, published))
        .transpose()?;
    state.revision = checked_revision(state.revision)?;
    let kernel_revision = state.revision;
    drop(state);

    push_event(
        shared,
        KernelEvent {
            sequence,
            kernel_revision,
            kind: KernelEventKind::AtlasRegistryCommitted {
                ner_revision: result.ner_revision,
                registry_revision: result.registry_revision,
                canonical_entities,
                scene_publication,
            },
        },
    )?;
    Ok(receipt(
        sequence,
        kernel_revision,
        KernelOutcome::NerEntitiesPublished(result),
    ))
}

fn ner_entity_record(entity: &AnalysisEntity) -> NerEntityRecord {
    let kind = analysis::entity_kind(entity.kind);
    NerEntityRecord {
        stable_id: entity.stable_id,
        label: entity.label.clone(),
        kind,
        // Older producer binaries used `custom_kind` as a classifier trace.
        // Once the typed kind is known, that trace is cold provenance rather
        // than a custom entity identity and must not enter the canonical
        // registry's stricter kind contract.
        custom_kind: (kind == EntityKind::Custom)
            .then(|| entity.custom_kind.clone())
            .flatten(),
        mention_count: entity.mention_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_analysis_contract::AnalysisEntityKind;

    #[test]
    fn typed_entity_kind_discards_legacy_classifier_trace() {
        let record = ner_entity_record(&AnalysisEntity {
            stable_id: 7,
            label: "Atlas Collective".into(),
            kind: AnalysisEntityKind::Network,
            custom_kind: Some("ENTITY".into()),
            mention_count: 3,
        });
        assert_eq!(record.kind, EntityKind::Network);
        assert_eq!(record.custom_kind, None);
    }

    #[test]
    fn custom_entity_preserves_its_required_kind() {
        let record = ner_entity_record(&AnalysisEntity {
            stable_id: 9,
            label: "Story-specific role".into(),
            kind: AnalysisEntityKind::Custom,
            custom_kind: Some("ROLE".into()),
            mention_count: 1,
        });
        assert_eq!(record.kind, EntityKind::Custom);
        assert_eq!(record.custom_kind.as_deref(), Some("ROLE"));
    }
}
