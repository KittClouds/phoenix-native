use crate::format::{
    hash_directory_cohort, ArchiveHeader, ArchiveManifold, PageDescriptor, PageKey, PageKind,
    DIRECTORY_ENTRY_SIZE, FORMAT_VERSION, HEADER_SIZE, MAGIC, MAX_ARCHIVE_BYTES, MAX_PAGES,
    MAX_PAGE_BYTES, PAGE_ALIGNMENT, PAGE_VERSION, SHARED_MANIFOLD,
};
use crate::records::{
    EdgeRecord, GuidePageHeader, GuideStrokeRecord, NodeIdentityRecord, NodeStyleRecord,
    PathPageHeader, PathRecord, PositionRecord, TopologyRecord,
};
use crate::ArchiveError;
use bytemuck::Pod;
use hashbrown::{HashMap, HashSet};
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

const UNVERIFIED: u8 = 0;
const VERIFYING: u8 = 1;
const VERIFIED: u8 = 2;
const CORRUPT: u8 = 3;

#[derive(Debug)]
pub struct PhoenixSceneArchiveV1 {
    mmap: Mmap,
    header: ArchiveHeader,
    descriptors: Box<[PageDescriptor]>,
    index: HashMap<PageKey, usize>,
    verification: Box<[AtomicU8]>,
    verification_count: AtomicU64,
}

impl PhoenixSceneArchiveV1 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ArchiveError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ArchiveError::MissingArchive(path.to_path_buf())
            } else {
                ArchiveError::Io(error)
            }
        })?;
        let file_len = file.metadata()?.len();
        if file_len > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::OversizedArchive {
                actual: file_len,
                limit: MAX_ARCHIVE_BYTES,
            });
        }
        if file_len < HEADER_SIZE as u64 {
            return Err(ArchiveError::CorruptHeader("file is shorter than header"));
        }
        // SAFETY: the file is opened read-only and the returned mapping is never exposed mutably.
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        Self::from_mmap(mmap, file_len)
    }

    fn from_mmap(mmap: Mmap, actual_file_len: u64) -> Result<Self, ArchiveError> {
        let header = decode_header(&mmap[..HEADER_SIZE])?;
        if header.file_len != actual_file_len {
            return Err(ArchiveError::CorruptHeader("recorded file length mismatch"));
        }
        if header.page_count > MAX_PAGES {
            return Err(ArchiveError::OversizedArchive {
                actual: u64::from(header.page_count),
                limit: u64::from(MAX_PAGES),
            });
        }
        let directory_len = usize::try_from(header.page_count)
            .ok()
            .and_then(|count| count.checked_mul(DIRECTORY_ENTRY_SIZE))
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        let directory_end = HEADER_SIZE
            .checked_add(directory_len)
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        if directory_end > mmap.len() {
            return Err(ArchiveError::CorruptDirectory(
                "directory extends beyond file",
            ));
        }
        let directory = &mmap[HEADER_SIZE..directory_end];
        if blake3::hash(directory).as_bytes() != &header.directory_hash {
            return Err(ArchiveError::CorruptDirectory("directory hash mismatch"));
        }

        let data_start = crate::format::align_up(directory_end as u64, PAGE_ALIGNMENT)?;
        let mut descriptors = Vec::with_capacity(header.page_count as usize);
        let mut keys = HashSet::with_capacity(header.page_count as usize);
        for bytes in directory.chunks_exact(DIRECTORY_ENTRY_SIZE) {
            let descriptor = decode_descriptor(bytes)?;
            descriptor.key.validate()?;
            if descriptor.page_version != PAGE_VERSION {
                return Err(ArchiveError::UnsupportedPageVersion {
                    kind: descriptor.key.kind,
                    version: descriptor.page_version,
                });
            }
            if !keys.insert(descriptor.key) {
                return Err(ArchiveError::DuplicatePage(descriptor.key));
            }
            validate_descriptor(&descriptor, data_start, actual_file_len)?;
            descriptors.push(descriptor);
        }
        if hash_directory_cohort(&descriptors) != header.cohort_hash {
            return Err(ArchiveError::CorruptDirectory("cohort hash mismatch"));
        }
        validate_no_overlap(&descriptors)?;

        let index = descriptors
            .iter()
            .enumerate()
            .map(|(index, descriptor)| (descriptor.key, index))
            .collect();
        let verification = (0..descriptors.len())
            .map(|_| AtomicU8::new(UNVERIFIED))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Self {
            mmap,
            header,
            descriptors: descriptors.into_boxed_slice(),
            index,
            verification,
            verification_count: AtomicU64::new(0),
        })
    }

    pub const fn header(&self) -> ArchiveHeader {
        self.header
    }

    pub fn descriptors(&self) -> &[PageDescriptor] {
        &self.descriptors
    }

    pub fn has_page(&self, key: PageKey) -> bool {
        self.index.contains_key(&key)
    }

    pub fn verified_page_count(&self) -> u64 {
        self.verification_count.load(Ordering::Acquire)
    }

    pub fn page(&self, key: PageKey) -> Result<ArchivePage<'_>, ArchiveError> {
        let index = *self.index.get(&key).ok_or(ArchiveError::MissingPage(key))?;
        self.verify_page(index)?;
        let descriptor = &self.descriptors[index];
        let start =
            usize::try_from(descriptor.offset).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let len = usize::try_from(descriptor.stored_len)
            .map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let end = start
            .checked_add(len)
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        Ok(ArchivePage {
            descriptor,
            bytes: &self.mmap[start..end],
        })
    }

    pub fn typed_page<T: Pod>(&self, key: PageKey) -> Result<&[T], ArchiveError> {
        let page = self.page(key)?;
        let expected_stride =
            u32::try_from(size_of::<T>()).map_err(|_| ArchiveError::InvalidTypedPage(key))?;
        let expected_len = page
            .descriptor
            .element_count
            .checked_mul(u64::from(expected_stride))
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        if page.descriptor.element_stride != expected_stride
            || page.descriptor.stored_len != expected_len
        {
            return Err(ArchiveError::InvalidTypedPage(key));
        }
        bytemuck::try_cast_slice(page.bytes).map_err(|_| ArchiveError::InvalidTypedPage(key))
    }

    pub fn open_manifold(
        &self,
        manifold: ArchiveManifold,
    ) -> Result<ManifoldPageSet<'_>, ArchiveError> {
        let identities = self.typed_page(PageKey::shared(PageKind::NodeIdentity))?;
        let styles = self.typed_page(PageKey::shared(PageKind::NodeStyle))?;
        let topology = self.typed_page(PageKey::shared(PageKind::Topology))?;
        let edges = self.typed_page(PageKey::shared(PageKind::Edge))?;
        let positions = self.typed_page(PageKey::manifold(PageKind::Positions, manifold))?;
        if identities.len() != styles.len() || identities.len() != positions.len() {
            return Err(ArchiveError::CorruptPage(PageKey::manifold(
                PageKind::Positions,
                manifold,
            )));
        }
        if topology.len() != edges.len() {
            return Err(ArchiveError::CorruptPage(PageKey::shared(
                PageKind::Topology,
            )));
        }
        Ok(ManifoldPageSet {
            identities,
            styles,
            topology,
            edges,
            positions,
        })
    }

    pub fn guides(&self, manifold: ArchiveManifold) -> Result<GuidePageView<'_>, ArchiveError> {
        let key = PageKey::manifold(PageKind::Guides, manifold);
        GuidePageView::parse(key, self.page(key)?.bytes)
    }

    pub fn paths(
        &self,
        kind: PageKind,
        manifold: ArchiveManifold,
    ) -> Result<PathPageView<'_>, ArchiveError> {
        if !matches!(
            kind,
            PageKind::StraightPaths | PageKind::CurvedPaths | PageKind::BundledPaths
        ) {
            return Err(ArchiveError::InvalidPageScope(PageKey::manifold(
                kind, manifold,
            )));
        }
        let key = PageKey::manifold(kind, manifold);
        PathPageView::parse(key, self.page(key)?.bytes)
    }

    fn verify_page(&self, index: usize) -> Result<(), ArchiveError> {
        let state = &self.verification[index];
        loop {
            match state.load(Ordering::Acquire) {
                VERIFIED => return Ok(()),
                CORRUPT => return Err(ArchiveError::CorruptPage(self.descriptors[index].key)),
                UNVERIFIED => {
                    if state
                        .compare_exchange(
                            UNVERIFIED,
                            VERIFYING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        break;
                    }
                }
                VERIFYING => std::thread::yield_now(),
                _ => return Err(ArchiveError::CorruptPage(self.descriptors[index].key)),
            }
        }
        self.verification_count.fetch_add(1, Ordering::AcqRel);
        let descriptor = &self.descriptors[index];
        let start =
            usize::try_from(descriptor.offset).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let end = start
            .checked_add(
                usize::try_from(descriptor.stored_len)
                    .map_err(|_| ArchiveError::ArchiveRangeOverflow)?,
            )
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        if blake3::hash(&self.mmap[start..end]).as_bytes() == &descriptor.page_hash {
            state.store(VERIFIED, Ordering::Release);
            Ok(())
        } else {
            state.store(CORRUPT, Ordering::Release);
            Err(ArchiveError::CorruptPage(descriptor.key))
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArchivePage<'a> {
    pub descriptor: &'a PageDescriptor,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
pub struct ManifoldPageSet<'a> {
    pub identities: &'a [NodeIdentityRecord],
    pub styles: &'a [NodeStyleRecord],
    pub topology: &'a [TopologyRecord],
    pub edges: &'a [EdgeRecord],
    pub positions: &'a [PositionRecord],
}

#[derive(Clone, Copy, Debug)]
pub struct GuidePageView<'a> {
    pub strokes: &'a [GuideStrokeRecord],
    pub points: &'a [PositionRecord],
}

impl<'a> GuidePageView<'a> {
    fn parse(key: PageKey, bytes: &'a [u8]) -> Result<Self, ArchiveError> {
        let header = record_at::<GuidePageHeader>(key, bytes, 0)?;
        let strokes_offset = size_of::<GuidePageHeader>();
        let strokes = records_at::<GuideStrokeRecord>(
            key,
            bytes,
            strokes_offset,
            header.stroke_count as usize,
        )?;
        let points_offset = strokes_offset
            .checked_add(size_of_val(strokes))
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        let points =
            records_at::<PositionRecord>(key, bytes, points_offset, header.point_count as usize)?;
        validate_ranges(
            key,
            strokes
                .iter()
                .map(|stroke| (stroke.first_point, stroke.point_count)),
            points.len(),
        )?;
        Ok(Self { strokes, points })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PathPageView<'a> {
    pub style: u32,
    pub paths: &'a [PathRecord],
    pub points: &'a [PositionRecord],
}

impl<'a> PathPageView<'a> {
    fn parse(key: PageKey, bytes: &'a [u8]) -> Result<Self, ArchiveError> {
        let header = record_at::<PathPageHeader>(key, bytes, 0)?;
        let paths_offset = size_of::<PathPageHeader>();
        let paths = records_at::<PathRecord>(key, bytes, paths_offset, header.path_count as usize)?;
        let points_offset = paths_offset
            .checked_add(size_of_val(paths))
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        let points =
            records_at::<PositionRecord>(key, bytes, points_offset, header.point_count as usize)?;
        validate_ranges(
            key,
            paths
                .iter()
                .map(|path| (path.first_point, u32::from(path.point_count))),
            points.len(),
        )?;
        Ok(Self {
            style: header.path_style,
            paths,
            points,
        })
    }
}

fn decode_header(bytes: &[u8]) -> Result<ArchiveHeader, ArchiveError> {
    if bytes[0..8] != MAGIC {
        return Err(ArchiveError::CorruptHeader("magic mismatch"));
    }
    let version = get_u32(bytes, 8);
    if version != FORMAT_VERSION {
        return Err(ArchiveError::UnsupportedFormatVersion(version));
    }
    if get_u32(bytes, 12) != HEADER_SIZE as u32 {
        return Err(ArchiveError::CorruptHeader("header size mismatch"));
    }
    let generation_id = get_u64(bytes, 16);
    if generation_id == 0 {
        return Err(ArchiveError::ZeroGeneration);
    }
    if get_u32(bytes, 28) != DIRECTORY_ENTRY_SIZE as u32 || get_u64(bytes, 32) != HEADER_SIZE as u64
    {
        return Err(ArchiveError::CorruptHeader("directory layout mismatch"));
    }
    let page_count = get_u32(bytes, 24);
    if get_u64(bytes, 40) != u64::from(page_count) * DIRECTORY_ENTRY_SIZE as u64 {
        return Err(ArchiveError::CorruptHeader("directory length mismatch"));
    }
    let mut cohort_hash = [0_u8; 32];
    cohort_hash.copy_from_slice(&bytes[56..88]);
    let mut directory_hash = [0_u8; 32];
    directory_hash.copy_from_slice(&bytes[88..120]);
    Ok(ArchiveHeader {
        generation_id,
        page_count,
        file_len: get_u64(bytes, 48),
        cohort_hash,
        directory_hash,
    })
}

fn decode_descriptor(bytes: &[u8]) -> Result<PageDescriptor, ArchiveError> {
    let kind = PageKind::decode(get_u16(bytes, 0))?;
    let manifold = match bytes[2] {
        SHARED_MANIFOLD => None,
        raw => Some(ArchiveManifold::decode(raw)?),
    };
    let mut page_hash = [0_u8; 32];
    page_hash.copy_from_slice(&bytes[40..72]);
    Ok(PageDescriptor {
        key: PageKey { kind, manifold },
        page_version: get_u32(bytes, 4),
        offset: get_u64(bytes, 8),
        stored_len: get_u64(bytes, 16),
        element_count: get_u64(bytes, 24),
        element_stride: get_u32(bytes, 32),
        page_hash,
    })
}

fn validate_descriptor(
    descriptor: &PageDescriptor,
    data_start: u64,
    file_len: u64,
) -> Result<(), ArchiveError> {
    if descriptor.stored_len > MAX_PAGE_BYTES {
        return Err(ArchiveError::OversizedPage {
            key: descriptor.key,
            actual: descriptor.stored_len,
            limit: MAX_PAGE_BYTES,
        });
    }
    if descriptor.offset < data_start || descriptor.offset % PAGE_ALIGNMENT != 0 {
        return Err(ArchiveError::CorruptDirectory("page offset is invalid"));
    }
    let end = descriptor
        .offset
        .checked_add(descriptor.stored_len)
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    if end > file_len {
        return Err(ArchiveError::CorruptDirectory("page extends beyond file"));
    }
    Ok(())
}

fn validate_no_overlap(descriptors: &[PageDescriptor]) -> Result<(), ArchiveError> {
    let mut ranges = descriptors
        .iter()
        .map(|descriptor| (descriptor.offset, descriptor.offset + descriptor.stored_len))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(ArchiveError::CorruptDirectory("page ranges overlap"));
    }
    Ok(())
}

fn record_at<T: Pod>(key: PageKey, bytes: &[u8], offset: usize) -> Result<&T, ArchiveError> {
    let end = offset
        .checked_add(size_of::<T>())
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    let slice = bytes
        .get(offset..end)
        .ok_or(ArchiveError::InvalidTypedPage(key))?;
    bytemuck::try_from_bytes(slice).map_err(|_| ArchiveError::InvalidTypedPage(key))
}

fn records_at<T: Pod>(
    key: PageKey,
    bytes: &[u8],
    offset: usize,
    count: usize,
) -> Result<&[T], ArchiveError> {
    let byte_len = count
        .checked_mul(size_of::<T>())
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    let end = offset
        .checked_add(byte_len)
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    let slice = bytes
        .get(offset..end)
        .ok_or(ArchiveError::InvalidTypedPage(key))?;
    bytemuck::try_cast_slice(slice).map_err(|_| ArchiveError::InvalidTypedPage(key))
}

fn validate_ranges(
    key: PageKey,
    ranges: impl Iterator<Item = (u32, u32)>,
    point_count: usize,
) -> Result<(), ArchiveError> {
    let point_count = u64::try_from(point_count).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
    for (first, count) in ranges {
        let end = u64::from(first)
            .checked_add(u64::from(count))
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        if end > point_count {
            return Err(ArchiveError::InvalidRecordRange(key));
        }
    }
    Ok(())
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}
