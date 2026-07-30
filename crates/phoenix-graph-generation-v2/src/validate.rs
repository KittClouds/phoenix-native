use crate::{
    expected_authority, expected_record_alignment, expected_record_size, expected_schema_hash,
    AuthorityClass, GenerationHeader, GraphGenerationV2Error, PageDescriptor, PageKind,
    GRAPH_GENERATION_V2_MAGIC, GRAPH_GENERATION_V2_VERSION, HEADER_FLAG_COMPLETE,
    MAX_GENERATION_BYTES, MAX_PAGE_COUNT, MAX_RECORDS_PER_PAGE, PAGE_ALIGNMENT, PAGE_FLAG_REQUIRED,
};
use std::mem::size_of;

pub fn validate_header(
    header: &GenerationHeader,
    actual_len: u64,
) -> Result<(), GraphGenerationV2Error> {
    let minimum = size_of::<GenerationHeader>() as u64;
    if actual_len < minimum {
        return Err(GraphGenerationV2Error::TooSmall {
            actual: actual_len,
            minimum,
        });
    }
    if actual_len > MAX_GENERATION_BYTES {
        return Err(GraphGenerationV2Error::OversizedGeneration {
            actual: actual_len,
            maximum: MAX_GENERATION_BYTES,
        });
    }
    if header.magic != GRAPH_GENERATION_V2_MAGIC {
        return Err(GraphGenerationV2Error::BadMagic);
    }
    if header.version != GRAPH_GENERATION_V2_VERSION {
        return Err(GraphGenerationV2Error::UnsupportedVersion {
            actual: header.version,
            expected: GRAPH_GENERATION_V2_VERSION,
        });
    }
    let expected_header_size = size_of::<GenerationHeader>() as u32;
    if header.header_size != expected_header_size {
        return Err(GraphGenerationV2Error::HeaderSize {
            actual: header.header_size,
            expected: expected_header_size,
        });
    }
    if header.total_len != actual_len {
        return Err(GraphGenerationV2Error::TotalLength {
            declared: header.total_len,
            actual: actual_len,
        });
    }
    let expected_page_count = PageKind::ALL.len() as u32;
    if header.page_count != expected_page_count || header.page_count as usize > MAX_PAGE_COUNT {
        return Err(GraphGenerationV2Error::PageCount {
            actual: header.page_count,
            expected: expected_page_count,
        });
    }
    if header.flags & HEADER_FLAG_COMPLETE == 0 {
        return Err(GraphGenerationV2Error::IncompleteGeneration);
    }
    if header.directory_offset % PAGE_ALIGNMENT != 0 {
        return Err(GraphGenerationV2Error::DirectoryMisaligned {
            offset: header.directory_offset,
            alignment: PAGE_ALIGNMENT,
        });
    }
    let expected_directory_offset = align_up(minimum, PAGE_ALIGNMENT);
    if header.directory_offset != expected_directory_offset {
        return Err(GraphGenerationV2Error::DirectoryOutOfBounds);
    }
    let expected_directory_len = (size_of::<PageDescriptor>() * PageKind::ALL.len()) as u64;
    if header.directory_len != expected_directory_len {
        return Err(GraphGenerationV2Error::DirectoryLength {
            actual: header.directory_len,
            expected: expected_directory_len,
        });
    }
    let directory_end = header
        .directory_offset
        .checked_add(header.directory_len)
        .ok_or(GraphGenerationV2Error::DirectoryOutOfBounds)?;
    if directory_end > actual_len {
        return Err(GraphGenerationV2Error::DirectoryOutOfBounds);
    }
    Ok(())
}

pub fn validate_directory(
    header: &GenerationHeader,
    directory: &[PageDescriptor],
    bytes: &[u8],
) -> Result<(), GraphGenerationV2Error> {
    let expected_count = PageKind::ALL.len();
    if directory.len() != expected_count {
        return Err(GraphGenerationV2Error::PageCount {
            actual: directory.len() as u32,
            expected: expected_count as u32,
        });
    }

    let pages_begin = align_up(
        header
            .directory_offset
            .checked_add(header.directory_len)
            .ok_or(GraphGenerationV2Error::DirectoryOutOfBounds)?,
        PAGE_ALIGNMENT,
    );
    let mut seen = [false; 29];
    let mut occupied = [(0_u64, 0_u64, PageKind::Strings); 28];
    let mut occupied_count = 0_usize;

    for (index, descriptor) in directory.iter().enumerate() {
        let kind = PageKind::from_raw(descriptor.kind)
            .ok_or(GraphGenerationV2Error::UnknownPageKind(descriptor.kind))?;
        let expected_kind = PageKind::ALL[index];
        if kind != expected_kind {
            return Err(GraphGenerationV2Error::UnexpectedPageOrder {
                index,
                actual: kind,
                expected: expected_kind,
            });
        }
        if seen[kind as usize] {
            return Err(GraphGenerationV2Error::DuplicatePage(kind));
        }
        seen[kind as usize] = true;

        let authority = AuthorityClass::from_raw(descriptor.authority).ok_or(
            GraphGenerationV2Error::UnknownAuthority(descriptor.authority),
        )?;
        let required_authority = expected_authority(kind);
        if authority != required_authority {
            return Err(GraphGenerationV2Error::WrongAuthority {
                page: kind,
                actual: authority,
                expected: required_authority,
            });
        }
        if descriptor.flags & PAGE_FLAG_REQUIRED == 0 {
            return Err(GraphGenerationV2Error::MissingPage(kind));
        }

        let record_size = expected_record_size(kind);
        if descriptor.record_size != record_size {
            return Err(GraphGenerationV2Error::RecordSize {
                page: kind,
                actual: descriptor.record_size,
                expected: record_size,
            });
        }
        let record_alignment = expected_record_alignment(kind);
        if descriptor.record_alignment != record_alignment {
            return Err(GraphGenerationV2Error::RecordAlignment {
                page: kind,
                actual: descriptor.record_alignment,
                expected: record_alignment,
            });
        }
        if descriptor.schema_hash != expected_schema_hash(kind) {
            return Err(GraphGenerationV2Error::SchemaHashMismatch { page: kind });
        }
        if descriptor.offset % PAGE_ALIGNMENT != 0 {
            return Err(GraphGenerationV2Error::PageMisaligned {
                page: kind,
                offset: descriptor.offset,
                alignment: PAGE_ALIGNMENT,
            });
        }
        if descriptor.offset < pages_begin {
            return Err(GraphGenerationV2Error::PageOutOfBounds { page: kind });
        }
        if descriptor.count > MAX_RECORDS_PER_PAGE {
            return Err(GraphGenerationV2Error::RecordCount {
                page: kind,
                actual: descriptor.count,
                maximum: MAX_RECORDS_PER_PAGE,
            });
        }
        let expected_length = descriptor.count.checked_mul(u64::from(record_size)).ok_or(
            GraphGenerationV2Error::PageLength {
                page: kind,
                actual: descriptor.length,
                expected: u64::MAX,
            },
        )?;
        if descriptor.length != expected_length {
            return Err(GraphGenerationV2Error::PageLength {
                page: kind,
                actual: descriptor.length,
                expected: expected_length,
            });
        }
        let end = descriptor
            .offset
            .checked_add(descriptor.length)
            .ok_or(GraphGenerationV2Error::PageOutOfBounds { page: kind })?;
        if end > header.total_len {
            return Err(GraphGenerationV2Error::PageOutOfBounds { page: kind });
        }
        let start_index = usize::try_from(descriptor.offset)
            .map_err(|_| GraphGenerationV2Error::PageOutOfBounds { page: kind })?;
        let end_index = usize::try_from(end)
            .map_err(|_| GraphGenerationV2Error::PageOutOfBounds { page: kind })?;
        let payload = bytes
            .get(start_index..end_index)
            .ok_or(GraphGenerationV2Error::PageOutOfBounds { page: kind })?;
        validate_page_payload(kind, descriptor, payload)?;
        if descriptor.length != 0 {
            occupied[occupied_count] = (descriptor.offset, end, kind);
            occupied_count += 1;
        }
    }

    for kind in PageKind::ALL {
        if !seen[kind as usize] {
            return Err(GraphGenerationV2Error::MissingPage(kind));
        }
    }

    let occupied = &mut occupied[..occupied_count];
    occupied.sort_unstable_by_key(|(start, _, _)| *start);
    for pair in occupied.windows(2) {
        let (_, left_end, left_kind) = pair[0];
        let (right_start, _, right_kind) = pair[1];
        if left_end > right_start {
            return Err(GraphGenerationV2Error::OverlappingPages {
                left: left_kind,
                right: right_kind,
            });
        }
    }

    if compute_generation_hash(header, directory) != header.generation_hash {
        return Err(GraphGenerationV2Error::GenerationHashMismatch);
    }
    Ok(())
}

pub fn validate_page_payload(
    kind: PageKind,
    descriptor: &PageDescriptor,
    payload: &[u8],
) -> Result<(), GraphGenerationV2Error> {
    if blake3::hash(payload).as_bytes() != &descriptor.hash {
        return Err(GraphGenerationV2Error::PageHashMismatch { page: kind });
    }
    Ok(())
}

pub fn compute_generation_hash(
    header: &GenerationHeader,
    directory: &[PageDescriptor],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v2/generation\0");
    hasher.update(&header.source_document_id_hash);
    hasher.update(&header.content_hash);
    hasher.update(&header.cohort_hash);
    hasher.update(&header.native_document_id.to_le_bytes());
    hasher.update(&header.document_revision.to_le_bytes());
    hasher.update(&header.registry_revision.to_le_bytes());
    hasher.update(&header.producer_generation.to_le_bytes());
    hasher.update(&header.published_generation.to_le_bytes());
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
