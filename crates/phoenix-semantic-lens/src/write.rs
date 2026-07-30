use crate::{
    validate_definition, SemanticCodeRecord, SemanticLensDefinition, SemanticLensError,
    SemanticLensHeader, MAX_PACK_BYTES, MAX_STRING_BYTES, SEMANTIC_LENS_MAGIC,
    SEMANTIC_LENS_VERSION,
};
use bytemuck::bytes_of;
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    mem::size_of,
    path::Path,
};

pub fn write_semantic_lens_pack_new(
    path: &Path,
    definition: &SemanticLensDefinition<'_>,
    bound_generation_hash: [u8; 32],
) -> Result<crate::VerifiedSemanticLensPackV1, SemanticLensError> {
    if bound_generation_hash == [0; 32] {
        return Err(SemanticLensError::MissingGenerationBinding);
    }
    let identity = validate_definition(definition)?;
    let mut strings = Vec::with_capacity(
        definition
            .codes
            .iter()
            .map(|code| code.stable_name.len())
            .sum(),
    );
    let mut records = Vec::with_capacity(definition.codes.len());
    for code in definition.codes {
        let name_offset = u32::try_from(strings.len()).map_err(|_| SemanticLensError::Oversized)?;
        let name_length =
            u32::try_from(code.stable_name.len()).map_err(|_| SemanticLensError::Oversized)?;
        strings.extend_from_slice(code.stable_name.as_bytes());
        records.push(SemanticCodeRecord {
            code: code.code,
            name_offset,
            name_length,
            class: code.class as u16,
            flags_u16: 0,
            source_mask: code.source_endpoints.0,
            target_mask: code.target_endpoints.0,
            flags: code.flags,
            reserved: 0,
        });
    }
    if strings.len() > MAX_STRING_BYTES {
        return Err(SemanticLensError::Oversized);
    }

    let header_len = size_of::<SemanticLensHeader>();
    let codes_len = size_of::<SemanticCodeRecord>()
        .checked_mul(records.len())
        .ok_or(SemanticLensError::Oversized)?;
    let strings_offset = header_len
        .checked_add(codes_len)
        .ok_or(SemanticLensError::Oversized)?;
    let total_len = strings_offset
        .checked_add(strings.len())
        .ok_or(SemanticLensError::Oversized)?;
    if total_len as u64 > MAX_PACK_BYTES {
        return Err(SemanticLensError::Oversized);
    }

    let record_bytes = bytemuck::cast_slice::<SemanticCodeRecord, u8>(&records);
    let mut payload_hasher = blake3::Hasher::new();
    payload_hasher.update(record_bytes);
    payload_hasher.update(&strings);

    let mut header = SemanticLensHeader {
        magic: SEMANTIC_LENS_MAGIC,
        version: SEMANTIC_LENS_VERSION,
        header_len: u16::try_from(header_len).map_err(|_| SemanticLensError::Oversized)?,
        flags: 0,
        lens_id: identity.lens_id,
        namespace_hash: identity.namespace_hash,
        vocabulary_hash: identity.vocabulary_hash,
        configuration_hash: identity.configuration_hash,
        bound_generation_hash,
        code_count: u32::try_from(records.len()).map_err(|_| SemanticLensError::Oversized)?,
        strings_len: u32::try_from(strings.len()).map_err(|_| SemanticLensError::Oversized)?,
        codes_offset: header_len as u64,
        strings_offset: strings_offset as u64,
        total_len: total_len as u64,
        payload_hash: *payload_hasher.finalize().as_bytes(),
        header_hash: [0; 32],
        lens_version: identity.version,
        reserved_u32: [0; 11],
    };
    header.header_hash = hash_header(header);

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                SemanticLensError::AlreadyExists(path.to_path_buf())
            } else {
                SemanticLensError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(bytes_of(&header))
        .and_then(|_| writer.write_all(record_bytes))
        .and_then(|_| writer.write_all(&strings))
        .and_then(|_| writer.flush())
        .map_err(|source| SemanticLensError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| SemanticLensError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    crate::VerifiedSemanticLensPackV1::open(path)
}

pub(crate) fn hash_header(mut header: SemanticLensHeader) -> [u8; 32] {
    header.header_hash = [0; 32];
    *blake3::hash(bytes_of(&header)).as_bytes()
}
