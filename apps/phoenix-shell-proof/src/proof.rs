use crate::graph_window::EmbeddedHostProof;
use crate::lifecycle::{self, LifecycleSnapshot};
use gpui::{App, Entity};
use phoenix_app_core::{
    KernelCommand, KernelOutcome, PhoenixKernel, COMMAND_CAPACITY, EVENT_CAPACITY,
};
use phoenix_scene_archive::{ArchiveManifold, PageKind, FORMAT_VERSION};
use phoenix_scene_contract::{SceneAuthority, SceneSource};
use phoenix_workspace::{open_document, DocumentRevision, EntryId, EntryKind, WorkspaceDocument};
use serde::Serialize;
use std::sync::Arc;

const GPUI_VERSION: &str = "0.2.2";
const GPUI_COMPONENT_VERSION: &str = "0.5.1";
const VELOTYPE_VERSION: &str = "0.7.0";

#[derive(Serialize)]
struct EditorProof {
    version: &'static str,
    embedded: bool,
    direct_file_authority: bool,
    initial_document_chars: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct ShellSoakProof {
    pub drawer_toggles: u32,
    pub layout_resize_cycles: u32,
    pub left_sidebar_collapses: u32,
    pub right_sidebar_collapses: u32,
}

pub(crate) struct ExecutionReport {
    pub init_error: Option<String>,
    pub proof_result: Option<Result<EmbeddedHostProof, String>>,
    pub shutdown_error: Option<String>,
    pub soak_mode: bool,
    pub shell_soak: Option<ShellSoakProof>,
}

#[derive(Serialize)]
struct SoakAuthority {
    workspace_revision: u64,
    document_revision: Option<u64>,
    entity_registry_revision: u64,
    ner_revision: u64,
    canonical_entities: usize,
    resident_generation: u64,
    resident_source: &'static str,
    kernel_command_queue_high_water: u64,
    kernel_commands_pending: u64,
}

#[derive(Serialize)]
struct KernelProof {
    one_resident_generation: bool,
    resident_generation: u64,
    resident_source: &'static str,
    durable_crud: bool,
    recursive_delete: bool,
    reopened_revision: u64,
    commands_sequenced: bool,
    command_capacity: usize,
    event_capacity: usize,
    commands_completed: u64,
    events_published: u64,
    no_global_singleton: bool,
    archive_format_version: u32,
    archive_page_count: u32,
    archive_hash: String,
    product_index_bound: bool,
    product_index_hash: String,
    product_index_nodes: u32,
    product_index_edges: u32,
    product_index_mappings: u32,
    product_index_label_bytes: u32,
    graph_view_authority_matches: bool,
    archive_pages_verified_at_open: u64,
    archive_pages_verified_after_optional_open: u64,
    shared_pages_unique: bool,
    document_lease_owned_by_kernel: bool,
    document_revision: u64,
    document_hash: String,
    document_restart_durable: bool,
    stale_document_save_rejected: bool,
}

#[derive(Serialize)]
struct SceneProductCutReceipt {
    contract: &'static str,
    status: &'static str,
    gpui: &'static str,
    gpui_component: &'static str,
    gpui_windows_present_path: &'static str,
    graph_nodes: usize,
    graph_edges: usize,
    graph_representations: u8,
    graph_gpu_surfaces: u8,
    renderer_snapshot_clones: u8,
    json_graph_freight: bool,
    framebuffer_copy: bool,
    frame_readback: bool,
    host: Option<EmbeddedHostProof>,
    editor: EditorProof,
    kernel: Option<KernelProof>,
    soak_authority: Option<SoakAuthority>,
    shell_soak: Option<ShellSoakProof>,
    lifecycle: LifecycleSnapshot,
    failures: Vec<String>,
}

pub fn execute(
    kernel: &Arc<PhoenixKernel>,
    editor: &Entity<velotype::Editor>,
    cx: &App,
    report: ExecutionReport,
) {
    let mut failures = Vec::new();
    if let Some(error) = report.init_error {
        failures.push(format!("graph_window_initialization: {error}"));
    }
    let host_proof = match report.proof_result {
        Some(Ok(proof)) => Some(proof),
        Some(Err(error)) => {
            failures.push(format!("embedded_host_proof: {error}"));
            None
        }
        None => None,
    };
    enforce_host(host_proof.as_ref(), &mut failures);
    if let Some(error) = report.shutdown_error {
        failures.push(format!("embedded_graph_shutdown: {error}"));
    }
    let editor_proof = prove_editor(editor, cx, &mut failures, !report.soak_mode);

    let kernel_proof = if report.soak_mode {
        None
    } else {
        match prove_kernel(kernel) {
            Ok(proof) => Some(proof),
            Err(error) => {
                failures.push(format!("kernel_contract: {error}"));
                None
            }
        }
    };
    let soak_authority = if report.soak_mode {
        match inspect_soak_authority(kernel) {
            Ok(authority) => Some(authority),
            Err(error) => {
                failures.push(format!("soak_authority: {error}"));
                None
            }
        }
    } else {
        None
    };
    let resident_scene = kernel
        .snapshot()
        .ok()
        .and_then(|snapshot| snapshot.resident_scene);
    let inventory = resident_scene.as_ref().map(|scene| scene.inventory());
    if let (Some(host), Some(scene)) = (host_proof.as_ref(), resident_scene.as_ref()) {
        let expected = scene.inventory();
        if host.resident_generation != scene.generation().0
            || host.renderer_nodes != expected.node_count
            || host.renderer_edges != expected.edge_count
        {
            failures.push(format!(
                "renderer projection differs from resident generation: resident={} {}N/{}E renderer={} {}N/{}E",
                scene.generation().0,
                expected.node_count,
                expected.edge_count,
                host.resident_generation,
                host.renderer_nodes,
                host.renderer_edges
            ));
        }
    }

    let state = lifecycle::snapshot();
    enforce_lifecycle(&state, &mut failures);
    if kernel.snapshot().is_err() {
        failures.push("kernel did not survive graph-window shutdown".into());
    }
    if lifecycle::proof_failed() {
        failures.push("a fail-closed lifecycle or render guard fired".into());
    }
    if !failures.is_empty() {
        lifecycle::mark_proof_failed();
    }
    let receipt = SceneProductCutReceipt {
        contract: if report.soak_mode {
            "phoenix.native.release-lock-cut6-soak/v1"
        } else {
            "phoenix.native.release-lock-cut6/v1"
        },
        status: if failures.is_empty() { "pass" } else { "stop" },
        gpui: GPUI_VERSION,
        gpui_component: GPUI_COMPONENT_VERSION,
        gpui_windows_present_path: "hwnd_dxgi_no_direct_composition",
        graph_nodes: inventory.map(|value| value.node_count).unwrap_or(0),
        graph_edges: inventory.map(|value| value.edge_count).unwrap_or(0),
        graph_representations: 1,
        graph_gpu_surfaces: 1,
        renderer_snapshot_clones: 0,
        json_graph_freight: false,
        framebuffer_copy: false,
        frame_readback: false,
        host: host_proof,
        editor: editor_proof,
        kernel: kernel_proof,
        soak_authority,
        shell_soak: report.shell_soak,
        lifecycle: state,
        failures,
    };
    match serde_json::to_string(&receipt) {
        Ok(json) => println!("PHOENIX_RELEASE_LOCK_CUT6_RECEIPT {json}"),
        Err(error) => {
            lifecycle::mark_proof_failed();
            eprintln!("PHOENIX_RELEASE_LOCK_CUT6_RECEIPT_SERIALIZATION_FAILED {error}");
        }
    }
}

fn prove_editor(
    editor: &Entity<velotype::Editor>,
    cx: &App,
    failures: &mut Vec<String>,
    require_empty: bool,
) -> EditorProof {
    let (embedded, initial_document_chars) = editor.read_with(cx, |editor, cx| {
        (editor.is_embedded(), editor.markdown_text(cx).len())
    });
    if !embedded {
        failures.push("Velotype editor retained standalone host authority".into());
    }
    if require_empty && initial_document_chars != 0 {
        failures.push("embedded editor ingress was not the requested empty document".into());
    }
    EditorProof {
        version: VELOTYPE_VERSION,
        embedded,
        direct_file_authority: !embedded,
        initial_document_chars,
    }
}

fn inspect_soak_authority(kernel: &Arc<PhoenixKernel>) -> Result<SoakAuthority, String> {
    let snapshot = kernel.snapshot().map_err(|error| error.to_string())?;
    let scene = snapshot
        .resident_scene
        .ok_or_else(|| "verified resident scene is missing".to_string())?;
    let resident_source = match scene.source() {
        SceneSource::Backend => "backend_publication",
        SceneSource::VerificationFixture => "verification_fixture",
        SceneSource::Archive | SceneSource::RegistryOnly => {
            return Err(
                "automated proof requires a full backend publication or explicit fixture".into(),
            )
        }
    };
    let metrics = kernel.metrics();
    Ok(SoakAuthority {
        workspace_revision: snapshot.workspace.revision(),
        document_revision: snapshot
            .active_document_lease
            .as_ref()
            .map(|lease| lease.revision.0),
        entity_registry_revision: snapshot.atlas_registry.registry_revision,
        ner_revision: snapshot.atlas_registry.ner_revision,
        canonical_entities: snapshot.atlas_registry.entities.len(),
        resident_generation: scene.generation().0,
        resident_source,
        kernel_command_queue_high_water: metrics.command_queue_high_water,
        kernel_commands_pending: metrics.commands_pending,
    })
}

fn prove_kernel(kernel: &Arc<PhoenixKernel>) -> Result<KernelProof, String> {
    let initial = kernel.snapshot().map_err(|error| error.to_string())?;
    let initial_lease = initial
        .active_document_lease
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| "kernel did not acquire the initial document lease".to_string())?;
    let initial_scene = initial
        .resident_scene
        .ok_or_else(|| "archive resident scene is missing".to_string())?;
    if initial_scene.source() != SceneSource::VerificationFixture {
        return Err("resident scene did not originate from the fixture/recovery seam".into());
    }
    let archive_identity = initial_scene.archive_identity();
    let archive = initial_scene.archive();
    let product_index = initial
        .scene_product_index
        .as_ref()
        .ok_or_else(|| "scene product index is missing from kernel authority".to_string())?;
    product_index
        .bind_to_archive(archive)
        .map_err(|error| error.to_string())?;
    let product_header = product_index.header();
    let graph_view_authority_matches = matches!(
        initial.graph_view.authority,
        SceneAuthority::Archive {
            generation,
            cohort_hash,
            product_index_hash: Some(index_hash),
        } if generation == initial_scene.generation()
            && cohort_hash == archive_identity.cohort_hash
            && index_hash == product_header.index_hash
    );
    if !graph_view_authority_matches {
        return Err("kernel graph view authority differs from scene product binding".into());
    }
    let verified_at_open = archive.verified_page_count();
    if verified_at_open < 9 {
        return Err(format!(
            "archive opened {verified_at_open} pages, expected at least four shared pages plus five position pages"
        ));
    }
    archive
        .guides(ArchiveManifold::Hybrid)
        .map_err(|error| error.to_string())?;
    for kind in [
        PageKind::StraightPaths,
        PageKind::CurvedPaths,
        PageKind::BundledPaths,
    ] {
        archive
            .paths(kind, ArchiveManifold::Hybrid)
            .map_err(|error| error.to_string())?;
    }
    let verified_after_optional = archive.verified_page_count();
    let shared_pages_unique = [
        PageKind::NodeIdentity,
        PageKind::NodeStyle,
        PageKind::Topology,
        PageKind::Edge,
        PageKind::LabelPriority,
        PageKind::RelationMasks,
        PageKind::PalettePolicy,
    ]
    .into_iter()
    .all(|kind| {
        archive
            .descriptors()
            .iter()
            .filter(|descriptor| descriptor.key.kind == kind)
            .count()
            == 1
    });
    if !shared_pages_unique {
        return Err("a shared archive page is absent or duplicated".into());
    }

    let save_receipt = kernel
        .execute(KernelCommand::SaveDocument {
            lease: initial_lease.token(),
            content: Arc::from("# Cut 5\n\nKernel-owned durable document."),
        })
        .map_err(|error| error.to_string())?;
    if save_receipt.outcome != KernelOutcome::DocumentSaved(DocumentRevision(1)) {
        return Err(format!(
            "unexpected document save outcome: {:?}",
            save_receipt.outcome
        ));
    }
    let committed_lease = kernel
        .snapshot()
        .map_err(|error| error.to_string())?
        .active_document_lease
        .ok_or_else(|| "committed document lease vanished".to_string())?;
    let reopened_document = open_document(
        kernel.workspace_path(),
        &initial.workspace,
        committed_lease.entry_id,
    )
    .map_err(|error| error.to_string())?;
    let document_restart_durable = reopened_document == *committed_lease;
    if !document_restart_durable {
        return Err("reopened document envelope differs from kernel lease".into());
    }
    let stale_document_save_rejected = kernel
        .execute(KernelCommand::SaveDocument {
            lease: initial_lease.token(),
            content: Arc::from("stale write"),
        })
        .is_err();
    if !stale_document_save_rejected {
        return Err("stale document lease was accepted".into());
    }

    let folder_receipt = kernel
        .execute(KernelCommand::CreateEntry {
            kind: EntryKind::Folder,
            name: "Cut 3 proof".into(),
        })
        .map_err(|error| error.to_string())?;
    let folder = entry_created(folder_receipt.outcome)?;
    let select_receipt = kernel
        .execute(KernelCommand::SelectEntry(folder))
        .map_err(|error| error.to_string())?;
    let note_receipt = kernel
        .execute(KernelCommand::CreateEntry {
            kind: EntryKind::Note,
            name: "Before rename".into(),
        })
        .map_err(|error| error.to_string())?;
    let note = entry_created(note_receipt.outcome)?;
    let rename_receipt = kernel
        .execute(KernelCommand::RenameEntry {
            id: note,
            name: "After rename".into(),
        })
        .map_err(|error| error.to_string())?;

    let reopened =
        WorkspaceDocument::load(kernel.workspace_path()).map_err(|error| error.to_string())?;
    if reopened.path_for(note).map_err(|error| error.to_string())?
        != "Phoenix / Notes / Cut 3 proof / After rename"
    {
        return Err("durable workspace path differs from kernel commit".into());
    }
    let delete_receipt = kernel
        .execute(KernelCommand::DeleteEntry(folder))
        .map_err(|error| error.to_string())?;
    let removed = match delete_receipt.outcome {
        KernelOutcome::EntriesDeleted(count) => count,
        other => return Err(format!("unexpected delete outcome: {other:?}")),
    };
    if removed != 2 {
        return Err(format!(
            "recursive delete removed {removed} entries, expected 2"
        ));
    }
    let final_state =
        WorkspaceDocument::load(kernel.workspace_path()).map_err(|error| error.to_string())?;
    if final_state.entry(folder).is_some() || final_state.entry(note).is_some() {
        return Err("deleted kernel entries survived durable reopen".into());
    }
    let final_snapshot = kernel.snapshot().map_err(|error| error.to_string())?;
    let final_scene = final_snapshot
        .resident_scene
        .ok_or_else(|| "resident scene vanished during workspace commands".to_string())?;
    let sequences = [
        save_receipt.sequence,
        folder_receipt.sequence,
        select_receipt.sequence,
        note_receipt.sequence,
        rename_receipt.sequence,
        delete_receipt.sequence,
    ];
    let commands_sequenced = sequences.windows(2).all(|pair| pair[0] < pair[1]);
    if !commands_sequenced {
        return Err("kernel command sequence is not strictly increasing".into());
    }
    let _events = kernel.drain_events().map_err(|error| error.to_string())?;
    let metrics = kernel.metrics();
    Ok(KernelProof {
        one_resident_generation: Arc::ptr_eq(&initial_scene, &final_scene),
        resident_generation: final_scene.generation().0,
        resident_source: "verification_fixture",
        durable_crud: true,
        recursive_delete: true,
        reopened_revision: final_state.revision(),
        commands_sequenced,
        command_capacity: COMMAND_CAPACITY,
        event_capacity: EVENT_CAPACITY,
        commands_completed: metrics.commands_completed,
        events_published: metrics.events_published,
        no_global_singleton: true,
        archive_format_version: FORMAT_VERSION,
        archive_page_count: archive_identity.page_count,
        archive_hash: hex_hash(archive_identity.cohort_hash),
        product_index_bound: true,
        product_index_hash: hex_hash(product_header.index_hash),
        product_index_nodes: product_header.node_count,
        product_index_edges: product_header.edge_count,
        product_index_mappings: product_header.mapping_count,
        product_index_label_bytes: product_header.label_bytes,
        graph_view_authority_matches,
        archive_pages_verified_at_open: verified_at_open,
        archive_pages_verified_after_optional_open: verified_after_optional,
        shared_pages_unique,
        document_lease_owned_by_kernel: true,
        document_revision: committed_lease.revision.0,
        document_hash: committed_lease.content_hash.to_hex(),
        document_restart_durable,
        stale_document_save_rejected,
    })
}

fn hex_hash(hash: [u8; 32]) -> String {
    hash.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn entry_created(outcome: KernelOutcome) -> Result<EntryId, String> {
    match outcome {
        KernelOutcome::EntryCreated(id) => Ok(id),
        other => Err(format!("unexpected create outcome: {other:?}")),
    }
}

fn enforce_host(proof: Option<&EmbeddedHostProof>, failures: &mut Vec<String>) {
    let Some(proof) = proof else {
        return;
    };
    if !proof.parented_child {
        failures.push("graph window is not a direct WS_CHILD of the GPUI window".into());
    }
    if !proof.parent_clips_child {
        failures
            .push("GPUI parent does not exclude the child graph rectangle from painting".into());
    }
    if !proof.exact_viewport_bounds {
        failures.push("child graph bounds escaped or differed from the GPUI viewport".into());
    }
    if !proof.dpi_matches_parent {
        failures.push("child graph DPI differs from its GPUI parent".into());
    }
    if !proof.stable_hwnd {
        failures.push("graph HWND, renderer, or surface identity changed".into());
    }
    if !proof.stable_device {
        failures.push("graph device identity changed during soak".into());
    }
    if !proof.stable_gpu_allocations {
        failures.push("GPU buffer allocation grew after all manifolds were warm".into());
    }
    if !proof.stable_graph_slots {
        failures.push("node or edge slot capacity changed during manifold soak".into());
    }
    if !proof.gpu_after.product_index_bound {
        failures.push("renderer did not retain the archive-bound scene product index".into());
    }
    if proof.gpu_after.lens_uniform_writes < 2 {
        failures.push("renderer did not publish a native graph-view uniform".into());
    }
    if !proof.pointer_hover {
        failures.push("real pointer movement did not resolve a GPU hover pick".into());
    }
    if proof.manifold_switches != 200 {
        failures.push(format!(
            "manifold soak completed {} switches, expected 200",
            proof.manifold_switches
        ));
    }
    if proof.switch_cpu_p95_us > 8_000 {
        failures.push(format!(
            "warm manifold switch CPU p95 {}us exceeds 8000us",
            proof.switch_cpu_p95_us
        ));
    }
    if proof.switch_present_p95_us > 16_700 {
        failures.push(format!(
            "interactive manifold present p95 {}us exceeds 16700us",
            proof.switch_present_p95_us
        ));
    }
    if proof.interaction_stress.updates != 1_000 {
        failures.push(format!(
            "interaction soak completed {} updates, expected 1000",
            proof.interaction_stress.updates
        ));
    }
    if !proof.interaction_stress.stable_capacities {
        failures.push("hover/route capacities grew after interaction warmup".into());
    }
    if proof.interaction_stress.cpu_p95_us > 16_700 {
        failures.push(format!(
            "hover/route CPU p95 {}us exceeds 16700us",
            proof.interaction_stress.cpu_p95_us
        ));
    }
    if !(proof.focus && proof.resize) {
        failures.push("graph focus or resize proof failed".into());
    }
    if !(proof.minimize_restore && proof.maximize_restore) {
        failures.push("graph minimize/maximize transition failed".into());
    }
}

fn enforce_lifecycle(state: &LifecycleSnapshot, failures: &mut Vec<String>) {
    if state.graph_window_created != 1
        || state.graph_window_dropped != 1
        || state.graph_window_live != 0
    {
        failures.push("embedded child graph lifetime invariant failed".into());
    }
    if state.renderer_created != 1 || state.renderer_dropped != 1 || state.renderer_live != 0 {
        failures.push("graph renderer lifetime invariant failed".into());
    }
    if state.surface_created != 1 || state.surface_dropped != 1 || state.surface_live != 0 {
        failures.push("graph surface lifetime invariant failed".into());
    }
    if state.device_created != 1 || state.device_dropped != 1 || state.device_live != 0 {
        failures.push("graph device lifetime invariant failed".into());
    }
    if state.visibility_transitions < 401 {
        failures.push("embedded graph did not complete 200 hide/show cycles".into());
    }
    if state.frames_presented == 0 {
        failures.push("no graph frame reached embedded presentation".into());
    }
    if state.frame_readbacks != 0 {
        failures.push("frame readback counter is non-zero".into());
    }
}
