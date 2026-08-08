use phoenix_scene_contract::{RelationFamily, VisualNodeKind, TOPOLOGY_INVENTORY_CONTRACT};
use phoenix_scene_publisher::ScenePublicationStore;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: topology_model_census <scene-publication-root>")?;
    let published = ScenePublicationStore::open_current_at(&root)?.ok_or("no current scene")?;
    let census = published.scene.topology_census(&published.product_index)?;

    println!(
        "TOPOLOGY_MODEL_CENSUS contract={} generation={} nodes={} edges={} manifolds={}",
        TOPOLOGY_INVENTORY_CONTRACT,
        published.receipt.generation_id,
        census.node_count,
        census.edge_count,
        census.manifold_count
    );
    for kind in VisualNodeKind::ALL {
        println!("node_kind={kind:?} count={}", census.node_kind_count(kind));
    }
    println!("fact_lane_total={}", census.fact_node_count());
    for family in RelationFamily::ALL {
        println!(
            "relation_family={family:?} count={}",
            census.relation_count(family)
        );
    }
    Ok(())
}
