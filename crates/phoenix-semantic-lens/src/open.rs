use crate::{
    format::compute_lens_identity_from_parts, write::hash_header, CoreSemanticClass, EndpointMask,
    LensIdentity, SemanticCodeRecord, SemanticLensError, SemanticLensHeader, MAX_CODE_COUNT,
    MAX_PACK_BYTES, MAX_STRING_BYTES, SEMANTIC_LENS_MAGIC, SEMANTIC_LENS_VERSION,
};
use hashbrown::HashSet;
use memmap2::Mmap;
use std::{fs::File, mem::size_of, path::Path};

pub struct VerifiedSemanticLensPackV1 {
    mmap: Mmap,
    header: SemanticLensHeader,
}

impl std::fmt::Debug for VerifiedSemanticLensPackV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedSemanticLensPackV1")
            .field("identity", &self.identity())
            .field("code_count", &self.header.code_count)
            .finish_non_exhaustive()
    }
}

impl VerifiedSemanticLensPackV1 {
    pub fn open(path: &Path) -> Result<Self, SemanticLensError> {
        let file = File::open(path).map_err(|source| SemanticLensError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let length = file
            .metadata()
            .map_err(|source| SemanticLensError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .len();
        if length > MAX_PACK_BYTES {
            return Err(SemanticLensError::Oversized);
        }
        if length < size_of::<SemanticLensHeader>() as u64 {
            return Err(SemanticLensError::Truncated);
        }
        // SAFETY: The file is kept alive by the mapping and is never mutated by
        // this crate. All offsets, sizes, hashes, and record layouts are checked
        // before any typed slice is exposed.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|source| SemanticLensError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let header = bytemuck::pod_read_unaligned::<SemanticLensHeader>(
            &mmap[..size_of::<SemanticLensHeader>()],
        );
        validate_header(&header, length)?;
        if hash_header(header) != header.header_hash {
            return Err(SemanticLensError::HeaderHashMismatch);
        }

        let pack = Self { mmap, header };
        pack.validate_payload()?;
        Ok(pack)
    }

    pub const fn header(&self) -> &SemanticLensHeader {
        &self.header
    }

    pub const fn identity(&self) -> LensIdentity {
        LensIdentity {
            lens_id: self.header.lens_id,
            namespace_hash: self.header.namespace_hash,
            vocabulary_hash: self.header.vocabulary_hash,
            configuration_hash: self.header.configuration_hash,
            version: self.header.lens_version,
            reserved: [0; 3],
        }
    }

    pub fn codes(&self) -> &[SemanticCodeRecord] {
        let start = self.header.codes_offset as usize;
        let end = self.header.strings_offset as usize;
        bytemuck::cast_slice(&self.mmap[start..end])
    }

    pub fn code(&self, value: u32) -> Option<&SemanticCodeRecord> {
        self.codes()
            .binary_search_by_key(&value, |record| record.code)
            .ok()
            .map(|index| &self.codes()[index])
    }

    pub fn code_name(&self, record: &SemanticCodeRecord) -> Result<&str, SemanticLensError> {
        let strings_start = self.header.strings_offset as usize;
        let start = strings_start
            .checked_add(record.name_offset as usize)
            .ok_or(SemanticLensError::InvalidStringReference)?;
        let end = start
            .checked_add(record.name_length as usize)
            .ok_or(SemanticLensError::InvalidStringReference)?;
        if end > self.mmap.len() {
            return Err(SemanticLensError::InvalidStringReference);
        }
        std::str::from_utf8(&self.mmap[start..end])
            .map_err(|_| SemanticLensError::InvalidStringReference)
    }

    fn validate_payload(&self) -> Result<(), SemanticLensError> {
        let payload_start = self.header.codes_offset as usize;
        let payload = &self.mmap[payload_start..];
        if *blake3::hash(payload).as_bytes() != self.header.payload_hash {
            return Err(SemanticLensError::PayloadHashMismatch);
        }
        let codes = self.codes();
        let mut previous = 0_u32;
        let mut names = HashSet::with_capacity(codes.len());
        let mut vocabulary_hasher = blake3::Hasher::new();
        vocabulary_hasher.update(b"phoenix.semantic-lens-vocabulary/v1\0");
        for record in codes {
            if record.code == 0 || record.code <= previous {
                return Err(SemanticLensError::DuplicateCode(record.code));
            }
            previous = record.code;
            let name = self.code_name(record)?;
            if name.is_empty() || !names.insert(name) {
                return Err(SemanticLensError::InvalidCodeName);
            }
            if CoreSemanticClass::from_raw(record.class).is_none() {
                return Err(SemanticLensError::InvalidSemanticClass(record.code));
            }
            let source = EndpointMask(record.source_mask);
            let target = EndpointMask(record.target_mask);
            if !source.is_valid() || !target.is_valid() {
                return Err(SemanticLensError::InvalidEndpointMask(record.code));
            }
            vocabulary_hasher.update(&record.code.to_le_bytes());
            update_bytes(&mut vocabulary_hasher, name.as_bytes());
            vocabulary_hasher.update(&record.class.to_le_bytes());
            vocabulary_hasher.update(&record.source_mask.to_le_bytes());
            vocabulary_hasher.update(&record.target_mask.to_le_bytes());
            vocabulary_hasher.update(&record.flags.to_le_bytes());
        }
        if *vocabulary_hasher.finalize().as_bytes() != self.header.vocabulary_hash {
            return Err(SemanticLensError::VocabularyHashMismatch);
        }
        let identity = compute_lens_identity_from_parts(
            self.header.namespace_hash,
            self.header.vocabulary_hash,
            self.header.configuration_hash,
            self.header.lens_version,
        );
        if identity.lens_id != self.header.lens_id {
            return Err(SemanticLensError::LensIdentityMismatch);
        }
        Ok(())
    }
}

fn validate_header(
    header: &SemanticLensHeader,
    actual_length: u64,
) -> Result<(), SemanticLensError> {
    if header.magic != SEMANTIC_LENS_MAGIC {
        return Err(SemanticLensError::InvalidMagic);
    }
    if header.version != SEMANTIC_LENS_VERSION {
        return Err(SemanticLensError::UnsupportedVersion(header.version));
    }
    let header_len = size_of::<SemanticLensHeader>() as u64;
    let code_bytes = (header.code_count as u64)
        .checked_mul(size_of::<SemanticCodeRecord>() as u64)
        .ok_or(SemanticLensError::Oversized)?;
    let expected_strings_offset = header_len
        .checked_add(code_bytes)
        .ok_or(SemanticLensError::Oversized)?;
    let expected_total = expected_strings_offset
        .checked_add(header.strings_len as u64)
        .ok_or(SemanticLensError::Oversized)?;
    if header.header_len as u64 != header_len
        || header.lens_version == 0
        || header.lens_id == [0; 32]
        || header.namespace_hash == [0; 32]
        || header.vocabulary_hash == [0; 32]
        || header.bound_generation_hash == [0; 32]
        || header.code_count == 0
        || header.code_count as usize > MAX_CODE_COUNT
        || header.strings_len as usize > MAX_STRING_BYTES
        || header.codes_offset != header_len
        || header.strings_offset != expected_strings_offset
        || header.total_len != expected_total
        || header.total_len != actual_length
    {
        return Err(SemanticLensError::InvalidHeader);
    }
    Ok(())
}

fn update_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
