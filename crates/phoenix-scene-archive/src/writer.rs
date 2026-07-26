use crate::format::{
    align_up, hash_directory_cohort, PageDescriptor, PageKey, DIRECTORY_ENTRY_SIZE, FORMAT_VERSION,
    HEADER_SIZE, MAGIC, MAX_ARCHIVE_BYTES, MAX_PAGES, MAX_PAGE_BYTES, PAGE_ALIGNMENT, PAGE_VERSION,
};
use crate::ArchiveError;
use bytemuck::Pod;
use hashbrown::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug)]
struct PendingPage {
    key: PageKey,
    bytes: Vec<u8>,
    element_count: u64,
    element_stride: u32,
}

#[derive(Debug)]
pub struct PhoenixSceneArchiveBuilderV1 {
    generation_id: u64,
    pages: Vec<PendingPage>,
    keys: HashSet<PageKey>,
}

impl PhoenixSceneArchiveBuilderV1 {
    pub fn new(generation_id: u64) -> Result<Self, ArchiveError> {
        if generation_id == 0 {
            return Err(ArchiveError::ZeroGeneration);
        }
        Ok(Self {
            generation_id,
            pages: Vec::new(),
            keys: HashSet::new(),
        })
    }

    pub fn add_records<T: Pod>(
        &mut self,
        key: PageKey,
        records: &[T],
    ) -> Result<&mut Self, ArchiveError> {
        let count = u64::try_from(records.len()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let stride =
            u32::try_from(size_of::<T>()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        self.add_page(key, bytemuck::cast_slice(records).to_vec(), count, stride)
    }

    pub fn add_page(
        &mut self,
        key: PageKey,
        bytes: Vec<u8>,
        element_count: u64,
        element_stride: u32,
    ) -> Result<&mut Self, ArchiveError> {
        key.validate()?;
        if !self.keys.insert(key) {
            return Err(ArchiveError::DuplicatePage(key));
        }
        let byte_len =
            u64::try_from(bytes.len()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        if byte_len > MAX_PAGE_BYTES {
            return Err(ArchiveError::OversizedPage {
                key,
                actual: byte_len,
                limit: MAX_PAGE_BYTES,
            });
        }
        if self.pages.len() >= MAX_PAGES as usize {
            return Err(ArchiveError::OversizedArchive {
                actual: (self.pages.len() + 1) as u64,
                limit: u64::from(MAX_PAGES),
            });
        }
        self.pages.push(PendingPage {
            key,
            bytes,
            element_count,
            element_stride,
        });
        Ok(self)
    }

    pub fn write_to_path(
        mut self,
        path: impl AsRef<Path>,
    ) -> Result<ArchiveBuildReceipt, ArchiveError> {
        self.pages.sort_unstable_by_key(|page| page.key);
        let directory_len = self
            .pages
            .len()
            .checked_mul(DIRECTORY_ENTRY_SIZE)
            .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        let data_start = align_up(
            u64::try_from(HEADER_SIZE + directory_len)
                .map_err(|_| ArchiveError::ArchiveRangeOverflow)?,
            PAGE_ALIGNMENT,
        )?;

        let mut next_offset = data_start;
        let mut descriptors = Vec::with_capacity(self.pages.len());
        for page in &self.pages {
            next_offset = align_up(next_offset, PAGE_ALIGNMENT)?;
            let stored_len =
                u64::try_from(page.bytes.len()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
            let page_hash = *blake3::hash(&page.bytes).as_bytes();
            descriptors.push(PageDescriptor {
                key: page.key,
                page_version: PAGE_VERSION,
                offset: next_offset,
                stored_len,
                element_count: page.element_count,
                element_stride: page.element_stride,
                page_hash,
            });
            next_offset = next_offset
                .checked_add(stored_len)
                .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        }
        let file_len = next_offset;
        if file_len > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::OversizedArchive {
                actual: file_len,
                limit: MAX_ARCHIVE_BYTES,
            });
        }

        let mut directory = vec![0_u8; directory_len];
        for (index, descriptor) in descriptors.iter().enumerate() {
            let start = index * DIRECTORY_ENTRY_SIZE;
            encode_descriptor(
                descriptor,
                &mut directory[start..start + DIRECTORY_ENTRY_SIZE],
            );
        }
        let directory_hash = *blake3::hash(&directory).as_bytes();
        let cohort_hash = hash_directory_cohort(&descriptors);
        let header = encode_header(
            self.generation_id,
            descriptors.len() as u32,
            directory.len() as u64,
            file_len,
            cohort_hash,
            directory_hash,
        );

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&header)?;
        writer.write_all(&directory)?;
        write_zero_padding(
            &mut writer,
            (HEADER_SIZE + directory_len) as u64,
            data_start,
        )?;
        let mut cursor = data_start;
        for (page, descriptor) in self.pages.iter().zip(&descriptors) {
            write_zero_padding(&mut writer, cursor, descriptor.offset)?;
            writer.write_all(&page.bytes)?;
            cursor = descriptor
                .offset
                .checked_add(descriptor.stored_len)
                .ok_or(ArchiveError::ArchiveRangeOverflow)?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;

        Ok(ArchiveBuildReceipt {
            generation_id: self.generation_id,
            page_count: descriptors.len() as u32,
            file_len,
            cohort_hash,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveBuildReceipt {
    pub generation_id: u64,
    pub page_count: u32,
    pub file_len: u64,
    pub cohort_hash: [u8; 32],
}

fn encode_header(
    generation_id: u64,
    page_count: u32,
    directory_len: u64,
    file_len: u64,
    cohort_hash: [u8; 32],
    directory_hash: [u8; 32],
) -> [u8; HEADER_SIZE] {
    let mut bytes = [0_u8; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&MAGIC);
    put_u32(&mut bytes, 8, FORMAT_VERSION);
    put_u32(&mut bytes, 12, HEADER_SIZE as u32);
    put_u64(&mut bytes, 16, generation_id);
    put_u32(&mut bytes, 24, page_count);
    put_u32(&mut bytes, 28, DIRECTORY_ENTRY_SIZE as u32);
    put_u64(&mut bytes, 32, HEADER_SIZE as u64);
    put_u64(&mut bytes, 40, directory_len);
    put_u64(&mut bytes, 48, file_len);
    bytes[56..88].copy_from_slice(&cohort_hash);
    bytes[88..120].copy_from_slice(&directory_hash);
    bytes
}

fn encode_descriptor(descriptor: &PageDescriptor, bytes: &mut [u8]) {
    put_u16(bytes, 0, descriptor.key.kind as u16);
    bytes[2] = descriptor.key.manifold_byte();
    put_u32(bytes, 4, descriptor.page_version);
    put_u64(bytes, 8, descriptor.offset);
    put_u64(bytes, 16, descriptor.stored_len);
    put_u64(bytes, 24, descriptor.element_count);
    put_u32(bytes, 32, descriptor.element_stride);
    bytes[40..72].copy_from_slice(&descriptor.page_hash);
}

fn write_zero_padding(
    writer: &mut impl Write,
    current: u64,
    target: u64,
) -> Result<(), ArchiveError> {
    let count = target
        .checked_sub(current)
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    if count > 0 {
        let zeros = [0_u8; PAGE_ALIGNMENT as usize];
        let count = usize::try_from(count).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        writer.write_all(&zeros[..count])?;
    }
    Ok(())
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
