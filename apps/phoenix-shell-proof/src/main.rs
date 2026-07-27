#[cfg(feature = "legacy-graph-adapter")]
compile_error!(
    "PHOENIX_LEGACY_GRAPH_ADAPTER_FORBIDDEN: the native release has no legacy graph fallback"
);

mod graph_window;
mod lifecycle;
mod proof;
mod shell;

use gpui::{px, size, App, AppContext as _, Application, Bounds, WindowBounds, WindowOptions};
use gpui_component::Root;
use phoenix_app_core::PhoenixKernel;
use phoenix_scene_archive::PhoenixSceneArchiveV1;
use phoenix_scene_contract::{ResidentScene, ResidentSceneLoadError, SceneSource};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use phoenix_scene_publisher::{ScenePublicationReceipt, ScenePublicationStore};
use phoenix_workspace::default_workspace_path;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::{filter::Directive, EnvFilter};
use velotype::VelotypeAssets;

fn main() {
    configure_gpui_child_window_hosting();
    initialize_tracing();
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let proof_mode = arguments.iter().any(|argument| argument == "--proof");
    let soak_mode = arguments.iter().any(|argument| argument == "--soak");
    let design_preview = arguments
        .iter()
        .any(|argument| argument == "--design-preview");
    let require_full_scene = arguments
        .iter()
        .any(|argument| argument == "--require-full-scene");
    if proof_mode && soak_mode {
        fail_start(
            "PHOENIX_RUN_MODE_CONFLICT",
            "--proof and --soak are mutually exclusive",
        );
    }
    let automated_mode = proof_mode || soak_mode;
    let isolated_mode = automated_mode || design_preview;
    let archive_path = match scene_archive_argument(&arguments) {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_SCENE_ARCHIVE_ARGUMENT_FAILED", error),
    };
    let product_index_path = match scene_product_index_argument(&arguments) {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_SCENE_PRODUCT_INDEX_ARGUMENT_FAILED", error),
    };
    let publication_root = match scene_publication_root_argument(&arguments) {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_SCENE_PUBLICATION_ROOT_ARGUMENT_FAILED", error),
    };
    if publication_root.is_some() && (archive_path.is_some() || product_index_path.is_some()) {
        fail_start(
            "PHOENIX_SCENE_AUTHORITY_CONFLICT",
            "--scene-publication-root cannot be combined with archive or product-index paths",
        );
    }
    if automated_mode && product_index_path.is_none() && publication_root.is_none() {
        fail_start(
            "PHOENIX_SCENE_PRODUCT_INDEX_REQUIRED",
            "--proof and --soak require a product index or publication root",
        );
    }
    if product_index_path.is_some() && archive_path.is_none() {
        fail_start(
            "PHOENIX_SCENE_PRODUCT_INDEX_WITHOUT_ARCHIVE",
            "--scene-product-index requires --scene-archive",
        );
    }
    let workspace_path = if isolated_mode {
        proof_workspace_path()
    } else {
        match default_workspace_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("PHOENIX_KERNEL_WORKSPACE_PATH_FAILED {error}");
                std::process::exit(1);
            }
        }
    };
    let mut publication_receipt = None;
    let mut published_product_index = None;
    let (initial_scene, scene_error) = match publication_root.as_ref() {
        Some(root) => match ScenePublicationStore::open_current_at(root) {
            Ok(Some(published)) => {
                publication_receipt = Some(published.receipt);
                published_product_index = Some(published.product_index);
                (Some(published.scene), None)
            }
            Ok(None) => fail_start(
                "PHOENIX_SCENE_PUBLICATION_MISSING",
                "publication root has no current manifest",
            ),
            Err(error) => fail_start("PHOENIX_SCENE_PUBLICATION_OPEN_FAILED", error),
        },
        None => match archive_path.as_ref() {
            Some(path) => match load_resident_scene(path) {
                Ok(scene) => (Some(scene), None),
                Err(error) if automated_mode => fail_start(error.code(), error.detail()),
                Err(error) => {
                    eprintln!("{error}");
                    (None, Some(error))
                }
            },
            None if automated_mode => fail_start(
                "PHOENIX_SCENE_ARCHIVE_REQUIRED",
                "--proof and --soak require a scene archive or publication root",
            ),
            None => (None, None),
        },
    };
    let initial_product_index = match (published_product_index, product_index_path) {
        (Some(index), None) => Some(index),
        (Some(_), Some(_)) => fail_start(
            "PHOENIX_SCENE_AUTHORITY_CONFLICT",
            "publication root cannot be combined with a product index path",
        ),
        (None, Some(path)) => {
            let Some(scene) = initial_scene.as_ref() else {
                fail_start(
                    "PHOENIX_SCENE_PRODUCT_INDEX_WITHOUT_ARCHIVE",
                    "--scene-product-index requires --scene-archive",
                )
            };
            let index = match PhoenixSceneProductIndexV1::open(&path) {
                Ok(index) => index,
                Err(error) => fail_start("PHOENIX_SCENE_PRODUCT_INDEX_OPEN_FAILED", error),
            };
            if let Err(error) = index.bind_to_archive(scene.archive()) {
                fail_start("PHOENIX_SCENE_PRODUCT_INDEX_BIND_FAILED", error);
            }
            Some(Arc::new(index))
        }
        (None, None) => None,
    };
    let kernel_result = if let Some(root) = publication_root {
        PhoenixKernel::start_production_at_root(workspace_path.clone(), root)
    } else if archive_path.is_some() {
        PhoenixKernel::start_with_product_index(
            workspace_path.clone(),
            initial_scene,
            initial_product_index,
        )
    } else {
        PhoenixKernel::start_production(workspace_path.clone())
    };
    let kernel = match kernel_result {
        Ok(kernel) => kernel,
        Err(error) => {
            eprintln!("PHOENIX_KERNEL_START_FAILED {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = validate_required_full_scene(&kernel, require_full_scene) {
        fail_start("PHOENIX_FULL_SCENE_REQUIRED", error);
    }
    report_active_publication(&kernel, publication_receipt);
    let app_kernel = Arc::clone(&kernel);
    Application::new()
        .with_assets(VelotypeAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            velotype::init_embedded(cx);
            // GPUI keeps its Windows message loop alive after the last window closes.
            // Phoenix owns one GPUI shell window, so closing it is the application
            // shutdown boundary. Quitting here also gives `on_app_quit` time to stop
            // the child renderer and kernel before the process exits.
            cx.on_window_closed(|cx| cx.quit()).detach();
            let bounds = Bounds::centered(None, size(px(1_280.0), px(800.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    let kernel = Arc::clone(&app_kernel);
                    let scene_error = scene_error.clone();
                    let shell = cx.new(|cx| {
                        shell::PhoenixShell::new(
                            proof_mode,
                            soak_mode,
                            design_preview,
                            kernel,
                            scene_error,
                            window,
                            cx,
                        )
                    });
                    cx.new(|cx| Root::new(shell, window, cx))
                },
            );
            if let Err(error) = opened {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_SHELL_CUT5_WINDOW_OPEN_FAILED {error:#}");
                cx.quit();
            }
        });
    if let Err(error) = kernel.shutdown() {
        lifecycle::mark_proof_failed();
        eprintln!("PHOENIX_KERNEL_SHUTDOWN_FAILED {error}");
    }
    if automated_mode {
        if let Some(parent) = workspace_path.parent() {
            if let Err(error) = std::fs::remove_dir_all(parent) {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_KERNEL_PROOF_CLEANUP_FAILED {error}");
            }
        }
    }
    if lifecycle::proof_failed() {
        std::process::exit(1);
    }
}

fn load_resident_scene(
    path: &std::path::Path,
) -> Result<Arc<ResidentScene>, ResidentSceneLoadError> {
    let archive = PhoenixSceneArchiveV1::open(path).map_err(ResidentSceneLoadError::from)?;
    let scene = ResidentScene::from_archive_with_source(
        Arc::new(archive),
        None,
        SceneSource::VerificationFixture,
    )
    .map_err(ResidentSceneLoadError::from)?;
    Ok(Arc::new(scene))
}

fn report_active_publication(
    kernel: &PhoenixKernel,
    external_receipt: Option<ScenePublicationReceipt>,
) {
    let Ok(snapshot) = kernel.snapshot() else {
        return;
    };
    let Some(receipt) = snapshot.scene_publication.or(external_receipt) else {
        println!("PHOENIX_SCENE_AUTHORITY fixture_or_recovery");
        return;
    };
    println!(
        "PHOENIX_SCENE_PUBLICATION_ACTIVE generation={} kind={:?} registry_revision={} \
         nodes={} edges={} entities={} archive_hash={} product_index_hash={}",
        receipt.generation_id,
        receipt.kind,
        receipt.registry_revision,
        receipt.node_count,
        receipt.edge_count,
        receipt.entity_count,
        hex_hash(receipt.archive_cohort_hash),
        hex_hash(receipt.product_index_hash),
    );
}

fn validate_required_full_scene(
    kernel: &PhoenixKernel,
    required: bool,
) -> Result<(), &'static str> {
    if !required {
        return Ok(());
    }
    let snapshot = kernel
        .snapshot()
        .map_err(|_| "kernel snapshot is unavailable")?;
    let scene = snapshot
        .resident_scene
        .as_ref()
        .ok_or("no resident scene is available")?;
    let inventory = scene.inventory();
    if inventory.node_count == 0 || inventory.edge_count == 0 {
        return Err("resident scene is registry-only or empty");
    }
    if snapshot.scene_product_index.is_none() {
        return Err("resident scene has no verified product index");
    }
    Ok(())
}

fn hex_hash(hash: [u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(64);
    for byte in hash {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(target_os = "windows")]
fn configure_gpui_child_window_hosting() {
    // SAFETY: This is the first operation in `main`, before GPUI, winit, the
    // kernel, tracing, or any worker thread exists. GPUI reads this setting
    // while constructing its Windows platform and then uses an HWND-bound
    // DXGI swap chain whose client painting honors WS_CLIPCHILDREN.
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_gpui_child_window_hosting() {}

fn initialize_tracing() {
    let filter = mandatory_runtime_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("phoenix_shell=info,graph_render_wgpu=info")),
    );
    if let Err(error) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        eprintln!("PHOENIX_SHELL_CUT5_TRACING_INIT_FAILED {error}");
    }
}

fn mandatory_runtime_filter(filter: EnvFilter) -> EnvFilter {
    const WGPU_VULKAN_CONVERSION_CEILING: &str = "wgpu_hal::vulkan::conv=error";
    match WGPU_VULKAN_CONVERSION_CEILING.parse::<Directive>() {
        Ok(directive) => filter.add_directive(directive),
        Err(error) => {
            eprintln!("PHOENIX_RUNTIME_FILTER_INVALID {error}");
            filter
        }
    }
}

fn proof_workspace_path() -> PathBuf {
    std::env::temp_dir()
        .join(format!("phoenix-shell-cut5-proof-{}", std::process::id()))
        .join("workspace.json")
}

fn scene_archive_argument(arguments: &[OsString]) -> Result<Option<PathBuf>, &'static str> {
    path_argument(arguments, "--scene-archive")
}

fn scene_product_index_argument(arguments: &[OsString]) -> Result<Option<PathBuf>, &'static str> {
    path_argument(arguments, "--scene-product-index")
}

fn scene_publication_root_argument(
    arguments: &[OsString],
) -> Result<Option<PathBuf>, &'static str> {
    path_argument(arguments, "--scene-publication-root")
}

fn path_argument(
    arguments: &[OsString],
    name: &'static str,
) -> Result<Option<PathBuf>, &'static str> {
    let mut values = arguments.iter().skip(1);
    while let Some(argument) = values.next() {
        if argument == name {
            return values
                .next()
                .map(PathBuf::from)
                .map(Some)
                .ok_or("scene artifact argument requires a path");
        }
        if let Some(argument) = argument.to_str() {
            let prefix = format!("{name}=");
            if let Some(path) = argument.strip_prefix(&prefix) {
                if path.is_empty() {
                    return Err("scene artifact argument requires a path");
                }
                return Ok(Some(PathBuf::from(path)));
            }
        }
    }
    Ok(None)
}

fn fail_start(marker: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("{marker} {error}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{mandatory_runtime_filter, scene_publication_root_argument};
    use std::ffi::OsString;
    use tracing_subscriber::EnvFilter;

    #[test]
    fn mandatory_runtime_filter_survives_user_warning_filter() {
        let filter = mandatory_runtime_filter(EnvFilter::new("warn"));
        assert!(filter.to_string().contains("wgpu_hal::vulkan::conv=error"));
    }

    #[test]
    fn publication_root_accepts_split_and_equals_arguments() {
        let split = [
            OsString::from("phoenix-shell"),
            OsString::from("--scene-publication-root"),
            OsString::from(r"C:\verified\scene-publications-v1"),
        ];
        let equals = [
            OsString::from("phoenix-shell"),
            OsString::from(r"--scene-publication-root=C:\verified\scene-publications-v1"),
        ];
        let expected = Some(r"C:\verified\scene-publications-v1".into());
        assert_eq!(
            scene_publication_root_argument(&split),
            Ok(expected.clone())
        );
        assert_eq!(scene_publication_root_argument(&equals), Ok(expected));
    }
}
