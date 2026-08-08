//! Explicit recovery/visual-QA harness for republishing a verified V1 pair
//! with the current prepared visual pages. Production never calls this path.

use phoenix_scene_archive::{ArchiveManifold, PhoenixSceneArchiveV1};
use phoenix_scene_contract::{EntityFamily, HighlightPalette};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use phoenix_scene_publisher::{
    NativeScenePublication, SceneEdgeProduct, SceneNodeProduct, ScenePublicationKind,
    ScenePublicationStore,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let archive_path = PathBuf::from(args.next().ok_or("expected source .psa path")?);
    let index_path = PathBuf::from(args.next().ok_or("expected source .pspi path")?);
    let output_workspace = PathBuf::from(args.next().ok_or("expected output workspace path")?);
    if args.next().is_some() {
        return Err("unexpected extra arguments".into());
    }

    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let index = PhoenixSceneProductIndexV1::open(&index_path)?;
    index.bind_to_archive(&archive)?;
    let hybrid = archive.open_manifold(ArchiveManifold::Hybrid)?;
    let identities = hybrid.identities.to_vec();
    let topology = hybrid.topology.to_vec();
    let palette = HighlightPalette::default();
    let slots = identities
        .iter()
        .enumerate()
        .map(|(slot, identity)| (identity.id, slot))
        .collect::<HashMap<_, _>>();
    let mut degrees = vec![0_u32; identities.len()];
    for edge in &topology {
        if let Some(slot) = slots.get(&edge.source_id) {
            degrees[*slot] = degrees[*slot].saturating_add(1);
        }
        if let Some(slot) = slots.get(&edge.target_id) {
            degrees[*slot] = degrees[*slot].saturating_add(1);
        }
    }
    let mut styles = hybrid.styles.to_vec();
    for (slot, style) in styles.iter_mut().enumerate() {
        let family = family_from_mask(index.nodes()[slot].family_mask);
        style.color = palette.for_family(family).primary;
        style.color[3] = 0.92;
        style.radius = (0.65 + (degrees[slot] as f32 + 1.0).ln() * 0.24).min(2.2);
    }
    let mut edges = hybrid.edges.to_vec();
    for (slot, edge) in edges.iter_mut().enumerate() {
        let topology = topology[slot];
        let source = styles[slots[&topology.source_id]].color;
        let target = styles[slots[&topology.target_id]].color;
        edge.color = [
            (source[0] + target[0]) * 0.5,
            (source[1] + target[1]) * 0.5,
            (source[2] + target[2]) * 0.5,
            0.045,
        ];
        edge.width = (0.36 + edge.width * 0.12).clamp(0.36, 0.72);
    }
    let positions = ArchiveManifold::ALL.map(|manifold| {
        archive
            .open_manifold(manifold)
            .map(|pages| pages.positions.to_vec())
    });
    let positions = positions
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| "six manifold pages required")?;
    let node_products = index
        .nodes()
        .iter()
        .enumerate()
        .map(|(slot, product)| {
            let label = index.label(slot).ok_or("invalid label slot")?;
            let label: Arc<str> = if label.is_empty() {
                Arc::from(format!("Node {}", product.node_id))
            } else {
                Arc::from(label)
            };
            Ok(SceneNodeProduct {
                node_id: product.node_id,
                family_mask: product.family_mask,
                scope_mask: product.scope_mask,
                review_mask: product.review_mask,
                label,
                inspector_ref: product.inspector_ref,
                provenance_ref: product.provenance_ref,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let edge_products = index
        .edges()
        .iter()
        .map(|product| SceneEdgeProduct {
            edge_id: product.edge_id,
            family_mask: product.family_mask,
            scope_mask: product.scope_mask,
            relation_mask: product.relation_mask,
            review_mask: product.review_mask,
            inspector_ref: product.inspector_ref,
            provenance_ref: product.provenance_ref,
        })
        .collect();
    let publication = NativeScenePublication {
        generation_id: archive.header().generation_id,
        kind: ScenePublicationKind::Full,
        registry_revision: 0,
        document_id: None,
        identities,
        styles,
        topology,
        edges,
        positions,
        caps_guides: Vec::new(),
        node_products,
        edge_products,
        entity_mappings: index.mappings().to_vec(),
        references: index.references().to_vec(),
    };
    let store = ScenePublicationStore::for_workspace(&output_workspace)?;
    let published = store.publish(publication)?;
    println!(
        "PHOENIX_VISUAL_FIXTURE_READY root={} generation={} nodes={} edges={} archive_hash={}",
        store.root().display(),
        published.receipt.generation_id,
        published.receipt.node_count,
        published.receipt.edge_count,
        hex(published.receipt.archive_cohort_hash)
    );
    Ok(())
}

fn family_from_mask(mask: u64) -> EntityFamily {
    match mask.trailing_zeros() {
        0 => EntityFamily::Character,
        1 => EntityFamily::Location,
        2 => EntityFamily::Organization,
        3 => EntityFamily::Item,
        4 => EntityFamily::Concept,
        5 => EntityFamily::Event,
        6 => EntityFamily::Structure,
        _ => EntityFamily::Other,
    }
}

fn hex(hash: [u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(64);
    for byte in hash {
        let _ = write!(value, "{byte:02x}");
    }
    value
}
