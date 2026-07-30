use crate::contract::{
    GenerationAuthorityHeaderV1, AUTHORITY_EXTENSION, AUTHORITY_MAGIC, MAX_AUTHORITY_BYTES,
    REVIEW_VERSION,
};
use crate::SemanticReviewError;
use bytemuck::{bytes_of, try_from_bytes};
use memmap2::MmapOptions;
use phoenix_graph_generation_v2::VerifiedGraphGenerationV2;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};

pub struct VerifiedGenerationAuthority {
    header: GenerationAuthorityHeaderV1,
    path: PathBuf,
    active_name: String,
    previous_name: Option<String>,
    generation: VerifiedGraphGenerationV2,
}

impl VerifiedGenerationAuthority {
    pub fn header(&self) -> &GenerationAuthorityHeaderV1 {
        &self.header
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn active_name(&self) -> &str {
        &self.active_name
    }

    pub fn previous_name(&self) -> Option<&str> {
        self.previous_name.as_deref()
    }

    pub fn generation(&self) -> &VerifiedGraphGenerationV2 {
        &self.generation
    }
}

struct OpenAuthorityRecord {
    header: GenerationAuthorityHeaderV1,
    path: PathBuf,
    active_name: String,
    previous_name: Option<String>,
}

pub fn append_authority_record(
    root: impl AsRef<Path>,
    generation_path: impl AsRef<Path>,
) -> Result<VerifiedGenerationAuthority, SemanticReviewError> {
    let root = root.as_ref();
    fs::create_dir_all(root).map_err(|source| SemanticReviewError::io(root, source))?;
    let generation_path = generation_path.as_ref();
    let active_name = direct_file_name(root, generation_path)?;
    let generation = VerifiedGraphGenerationV2::open(generation_path)?;
    let previous = open_current_record(root)?;
    let sequence = match previous.as_ref() {
        Some(record) => record
            .header
            .sequence
            .checked_add(1)
            .ok_or(SemanticReviewError::GenerationAuthorityMismatch)?,
        None => 1,
    };
    let previous_hash = previous
        .as_ref()
        .map_or([0; 32], |record| record.header.active_generation_hash);
    let previous_name = previous.as_ref().map(|record| record.active_name.as_str());
    let parent_hash = previous
        .as_ref()
        .map_or([0; 32], |record| record.header.authority_hash);
    append_record(
        root,
        sequence,
        &active_name,
        generation.header().generation_hash,
        previous_name,
        previous_hash,
        parent_hash,
    )?;
    open_current_authority(root)
}

pub fn rollback_authority(
    root: impl AsRef<Path>,
) -> Result<VerifiedGenerationAuthority, SemanticReviewError> {
    let root = root.as_ref();
    let current = open_current_record(root)?.ok_or(SemanticReviewError::MissingAuthority)?;
    let previous_name = current
        .previous_name
        .as_deref()
        .ok_or(SemanticReviewError::NoRollbackGeneration)?;
    let previous_path = root.join(previous_name);
    let previous_generation = VerifiedGraphGenerationV2::open(&previous_path)?;
    if previous_generation.header().generation_hash != current.header.previous_generation_hash {
        return Err(SemanticReviewError::GenerationAuthorityMismatch);
    }
    append_record(
        root,
        current
            .header
            .sequence
            .checked_add(1)
            .ok_or(SemanticReviewError::GenerationAuthorityMismatch)?,
        previous_name,
        previous_generation.header().generation_hash,
        Some(&current.active_name),
        current.header.active_generation_hash,
        current.header.authority_hash,
    )?;
    open_current_authority(root)
}

pub fn open_current_authority(
    root: impl AsRef<Path>,
) -> Result<VerifiedGenerationAuthority, SemanticReviewError> {
    let root = root.as_ref();
    let current = open_current_record(root)?.ok_or(SemanticReviewError::MissingAuthority)?;
    let generation_path = root.join(&current.active_name);
    let generation = VerifiedGraphGenerationV2::open(&generation_path)?;
    if generation.header().generation_hash != current.header.active_generation_hash {
        return Err(SemanticReviewError::GenerationAuthorityMismatch);
    }
    Ok(VerifiedGenerationAuthority {
        header: current.header,
        path: current.path,
        active_name: current.active_name,
        previous_name: current.previous_name,
        generation,
    })
}

fn open_current_record(root: &Path) -> Result<Option<OpenAuthorityRecord>, SemanticReviewError> {
    if !root.exists() {
        return Ok(None);
    }
    let mut paths = fs::read_dir(root)
        .map_err(|source| SemanticReviewError::io(root, source))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|value| value.to_str()) == Some(AUTHORITY_EXTENSION)
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    let mut current: Option<OpenAuthorityRecord> = None;
    for path in paths {
        let record = open_record(&path)?;
        let expected_sequence = match current.as_ref() {
            Some(previous) => previous
                .header
                .sequence
                .checked_add(1)
                .ok_or(SemanticReviewError::GenerationAuthorityMismatch)?,
            None => 1,
        };
        let expected_parent = current
            .as_ref()
            .map_or([0; 32], |previous| previous.header.authority_hash);
        if record.header.sequence != expected_sequence
            || record.header.parent_authority_hash != expected_parent
        {
            return Err(SemanticReviewError::GenerationAuthorityMismatch);
        }
        current = Some(record);
    }
    Ok(current)
}

fn append_record(
    root: &Path,
    sequence: u64,
    active_name: &str,
    active_hash: [u8; 32],
    previous_name: Option<&str>,
    previous_hash: [u8; 32],
    parent_hash: [u8; 32],
) -> Result<(), SemanticReviewError> {
    validate_name(active_name)?;
    if let Some(name) = previous_name {
        validate_name(name)?;
    }
    let active = active_name.as_bytes();
    let previous = previous_name.unwrap_or("").as_bytes();
    let total_len = size_of::<GenerationAuthorityHeaderV1>() + active.len() + previous.len();
    if total_len as u64 > MAX_AUTHORITY_BYTES {
        return Err(SemanticReviewError::OversizedArtifact {
            actual: total_len as u64,
            maximum: MAX_AUTHORITY_BYTES,
        });
    }
    let mut header = GenerationAuthorityHeaderV1 {
        magic: AUTHORITY_MAGIC,
        version: REVIEW_VERSION,
        header_size: size_of::<GenerationAuthorityHeaderV1>() as u32,
        total_len: total_len as u64,
        sequence,
        active_generation_hash: active_hash,
        previous_generation_hash: previous_hash,
        parent_authority_hash: parent_hash,
        authority_hash: [0; 32],
        active_name_len: active.len() as u32,
        previous_name_len: previous.len() as u32,
        flags: 0,
        reserved: 0,
    };
    header.authority_hash = authority_hash(&header, active, previous);
    let path = root.join(format!(
        "authority-{sequence:020}-{}.{}",
        short_hex(header.authority_hash),
        AUTHORITY_EXTENSION
    ));
    let pending = path.with_extension("pending");
    if pending.exists() {
        fs::remove_file(&pending).map_err(|source| SemanticReviewError::io(&pending, source))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|source| SemanticReviewError::io(&pending, source))?;
    let write_result = file
        .write_all(bytes_of(&header))
        .and_then(|_| file.write_all(active))
        .and_then(|_| file.write_all(previous))
        .and_then(|_| file.sync_all());
    if let Err(source) = write_result {
        let _ = fs::remove_file(&pending);
        return Err(SemanticReviewError::io(&pending, source));
    }
    if let Err(source) = fs::rename(&pending, &path) {
        let _ = fs::remove_file(&pending);
        return Err(SemanticReviewError::io(path, source));
    }
    Ok(())
}

fn open_record(path: &Path) -> Result<OpenAuthorityRecord, SemanticReviewError> {
    let file = File::open(path).map_err(|source| SemanticReviewError::io(path, source))?;
    let len = file
        .metadata()
        .map_err(|source| SemanticReviewError::io(path, source))?
        .len();
    if len > MAX_AUTHORITY_BYTES {
        return Err(SemanticReviewError::OversizedArtifact {
            actual: len,
            maximum: MAX_AUTHORITY_BYTES,
        });
    }
    // SAFETY: the file is mapped read-only for the duration of validation.
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .map_err(|source| SemanticReviewError::io(path, source))?
    };
    let header = *try_from_bytes::<GenerationAuthorityHeaderV1>(
        mmap.get(..size_of::<GenerationAuthorityHeaderV1>())
            .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?,
    )
    .map_err(|_| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let active_start = header.header_size as usize;
    let active_end = active_start
        .checked_add(header.active_name_len as usize)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let previous_end = active_end
        .checked_add(header.previous_name_len as usize)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let active = mmap
        .get(active_start..active_end)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    let previous = mmap
        .get(active_end..previous_end)
        .ok_or_else(|| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?;
    if header.magic != AUTHORITY_MAGIC
        || header.version != REVIEW_VERSION
        || header.header_size as usize != size_of::<GenerationAuthorityHeaderV1>()
        || header.total_len != len
        || previous_end != len as usize
        || header.authority_hash != authority_hash(&header, active, previous)
    {
        return Err(SemanticReviewError::CorruptArtifact(path.to_path_buf()));
    }
    let active_name = std::str::from_utf8(active)
        .map_err(|_| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?
        .to_owned();
    let previous_name = if previous.is_empty() {
        None
    } else {
        Some(
            std::str::from_utf8(previous)
                .map_err(|_| SemanticReviewError::CorruptArtifact(path.to_path_buf()))?
                .to_owned(),
        )
    };
    validate_name(&active_name)?;
    if let Some(name) = previous_name.as_deref() {
        validate_name(name)?;
    }
    Ok(OpenAuthorityRecord {
        header,
        path: path.to_path_buf(),
        active_name,
        previous_name,
    })
}

fn direct_file_name(root: &Path, generation: &Path) -> Result<String, SemanticReviewError> {
    let root = fs::canonicalize(root).map_err(|source| SemanticReviewError::io(root, source))?;
    let parent = generation
        .parent()
        .ok_or(SemanticReviewError::InvalidGenerationPath)?;
    let parent =
        fs::canonicalize(parent).map_err(|source| SemanticReviewError::io(parent, source))?;
    if root != parent {
        return Err(SemanticReviewError::InvalidGenerationPath);
    }
    let name = generation
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SemanticReviewError::InvalidGenerationPath)?
        .to_owned();
    validate_name(&name)?;
    Ok(name)
}

fn validate_name(name: &str) -> Result<(), SemanticReviewError> {
    if name.is_empty()
        || name.as_bytes().contains(&0)
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
    {
        return Err(SemanticReviewError::InvalidGenerationPath);
    }
    Ok(())
}

fn authority_hash(
    header: &GenerationAuthorityHeaderV1,
    active: &[u8],
    previous: &[u8],
) -> [u8; 32] {
    let mut unhashed = *header;
    unhashed.authority_hash = [0; 32];
    let mut hash = blake3::Hasher::new();
    hash.update(bytes_of(&unhashed));
    hash.update(active);
    hash.update(previous);
    *hash.finalize().as_bytes()
}

fn short_hex(value: [u8; 32]) -> String {
    let mut output = String::with_capacity(16);
    for byte in &value[..8] {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}
