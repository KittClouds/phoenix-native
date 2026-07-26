use crate::ScenePublicationError;

pub const MANIFEST_CONTRACT: &str = "phoenix.native.scene-publication-manifest/v1";
pub(crate) const MANIFEST_FILE: &str = "current.pspm";
pub(crate) const MANIFEST_SIZE: usize = 192;
const MAGIC: [u8; 8] = *b"PHXPUB1\0";
const FORMAT_VERSION: u32 = 1;
const HASH_START: usize = 152;
const HASH_END: usize = 184;
const DOCUMENT_PRESENT: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ScenePublicationKind {
    RegistryOnly = 1,
    Full = 2,
}

impl ScenePublicationKind {
    fn decode(raw: u32) -> Result<Self, ScenePublicationError> {
        match raw {
            1 => Ok(Self::RegistryOnly),
            2 => Ok(Self::Full),
            _ => Err(ScenePublicationError::CorruptManifest(
                "invalid publication kind",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScenePublicationReceipt {
    pub generation_id: u64,
    pub kind: ScenePublicationKind,
    pub registry_revision: u64,
    pub document_id: Option<u64>,
    pub node_count: u64,
    pub edge_count: u64,
    pub entity_count: u64,
    pub archive_bytes: u64,
    pub product_index_bytes: u64,
    pub archive_cohort_hash: [u8; 32],
    pub product_index_hash: [u8; 32],
}

impl ScenePublicationReceipt {
    pub(crate) fn encode(self) -> [u8; MANIFEST_SIZE] {
        let mut bytes = [0_u8; MANIFEST_SIZE];
        bytes[0..8].copy_from_slice(&MAGIC);
        put_u32(&mut bytes, 8, FORMAT_VERSION);
        put_u32(&mut bytes, 12, MANIFEST_SIZE as u32);
        put_u64(&mut bytes, 16, self.generation_id);
        put_u64(&mut bytes, 24, self.registry_revision);
        put_u64(&mut bytes, 32, self.node_count);
        put_u64(&mut bytes, 40, self.edge_count);
        put_u64(&mut bytes, 48, self.entity_count);
        put_u64(&mut bytes, 56, self.archive_bytes);
        put_u64(&mut bytes, 64, self.product_index_bytes);
        put_u64(&mut bytes, 72, self.document_id.unwrap_or_default());
        put_u32(&mut bytes, 80, self.kind as u32);
        put_u32(
            &mut bytes,
            84,
            if self.document_id.is_some() {
                DOCUMENT_PRESENT
            } else {
                0
            },
        );
        bytes[88..120].copy_from_slice(&self.archive_cohort_hash);
        bytes[120..152].copy_from_slice(&self.product_index_hash);
        let hash = manifest_hash(&bytes);
        bytes[HASH_START..HASH_END].copy_from_slice(&hash);
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, ScenePublicationError> {
        if bytes.len() != MANIFEST_SIZE {
            return Err(ScenePublicationError::CorruptManifest(
                "unexpected manifest length",
            ));
        }
        if bytes[0..8] != MAGIC {
            return Err(ScenePublicationError::CorruptManifest("bad magic"));
        }
        let version = get_u32(bytes, 8);
        if version != FORMAT_VERSION {
            return Err(ScenePublicationError::UnsupportedManifestVersion(version));
        }
        if get_u32(bytes, 12) as usize != MANIFEST_SIZE {
            return Err(ScenePublicationError::CorruptManifest(
                "header size mismatch",
            ));
        }
        let expected_hash = manifest_hash(bytes);
        if bytes[HASH_START..HASH_END] != expected_hash {
            return Err(ScenePublicationError::CorruptManifestHash);
        }
        if bytes[184..].iter().any(|byte| *byte != 0) {
            return Err(ScenePublicationError::CorruptManifest(
                "reserved bytes are nonzero",
            ));
        }
        let flags = get_u32(bytes, 84);
        if flags & !DOCUMENT_PRESENT != 0 {
            return Err(ScenePublicationError::CorruptManifest(
                "unknown manifest flags",
            ));
        }
        let generation_id = get_u64(bytes, 16);
        if generation_id == 0 {
            return Err(ScenePublicationError::ZeroGeneration);
        }
        Ok(Self {
            generation_id,
            kind: ScenePublicationKind::decode(get_u32(bytes, 80))?,
            registry_revision: get_u64(bytes, 24),
            document_id: (flags & DOCUMENT_PRESENT != 0).then(|| get_u64(bytes, 72)),
            node_count: get_u64(bytes, 32),
            edge_count: get_u64(bytes, 40),
            entity_count: get_u64(bytes, 48),
            archive_bytes: get_u64(bytes, 56),
            product_index_bytes: get_u64(bytes, 64),
            archive_cohort_hash: bytes[88..120]
                .try_into()
                .map_err(|_| ScenePublicationError::CorruptManifest("archive hash"))?,
            product_index_hash: bytes[120..152]
                .try_into()
                .map_err(|_| ScenePublicationError::CorruptManifest("index hash"))?,
        })
    }
}

fn manifest_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-scene-publication-manifest-v1");
    hasher.update(&bytes[..HASH_START]);
    hasher.update(&[0_u8; HASH_END - HASH_START]);
    hasher.update(&bytes[HASH_END..]);
    *hasher.finalize().as_bytes()
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
