use super::*;
use phoenix_scene_archive::{
    EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord, TopologyRecord,
};
use phoenix_scene_contract::SceneSource;
use phoenix_scene_product_index::{EntityNodeMappingRecord, ReviewState};
use std::path::PathBuf;
use std::sync::Arc;

fn test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phoenix-scene-publisher-{label}-{}",
        std::process::id()
    ))
}

fn publication(generation_id: u64, kind: ScenePublicationKind) -> NativeScenePublication {
    let mut identities = vec![NodeIdentityRecord { id: 41 }];
    let mut styles = vec![NodeStyleRecord {
        color: [0.1, 0.8, 0.5, 1.0],
        radius: 4.0,
        kind: 1,
        flags: 0,
    }];
    let mut node_products = vec![SceneNodeProduct {
        node_id: 41,
        family_mask: 1,
        scope_mask: 1,
        review_mask: ReviewState::Accepted as u32,
        label: Arc::from("Atlas entity"),
        inspector_ref: u32::MAX,
        provenance_ref: u32::MAX,
    }];
    let mut mappings = vec![EntityNodeMappingRecord {
        entity_id: 41,
        node_id: 41,
    }];
    let (topology, edges, edge_products) = if kind == ScenePublicationKind::Full {
        identities.push(NodeIdentityRecord { id: 82 });
        styles.push(styles[0]);
        node_products.push(SceneNodeProduct {
            node_id: 82,
            label: Arc::from("Graph node"),
            ..node_products[0].clone()
        });
        mappings.clear();
        (
            vec![TopologyRecord {
                source_id: 41,
                target_id: 82,
            }],
            vec![EdgeRecord {
                id: 99,
                color: [0.4, 0.8, 0.7, 0.5],
                width: 1.0,
                kind: 1,
                flags: 0,
            }],
            vec![SceneEdgeProduct {
                edge_id: 99,
                family_mask: 1,
                scope_mask: 1,
                relation_mask: 1,
                review_mask: ReviewState::Accepted as u32,
                inspector_ref: u32::MAX,
                provenance_ref: u32::MAX,
            }],
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let positions = std::array::from_fn(|manifold| {
        identities
            .iter()
            .enumerate()
            .map(|(slot, _)| PositionRecord {
                position: [slot as f32, manifold as f32, 0.0],
            })
            .collect()
    });
    NativeScenePublication {
        generation_id,
        kind,
        registry_revision: 7,
        document_id: None,
        identities,
        styles,
        topology,
        edges,
        positions,
        node_products,
        edge_products,
        entity_mappings: mappings,
        references: Vec::new(),
    }
}

#[test]
fn atomic_manifest_reopens_the_exact_pair() -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("reopen");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let workspace = root.join("workspace.json");
    let store = ScenePublicationStore::for_workspace(&workspace)?;
    let published = store.publish(publication(1, ScenePublicationKind::RegistryOnly))?;
    let reopened = store
        .open_current()?
        .ok_or("publication manifest was not visible")?;
    assert_eq!(published.receipt, reopened.receipt);
    assert_eq!(reopened.scene.generation().0, 1);
    assert_eq!(reopened.scene.source(), SceneSource::RegistryOnly);
    assert_eq!(
        reopened
            .product_index
            .label(0)
            .ok_or("missing product label")?,
        "Atlas entity"
    );
    assert_eq!(
        reopened
            .product_index
            .node_for_entity(phoenix_scene_product_index::EntityId(41)),
        Some(phoenix_scene_product_index::NodeId(41))
    );
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn registry_publication_cannot_demote_a_full_generation() -> Result<(), Box<dyn std::error::Error>>
{
    let root = test_root("precedence");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let store = ScenePublicationStore::for_workspace(&root.join("workspace.json"))?;
    store.publish(publication(1, ScenePublicationKind::RegistryOnly))?;
    let full = store.publish(publication(2, ScenePublicationKind::Full))?;
    let error = store
        .publish(publication(3, ScenePublicationKind::RegistryOnly))
        .expect_err("registry demotion must fail");
    assert!(matches!(
        error,
        ScenePublicationError::FullGenerationProtected {
            current: 2,
            incoming: 3
        }
    ));
    assert_eq!(
        store.open_current()?.map(|scene| scene.receipt),
        Some(full.receipt)
    );
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn corrupt_manifest_fails_closed_without_scanning_old_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("corrupt");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let store = ScenePublicationStore::for_workspace(&root.join("workspace.json"))?;
    store.publish(publication(1, ScenePublicationKind::RegistryOnly))?;
    let manifest = store.root().join("current.pspm");
    let mut bytes = std::fs::read(&manifest)?;
    bytes[40] ^= 0x5a;
    std::fs::write(&manifest, bytes)?;
    assert!(matches!(
        store.open_current(),
        Err(ScenePublicationError::CorruptManifestHash)
    ));
    std::fs::remove_dir_all(root)?;
    Ok(())
}
