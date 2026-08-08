mod hash;
mod model;

use anyhow::{anyhow, bail, Context, Result};
use hash::{galaxy_hash, hex_hash};
use hashbrown::HashMap;
use model::{
    AngularReceipt, DocumentReceipt, FrozenCohort, GateReceipt, NativeManifoldReceipt,
    NativeReceipt, ReleaseReceipt, ReleaseResult, CONTRACT,
};
use phoenix_scene_archive::{ArchiveManifold, EdgeRecord, PhoenixSceneArchiveV1, PositionRecord};
use phoenix_scene_product_index::PhoenixSceneProductIndexV1;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    match run() {
        Ok(receipt) => {
            let passed = receipt.result == ReleaseResult::Pass;
            match serde_json::to_string_pretty(&receipt) {
                Ok(json) => println!("{json}"),
                Err(error) => {
                    eprintln!("PHOENIX_RELEASE_LOCK_SERIALIZE_FAILED {error}");
                    std::process::exit(2);
                }
            }
            if !passed {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("PHOENIX_RELEASE_LOCK_FAILED {error:#}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<ReleaseReceipt> {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let manifest_path = required_path(&arguments, "--manifest")?;
    let archive_path = required_path(&arguments, "--scene-archive")?;
    let index_path = required_path(&arguments, "--scene-product-index")?;
    let frozen: FrozenCohort = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("read frozen cohort {}", manifest_path.display()))?,
    )
    .context("decode frozen cohort")?;
    if frozen.contract != CONTRACT {
        bail!(
            "PHOENIX_RELEASE_LOCK_CONTRACT_UNSUPPORTED expected={CONTRACT} actual={}",
            frozen.contract
        );
    }

    let archive = PhoenixSceneArchiveV1::open(&archive_path)
        .with_context(|| format!("open archive {}", archive_path.display()))?;
    let index = PhoenixSceneProductIndexV1::open(&index_path)
        .with_context(|| format!("open product index {}", index_path.display()))?;
    index
        .bind_to_archive(&archive)
        .context("bind product index to archive")?;

    let mut gates = Vec::with_capacity(20);
    let native_manifolds = inspect_native_manifolds(&archive)?;
    let first = native_manifolds
        .first()
        .ok_or_else(|| anyhow!("native archive has no manifold pages"))?;
    let archive_header = archive.header();
    let index_header = index.header();

    gate(
        &mut gates,
        "COHORT_MANIFEST_COMPLETE",
        frozen.manifolds.len() == 5 && frozen_packet_metadata_complete(&frozen),
        format!(
            "frozen_manifolds={} metadata_complete={}",
            frozen.manifolds.len(),
            frozen_packet_metadata_complete(&frozen)
        ),
    );
    let frozen_generations = frozen
        .manifolds
        .iter()
        .map(|manifold| manifold.generation_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    gate(
        &mut gates,
        "ANGULAR_ONE_GENERATION",
        frozen_generations.len() == 1,
        format!("generation_ids={frozen_generations:?}"),
    );
    gate(
        &mut gates,
        "ANGULAR_STABLE_IDENTITIES_AND_TOPOLOGY",
        angular_shared_pages_are_stable(&frozen),
        angular_shared_page_detail(&frozen),
    );
    gate(
        &mut gates,
        "NATIVE_ONE_RESIDENT_GENERATION",
        index_header.binding.archive_generation == archive_header.generation_id,
        format!(
            "archive={} product_index={}",
            archive_header.generation_id, index_header.binding.archive_generation
        ),
    );
    let native_inventory_stable = native_manifolds.iter().all(|manifold| {
        manifold.node_count == first.node_count && manifold.edge_count == first.edge_count
    });
    gate(
        &mut gates,
        "NATIVE_SIX_MANIFOLD_INVENTORY",
        native_inventory_stable && native_manifolds.len() == 6,
        native_inventory_detail(&native_manifolds),
    );

    compare_reference_dimensions(&frozen, first, &mut gates);
    compare_position_pages(&frozen, &native_manifolds, &mut gates);
    gate(
        &mut gates,
        "DOCUMENT_ARCHIVE_BINDING",
        false,
        "PhoenixSceneArchiveV1 does not yet encode note id, note version, or text hash".into(),
    );
    gate(
        &mut gates,
        "MODEL_IDENTITIES_FROZEN",
        frozen.model_runtime.status == "not_used_for_scene_open"
            && frozen.model_runtime.identities.is_empty(),
        format!(
            "status={} identities={:?}",
            frozen.model_runtime.status, frozen.model_runtime.identities
        ),
    );
    gate(
        &mut gates,
        "FAMILIES_REVIEWS_SCOPES_PARITY",
        false,
        "Angular V2 packet exposes no canonical family/review/scope pages to compare".into(),
    );
    gate(
        &mut gates,
        "LABEL_PARITY",
        false,
        "Angular detail string freight is not a canonical node-label page".into(),
    );
    gate(
        &mut gates,
        "GUIDES_AND_PREPARED_PATHS_PARITY",
        false,
        "Angular guide pages and native packed guide/path pages lack a shared canonical digest"
            .into(),
    );
    gate(
        &mut gates,
        "ZERO_JSON_GRAPH_FREIGHT",
        true,
        "native archive and product index opened by mmap; JSON is cohort metadata only".into(),
    );
    gate(
        &mut gates,
        "ZERO_RUNTIME_FALLBACK",
        true,
        "release-lock reader has no compatibility or synthetic fallback path".into(),
    );
    gate(
        &mut gates,
        "LEGACY_ADAPTER_COMPILE_TIME_EXCLUDED",
        true,
        "phoenix-shell legacy-graph-adapter feature is an unconditional compile_error".into(),
    );

    let archive_bytes = fs::metadata(&archive_path)?.len();
    let product_index_bytes = fs::metadata(&index_path)?.len();
    let result = if gates.iter().all(|gate| gate.passed) {
        ReleaseResult::Pass
    } else {
        ReleaseResult::Stop
    };
    Ok(ReleaseReceipt {
        contract: CONTRACT,
        cohort_id: frozen.cohort_id,
        result,
        frozen_document: document_receipt(frozen.document),
        frozen_angular: AngularReceipt {
            captured_at: frozen.captured_at,
            binary_sha256: frozen.angular_runtime.binary_sha256,
            repo_head: frozen.angular_runtime.repo_head,
            dirty_status_sha256: frozen.angular_runtime.dirty_status_sha256,
            page_url: frozen.angular_runtime.page_url,
            packet_schema: frozen.angular_runtime.packet_schema,
            model_status: frozen.model_runtime.status,
            model_identities: frozen.model_runtime.identities,
        },
        native: NativeReceipt {
            binary_sha256: frozen.native_runtime.binary_sha256,
            proof_contract: frozen.native_runtime.proof_contract,
            fallback_count: 0,
            archive_generation: archive_header.generation_id,
            archive_cohort_hash_blake3: hex_hash(archive_header.cohort_hash),
            archive_pages: archive_header.page_count,
            archive_bytes,
            product_index_hash_blake3: hex_hash(index_header.index_hash),
            product_index_bytes,
            node_count: first.node_count,
            edge_count: first.edge_count,
            mapping_count: index_header.mapping_count,
            label_bytes: index_header.label_bytes,
            manifolds: native_manifolds,
        },
        gates,
    })
}

fn inspect_native_manifolds(archive: &PhoenixSceneArchiveV1) -> Result<Vec<NativeManifoldReceipt>> {
    ArchiveManifold::ALL
        .into_iter()
        .map(|manifold| {
            let pages = archive.open_manifold(manifold)?;
            let node_ids = bytemuck::cast_slice(pages.identities);
            let edge_ids = edge_identity_bytes(pages.edges);
            let topology = topology_slot_bytes(pages.identities, pages.topology)?;
            let node_colors = node_color_bytes(pages.styles);
            let edge_colors = edge_color_bytes(pages.edges);
            Ok(NativeManifoldReceipt {
                name: manifold_name(manifold).into(),
                node_count: pages.identities.len(),
                edge_count: pages.edges.len(),
                node_identity_hash: galaxy_hash(node_ids),
                edge_identity_hash: galaxy_hash(&edge_ids),
                topology_hash: galaxy_hash(&topology),
                positions_hash: galaxy_hash(bytemuck::cast_slice::<PositionRecord, u8>(
                    pages.positions,
                )),
                node_colors_hash: galaxy_hash(&node_colors),
                edge_colors_hash: galaxy_hash(&edge_colors),
            })
        })
        .collect()
}

fn edge_identity_bytes(edges: &[EdgeRecord]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(edges.len() * size_of::<u64>());
    for edge in edges {
        bytes.extend_from_slice(&edge.id.to_le_bytes());
    }
    bytes
}

fn topology_slot_bytes(
    identities: &[phoenix_scene_archive::NodeIdentityRecord],
    topology: &[phoenix_scene_archive::TopologyRecord],
) -> Result<Vec<u8>> {
    let mut slots = HashMap::with_capacity(identities.len());
    for (slot, identity) in identities.iter().enumerate() {
        slots.insert(
            identity.id,
            u32::try_from(slot).context("node slot exceeds u32")?,
        );
    }
    let mut bytes = Vec::with_capacity(topology.len() * size_of::<[u32; 2]>());
    for edge in topology {
        let source = slots
            .get(&edge.source_id)
            .ok_or_else(|| anyhow!("topology source {} is missing", edge.source_id))?;
        let target = slots
            .get(&edge.target_id)
            .ok_or_else(|| anyhow!("topology target {} is missing", edge.target_id))?;
        bytes.extend_from_slice(&source.to_le_bytes());
        bytes.extend_from_slice(&target.to_le_bytes());
    }
    Ok(bytes)
}

fn node_color_bytes(styles: &[phoenix_scene_archive::NodeStyleRecord]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(styles.len() * 4);
    for style in styles {
        bytes.extend(style.color.map(channel_to_u8));
    }
    bytes
}

fn edge_color_bytes(edges: &[EdgeRecord]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(edges.len() * 4);
    for edge in edges {
        bytes.extend(edge.color.map(channel_to_u8));
    }
    bytes
}

fn channel_to_u8(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn compare_reference_dimensions(
    frozen: &FrozenCohort,
    native: &NativeManifoldReceipt,
    gates: &mut Vec<GateReceipt>,
) {
    let Some(reference) = frozen
        .manifolds
        .iter()
        .find(|manifold| manifold.name == "hopf")
    else {
        gate(
            gates,
            "REFERENCE_MANIFOLD_PRESENT",
            false,
            "frozen HOPF reference is missing".into(),
        );
        return;
    };
    gate(
        gates,
        "REFERENCE_IDS_AND_COUNTS",
        reference.node_count == native.node_count
            && reference.edge_count == native.edge_count
            && page_hash(reference, "shared/node-identity-keys")
                == Some(native.node_identity_hash.as_str())
            && page_hash(reference, "shared/edge-identity-keys")
                == Some(native.edge_identity_hash.as_str()),
        format!(
            "angular={}N/{}E native={}N/{}E node_ids={} edge_ids={}",
            reference.node_count,
            reference.edge_count,
            native.node_count,
            native.edge_count,
            native.node_identity_hash,
            native.edge_identity_hash
        ),
    );
    compare_hash_gate(
        gates,
        "REFERENCE_TOPOLOGY",
        page_hash(reference, "shared/edge-pairs"),
        &native.topology_hash,
    );
    compare_hash_gate(
        gates,
        "REFERENCE_COLORS",
        page_hash(reference, "manifold/node-colors-rgba8"),
        &native.node_colors_hash,
    );
}

fn compare_position_pages(
    frozen: &FrozenCohort,
    native: &[NativeManifoldReceipt],
    gates: &mut Vec<GateReceipt>,
) {
    for actual in native {
        if actual.name == "hopf" {
            gate(
                gates,
                position_gate_id(&actual.name),
                !actual.positions_hash.is_empty(),
                format!(
                    "native clean-room Hopf page={}; Angular has no qualifying reference",
                    actual.positions_hash
                ),
            );
            continue;
        }
        let reference_name = if actual.name == "torus" {
            "hopf"
        } else {
            actual.name.as_str()
        };
        let expected = frozen
            .manifolds
            .iter()
            .find(|manifold| manifold.name == reference_name);
        let expected_hash =
            expected.and_then(|manifold| page_hash(manifold, "manifold/positions-3d"));
        compare_hash_gate(
            gates,
            position_gate_id(&actual.name),
            expected_hash,
            &actual.positions_hash,
        );
    }
}

fn angular_shared_pages_are_stable(frozen: &FrozenCohort) -> bool {
    let Some(first) = frozen.manifolds.first() else {
        return false;
    };
    ["shared/node-identity-keys", "shared/edge-pairs"]
        .into_iter()
        .all(|id| {
            let expected = page_hash(first, id);
            expected.is_some()
                && frozen
                    .manifolds
                    .iter()
                    .all(|manifold| page_hash(manifold, id) == expected)
        })
}

fn frozen_packet_metadata_complete(frozen: &FrozenCohort) -> bool {
    frozen.manifolds.iter().all(|manifold| {
        !manifold.layout_mode.is_empty()
            && manifold.authority_receipt.contains(&manifold.generation_id)
            && manifold.packet_hash.starts_with("fnv1a64:")
            && manifold.pages.values().all(|page| {
                page.hash.starts_with("fnv1a64:") && (page.elements == 0 || page.bytes > 0)
            })
    })
}

fn angular_shared_page_detail(frozen: &FrozenCohort) -> String {
    frozen
        .manifolds
        .iter()
        .map(|manifold| {
            format!(
                "{}={}N/{}E ids={} topology={}",
                manifold.name,
                manifold.node_count,
                manifold.edge_count,
                page_hash(manifold, "shared/node-identity-keys").unwrap_or("missing"),
                page_hash(manifold, "shared/edge-pairs").unwrap_or("missing")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn native_inventory_detail(native: &[NativeManifoldReceipt]) -> String {
    native
        .iter()
        .map(|manifold| {
            format!(
                "{}={}N/{}E",
                manifold.name, manifold.node_count, manifold.edge_count
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn compare_hash_gate(
    gates: &mut Vec<GateReceipt>,
    id: &'static str,
    expected: Option<&str>,
    actual: &str,
) {
    gate(
        gates,
        id,
        expected == Some(actual),
        format!("expected={} actual={actual}", expected.unwrap_or("missing")),
    );
}

fn page_hash<'a>(manifold: &'a model::FrozenManifold, id: &str) -> Option<&'a str> {
    manifold.pages.get(id).map(|page| page.hash.as_str())
}

fn gate(gates: &mut Vec<GateReceipt>, id: &'static str, passed: bool, detail: String) {
    gates.push(GateReceipt { id, passed, detail });
}

fn document_receipt(document: model::FrozenDocument) -> DocumentReceipt {
    DocumentReceipt {
        note_id: document.note_id,
        title: document.title,
        version: document.version,
        markdown_utf16_chars: document.markdown_utf16_chars,
        markdown_utf8_bytes: document.markdown_utf8_bytes,
        markdown_sha256: document.markdown_sha256,
        plain_text_utf16_chars: document.plain_text_utf16_chars,
        plain_text_utf8_bytes: document.plain_text_utf8_bytes,
        plain_text_sha256: document.plain_text_sha256,
        footer_words: document.footer_words,
        footer_chars_without_line_breaks: document.footer_chars_without_line_breaks,
    }
}

fn manifold_name(manifold: ArchiveManifold) -> &'static str {
    match manifold {
        ArchiveManifold::Hybrid => "hybrid",
        ArchiveManifold::Torus => "torus",
        ArchiveManifold::Hopf => "hopf",
        ArchiveManifold::Caps => "caps",
        ArchiveManifold::Transit => "transit",
        ArchiveManifold::Siegel => "siegel",
    }
}

fn position_gate_id(name: &str) -> &'static str {
    match name {
        "hybrid" => "POSITION_PAGE_HYBRID",
        "torus" => "POSITION_PAGE_TORUS",
        "hopf" => "POSITION_PAGE_HOPF",
        "caps" => "POSITION_PAGE_CAPS",
        "transit" => "POSITION_PAGE_TRANSIT",
        "siegel" => "POSITION_PAGE_SIEGEL",
        _ => "POSITION_PAGE_UNKNOWN",
    }
}

fn required_path(arguments: &[OsString], name: &'static str) -> Result<PathBuf> {
    let mut values = arguments.iter().skip(1);
    while let Some(argument) = values.next() {
        if argument == name {
            return values
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("{name} requires a path"));
        }
    }
    bail!("missing required {name}")
}

#[allow(dead_code)]
fn _assert_paths_are_files(paths: &[&Path]) -> Result<()> {
    for path in paths {
        if !path.is_file() {
            bail!("{} is not a file", path.display());
        }
    }
    Ok(())
}
