use super::*;
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PageKey, PageKind,
    PhoenixSceneArchiveBuilderV1, PhoenixSceneArchiveV1, PositionRecord, TopologyRecord,
};
use std::sync::atomic::{AtomicU64, Ordering};

const NONE: u32 = u32::MAX;
static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

#[test]
fn mmap_index_binds_labels_and_explicit_entity_ids() -> Result<(), Box<dyn std::error::Error>> {
    let archive_path = fixture_archive(41)?;
    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let index_path = unique_path("valid", "pspi");
    fixture_index(&archive, &index_path)?;

    let index = PhoenixSceneProductIndexV1::open(&index_path)?;
    index.bind_to_archive(&archive)?;
    assert_eq!(index.header().binding.archive_generation, 41);
    assert_eq!(index.label(0), Some("Alpha"));
    assert_eq!(index.label(1), Some("Same label"));
    assert_eq!(index.node_for_entity(EntityId(9001)), Some(NodeId(11)));
    assert_eq!(index.entity_for_node(NodeId(22)), Some(EntityId(9002)));
    assert_ne!(index.header().index_hash, [0; 32]);

    remove(&archive_path);
    remove(&index_path);
    Ok(())
}

#[test]
fn index_identity_is_not_inferred_from_duplicate_labels() -> Result<(), Box<dyn std::error::Error>>
{
    let archive_path = fixture_archive(42)?;
    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let path = unique_path("labels", "pspi");
    let binding = ProductIndexBinding::from_archive(&archive);
    let mut builder = PhoenixSceneProductIndexBuilderV1::new(binding);
    builder
        .push_node(node_record(11), "duplicate")?
        .push_node(node_record(22), "duplicate")?
        .push_edge(edge_record(91))
        .push_mapping(EntityNodeMappingRecord {
            entity_id: 700,
            node_id: 22,
        });
    builder.write_to_path(&path)?;
    let index = PhoenixSceneProductIndexV1::open(&path)?;
    index.bind_to_archive(&archive)?;
    assert_eq!(index.node_for_entity(EntityId(700)), Some(NodeId(22)));
    assert_eq!(index.entity_for_node(NodeId(11)), None);

    remove(&archive_path);
    remove(&path);
    Ok(())
}

#[test]
fn stale_and_corrupt_indexes_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let archive_path = fixture_archive(43)?;
    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let stale_path = unique_path("stale", "pspi");
    let stale_binding = ProductIndexBinding {
        archive_generation: 44,
        archive_cohort_hash: archive.header().cohort_hash,
    };
    PhoenixSceneProductIndexBuilderV1::new(stale_binding).write_to_path(&stale_path)?;
    let stale = PhoenixSceneProductIndexV1::open(&stale_path)?;
    assert!(matches!(
        stale.bind_to_archive(&archive),
        Err(ProductIndexError::StaleGeneration {
            index: 44,
            archive: 43
        })
    ));

    let valid_path = unique_path("before-corrupt", "pspi");
    fixture_index(&archive, &valid_path)?;
    let corrupt_path = unique_path("corrupt", "pspi");
    let mut bytes = std::fs::read(&valid_path)?;
    let last = bytes.len() - 1;
    bytes[last] ^= 0x5a;
    std::fs::write(&corrupt_path, bytes)?;
    assert!(matches!(
        PhoenixSceneProductIndexV1::open(&corrupt_path),
        Err(ProductIndexError::CorruptBody)
    ));

    remove(&archive_path);
    remove(&stale_path);
    remove(&valid_path);
    remove(&corrupt_path);
    Ok(())
}

#[test]
fn mismatched_slot_identity_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let archive_path = fixture_archive(45)?;
    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let index_path = unique_path("identity-mismatch", "pspi");
    let mut builder =
        PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding::from_archive(&archive));
    builder
        .push_node(node_record(22), "wrong order")?
        .push_node(node_record(11), "wrong order")?
        .push_edge(edge_record(91));
    builder.write_to_path(&index_path)?;
    let index = PhoenixSceneProductIndexV1::open(&index_path)?;
    assert!(matches!(
        index.bind_to_archive(&archive),
        Err(ProductIndexError::IdentityMismatch {
            resource: "node",
            slot: 0,
            ..
        })
    ));

    remove(&archive_path);
    remove(&index_path);
    Ok(())
}

fn fixture_index(
    archive: &PhoenixSceneArchiveV1,
    path: &std::path::Path,
) -> Result<(), ProductIndexError> {
    let mut builder =
        PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding::from_archive(archive));
    builder
        .push_reference(ProductReferenceRecord {
            stable_ref: 99,
            source_offset: 10,
            source_len: 20,
            kind: 1,
            flags: 0,
        })?
        .push_node(
            NodeProductRecord {
                inspector_ref: 0,
                provenance_ref: 0,
                ..node_record(11)
            },
            "Alpha",
        )?
        .push_node(node_record(22), "Same label")?
        .push_edge(edge_record(91))
        .push_mapping(EntityNodeMappingRecord {
            entity_id: 9002,
            node_id: 22,
        })
        .push_mapping(EntityNodeMappingRecord {
            entity_id: 9001,
            node_id: 11,
        });
    builder.write_to_path(path)?;
    Ok(())
}

fn node_record(node_id: u64) -> NodeProductRecord {
    NodeProductRecord {
        node_id,
        family_mask: 1,
        scope_mask: 1,
        review_mask: ReviewState::Accepted as u32,
        label_offset: 0,
        label_len: 0,
        inspector_ref: NONE,
        provenance_ref: NONE,
        reserved: 0,
    }
}

fn edge_record(edge_id: u64) -> EdgeProductRecord {
    EdgeProductRecord {
        edge_id,
        family_mask: 1,
        scope_mask: 1,
        relation_mask: 1,
        review_mask: ReviewState::Accepted as u32,
        inspector_ref: NONE,
        provenance_ref: NONE,
        reserved: 0,
    }
}

fn fixture_archive(generation: u64) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = unique_path("archive", "psa");
    let mut builder = PhoenixSceneArchiveBuilderV1::new(generation)?;
    builder
        .add_records(
            PageKey::shared(PageKind::NodeIdentity),
            &[NodeIdentityRecord { id: 11 }, NodeIdentityRecord { id: 22 }],
        )?
        .add_records(
            PageKey::shared(PageKind::NodeStyle),
            &[
                NodeStyleRecord {
                    color: [0.0, 1.0, 0.0, 1.0],
                    radius: 1.0,
                    kind: 1,
                    flags: 0,
                },
                NodeStyleRecord {
                    color: [0.0, 0.0, 1.0, 1.0],
                    radius: 1.0,
                    kind: 1,
                    flags: 0,
                },
            ],
        )?
        .add_records(
            PageKey::shared(PageKind::Topology),
            &[TopologyRecord {
                source_id: 11,
                target_id: 22,
            }],
        )?
        .add_records(
            PageKey::shared(PageKind::Edge),
            &[EdgeRecord {
                id: 91,
                color: [1.0; 4],
                width: 1.0,
                kind: 1,
                flags: 0,
            }],
        )?;
    for manifold in ArchiveManifold::ALL {
        builder.add_records(
            PageKey::manifold(PageKind::Positions, manifold),
            &[
                PositionRecord {
                    position: [0.0, 0.0, 0.0],
                },
                PositionRecord {
                    position: [1.0, 0.0, 0.0],
                },
            ],
        )?;
    }
    builder.write_to_path(&path)?;
    Ok(path)
}

fn unique_path(label: &str, extension: &str) -> std::path::PathBuf {
    let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "phoenix-product-index-{label}-{}-{sequence}.{extension}",
        std::process::id()
    ))
}

fn remove(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}
