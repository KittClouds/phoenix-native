use phoenix_scene_contract::{describe_node, Manifold, VisualNodeKind};
use phoenix_scene_publisher::ScenePublicationStore;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: caps_model_census <scene-publication-root>")?;
    let published = ScenePublicationStore::open_current_at(&root)?.ok_or("no current scene")?;
    let caps = published.scene.activate_manifold(Manifold::Caps)?;
    let products = published.product_index.nodes();
    if products.len() != caps.pages.positions.len() {
        return Err("CAPS/product inventory mismatch".into());
    }

    let mut published_counts = [0_usize; 256];
    let mut finite_caps_counts = [0_usize; 256];
    let mut radial_min = [f32::INFINITY; 256];
    let mut radial_max = [f32::NEG_INFINITY; 256];
    for (product, position) in products.iter().zip(caps.pages.positions) {
        let kind = describe_node(product.family_mask).kind;
        let slot = kind as usize;
        published_counts[slot] += 1;
        let radius = position.position[0]
            .hypot(position.position[1])
            .hypot(position.position[2]);
        if position.position.iter().all(|value| value.is_finite()) && radius.is_finite() {
            finite_caps_counts[slot] += 1;
            radial_min[slot] = radial_min[slot].min(radius);
            radial_max[slot] = radial_max[slot].max(radius);
        }
    }

    println!(
        "CAPS_MODEL_CENSUS generation={} nodes={} guide_strokes={}",
        published.receipt.generation_id,
        products.len(),
        caps.guides.map_or(0, |guides| guides.strokes.len())
    );
    for kind in VisualNodeKind::ALL {
        let slot = kind as usize;
        let total = published_counts[slot];
        let finite = finite_caps_counts[slot];
        if total != 0 {
            println!(
                "kind={kind:?} published={total} caps={finite} radius={:.3}..{:.3}",
                radial_min[slot], radial_max[slot]
            );
        }
        if total != finite {
            return Err(format!("CAPS omitted {kind:?}: published={total} caps={finite}").into());
        }
    }
    let unknown = VisualNodeKind::Unknown as usize;
    if published_counts[unknown] != finite_caps_counts[unknown] {
        return Err("CAPS omitted unknown-kind nodes".into());
    }
    Ok(())
}
