use anyhow::{bail, Context, Result};
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};

const GRAPH_RUN_CONTRACT: &str = "phoenix-graph-run-store/v1";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: String,
    manifest_id: String,
    run_handle: String,
    scope_id: String,
    snapshot_id: String,
    sections: BTreeMap<String, Section>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Section {
    identity: String,
    encoding: String,
    row_count: u64,
    raw_bytes: u64,
    compressed_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InspectReceipt {
    contract: &'static str,
    manifest_id: String,
    run_handle: String,
    scope_id: String,
    snapshot_id: String,
    sections: Vec<SectionReceipt>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SectionReceipt {
    name: String,
    identity: String,
    row_count: u64,
    manifest_raw_bytes: u64,
    decoded_raw_bytes: u64,
    raw_size_verified: bool,
    compressed_bytes: u64,
    content_blake3: String,
    json_shape: String,
    output: PathBuf,
}

pub(crate) fn decode_verified_sections(
    root: &Path,
    manifest_path: &Path,
    output: &Path,
) -> Result<InspectReceipt> {
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("read manifest {}", manifest_path.display()))?,
    )?;
    if manifest.schema_version != GRAPH_RUN_CONTRACT {
        bail!("unsupported graph-run contract {}", manifest.schema_version);
    }
    fs::create_dir_all(output)
        .with_context(|| format!("create output directory {}", output.display()))?;
    let mut receipts = Vec::with_capacity(manifest.sections.len());
    for (name, section) in manifest.sections {
        receipts.push(decode_section(root, output, name, section)?);
    }
    Ok(InspectReceipt {
        contract: GRAPH_RUN_CONTRACT,
        manifest_id: manifest.manifest_id,
        run_handle: manifest.run_handle,
        scope_id: manifest.scope_id,
        snapshot_id: manifest.snapshot_id,
        sections: receipts,
    })
}

fn decode_section(
    root: &Path,
    output: &Path,
    name: String,
    section: Section,
) -> Result<SectionReceipt> {
    if section.encoding != "zstd+json" {
        bail!("unsupported {name} encoding {}", section.encoding);
    }
    let blob = root.join("blobs").join(format!("{}.zst", section.identity));
    let file = File::open(&blob).with_context(|| format!("open {}", blob.display()))?;
    if file.metadata()?.len() != section.compressed_bytes {
        bail!("{name} compressed byte count mismatch");
    }
    // SAFETY: The isolated graph-run copy is opened read-only. The mapping is
    // consumed before the file handle leaves this function.
    let mapped = unsafe { MmapOptions::new().map(&file)? };
    let bytes = zstd::stream::decode_all(Cursor::new(&mapped[..]))?;
    let decoded_identity = format!("b3-{}", blake3::hash(&bytes).to_hex());
    let decoded_raw_bytes = bytes.len() as u64;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("decode {name} JSON"))?;
    let json_shape = shape(&value);
    let path = output.join(format!("{name}.json"));
    fs::write(&path, &bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(SectionReceipt {
        name,
        identity: section.identity,
        row_count: section.row_count,
        manifest_raw_bytes: section.raw_bytes,
        decoded_raw_bytes,
        raw_size_verified: decoded_raw_bytes == section.raw_bytes,
        compressed_bytes: section.compressed_bytes,
        content_blake3: decoded_identity,
        json_shape,
        output: path,
    })
}

fn shape(value: &Value) -> String {
    match value {
        Value::Array(values) => format!("array[{}]", values.len()),
        Value::Object(values) => {
            let mut keys = values.keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();
            format!("object{{{}}}", keys.join(","))
        }
        Value::Null => "null".into(),
        Value::Bool(_) => "bool".into(),
        Value::Number(_) => "number".into(),
        Value::String(_) => "string".into(),
    }
}
