//! Read-only measurements of an actual Hybrid position page.
use phoenix_scene_contract::Manifold;
use phoenix_scene_publisher::ScenePublicationStore;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected publication root")?,
    );
    let published = ScenePublicationStore::open_current_at(&root)?.ok_or("missing publication")?;
    let active = published.scene.activate_manifold(Manifold::Hybrid)?;
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    let mut mean = [0.0_f64; 3];
    let mut octants = [0_usize; 8];
    let mut max_radius = 0.0_f32;
    for p in active.pages.positions {
        let mut octant = 0;
        let mut norm = 0.0;
        for axis in 0..3 {
            let v = p.position[axis];
            assert!(v.is_finite());
            minimum[axis] = minimum[axis].min(v);
            maximum[axis] = maximum[axis].max(v);
            mean[axis] += f64::from(v);
            norm += v * v;
            if v >= 0.0 {
                octant |= 1 << axis;
            }
        }
        octants[octant] += 1;
        max_radius = max_radius.max(norm.sqrt());
    }
    for v in &mut mean {
        *v /= active.pages.positions.len().max(1) as f64;
    }
    println!("generation={} nodes={} min={minimum:?} max={maximum:?} centroid={mean:?} octants={octants:?} max_radius={max_radius}",
        published.receipt.generation_id, active.pages.positions.len());
    Ok(())
}
