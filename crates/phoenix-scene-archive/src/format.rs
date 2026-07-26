use crate::ArchiveError;

pub const MAGIC: [u8; 8] = *b"PHXSCN1\0";
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 128;
pub const DIRECTORY_ENTRY_SIZE: usize = 96;
pub const PAGE_ALIGNMENT: u64 = 64;
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PAGE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PAGES: u32 = 128;
pub const PAGE_VERSION: u32 = 1;
pub const SHARED_MANIFOLD: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ArchiveManifold {
    Hybrid = 0,
    Hopf = 1,
    Caps = 2,
    Transit = 3,
    Siegel = 4,
}

impl ArchiveManifold {
    pub const ALL: [Self; 5] = [
        Self::Hybrid,
        Self::Hopf,
        Self::Caps,
        Self::Transit,
        Self::Siegel,
    ];

    pub(crate) fn decode(raw: u8) -> Result<Self, ArchiveError> {
        match raw {
            0 => Ok(Self::Hybrid),
            1 => Ok(Self::Hopf),
            2 => Ok(Self::Caps),
            3 => Ok(Self::Transit),
            4 => Ok(Self::Siegel),
            _ => Err(ArchiveError::CorruptDirectory(
                "invalid manifold discriminator",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum PageKind {
    NodeIdentity = 1,
    NodeStyle = 2,
    Topology = 3,
    Edge = 4,
    Positions = 5,
    Guides = 6,
    StraightPaths = 7,
    CurvedPaths = 8,
    BundledPaths = 9,
    LabelPriority = 10,
    RelationMasks = 11,
    PalettePolicy = 12,
}

impl PageKind {
    pub(crate) fn decode(raw: u16) -> Result<Self, ArchiveError> {
        match raw {
            1 => Ok(Self::NodeIdentity),
            2 => Ok(Self::NodeStyle),
            3 => Ok(Self::Topology),
            4 => Ok(Self::Edge),
            5 => Ok(Self::Positions),
            6 => Ok(Self::Guides),
            7 => Ok(Self::StraightPaths),
            8 => Ok(Self::CurvedPaths),
            9 => Ok(Self::BundledPaths),
            10 => Ok(Self::LabelPriority),
            11 => Ok(Self::RelationMasks),
            12 => Ok(Self::PalettePolicy),
            raw => Err(ArchiveError::UnsupportedPageKind(raw)),
        }
    }

    pub const fn is_manifold_page(self) -> bool {
        matches!(
            self,
            Self::Positions
                | Self::Guides
                | Self::StraightPaths
                | Self::CurvedPaths
                | Self::BundledPaths
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageKey {
    pub kind: PageKind,
    pub manifold: Option<ArchiveManifold>,
}

impl PageKey {
    pub const fn shared(kind: PageKind) -> Self {
        Self {
            kind,
            manifold: None,
        }
    }

    pub const fn manifold(kind: PageKind, manifold: ArchiveManifold) -> Self {
        Self {
            kind,
            manifold: Some(manifold),
        }
    }

    pub(crate) fn validate(self) -> Result<(), ArchiveError> {
        if self.kind.is_manifold_page() != self.manifold.is_some() {
            return Err(ArchiveError::InvalidPageScope(self));
        }
        Ok(())
    }

    pub(crate) const fn manifold_byte(self) -> u8 {
        match self.manifold {
            Some(manifold) => manifold as u8,
            None => SHARED_MANIFOLD,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveHeader {
    pub generation_id: u64,
    pub page_count: u32,
    pub file_len: u64,
    pub cohort_hash: [u8; 32],
    pub directory_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageDescriptor {
    pub key: PageKey,
    pub page_version: u32,
    pub offset: u64,
    pub stored_len: u64,
    pub element_count: u64,
    pub element_stride: u32,
    pub page_hash: [u8; 32],
}

pub(crate) fn align_up(value: u64, alignment: u64) -> Result<u64, ArchiveError> {
    let mask = alignment
        .checked_sub(1)
        .ok_or(ArchiveError::CorruptDirectory("zero alignment"))?;
    value
        .checked_add(mask)
        .map(|sum| sum & !mask)
        .ok_or(ArchiveError::ArchiveRangeOverflow)
}

pub(crate) fn hash_directory_cohort(descriptors: &[PageDescriptor]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix-scene-archive-v1/cohort");
    for descriptor in descriptors {
        hasher.update(&(descriptor.key.kind as u16).to_le_bytes());
        hasher.update(&[descriptor.key.manifold_byte()]);
        hasher.update(&descriptor.page_version.to_le_bytes());
        hasher.update(&descriptor.stored_len.to_le_bytes());
        hasher.update(&descriptor.element_count.to_le_bytes());
        hasher.update(&descriptor.element_stride.to_le_bytes());
        hasher.update(&descriptor.page_hash);
    }
    *hasher.finalize().as_bytes()
}
