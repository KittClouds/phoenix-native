use std::fs::File;
use std::mem::size_of;
use std::path::Path;

use bytemuck::Pod;
use memmap2::{Mmap, MmapOptions};

use crate::{
    compute_artifact_hash, EmbeddingPageError, EmbeddingPageHeaderV1, EmbeddingRowV1,
    EmbeddingScalarFormat, EMBEDDING_PAGE_ALIGNMENT, EMBEDDING_PAGE_FLAG_COMPLETE,
    EMBEDDING_PAGE_MAGIC, EMBEDDING_PAGE_VERSION, MAX_EMBEDDING_DIMENSION,
    MAX_EMBEDDING_PAGE_BYTES, MAX_EMBEDDING_ROWS, ROW_FLAG_NORMALIZED,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct EmbeddingPageExpectation {
    pub generation_hash: Option<[u8; 32]>,
    pub source_set_hash: Option<[u8; 32]>,
    pub model_identity_hash: Option<[u8; 32]>,
    pub config_hash: Option<[u8; 32]>,
}

pub struct VerifiedEmbeddingPagesV1 {
    mmap: Mmap,
    header: EmbeddingPageHeaderV1,
}

impl std::fmt::Debug for VerifiedEmbeddingPagesV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedEmbeddingPagesV1")
            .field("artifact_hash", &self.header.artifact_hash)
            .field("row_count", &self.header.row_count)
            .field("dimension", &self.header.dimension)
            .finish()
    }
}

impl VerifiedEmbeddingPagesV1 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EmbeddingPageError> {
        Self::open_expected(path, EmbeddingPageExpectation::default())
    }

    pub fn open_expected(
        path: impl AsRef<Path>,
        expectation: EmbeddingPageExpectation,
    ) -> Result<Self, EmbeddingPageError> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?;
        let actual_len = file
            .metadata()
            .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?
            .len();
        if actual_len > MAX_EMBEDDING_PAGE_BYTES {
            return Err(EmbeddingPageError::Oversized {
                actual: actual_len,
                maximum: MAX_EMBEDDING_PAGE_BYTES,
            });
        }
        if actual_len < size_of::<EmbeddingPageHeaderV1>() as u64 {
            return Err(EmbeddingPageError::TooSmall {
                actual: actual_len,
                minimum: size_of::<EmbeddingPageHeaderV1>() as u64,
            });
        }
        // SAFETY: the map is read-only and retained by this owner. Header,
        // ranges, hashes, typed layouts and every normalized row are verified
        // before borrowed views are exposed.
        let mmap = unsafe {
            MmapOptions::new()
                .map(&file)
                .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?
        };
        let header = *bytemuck::try_from_bytes::<EmbeddingPageHeaderV1>(
            &mmap[..size_of::<EmbeddingPageHeaderV1>()],
        )
        .map_err(|_| EmbeddingPageError::InvalidLayout)?;
        validate_header(&header, actual_len)?;
        validate_expectation(&header, expectation)?;

        let pages = Self { mmap, header };
        pages.verify_pages()?;
        Ok(pages)
    }

    pub fn header(&self) -> &EmbeddingPageHeaderV1 {
        &self.header
    }

    pub fn rows(&self) -> Result<&[EmbeddingRowV1], EmbeddingPageError> {
        cast_page(&self.mmap, self.header.rows_offset, self.header.rows_len)
    }

    pub fn vectors(&self) -> Result<&[f32], EmbeddingPageError> {
        cast_page(
            &self.mmap,
            self.header.vectors_offset,
            self.header.vectors_len,
        )
    }

    pub fn vector(&self, row: usize) -> Result<Option<&[f32]>, EmbeddingPageError> {
        let Some(record) = self.rows()?.get(row) else {
            return Ok(None);
        };
        let start =
            usize::try_from(record.vector_start).map_err(|_| EmbeddingPageError::OutOfBounds)?;
        let end = start
            .checked_add(self.header.dimension as usize)
            .ok_or(EmbeddingPageError::OutOfBounds)?;
        Ok(self.vectors()?.get(start..end))
    }

    pub fn row_by_subject(
        &self,
        subject_id: u64,
    ) -> Result<Option<(usize, &EmbeddingRowV1)>, EmbeddingPageError> {
        let rows = self.rows()?;
        Ok(rows
            .binary_search_by_key(&subject_id, |row| row.subject_id)
            .ok()
            .map(|index| (index, &rows[index])))
    }

    fn verify_pages(&self) -> Result<(), EmbeddingPageError> {
        let rows_bytes = page_bytes(&self.mmap, self.header.rows_offset, self.header.rows_len)?;
        let vectors_bytes = page_bytes(
            &self.mmap,
            self.header.vectors_offset,
            self.header.vectors_len,
        )?;
        if *blake3::hash(rows_bytes).as_bytes() != self.header.rows_hash {
            return Err(EmbeddingPageError::HashMismatch { page: "rows" });
        }
        if *blake3::hash(vectors_bytes).as_bytes() != self.header.vectors_hash {
            return Err(EmbeddingPageError::HashMismatch { page: "vectors" });
        }
        let rows = bytemuck::try_cast_slice::<u8, EmbeddingRowV1>(rows_bytes)
            .map_err(|_| EmbeddingPageError::InvalidLayout)?;
        let vectors = bytemuck::try_cast_slice::<u8, f32>(vectors_bytes)
            .map_err(|_| EmbeddingPageError::InvalidLayout)?;
        validate_rows(&self.header, rows, vectors)
    }
}

fn validate_header(
    header: &EmbeddingPageHeaderV1,
    actual_len: u64,
) -> Result<(), EmbeddingPageError> {
    if header.magic != EMBEDDING_PAGE_MAGIC
        || header.version != EMBEDDING_PAGE_VERSION
        || header.header_size as usize != size_of::<EmbeddingPageHeaderV1>()
        || EmbeddingScalarFormat::from_raw(header.scalar_format) != Some(EmbeddingScalarFormat::F32)
    {
        return Err(EmbeddingPageError::Unsupported);
    }
    if header.flags & EMBEDDING_PAGE_FLAG_COMPLETE == 0 {
        return Err(EmbeddingPageError::Incomplete);
    }
    if header.total_len != actual_len
        || header.dimension == 0
        || header.dimension > MAX_EMBEDDING_DIMENSION
        || header.row_record_size as usize != size_of::<EmbeddingRowV1>()
        || header.row_count == 0
        || header.row_count > MAX_EMBEDDING_ROWS
        || header.rows_offset % EMBEDDING_PAGE_ALIGNMENT != 0
        || header.vectors_offset % EMBEDDING_PAGE_ALIGNMENT != 0
        || header.rows_len != header.row_count * size_of::<EmbeddingRowV1>() as u64
        || header.vectors_len
            != header.row_count * u64::from(header.dimension) * size_of::<f32>() as u64
        || header.generation_hash == [0; 32]
        || header.source_set_hash == [0; 32]
        || header.model_identity_hash == [0; 32]
        || header.model_asset_hash == [0; 32]
        || header.config_hash == [0; 32]
        || compute_artifact_hash(header) != header.artifact_hash
    {
        return Err(EmbeddingPageError::InvalidLayout);
    }
    page_bytes_raw(actual_len, header.rows_offset, header.rows_len)?;
    page_bytes_raw(actual_len, header.vectors_offset, header.vectors_len)?;
    if header.rows_offset + header.rows_len > header.vectors_offset {
        return Err(EmbeddingPageError::InvalidLayout);
    }
    Ok(())
}

fn validate_expectation(
    header: &EmbeddingPageHeaderV1,
    expectation: EmbeddingPageExpectation,
) -> Result<(), EmbeddingPageError> {
    for (expected, actual, field) in [
        (
            expectation.generation_hash,
            header.generation_hash,
            "generation hash",
        ),
        (
            expectation.source_set_hash,
            header.source_set_hash,
            "source set hash",
        ),
        (
            expectation.model_identity_hash,
            header.model_identity_hash,
            "model identity",
        ),
        (expectation.config_hash, header.config_hash, "configuration"),
    ] {
        if expected.is_some_and(|value| value != actual) {
            return Err(EmbeddingPageError::AuthorityMismatch { field });
        }
    }
    Ok(())
}

fn validate_rows(
    header: &EmbeddingPageHeaderV1,
    rows: &[EmbeddingRowV1],
    vectors: &[f32],
) -> Result<(), EmbeddingPageError> {
    let mut previous = None;
    for (index, row) in rows.iter().enumerate() {
        if previous.is_some_and(|id| id >= row.subject_id) {
            return Err(EmbeddingPageError::NonCanonicalRows);
        }
        previous = Some(row.subject_id);
        let expected_start = index as u64 * u64::from(header.dimension);
        if row.subject_id == 0
            || row.source_id == 0
            || row.content_hash == [0; 32]
            || row.vector_start != expected_start
            || row.dimension != header.dimension
            || row.source_start > row.source_end
            || row.source_kind == 0
            || row.content_kind == 0
            || row.flags & ROW_FLAG_NORMALIZED == 0
        {
            return Err(EmbeddingPageError::InvalidRow {
                row: index,
                reason: "authority or vector binding is invalid",
            });
        }
        let start = expected_start as usize;
        let end = start + header.dimension as usize;
        let vector = &vectors[start..end];
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(EmbeddingPageError::InvalidRow {
                row: index,
                reason: "vector contains a non-finite value",
            });
        }
        let norm_squared = vector.iter().map(|value| value * value).sum::<f32>();
        if !(0.998..=1.002).contains(&norm_squared) {
            return Err(EmbeddingPageError::InvalidRow {
                row: index,
                reason: "normalized vector norm is outside tolerance",
            });
        }
    }
    Ok(())
}

fn page_bytes(mmap: &[u8], offset: u64, length: u64) -> Result<&[u8], EmbeddingPageError> {
    let range = page_bytes_raw(mmap.len() as u64, offset, length)?;
    mmap.get(range).ok_or(EmbeddingPageError::OutOfBounds)
}

fn page_bytes_raw(
    total_len: u64,
    offset: u64,
    length: u64,
) -> Result<std::ops::Range<usize>, EmbeddingPageError> {
    let end = offset
        .checked_add(length)
        .ok_or(EmbeddingPageError::OutOfBounds)?;
    if end > total_len {
        return Err(EmbeddingPageError::OutOfBounds);
    }
    let start = usize::try_from(offset).map_err(|_| EmbeddingPageError::OutOfBounds)?;
    let end = usize::try_from(end).map_err(|_| EmbeddingPageError::OutOfBounds)?;
    Ok(start..end)
}

fn cast_page<T: Pod>(mmap: &[u8], offset: u64, length: u64) -> Result<&[T], EmbeddingPageError> {
    bytemuck::try_cast_slice(page_bytes(mmap, offset, length)?)
        .map_err(|_| EmbeddingPageError::InvalidLayout)
}
