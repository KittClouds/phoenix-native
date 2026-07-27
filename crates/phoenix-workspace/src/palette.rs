use crate::{move_new_file, replace_file, temporary_path, write_synced, WorkspaceError};
use phoenix_scene_contract::HighlightPalette;
use std::fs;
use std::path::{Path, PathBuf};

const PALETTE_FILE: &str = "highlight-palette-v1.json";

pub fn load_highlight_palette_or_default(
    workspace_path: &Path,
) -> Result<HighlightPalette, WorkspaceError> {
    let path = palette_path(workspace_path)?;
    if !path.exists() {
        return Ok(HighlightPalette::default());
    }
    let bytes = fs::read(&path).map_err(|source| WorkspaceError::Io {
        path: path.clone(),
        source,
    })?;
    let palette = serde_json::from_slice::<HighlightPalette>(&bytes).map_err(|source| {
        WorkspaceError::Json {
            path: path.clone(),
            source,
        }
    })?;
    palette
        .validate()
        .map_err(|_| WorkspaceError::InvalidManifest("highlight palette is invalid".into()))?;
    Ok(palette)
}

pub fn save_highlight_palette_atomic(
    workspace_path: &Path,
    palette: HighlightPalette,
) -> Result<PathBuf, WorkspaceError> {
    palette
        .validate()
        .map_err(|_| WorkspaceError::InvalidManifest("highlight palette is invalid".into()))?;
    let path = palette_path(workspace_path)?;
    let bytes = serde_json::to_vec_pretty(&palette).map_err(|source| WorkspaceError::Json {
        path: path.clone(),
        source,
    })?;
    let temp = temporary_path(&path);
    if let Err(error) = write_synced(&temp, &bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let result = if path.exists() {
        replace_file(&path, &temp)
    } else {
        move_new_file(&path, &temp)
    };
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map(|()| path)
}

fn palette_path(workspace_path: &Path) -> Result<PathBuf, WorkspaceError> {
    workspace_path
        .parent()
        .map(|parent| parent.join(PALETTE_FILE))
        .ok_or_else(|| WorkspaceError::InvalidManifest("workspace path has no parent".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn palette_round_trips_atomically() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "phoenix-highlight-palette-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root)?;
        let workspace = root.join("workspace-v1.json");
        let mut palette = HighlightPalette::default();
        palette.character.primary = [0.1, 0.2, 0.3, 1.0];
        save_highlight_palette_atomic(&workspace, palette)?;
        assert_eq!(load_highlight_palette_or_default(&workspace)?, palette);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
