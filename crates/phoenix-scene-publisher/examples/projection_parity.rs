//! Compare two actual app publications without altering either authority.
use phoenix_scene_contract::Manifold;
use phoenix_scene_publisher::ScenePublicationStore;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let roots: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if !(2..=3).contains(&roots.len()) {
        return Err(
            "usage: projection_parity <before-root> <after-root> [hybrid-only|caps-only]".into(),
        );
    }
    let before = ScenePublicationStore::open_current_at(&roots[0])?.ok_or("missing before")?;
    let after = ScenePublicationStore::open_current_at(&roots[1])?.ok_or("missing after")?;
    let base = before.scene.activate_manifold(Manifold::Caps)?;
    for manifold in Manifold::ALL {
        let previous = before.scene.activate_manifold(manifold)?;
        let current = after.scene.activate_manifold(manifold)?;
        assert_eq!(
            base.pages.identities, current.pages.identities,
            "node identities {manifold:?}"
        );
        assert_eq!(
            base.pages.topology, current.pages.topology,
            "edge topology {manifold:?}"
        );
        assert_eq!(
            base.pages.styles, current.pages.styles,
            "node styles {manifold:?}"
        );
        assert_eq!(
            base.pages.edges, current.pages.edges,
            "edge identities/styles {manifold:?}"
        );
        assert!(current
            .pages
            .positions
            .iter()
            .all(|p| p.position.iter().all(|v| v.is_finite())));
        let changed = previous.pages.positions != current.pages.positions;
        assert_eq!(
            changed,
            if roots
                .get(2)
                .is_some_and(|s| s == &PathBuf::from("caps-only"))
            {
                manifold == Manifold::Caps
            } else if roots.len() == 3 {
                manifold == Manifold::Hybrid
            } else {
                matches!(
                    manifold,
                    Manifold::Caps | Manifold::Siegel | Manifold::Transit
                )
            },
            "projection scope {manifold:?}"
        );
        println!(
            "{manifold:?}: nodes={} edges={} topology_equal=true positions_changed={} guides={}",
            current.pages.identities.len(),
            current.pages.edges.len(),
            changed,
            current.guides.map_or(0, |page| page.strokes.len())
        );
    }
    println!(
        "before_generation={} after_generation={}",
        before.receipt.generation_id, after.receipt.generation_id
    );
    Ok(())
}
