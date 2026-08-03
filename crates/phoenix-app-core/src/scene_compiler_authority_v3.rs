//! Durable authority for the V3 visual interpretation of a scene publication.
//!
//! The V2 authority remains readable for historical generations.  A V3 file
//! is written beside it and binds the publication hashes, source generation,
//! lane counts, and role digests in one small fixed-size record.

use phoenix_scene_compiler::{
    VisualContractReceiptV3, VISUAL_EDGE_KIND_COUNT, VISUAL_NODE_KIND_COUNT,
};
use phoenix_scene_contract::VISUAL_GRAPH_CONTRACT_V3;
use phoenix_scene_publisher::{ScenePublicationReceipt, ScenePublicationStore};
use std::fs::{self, OpenOptions};
use std::io::Write;

use super::KernelError;

const MAGIC: [u8; 8] = *b"PHXCAV3\0";
const FORMAT_VERSION: u32 = 1;
const RECORD_SIZE: usize = 720;
const PAYLOAD_END: usize = 680;
const HASH_END: usize = 712;

pub(super) fn write_new(
    publisher: &ScenePublicationStore,
    receipt: VisualContractReceiptV3,
) -> Result<(), KernelError> {
    let path = path_for(publisher, receipt.scene_generation_id);
    let bytes = encode(receipt);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(io_error),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read(&path).map_err(io_error)?;
            if existing != bytes {
                return Err(KernelError::InvalidV3VisualAuthority);
            }
            Ok(())
        }
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn verify(
    publisher: &ScenePublicationStore,
    published: ScenePublicationReceipt,
) -> Result<VisualContractReceiptV3, KernelError> {
    let path = path_for(publisher, published.generation_id);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(KernelError::MissingV3VisualAuthority);
        }
        Err(error) => return Err(io_error(error)),
    };
    let receipt = decode(&bytes)?;
    if receipt.scene_generation_id != published.generation_id
        || receipt.archive_hash != published.archive_cohort_hash
        || receipt.product_index_hash != published.product_index_hash
        || receipt.node_count != published.node_count
        || receipt.edge_count != published.edge_count
    {
        return Err(KernelError::InvalidV3VisualAuthority);
    }
    Ok(receipt)
}

fn path_for(publisher: &ScenePublicationStore, generation: u64) -> std::path::PathBuf {
    publisher
        .root()
        .join(format!("generation-{generation:020}.phxcav3"))
}

fn encode(receipt: VisualContractReceiptV3) -> [u8; RECORD_SIZE] {
    let mut bytes = [0_u8; RECORD_SIZE];
    bytes[..8].copy_from_slice(&MAGIC);
    put_u32(&mut bytes, 8, FORMAT_VERSION);
    put_u32(&mut bytes, 12, RECORD_SIZE as u32);
    put_u64(&mut bytes, 16, receipt.scene_generation_id);
    bytes[24..56].copy_from_slice(&receipt.source_generation_hash);
    bytes[56..88].copy_from_slice(&receipt.archive_hash);
    bytes[88..120].copy_from_slice(&receipt.product_index_hash);
    bytes[120..152].copy_from_slice(blake3::hash(VISUAL_GRAPH_CONTRACT_V3.as_bytes()).as_bytes());
    bytes[152..184].copy_from_slice(&receipt.node_identity_hash);
    bytes[184..216].copy_from_slice(&receipt.edge_topology_hash);
    bytes[216..248].copy_from_slice(&receipt.node_role_hash);
    bytes[248..280].copy_from_slice(&receipt.edge_role_hash);
    bytes[280..312].copy_from_slice(&receipt.projection_hash);
    put_u64(&mut bytes, 312, receipt.node_count);
    put_u64(&mut bytes, 320, receipt.edge_count);
    let mut offset = 328;
    for lane in receipt.lane_counts {
        put_u64(&mut bytes, offset, lane.nodes);
        put_u64(&mut bytes, offset + 8, lane.edges);
        offset += 16;
    }
    for count in receipt.node_kind_counts {
        put_u64(&mut bytes, offset, count);
        offset += 8;
    }
    for count in receipt.edge_kind_counts {
        put_u64(&mut bytes, offset, count);
        offset += 8;
    }
    debug_assert_eq!(offset, PAYLOAD_END);
    let checksum = record_hash(&bytes);
    bytes[PAYLOAD_END..HASH_END].copy_from_slice(&checksum);
    bytes
}

fn decode(bytes: &[u8]) -> Result<VisualContractReceiptV3, KernelError> {
    if bytes.len() != RECORD_SIZE
        || bytes[..8] != MAGIC
        || get_u32(bytes, 8) != FORMAT_VERSION
        || get_u32(bytes, 12) as usize != RECORD_SIZE
        || bytes[120..152] != *blake3::hash(VISUAL_GRAPH_CONTRACT_V3.as_bytes()).as_bytes()
        || bytes[PAYLOAD_END..HASH_END] != record_hash(bytes)
        || bytes[HASH_END..].iter().any(|byte| *byte != 0)
    {
        return Err(KernelError::InvalidV3VisualAuthority);
    }
    let mut lane_counts = [phoenix_scene_compiler::VisualLaneCount::default(); 4];
    let mut offset = 328;
    for lane in &mut lane_counts {
        lane.nodes = get_u64(bytes, offset);
        lane.edges = get_u64(bytes, offset + 8);
        offset += 16;
    }
    let mut node_kind_counts = [0_u64; VISUAL_NODE_KIND_COUNT];
    for count in &mut node_kind_counts {
        *count = get_u64(bytes, offset);
        offset += 8;
    }
    let mut edge_kind_counts = [0_u64; VISUAL_EDGE_KIND_COUNT];
    for count in &mut edge_kind_counts {
        *count = get_u64(bytes, offset);
        offset += 8;
    }
    if offset != PAYLOAD_END {
        return Err(KernelError::InvalidV3VisualAuthority);
    }
    Ok(VisualContractReceiptV3 {
        contract: VISUAL_GRAPH_CONTRACT_V3,
        scene_generation_id: get_u64(bytes, 16),
        source_generation_hash: array(bytes, 24)?,
        archive_hash: array(bytes, 56)?,
        product_index_hash: array(bytes, 88)?,
        node_identity_hash: array(bytes, 152)?,
        edge_topology_hash: array(bytes, 184)?,
        node_role_hash: array(bytes, 216)?,
        edge_role_hash: array(bytes, 248)?,
        projection_hash: array(bytes, 280)?,
        node_count: get_u64(bytes, 312),
        edge_count: get_u64(bytes, 320),
        lane_counts,
        node_kind_counts,
        edge_kind_counts,
    })
}

fn record_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-native-v3-visual-authority");
    hasher.update(&bytes[..PAYLOAD_END]);
    hasher.update(&[0_u8; HASH_END - PAYLOAD_END]);
    hasher.update(&bytes[HASH_END..]);
    *hasher.finalize().as_bytes()
}

fn array(bytes: &[u8], offset: usize) -> Result<[u8; 32], KernelError> {
    bytes
        .get(offset..offset + 32)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(KernelError::InvalidV3VisualAuthority)
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0_u8; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn io_error(error: std::io::Error) -> KernelError {
    KernelError::V3VisualAuthorityIo(error.to_string())
}
