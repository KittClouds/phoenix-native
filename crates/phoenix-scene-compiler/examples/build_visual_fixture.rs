//! Explicit design-preview fixture built through the real native compiler.

use phoenix_scene_compiler::{compile_active_document, NativeSceneCompilerInput};
use phoenix_scene_contract::{EntityKind, HighlightPalette};
use phoenix_scene_publisher::ScenePublicationStore;
use phoenix_workspace::{
    commit_document, open_document, EntityRegistry, EntityTag, WorkspaceDocument,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_workspace = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("expected output workspace path")?;
    if output_workspace.exists() {
        return Err("output workspace already exists".into());
    }
    if let Some(parent) = output_workspace.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (content, tags) = fixture_document(180);
    let workspace = WorkspaceDocument::seeded();
    workspace.save_atomic(&output_workspace)?;
    let initial = open_document(
        &output_workspace,
        &workspace,
        workspace.first_note().ok_or("fixture note missing")?,
    )?;
    let lease = commit_document(&output_workspace, &workspace, initial.token(), &content)?;
    let mut registry = EntityRegistry::empty();
    for tag in tags {
        registry.tag(&lease, tag)?;
    }
    let store = ScenePublicationStore::for_workspace(&output_workspace)?;
    let generation = store.next_generation()?;
    let compiled = compile_active_document(NativeSceneCompilerInput {
        generation_id: generation,
        registry_revision: registry.revision(),
        document: &lease,
        registry: &registry,
        palette: HighlightPalette::default(),
    })?;
    let compile = compiled.receipt;
    let published = store.publish(compiled.publication)?;
    println!(
        "PHOENIX_NATIVE_LAYOUT_FIXTURE_READY root={} generation={} nodes={} edges={} \
         entities={} chunks={} compile_us={}",
        store.root().display(),
        published.receipt.generation_id,
        published.receipt.node_count,
        published.receipt.edge_count,
        published.receipt.entity_count,
        compile.chunk_count,
        compile.compile_micros
    );
    Ok(())
}

fn fixture_document(entity_count: usize) -> (String, Vec<EntityTag>) {
    let kinds = [
        EntityKind::Character,
        EntityKind::Location,
        EntityKind::Npc,
        EntityKind::Faction,
        EntityKind::Event,
        EntityKind::Concept,
    ];
    let stems = ["Astra", "Vale", "Morrow", "Cinder", "Echo", "Lumen"];
    let mut content = String::with_capacity(entity_count * 24);
    let mut tags = Vec::with_capacity(entity_count);
    for ordinal in 0..entity_count {
        if ordinal != 0 {
            content.push_str(if ordinal % 6 == 0 { ".\n\n" } else { " met " });
        }
        let surface = format!("{} {:03}", stems[ordinal % stems.len()], ordinal + 1);
        let start = content.len();
        content.push_str(&surface);
        tags.push(EntityTag {
            kind: kinds[ordinal % kinds.len()],
            custom_kind: None,
            start: start as u32,
            end: content.len() as u32,
            surface,
        });
    }
    content.push('.');
    (content, tags)
}
