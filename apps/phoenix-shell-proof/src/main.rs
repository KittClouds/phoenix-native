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
use phoenix_app_core::{PhoenixKernel, PhoenixReleaseManifestV1};
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
    if let Err(error) = configure_analysis_runtime(&arguments) {
        fail_start("PHOENIX_ANALYSIS_RUNTIME_ARGUMENT_FAILED", error);
    }
    let proof_mode = arguments.iter().any(|argument| argument == "--proof");
    let soak_mode = arguments.iter().any(|argument| argument == "--soak");
    let design_preview = arguments
        .iter()
        .any(|argument| argument == "--design-preview");
    let require_full_scene = arguments
        .iter()
        .any(|argument| argument == "--require-full-scene");
    let footer_motion_arm = match footer_motion_argument(&arguments) {
        Ok(arm) => arm,
        Err(error) => fail_start("PHOENIX_FOOTER_MOTION_ARGUMENT_FAILED", error),
    };
    let release_manifest_only = arguments
        .iter()
        .any(|argument| argument == "--release-manifest-only");
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
    let workspace_override = match path_argument(&arguments, "--workspace") {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_WORKSPACE_ARGUMENT_FAILED", error),
    };
    let freeze_release_manifest = match path_argument(&arguments, "--freeze-release-manifest") {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_RELEASE_MANIFEST_ARGUMENT_FAILED", error),
    };
    let verify_release_manifest = match path_argument(&arguments, "--verify-release-manifest") {
        Ok(path) => path,
        Err(error) => fail_start("PHOENIX_RELEASE_MANIFEST_ARGUMENT_FAILED", error),
    };
    if freeze_release_manifest.is_some() && verify_release_manifest.is_some() {
        fail_start(
            "PHOENIX_RELEASE_MANIFEST_MODE_CONFLICT",
            "freeze and verify release-manifest modes are mutually exclusive",
        );
    }
    if release_manifest_only
        && freeze_release_manifest.is_none()
        && verify_release_manifest.is_none()
    {
        fail_start(
            "PHOENIX_RELEASE_MANIFEST_MODE_REQUIRED",
            "--release-manifest-only requires --freeze-release-manifest or --verify-release-manifest",
        );
    }
    if let Err(error) = validate_preview_authority(design_preview, publication_root.as_deref()) {
        fail_start("PHOENIX_SCENE_AUTHORITY_MODE_CONFLICT", error);
    }
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
    let generated_proof_workspace = isolated_mode && workspace_override.is_none();
    let workspace_path = if let Some(path) = workspace_override {
        path
    } else if generated_proof_workspace {
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
    report_runtime_identity(&workspace_path, footer_motion_arm, arguments.len());
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
    if let Err(error) = apply_release_manifest_mode(
        &kernel,
        freeze_release_manifest.as_deref(),
        verify_release_manifest.as_deref(),
    ) {
        fail_start("PHOENIX_RELEASE_MANIFEST_FAILED", error);
    }
    report_active_publication(&kernel, publication_receipt);
    if release_manifest_only {
        if let Err(error) = kernel.shutdown() {
            fail_start("PHOENIX_KERNEL_SHUTDOWN_FAILED", error);
        }
        println!("PHOENIX_RELEASE_MANIFEST_ONLY_COMPLETE");
        return;
    }
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
                    window.set_window_title("Phoenix Native");
                    let kernel = Arc::clone(&app_kernel);
                    let scene_error = scene_error.clone();
                    let shell = cx.new(|cx| {
                        shell::PhoenixShell::new(
                            proof_mode,
                            soak_mode,
                            design_preview,
                            footer_motion_arm,
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
    if generated_proof_workspace {
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

fn apply_release_manifest_mode(
    kernel: &PhoenixKernel,
    freeze_path: Option<&std::path::Path>,
    verify_path: Option<&std::path::Path>,
) -> Result<(), String> {
    if freeze_path.is_none() && verify_path.is_none() {
        return Ok(());
    }
    let current = kernel
        .release_manifest()
        .map_err(|error| format!("current authority is not releasable: {error}"))?;
    if let Some(path) = verify_path {
        let frozen = PhoenixReleaseManifestV1::open(path)
            .map_err(|error| format!("open frozen manifest {}: {error}", path.display()))?;
        let mismatches = frozen.exact_semantic_mismatches(&current);
        if !mismatches.is_empty() {
            return Err(format!(
                "exact cohort drift at {}: {}",
                path.display(),
                mismatches.join(", ")
            ));
        }
        println!(
            "PHOENIX_RELEASE_MANIFEST_VERIFIED path={} document={} revision={} generation={} semantic_digest={}",
            path.display(),
            current.authority.document_id,
            current.authority.document_revision,
            current.authority.scene_generation,
            hex_hash(current.digests.shared_scene_pages),
        );
    }
    if let Some(path) = freeze_path {
        let payload_hash = current
            .write_new(path)
            .map_err(|error| format!("freeze manifest {}: {error}", path.display()))?;
        println!(
            "PHOENIX_RELEASE_MANIFEST_FROZEN path={} document={} revision={} generation={} payload_hash={}",
            path.display(),
            current.authority.document_id,
            current.authority.document_revision,
            current.authority.scene_generation,
            hex_hash(payload_hash),
        );
    }
    Ok(())
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

fn report_runtime_identity(
    workspace_path: &std::path::Path,
    footer_motion_arm: shell::FooterMotionArm,
    argument_count: usize,
) {
    let executable = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "<unavailable>".to_string());
    eprintln!(
        "PHOENIX_RUNTIME_IDENTITY pid={} exe={} workspace={} args={} scheduler=pick-wake-v2 footer_motion={}",
        std::process::id(),
        executable,
        workspace_path.display(),
        argument_count,
        footer_motion_arm.as_str(),
    );
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

fn footer_motion_argument(arguments: &[OsString]) -> Result<shell::FooterMotionArm, &'static str> {
    match text_argument(arguments, "--footer-motion")? {
        None => Ok(shell::FooterMotionArm::default()),
        Some(value) => shell::FooterMotionArm::parse(&value)
            .ok_or("--footer-motion must be pulse, animated, or static"),
    }
}

fn configure_analysis_runtime(arguments: &[OsString]) -> Result<(), &'static str> {
    let producer = path_argument(arguments, "--producer")?;
    let ner = path_argument(arguments, "--ner-model-root")?;
    let nli = path_argument(arguments, "--nli-model-root")?;
    match (producer, ner, nli) {
        (None, None, None) => {}
        (Some(producer), Some(ner), Some(nli)) => {
            std::env::set_var("PHOENIX_NATIVE_PRODUCER", producer);
            std::env::set_var("PHOENIX_NATIVE_NER_MODEL_ROOT", ner);
            std::env::set_var("PHOENIX_NATIVE_NLI_MODEL_ROOT", nli);
        }
        _ => {
            return Err(
                "--producer, --ner-model-root, and --nli-model-root must be supplied together",
            );
        }
    }
    if let Some(source_document_id) = text_argument(arguments, "--source-document-id")? {
        std::env::set_var("PHOENIX_NATIVE_SOURCE_DOCUMENT_ID", source_document_id);
    }
    Ok(())
}

fn validate_preview_authority(
    design_preview: bool,
    publication_root: Option<&std::path::Path>,
) -> Result<(), &'static str> {
    if design_preview && publication_root.is_some() {
        Err(
            "--design-preview cannot open a production publication root; use an explicit archive and product index",
        )
    } else {
        Ok(())
    }
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
                .ok_or("path argument requires a value");
        }
        if let Some(argument) = argument.to_str() {
            let prefix = format!("{name}=");
            if let Some(path) = argument.strip_prefix(&prefix) {
                if path.is_empty() {
                    return Err("path argument requires a value");
                }
                return Ok(Some(PathBuf::from(path)));
            }
        }
    }
    Ok(None)
}

fn text_argument(
    arguments: &[OsString],
    name: &'static str,
) -> Result<Option<String>, &'static str> {
    let mut values = arguments.iter().skip(1);
    while let Some(argument) = values.next() {
        if argument == name {
            return values
                .next()
                .map(|value| value.to_string_lossy().into_owned())
                .filter(|value| !value.is_empty())
                .map(Some)
                .ok_or("text argument requires a value");
        }
        if let Some(argument) = argument.to_str() {
            let prefix = format!("{name}=");
            if let Some(value) = argument.strip_prefix(&prefix) {
                if value.is_empty() {
                    return Err("text argument requires a value");
                }
                return Ok(Some(value.to_owned()));
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
    use super::{
        configure_analysis_runtime, footer_motion_argument, mandatory_runtime_filter,
        scene_publication_root_argument, validate_preview_authority,
    };
    use crate::shell::FooterMotionArm;
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

    #[test]
    fn design_preview_cannot_masquerade_as_backend_publication() {
        assert!(validate_preview_authority(true, Some(std::path::Path::new("authority"))).is_err());
        assert!(validate_preview_authority(true, None).is_ok());
        assert!(validate_preview_authority(false, Some(std::path::Path::new("authority"))).is_ok());
    }

    #[test]
    fn analysis_runtime_arguments_are_all_or_none() {
        let incomplete = [
            OsString::from("phoenix-shell"),
            OsString::from("--producer"),
            OsString::from(r"C:\bin\producer.exe"),
        ];
        assert!(configure_analysis_runtime(&incomplete).is_err());
    }

    #[test]
    fn footer_motion_argument_defaults_to_bounded_motion_and_supports_ab_arms() {
        let default = [OsString::from("phoenix-shell")];
        assert_eq!(footer_motion_argument(&default), Ok(FooterMotionArm::Pulse));

        let pulse_arm = [
            OsString::from("phoenix-shell"),
            OsString::from("--footer-motion=pulse"),
        ];
        assert_eq!(
            footer_motion_argument(&pulse_arm),
            Ok(FooterMotionArm::Pulse)
        );

        let static_arm = [
            OsString::from("phoenix-shell"),
            OsString::from("--footer-motion=static"),
        ];
        assert_eq!(
            footer_motion_argument(&static_arm),
            Ok(FooterMotionArm::Static)
        );

        let invalid = [
            OsString::from("phoenix-shell"),
            OsString::from("--footer-motion"),
            OsString::from("fast"),
        ];
        assert!(footer_motion_argument(&invalid).is_err());
    }
}
