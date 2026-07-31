use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

use bytemuck::{bytes_of, cast_slice};

use crate::{
    compute_artifact_hash, format::align_up, EmbeddingPageError, EmbeddingPageHeaderV1,
    EmbeddingRowV1, EmbeddingScalarFormat, VerifiedEmbeddingPagesV1, EMBEDDING_PAGE_ALIGNMENT,
    EMBEDDING_PAGE_FLAG_COMPLETE, EMBEDDING_PAGE_MAGIC, EMBEDDING_PAGE_VERSION,
    MAX_EMBEDDING_DIMENSION, MAX_EMBEDDING_PAGE_BYTES, MAX_EMBEDDING_ROWS, ROW_FLAG_NORMALIZED,
};

#[derive(Clone, Copy, Debug)]
pub struct EmbeddingPageWriteAuthority {
    pub generation_hash: [u8; 32],
    pub source_set_hash: [u8; 32],
    pub model_identity_hash: [u8; 32],
    pub model_asset_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub dimension: u32,
}

pub fn write_embedding_pages_new(
    path: impl AsRef<Path>,
    authority: EmbeddingPageWriteAuthority,
    rows: &[EmbeddingRowV1],
    vectors: &[f32],
) -> Result<VerifiedEmbeddingPagesV1, EmbeddingPageError> {
    validate_write_inputs(authority, rows, vectors)?;
    let path = path.as_ref();
    let result = write_file(path, authority, rows, vectors);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result?;
    match VerifiedEmbeddingPagesV1::open(path) {
        Ok(pages) => Ok(pages),
        Err(error) => {
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

fn validate_write_inputs(
    authority: EmbeddingPageWriteAuthority,
    rows: &[EmbeddingRowV1],
    vectors: &[f32],
) -> Result<(), EmbeddingPageError> {
    if authority.dimension == 0 || authority.dimension > MAX_EMBEDDING_DIMENSION {
        return Err(EmbeddingPageError::InvalidLayout);
    }
    if rows.is_empty() || rows.len() as u64 > MAX_EMBEDDING_ROWS {
        return Err(EmbeddingPageError::InvalidLayout);
    }
    let expected_values = rows
        .len()
        .checked_mul(authority.dimension as usize)
        .ok_or(EmbeddingPageError::InvalidLayout)?;
    if vectors.len() != expected_values {
        return Err(EmbeddingPageError::InvalidLayout);
    }
    let mut previous = None;
    for (index, row) in rows.iter().enumerate() {
        if previous.is_some_and(|id| id >= row.subject_id) {
            return Err(EmbeddingPageError::NonCanonicalRows);
        }
        previous = Some(row.subject_id);
        let expected_start = index
            .checked_mul(authority.dimension as usize)
            .ok_or(EmbeddingPageError::InvalidLayout)? as u64;
        if row.subject_id == 0
            || row.source_id == 0
            || row.content_hash == [0; 32]
            || row.dimension != authority.dimension
            || row.vector_start != expected_start
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
        let end = start + authority.dimension as usize;
        validate_vector(index, &vectors[start..end])?;
    }
    Ok(())
}

fn validate_vector(row: usize, vector: &[f32]) -> Result<(), EmbeddingPageError> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingPageError::InvalidRow {
            row,
            reason: "vector contains a non-finite value",
        });
    }
    let norm_squared = vector.iter().map(|value| value * value).sum::<f32>();
    if !(0.998..=1.002).contains(&norm_squared) {
        return Err(EmbeddingPageError::InvalidRow {
            row,
            reason: "normalized vector norm is outside tolerance",
        });
    }
    Ok(())
}

fn write_file(
    path: &Path,
    authority: EmbeddingPageWriteAuthority,
    rows: &[EmbeddingRowV1],
    vectors: &[f32],
) -> Result<(), EmbeddingPageError> {
    let header_size = size_of::<EmbeddingPageHeaderV1>() as u64;
    let rows_offset = align_up(header_size, EMBEDDING_PAGE_ALIGNMENT);
    let rows_len = std::mem::size_of_val(rows) as u64;
    let vectors_offset = align_up(rows_offset + rows_len, EMBEDDING_PAGE_ALIGNMENT);
    let vectors_len = std::mem::size_of_val(vectors) as u64;
    let total_len = align_up(vectors_offset + vectors_len, EMBEDDING_PAGE_ALIGNMENT);
    if total_len > MAX_EMBEDDING_PAGE_BYTES {
        return Err(EmbeddingPageError::Oversized {
            actual: total_len,
            maximum: MAX_EMBEDDING_PAGE_BYTES,
        });
    }

    let rows_bytes = cast_slice(rows);
    let vectors_bytes = cast_slice(vectors);
    let mut header = EmbeddingPageHeaderV1 {
        magic: EMBEDDING_PAGE_MAGIC,
        version: EMBEDDING_PAGE_VERSION,
        header_size: header_size as u32,
        flags: EMBEDDING_PAGE_FLAG_COMPLETE,
        scalar_format: EmbeddingScalarFormat::F32 as u16,
        reserved_u16: 0,
        dimension: authority.dimension,
        row_record_size: size_of::<EmbeddingRowV1>() as u32,
        total_len,
        rows_offset,
        rows_len,
        vectors_offset,
        vectors_len,
        row_count: rows.len() as u64,
        generation_hash: authority.generation_hash,
        source_set_hash: authority.source_set_hash,
        model_identity_hash: authority.model_identity_hash,
        model_asset_hash: authority.model_asset_hash,
        config_hash: authority.config_hash,
        rows_hash: *blake3::hash(rows_bytes).as_bytes(),
        vectors_hash: *blake3::hash(vectors_bytes).as_bytes(),
        artifact_hash: [0; 32],
        reserved: [0; 4],
    };
    header.artifact_hash = compute_artifact_hash(&header);

    let mut provisional = header;
    provisional.flags = 0;
    provisional.artifact_hash = [0; 32];
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?;
    write_at(&mut file, 0, bytes_of(&provisional), path)?;
    write_at(&mut file, rows_offset, rows_bytes, path)?;
    write_at(&mut file, vectors_offset, vectors_bytes, path)?;
    file.set_len(total_len)
        .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?;
    file.sync_all()
        .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))?;
    write_at(&mut file, 0, bytes_of(&header), path)?;
    file.sync_all()
        .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))
}

fn write_at(
    file: &mut std::fs::File,
    offset: u64,
    bytes: &[u8],
    path: &Path,
) -> Result<(), EmbeddingPageError> {
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.write_all(bytes))
        .map_err(|source| EmbeddingPageError::io(path.to_path_buf(), source))
}
