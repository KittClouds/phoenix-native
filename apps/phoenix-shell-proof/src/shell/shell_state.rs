use super::{
    drawer::{DrawerLayout, DrawerTab},
    kammi::RightSidebarPage,
    style_hub::GraphSidebarPanel,
    PhoenixShell, LEFT_SIDEBAR_MAX_WIDTH, LEFT_SIDEBAR_MIN_WIDTH, RIGHT_SIDEBAR_MAX_WIDTH,
    RIGHT_SIDEBAR_MIN_WIDTH,
};
use anyhow::{bail, Context, Result};
use phoenix_scene_contract::{GraphViewState, SceneAuthority};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
};

const FORMAT: &str = "phoenix-shell-state-v1";
const MAX_STATE_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ShellStateV1 {
    format: String,
    pub(super) left_open: bool,
    pub(super) right_open: bool,
    #[serde(default)]
    pub(super) right_sidebar_page: RightSidebarPage,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    drawer_open: bool,
    drawer_full_page: bool,
    drawer_height: f32,
    atlas_width: f32,
    pub(super) drawer_tab: DrawerTab,
    pub(super) graph_sidebar_panel: GraphSidebarPanel,
    graph_view: GraphViewState,
}

impl ShellStateV1 {
    pub(super) fn capture(shell: &PhoenixShell) -> Self {
        let (drawer_open, drawer_full_page, drawer_height, atlas_width) =
            shell.drawer_layout.snapshot();
        let graph_view = shell
            .kernel_snapshot()
            .map_or_else(GraphViewState::default, |snapshot| snapshot.graph_view);
        Self {
            format: FORMAT.into(),
            left_open: shell.left_open,
            right_open: shell.right_open,
            right_sidebar_page: shell.right_sidebar_page,
            left_sidebar_width: shell.left_sidebar_width,
            right_sidebar_width: shell.right_sidebar_width,
            drawer_open,
            drawer_full_page,
            drawer_height,
            atlas_width,
            drawer_tab: shell.drawer_tab,
            graph_sidebar_panel: shell.graph_sidebar_panel,
            graph_view,
        }
    }

    pub(super) fn load(workspace_path: &Path) -> Result<Option<Self>> {
        let path = state_path(workspace_path)?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        if bytes.len() > MAX_STATE_BYTES {
            bail!(
                "shell state at {} is oversized: {} bytes",
                path.display(),
                bytes.len()
            );
        }
        let mut state: Self =
            serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))?;
        state.graph_view.normalize_persisted_masks();
        state.validate()?;
        Ok(Some(state))
    }

    pub(super) fn save(&self, workspace_path: &Path) -> Result<()> {
        self.validate()?;
        let path = state_path(workspace_path)?;
        let bytes = serde_json::to_vec_pretty(self).context("encode shell state")?;
        if bytes.len() > MAX_STATE_BYTES {
            bail!("encoded shell state exceeds {MAX_STATE_BYTES} bytes");
        }
        let parent = path
            .parent()
            .context("shell state path has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        let pending = pending_path(&path);
        write_synced(&pending, &bytes)?;
        let replace = if path.exists() {
            replace_file(&path, &pending)
        } else {
            move_new_file(&path, &pending)
        };
        if replace.is_err() {
            let _ = fs::remove_file(&pending);
        }
        replace
    }

    pub(super) fn drawer_layout(&self) -> DrawerLayout {
        DrawerLayout::restore(
            self.drawer_open,
            self.drawer_full_page,
            self.drawer_height,
            self.atlas_width,
        )
    }

    pub(super) fn left_sidebar_width(&self) -> f32 {
        self.left_sidebar_width
            .clamp(LEFT_SIDEBAR_MIN_WIDTH, LEFT_SIDEBAR_MAX_WIDTH)
    }

    pub(super) fn right_sidebar_width(&self) -> f32 {
        self.right_sidebar_width
            .clamp(RIGHT_SIDEBAR_MIN_WIDTH, RIGHT_SIDEBAR_MAX_WIDTH)
    }

    pub(super) fn rebind_graph_view(&self, authority: SceneAuthority) -> GraphViewState {
        GraphViewState {
            authority,
            ..self.graph_view
        }
    }

    fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!("unsupported shell state format '{}'", self.format);
        }
        if !self.left_sidebar_width.is_finite()
            || !self.right_sidebar_width.is_finite()
            || !self.drawer_height.is_finite()
            || !self.atlas_width.is_finite()
        {
            bail!("shell state contains a non-finite dimension");
        }
        if !self.graph_view.is_valid() {
            bail!("shell state contains an invalid graph view");
        }
        Ok(())
    }
}

fn state_path(workspace_path: &Path) -> Result<PathBuf> {
    let file_name = workspace_path
        .file_name()
        .context("workspace path has no file name")?;
    let mut state_name = file_name.to_os_string();
    state_name.push(".shell-state-v1.json");
    Ok(workspace_path.with_file_name(state_name))
}

fn pending_path(path: &Path) -> PathBuf {
    let mut pending = path.as_os_str().to_os_string();
    pending.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(pending)
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("write {}", path.display()))
}

fn replace_file(destination: &Path, replacement: &Path) -> Result<()> {
    let destination = wide_null(destination);
    let replacement = wide_null(replacement);
    // SAFETY: Both UTF-16 buffers are NUL terminated and live for this call.
    unsafe {
        ReplaceFileW(
            PCWSTR(destination.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .context("replace shell state atomically")
}

fn move_new_file(destination: &Path, replacement: &Path) -> Result<()> {
    let destination = wide_null(destination);
    let replacement = wide_null(replacement);
    // SAFETY: Both UTF-16 buffers are NUL terminated and live for this call.
    unsafe {
        MoveFileExW(
            PCWSTR(replacement.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .context("publish shell state atomically")
}

fn wide_null(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_contract::{GraphCanvas, GraphGeneration, GraphSurface, Manifold};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn state() -> ShellStateV1 {
        let mut graph_view =
            GraphViewState::for_archive(GraphGeneration(9), [7; 32], Some([8; 32]));
        graph_view.surface = GraphSurface::Atlas;
        graph_view.manifold = Manifold::Hopf;
        graph_view.canvas = GraphCanvas::Grid;
        ShellStateV1 {
            format: FORMAT.into(),
            left_open: true,
            right_open: true,
            right_sidebar_page: RightSidebarPage::Kammi,
            left_sidebar_width: 344.0,
            right_sidebar_width: 320.0,
            drawer_open: true,
            drawer_full_page: false,
            drawer_height: 420.0,
            atlas_width: 336.0,
            drawer_tab: DrawerTab::Graph,
            graph_sidebar_panel: GraphSidebarPanel::Registry,
            graph_view,
        }
    }

    #[test]
    fn persisted_view_rebinds_to_current_scene_authority() {
        let state = state();
        let current = SceneAuthority::Archive {
            generation: GraphGeneration(12),
            cohort_hash: [3; 32],
            product_index_hash: Some([4; 32]),
        };
        let rebound = state.rebind_graph_view(current);
        assert_eq!(rebound.authority, current);
        assert_eq!(rebound.surface, GraphSurface::Atlas);
        assert_eq!(rebound.manifold, Manifold::Hopf);
    }

    #[test]
    fn shell_state_round_trips_atomically() -> Result<()> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "phoenix-shell-state-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        let workspace = root.join("workspace-v1.json");
        let expected = state();
        expected.save(&workspace)?;
        let actual = ShellStateV1::load(&workspace)?.context("state missing")?;
        assert_eq!(actual.drawer_tab, DrawerTab::Graph);
        assert_eq!(actual.graph_view.surface, GraphSurface::Atlas);
        assert_eq!(actual.graph_view.manifold, Manifold::Hopf);
        assert_eq!(actual.graph_view.canvas, GraphCanvas::Grid);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn legacy_entity_lane_bits_are_migrated_when_shell_state_loads() -> Result<()> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "phoenix-shell-state-legacy-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        let workspace = root.join("workspace-v1.json");
        let mut legacy = state();
        legacy.graph_view.families = phoenix_scene_contract::FamilyMask(
            phoenix_scene_contract::FamilyMask::ALL.0
                | phoenix_scene_contract::FamilyMask::CHARACTERS.0,
        );
        let path = state_path(&workspace)?;
        fs::write(&path, serde_json::to_vec_pretty(&legacy)?)?;

        let loaded = ShellStateV1::load(&workspace)?.context("state missing")?;
        assert_eq!(
            loaded.graph_view.families,
            phoenix_scene_contract::FamilyMask::ALL
        );
        assert!(loaded
            .graph_view
            .entity_families
            .contains(phoenix_scene_contract::FamilyMask::CHARACTERS));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
