use crate::{
    validate_directory, validate_header, GenerationHeader, GraphGenerationV2Error, PageDescriptor,
    PageKind,
};
use bytemuck::Pod;
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::mem::size_of;
use std::path::Path;

pub struct VerifiedGraphGenerationV2 {
    mmap: Mmap,
    header: GenerationHeader,
    directory: [PageDescriptor; 28],
}

impl std::fmt::Debug for VerifiedGraphGenerationV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedGraphGenerationV2")
            .field("generation_hash", &self.header.generation_hash)
            .field("document_revision", &self.header.document_revision)
            .field("registry_revision", &self.header.registry_revision)
            .field("page_count", &self.directory.len())
            .finish()
    }
}

impl VerifiedGraphGenerationV2 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GraphGenerationV2Error> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?;
        let actual_len = file
            .metadata()
            .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?
            .len();

        // SAFETY: the map is read-only and retained by the returned owner. All
        // typed views are validated against page bounds, alignment, record
        // sizes, and `Pod` before they are exposed.
        let mmap = unsafe {
            MmapOptions::new()
                .map(&file)
                .map_err(|source| GraphGenerationV2Error::io(path.to_path_buf(), source))?
        };
        let header_bytes =
            mmap.get(..size_of::<GenerationHeader>())
                .ok_or(GraphGenerationV2Error::TooSmall {
                    actual: actual_len,
                    minimum: size_of::<GenerationHeader>() as u64,
                })?;
        let header = *bytemuck::try_from_bytes::<GenerationHeader>(header_bytes).map_err(|_| {
            GraphGenerationV2Error::InvalidRecordLayout {
                page: PageKind::Documents,
            }
        })?;
        validate_header(&header, actual_len)?;

        let directory_start = usize::try_from(header.directory_offset)
            .map_err(|_| GraphGenerationV2Error::DirectoryOutOfBounds)?;
        let directory_end_u64 = header
            .directory_offset
            .checked_add(header.directory_len)
            .ok_or(GraphGenerationV2Error::DirectoryOutOfBounds)?;
        let directory_end = usize::try_from(directory_end_u64)
            .map_err(|_| GraphGenerationV2Error::DirectoryOutOfBounds)?;
        let directory_bytes = mmap
            .get(directory_start..directory_end)
            .ok_or(GraphGenerationV2Error::DirectoryOutOfBounds)?;
        let directory: [PageDescriptor; 28] =
            bytemuck::try_cast_slice::<u8, PageDescriptor>(directory_bytes)
                .map_err(|_| GraphGenerationV2Error::DirectoryOutOfBounds)?
                .try_into()
                .map_err(|_| GraphGenerationV2Error::DirectoryOutOfBounds)?;
        validate_directory(&header, &directory, &mmap)?;

        Ok(Self {
            mmap,
            header,
            directory,
        })
    }

    pub fn header(&self) -> &GenerationHeader {
        &self.header
    }

    pub fn directory(&self) -> &[PageDescriptor] {
        &self.directory
    }

    pub fn descriptor(&self, kind: PageKind) -> &PageDescriptor {
        &self.directory[(kind as usize) - 1]
    }

    pub fn page_bytes(&self, kind: PageKind) -> &[u8] {
        let descriptor = self.descriptor(kind);
        let start = descriptor.offset as usize;
        let end = start + descriptor.length as usize;
        &self.mmap[start..end]
    }

    pub fn typed_page<T: Pod>(&self, kind: PageKind) -> Result<&[T], GraphGenerationV2Error> {
        let descriptor = self.descriptor(kind);
        if descriptor.record_size as usize != size_of::<T>() {
            return Err(GraphGenerationV2Error::InvalidRecordLayout { page: kind });
        }
        bytemuck::try_cast_slice(self.page_bytes(kind))
            .map_err(|_| GraphGenerationV2Error::InvalidRecordLayout { page: kind })
    }

    pub fn resolve_string(
        &self,
        reference: crate::StringRef,
    ) -> Result<&str, GraphGenerationV2Error> {
        let strings = self.page_bytes(PageKind::Strings);
        let start = usize::try_from(reference.offset)
            .map_err(|_| GraphGenerationV2Error::InvalidStringRef)?;
        let end = start
            .checked_add(reference.length as usize)
            .ok_or(GraphGenerationV2Error::InvalidStringRef)?;
        let bytes = strings
            .get(start..end)
            .ok_or(GraphGenerationV2Error::InvalidStringRef)?;
        std::str::from_utf8(bytes).map_err(|_| GraphGenerationV2Error::InvalidStringRef)
    }
}
