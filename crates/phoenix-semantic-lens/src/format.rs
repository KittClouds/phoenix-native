use bytemuck::{Pod, Zeroable};

pub const SEMANTIC_LENS_CONTRACT: &str = "phoenix-semantic-lens-pack/v1";
pub const SEMANTIC_LENS_MAGIC: [u8; 8] = *b"PHXLENS1";
pub const SEMANTIC_LENS_VERSION: u16 = 1;
pub const MAX_NAMESPACE_BYTES: usize = 128;
pub const MAX_CODE_NAME_BYTES: usize = 192;
pub const MAX_CODE_COUNT: usize = 4_096;
pub const MAX_STRING_BYTES: usize = 1024 * 1024;
pub const MAX_PACK_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum CoreSemanticClass {
    Identity = 1,
    Relation = 2,
    Occurrence = 3,
    Grouping = 4,
    TemporalConstraint = 5,
    Influence = 6,
    AttributedState = 7,
    ContextualEvidence = 8,
}

impl CoreSemanticClass {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Identity),
            2 => Some(Self::Relation),
            3 => Some(Self::Occurrence),
            4 => Some(Self::Grouping),
            5 => Some(Self::TemporalConstraint),
            6 => Some(Self::Influence),
            7 => Some(Self::AttributedState),
            8 => Some(Self::ContextualEvidence),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum EndpointKind {
    Document = 1,
    Chapter = 2,
    Paragraph = 3,
    Sentence = 4,
    Chunk = 5,
    Span = 6,
    Entity = 7,
    Occurrence = 8,
    Grouping = 9,
    State = 10,
}

impl EndpointKind {
    pub const fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Document),
            2 => Some(Self::Chapter),
            3 => Some(Self::Paragraph),
            4 => Some(Self::Sentence),
            5 => Some(Self::Chunk),
            6 => Some(Self::Span),
            7 => Some(Self::Entity),
            8 => Some(Self::Occurrence),
            9 => Some(Self::Grouping),
            10 => Some(Self::State),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct EndpointMask(pub u32);

impl EndpointMask {
    pub const NONE: Self = Self(0);
    pub const DOCUMENT: Self = Self(1 << 0);
    pub const CHAPTER: Self = Self(1 << 1);
    pub const PARAGRAPH: Self = Self(1 << 2);
    pub const SENTENCE: Self = Self(1 << 3);
    pub const CHUNK: Self = Self(1 << 4);
    pub const SPAN: Self = Self(1 << 5);
    pub const ENTITY: Self = Self(1 << 6);
    pub const OCCURRENCE: Self = Self(1 << 7);
    pub const GROUPING: Self = Self(1 << 8);
    pub const STATE: Self = Self(1 << 9);
    pub const VALID_BITS: u32 = (1 << 10) - 1;

    pub const fn contains(self, kind: EndpointKind) -> bool {
        let bit = 1_u32 << ((kind as u16) - 1);
        self.0 & bit != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn is_valid(self) -> bool {
        self.0 & !Self::VALID_BITS == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensCodeDefinition<'a> {
    pub code: u32,
    pub stable_name: &'a str,
    pub class: CoreSemanticClass,
    pub source_endpoints: EndpointMask,
    pub target_endpoints: EndpointMask,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticLensDefinition<'a> {
    pub namespace: &'a str,
    pub version: u32,
    pub configuration_hash: [u8; 32],
    pub codes: &'a [LensCodeDefinition<'a>],
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct LensIdentity {
    pub lens_id: [u8; 32],
    pub namespace_hash: [u8; 32],
    pub vocabulary_hash: [u8; 32],
    pub configuration_hash: [u8; 32],
    pub version: u32,
    pub reserved: [u32; 3],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SemanticCodeRecord {
    pub code: u32,
    pub name_offset: u32,
    pub name_length: u32,
    pub class: u16,
    pub flags_u16: u16,
    pub source_mask: u32,
    pub target_mask: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SemanticLensHeader {
    pub magic: [u8; 8],
    pub version: u16,
    pub header_len: u16,
    pub flags: u32,
    pub lens_id: [u8; 32],
    pub namespace_hash: [u8; 32],
    pub vocabulary_hash: [u8; 32],
    pub configuration_hash: [u8; 32],
    pub bound_generation_hash: [u8; 32],
    pub code_count: u32,
    pub strings_len: u32,
    pub codes_offset: u64,
    pub strings_offset: u64,
    pub total_len: u64,
    pub payload_hash: [u8; 32],
    pub header_hash: [u8; 32],
    pub lens_version: u32,
    pub reserved_u32: [u32; 11],
}

pub fn compute_vocabulary_hash(definition: &SemanticLensDefinition<'_>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.semantic-lens-vocabulary/v1\0");
    for code in definition.codes {
        hasher.update(&code.code.to_le_bytes());
        update_bytes(&mut hasher, code.stable_name.as_bytes());
        hasher.update(&(code.class as u16).to_le_bytes());
        hasher.update(&code.source_endpoints.0.to_le_bytes());
        hasher.update(&code.target_endpoints.0.to_le_bytes());
        hasher.update(&code.flags.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub fn compute_lens_identity(definition: &SemanticLensDefinition<'_>) -> LensIdentity {
    let namespace_hash = *blake3::hash(definition.namespace.as_bytes()).as_bytes();
    let vocabulary_hash = compute_vocabulary_hash(definition);
    compute_lens_identity_from_parts(
        namespace_hash,
        vocabulary_hash,
        definition.configuration_hash,
        definition.version,
    )
}

pub(crate) fn compute_lens_identity_from_parts(
    namespace_hash: [u8; 32],
    vocabulary_hash: [u8; 32],
    configuration_hash: [u8; 32],
    version: u32,
) -> LensIdentity {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.semantic-lens-identity/v1\0");
    hasher.update(&namespace_hash);
    hasher.update(&version.to_le_bytes());
    hasher.update(&vocabulary_hash);
    hasher.update(&configuration_hash);
    LensIdentity {
        lens_id: *hasher.finalize().as_bytes(),
        namespace_hash,
        vocabulary_hash,
        configuration_hash,
        version,
        reserved: [0; 3],
    }
}

fn update_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
