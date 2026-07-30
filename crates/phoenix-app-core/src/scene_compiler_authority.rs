use super::*;
use phoenix_scene_contract::NATIVE_SCENE_COMPILER_V2_CONTRACT;
use std::fs::{self, OpenOptions};
use std::io::Write;

const MAGIC: [u8; 8] = *b"PHXCAV2\0";
const FORMAT_VERSION: u32 = 1;
const RECORD_SIZE: usize = 192;
const PAYLOAD_END: usize = 152;
const HASH_END: usize = 184;

pub(super) fn write_new(
    publisher: &ScenePublicationStore,
    published: ScenePublicationReceipt,
    source_generation_hash: [u8; 32],
) -> Result<(), KernelError> {
    let path = path_for(publisher, published.generation_id);
    let bytes = encode(published, source_generation_hash);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(io_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read(&path).map_err(io_error)?;
            if existing != bytes {
                return Err(KernelError::InvalidV2CompilerAuthority);
            }
        }
        Err(error) => return Err(io_error(error)),
    }
    Ok(())
}

pub(super) fn verify(
    publisher: &ScenePublicationStore,
    published: ScenePublicationReceipt,
) -> Result<[u8; 32], KernelError> {
    let path = path_for(publisher, published.generation_id);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(KernelError::MissingV2CompilerAuthority);
        }
        Err(error) => return Err(io_error(error)),
    };
    let decoded = decode(&bytes)?;
    if decoded.scene_generation != published.generation_id
        || decoded.archive_hash != published.archive_cohort_hash
        || decoded.product_index_hash != published.product_index_hash
    {
        return Err(KernelError::InvalidV2CompilerAuthority);
    }
    Ok(decoded.source_generation_hash)
}

fn path_for(publisher: &ScenePublicationStore, generation: u64) -> PathBuf {
    publisher
        .root()
        .join(format!("generation-{generation:020}.phxcav2"))
}

fn encode(
    published: ScenePublicationReceipt,
    source_generation_hash: [u8; 32],
) -> [u8; RECORD_SIZE] {
    let mut bytes = [0_u8; RECORD_SIZE];
    bytes[..8].copy_from_slice(&MAGIC);
    put_u32(&mut bytes, 8, FORMAT_VERSION);
    put_u32(&mut bytes, 12, RECORD_SIZE as u32);
    put_u64(&mut bytes, 16, published.generation_id);
    bytes[24..56].copy_from_slice(&source_generation_hash);
    bytes[56..88].copy_from_slice(&published.archive_cohort_hash);
    bytes[88..120].copy_from_slice(&published.product_index_hash);
    bytes[120..152]
        .copy_from_slice(blake3::hash(NATIVE_SCENE_COMPILER_V2_CONTRACT.as_bytes()).as_bytes());
    let checksum = record_hash(&bytes);
    bytes[PAYLOAD_END..HASH_END].copy_from_slice(&checksum);
    bytes
}

fn decode(bytes: &[u8]) -> Result<DecodedAuthority, KernelError> {
    if bytes.len() != RECORD_SIZE
        || bytes[..8] != MAGIC
        || get_u32(bytes, 8) != FORMAT_VERSION
        || get_u32(bytes, 12) as usize != RECORD_SIZE
        || bytes[120..152] != *blake3::hash(NATIVE_SCENE_COMPILER_V2_CONTRACT.as_bytes()).as_bytes()
        || bytes[PAYLOAD_END..HASH_END] != record_hash(bytes)
        || bytes[HASH_END..].iter().any(|byte| *byte != 0)
    {
        return Err(KernelError::InvalidV2CompilerAuthority);
    }
    Ok(DecodedAuthority {
        scene_generation: get_u64(bytes, 16),
        source_generation_hash: array(bytes, 24)?,
        archive_hash: array(bytes, 56)?,
        product_index_hash: array(bytes, 88)?,
    })
}

struct DecodedAuthority {
    scene_generation: u64,
    source_generation_hash: [u8; 32],
    archive_hash: [u8; 32],
    product_index_hash: [u8; 32],
}

fn record_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-native-v2-compiler-authority");
    hasher.update(&bytes[..PAYLOAD_END]);
    hasher.update(&[0_u8; HASH_END - PAYLOAD_END]);
    hasher.update(&bytes[HASH_END..]);
    *hasher.finalize().as_bytes()
}

fn array(bytes: &[u8], offset: usize) -> Result<[u8; 32], KernelError> {
    bytes
        .get(offset..offset + 32)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(KernelError::InvalidV2CompilerAuthority)
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
    KernelError::V2CompilerAuthorityIo(error.to_string())
}
