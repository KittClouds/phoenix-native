//! Seed a disposable UI qualification workspace using the real document codec.
use phoenix_workspace::{commit_document, open_document, EntryId, WorkspaceDocument};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or("source required")?);
    let target = PathBuf::from(args.next().ok_or("workspace required")?);
    if target.exists() {
        return Err("refusing to overwrite an existing workspace".into());
    }
    std::fs::create_dir_all(target.parent().ok_or("workspace parent required")?)?;
    let workspace = WorkspaceDocument::seeded();
    workspace.save_atomic(&target)?;
    let lease = open_document(&target, &workspace, EntryId(3))?;
    let text = std::fs::read_to_string(source)?;
    let saved = commit_document(&target, &workspace, lease.token(), &text)?;
    println!(
        "revision={} bytes={} blake3={}",
        saved.revision.0,
        saved.content.len(),
        saved.content_hash.to_hex()
    );
    Ok(())
}
