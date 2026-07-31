use crate::{
    expected_authority, expected_record_alignment, expected_record_size, expected_schema_hash,
    GenerationHeaderV3, MemoryContractError, PageDescriptorV3, PageKindV3,
    GRAPH_GENERATION_V3_MAGIC, GRAPH_GENERATION_V3_VERSION, HEADER_FLAG_COMPLETE,
    MAX_GENERATION_BYTES, MAX_PAGE_COUNT, MAX_RECORDS_PER_PAGE, PAGE_ALIGNMENT, PAGE_FLAG_REQUIRED,
};
use phoenix_graph_generation_v2::AuthorityClass;
use std::mem::size_of;

pub fn validate_header(
    header: &GenerationHeaderV3,
    actual_len: u64,
) -> Result<(), MemoryContractError> {
    let minimum = size_of::<GenerationHeaderV3>() as u64;
    if actual_len < minimum {
        return Err(MemoryContractError::TooSmall {
            actual: actual_len,
            minimum,
        });
    }
    if actual_len > MAX_GENERATION_BYTES || header.total_len > MAX_GENERATION_BYTES {
        return Err(MemoryContractError::OversizedGeneration {
            actual: actual_len.max(header.total_len),
            maximum: MAX_GENERATION_BYTES,
        });
    }
    if header.magic != GRAPH_GENERATION_V3_MAGIC {
        return Err(MemoryContractError::BadMagic);
    }
    if header.version != GRAPH_GENERATION_V3_VERSION {
        return Err(MemoryContractError::UnsupportedVersion {
            actual: header.version,
            expected: GRAPH_GENERATION_V3_VERSION,
        });
    }
    let expected_header_size = size_of::<GenerationHeaderV3>() as u32;
    if header.header_size != expected_header_size {
        return Err(MemoryContractError::HeaderSize {
            actual: header.header_size,
            expected: expected_header_size,
        });
    }
    if header.total_len != actual_len {
        return Err(MemoryContractError::TotalLength {
            declared: header.total_len,
            actual: actual_len,
        });
    }
    let expected_page_count = PageKindV3::ALL.len() as u32;
    if header.page_count != expected_page_count || header.page_count as usize > MAX_PAGE_COUNT {
        return Err(MemoryContractError::PageCount {
            actual: header.page_count,
            expected: expected_page_count,
        });
    }
    if header.flags & HEADER_FLAG_COMPLETE == 0 {
        return Err(MemoryContractError::IncompleteGeneration);
    }
    let expected_directory_offset = align_up(minimum, PAGE_ALIGNMENT);
    if header.directory_offset != expected_directory_offset
        || header.directory_offset % PAGE_ALIGNMENT != 0
    {
        return Err(MemoryContractError::DirectoryOutOfBounds);
    }
    let expected_directory_len = (size_of::<PageDescriptorV3>() * PageKindV3::ALL.len()) as u64;
    if header.directory_len != expected_directory_len {
        return Err(MemoryContractError::DirectoryLength {
            actual: header.directory_len,
            expected: expected_directory_len,
        });
    }
    if header
        .directory_offset
        .checked_add(header.directory_len)
        .is_none_or(|end| end > actual_len)
    {
        return Err(MemoryContractError::DirectoryOutOfBounds);
    }
    Ok(())
}

pub fn validate_directory(
    header: &GenerationHeaderV3,
    directory: &[PageDescriptorV3],
    bytes: &[u8],
) -> Result<(), MemoryContractError> {
    if directory.len() != PageKindV3::ALL.len() {
        return Err(MemoryContractError::PageCount {
            actual: directory.len() as u32,
            expected: PageKindV3::ALL.len() as u32,
        });
    }
    let pages_begin = align_up(
        header
            .directory_offset
            .checked_add(header.directory_len)
            .ok_or(MemoryContractError::DirectoryOutOfBounds)?,
        PAGE_ALIGNMENT,
    );
    let mut occupied = Vec::with_capacity(directory.len());

    for (index, descriptor) in directory.iter().enumerate() {
        let kind = PageKindV3::from_raw(descriptor.kind)
            .ok_or(MemoryContractError::UnknownPageKind(descriptor.kind))?;
        let expected_kind = PageKindV3::ALL[index];
        if kind != expected_kind {
            return Err(MemoryContractError::UnexpectedPageOrder {
                index,
                actual: kind,
                expected: expected_kind,
            });
        }
        let authority = AuthorityClass::from_raw(descriptor.authority)
            .ok_or(MemoryContractError::UnknownAuthority(descriptor.authority))?;
        let required_authority = expected_authority(kind);
        if authority != required_authority {
            return Err(MemoryContractError::WrongAuthority {
                page: kind,
                actual: authority,
                expected: required_authority,
            });
        }
        if descriptor.flags & PAGE_FLAG_REQUIRED == 0 {
            return Err(MemoryContractError::InvalidSourceModel(
                "required page is not marked required",
            ));
        }
        let record_size = expected_record_size(kind);
        if descriptor.record_size != record_size {
            return Err(MemoryContractError::RecordSize {
                page: kind,
                actual: descriptor.record_size,
                expected: record_size,
            });
        }
        let record_alignment = expected_record_alignment(kind);
        if descriptor.record_alignment != record_alignment {
            return Err(MemoryContractError::RecordAlignment {
                page: kind,
                actual: descriptor.record_alignment,
                expected: record_alignment,
            });
        }
        if descriptor.schema_hash != expected_schema_hash(kind) {
            return Err(MemoryContractError::SchemaHashMismatch { page: kind });
        }
        if descriptor.offset < pages_begin || descriptor.offset % PAGE_ALIGNMENT != 0 {
            return Err(MemoryContractError::Misaligned {
                offset: descriptor.offset,
                alignment: PAGE_ALIGNMENT,
            });
        }
        if descriptor.count > MAX_RECORDS_PER_PAGE {
            return Err(MemoryContractError::RecordCount {
                page: kind,
                actual: descriptor.count,
                maximum: MAX_RECORDS_PER_PAGE,
            });
        }
        let expected_length = descriptor.count.checked_mul(u64::from(record_size)).ok_or(
            MemoryContractError::PageLength {
                page: kind,
                actual: descriptor.length,
                expected: u64::MAX,
            },
        )?;
        if descriptor.length != expected_length {
            return Err(MemoryContractError::PageLength {
                page: kind,
                actual: descriptor.length,
                expected: expected_length,
            });
        }
        let end = descriptor
            .offset
            .checked_add(descriptor.length)
            .ok_or(MemoryContractError::PageOutOfBounds { page: kind })?;
        if end > header.total_len {
            return Err(MemoryContractError::PageOutOfBounds { page: kind });
        }
        let start = usize::try_from(descriptor.offset)
            .map_err(|_| MemoryContractError::PageOutOfBounds { page: kind })?;
        let end = usize::try_from(end)
            .map_err(|_| MemoryContractError::PageOutOfBounds { page: kind })?;
        let payload = bytes
            .get(start..end)
            .ok_or(MemoryContractError::PageOutOfBounds { page: kind })?;
        if blake3::hash(payload).as_bytes() != &descriptor.hash {
            return Err(MemoryContractError::PageHashMismatch { page: kind });
        }
        if descriptor.length != 0 {
            occupied.push((descriptor.offset, end as u64, kind));
        }
    }

    occupied.sort_unstable_by_key(|entry| entry.0);
    for pair in occupied.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(MemoryContractError::OverlappingPages {
                left: pair[0].2,
                right: pair[1].2,
            });
        }
    }
    if compute_generation_hash(header, directory) != header.generation_hash {
        return Err(MemoryContractError::GenerationHashMismatch);
    }
    Ok(())
}

pub fn compute_source_set_hash(
    strings_hash: &[u8; 32],
    source_page_hashes: &[[u8; 32]],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v3/source-set\0");
    hasher.update(strings_hash);
    for hash in source_page_hashes {
        hasher.update(hash);
    }
    *hasher.finalize().as_bytes()
}

pub fn compute_generation_hash(
    header: &GenerationHeaderV3,
    directory: &[PageDescriptorV3],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v3/generation\0");
    hasher.update(&header.namespace_hash);
    hasher.update(&header.source_set_hash);
    hasher.update(&header.cohort_hash);
    hasher.update(&header.registry_revision.to_le_bytes());
    hasher.update(&header.producer_generation.to_le_bytes());
    hasher.update(&header.published_generation.to_le_bytes());
    hasher.update(&header.source_count.to_le_bytes());
    hasher.update(&header.document_revision_count.to_le_bytes());
    hasher.update(&header.conversation_count.to_le_bytes());
    hasher.update(&header.turn_count.to_le_bytes());
    for descriptor in directory {
        hasher.update(&descriptor.kind.to_le_bytes());
        hasher.update(&descriptor.authority.to_le_bytes());
        hasher.update(&descriptor.record_size.to_le_bytes());
        hasher.update(&descriptor.record_alignment.to_le_bytes());
        hasher.update(&descriptor.flags.to_le_bytes());
        hasher.update(&descriptor.length.to_le_bytes());
        hasher.update(&descriptor.count.to_le_bytes());
        hasher.update(&descriptor.hash);
        hasher.update(&descriptor.schema_hash);
    }
    *hasher.finalize().as_bytes()
}

pub const fn align_up(value: u64, alignment: u64) -> u64 {
    let mask = alignment - 1;
    (value + mask) & !mask
}
