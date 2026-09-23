//! Read-only verification of the actual application's CAPS publication.
use phoenix_scene_contract::{
    CapsRole, Manifold, CHAPTER_NODE_KIND, DOCUMENT_NODE_KIND, GUIDE_FLAG_CAP_BOUNDARY,
    GUIDE_FLAG_SHELL, PARAGRAPH_NODE_KIND, SENTENCE_NODE_KIND,
};
use phoenix_scene_publisher::ScenePublicationStore;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected publication root")?,
    );
    let publication = ScenePublicationStore::open_current_at(&root)?.ok_or("no publication")?;
    let active = publication.scene.activate_manifold(Manifold::Caps)?;
    for (kind, role) in [
        (DOCUMENT_NODE_KIND, CapsRole::Document),
        (CHAPTER_NODE_KIND, CapsRole::Chapter),
        (PARAGRAPH_NODE_KIND, CapsRole::Paragraph),
        (SENTENCE_NODE_KIND, CapsRole::Sentence),
    ] {
        let mut count = 0;
        let mut minimum = f32::INFINITY;
        let mut maximum = 0.0_f32;
        for (style, position) in active.pages.styles.iter().zip(active.pages.positions) {
            if style.kind != kind {
                continue;
            }
            let radius = position.position.iter().map(|v| v * v).sum::<f32>().sqrt();
            let [near, far] = role.klein_depth_range();
            assert!(
                (near * 40.0..=far * 40.0).contains(&radius),
                "wrong shell: {role:?}"
            );
            minimum = minimum.min(radius);
            maximum = maximum.max(radius);
            count += 1;
        }
        println!("{role:?}: count={count} radius={minimum:.4}..{maximum:.4}");
    }
    let guides = active.guides.ok_or("no CAPS guides")?;
    let shells = guides
        .strokes
        .iter()
        .filter(|s| s.flags == GUIDE_FLAG_SHELL)
        .count();
    let caps = guides
        .strokes
        .iter()
        .filter(|s| s.flags == GUIDE_FLAG_CAP_BOUNDARY)
        .count();
    assert_eq!(shells, 36);
    assert!(caps <= 96);
    assert!(guides
        .points
        .iter()
        .all(|p| p.position.iter().all(|v| v.is_finite())));
    println!(
        "generation={} shells={} major_caps={} finite=true",
        publication.receipt.generation_id, shells, caps
    );
    Ok(())
}
