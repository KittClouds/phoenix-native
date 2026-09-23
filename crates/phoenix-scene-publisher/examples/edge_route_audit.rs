//! Read-only endpoint and overshoot audit of the actual published path pages.
use hashbrown::HashMap;
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
    println!("generation={}", published.receipt.generation_id);
    for manifold in [Manifold::Hybrid, Manifold::Siegel] {
        let active = published.scene.activate_manifold(manifold)?;
        let slots: HashMap<_, _> = active
            .pages
            .identities
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let paths = active.prepared_paths.ok_or("missing prepared paths")?;
        let mut overshoot = 0;
        let mut detached = 0;
        let mut max_excess = 0.0_f32;
        for path in paths.paths {
            if manifold == Manifold::Hybrid {
                assert_eq!(
                    path.point_count, 2,
                    "Hybrid edges must have no intermediate elbows"
                );
            }
            let edge = active.pages.topology[path.edge_slot as usize];
            let a = active.pages.positions[slots[&edge.source_id]].position;
            let b = active.pages.positions[slots[&edge.target_id]].position;
            let start = path.first_point as usize;
            let points = &paths.points[start..start + path.point_count as usize];
            detached += usize::from(
                points.first().map(|p| p.position) != Some(a)
                    || points.last().map(|p| p.position) != Some(b),
            );
            let mut outside = false;
            for p in points {
                for i in 0..3 {
                    let excess =
                        (a[i].min(b[i]) - p.position[i]).max(p.position[i] - a[i].max(b[i]));
                    assert!(p.position[i].is_finite());
                    max_excess = max_excess.max(excess);
                    outside |= excess > 0.00001;
                }
            }
            overshoot += usize::from(outside);
        }
        println!("{manifold:?}: edges={} detached={detached} overshooting={overshoot} max_axis_excess={max_excess:.6}", paths.paths.len());
        assert_eq!(detached, 0);
        if std::env::args().any(|arg| arg == "--require-bounded") {
            assert_eq!(overshoot, 0);
        }
    }
    Ok(())
}
