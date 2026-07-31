use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use memmap2::Mmap;
use serde::de::DeserializeOwned;
use serde::Serialize;

const HEADER_BYTES: usize = 8 + 8 + 32;
const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub fn write_artifact<T: Serialize>(path: &Path, magic: [u8; 8], value: &T) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite existing artifact {}", path.display());
    }
    let payload = postcard::to_allocvec(value).context("encode postcard artifact")?;
    let payload_len = u64::try_from(payload.len()).context("artifact payload length overflow")?;
    if payload_len > MAX_ARTIFACT_BYTES {
        bail!("artifact payload is oversized: {payload_len} bytes");
    }
    let digest = blake3::hash(&payload);
    let temp = temporary_sibling(path)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("create {}", temp.display()))?;
        file.write_all(&magic)?;
        file.write_all(&payload_len.to_le_bytes())?;
        file.write_all(digest.as_bytes())?;
        file.write_all(&payload)?;
        file.sync_all()?;
        fs::rename(&temp, path).with_context(|| format!("publish artifact {}", path.display()))?;
        sync_parent(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn read_artifact<T: DeserializeOwned>(path: &Path, magic: [u8; 8]) -> Result<T> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let metadata = file.metadata()?;
    if metadata.len() < HEADER_BYTES as u64 || metadata.len() > MAX_ARTIFACT_BYTES {
        bail!("artifact size is invalid: {} bytes", metadata.len());
    }
    // SAFETY: the mapping is read-only and remains valid because `Mmap` owns the
    // OS mapping independently after creation. No writer is opened here.
    let mapped = unsafe { Mmap::map(&file) }.context("map artifact read-only")?;
    if mapped[..8] != magic {
        bail!("artifact type mismatch for {}", path.display());
    }
    let payload_len = u64::from_le_bytes(mapped[8..16].try_into()?);
    let expected_total = HEADER_BYTES as u64 + payload_len;
    if expected_total != metadata.len() {
        bail!(
            "artifact length mismatch: header says {expected_total}, file has {}",
            metadata.len()
        );
    }
    let payload = &mapped[HEADER_BYTES..];
    let digest = blake3::hash(payload);
    if digest.as_bytes() != &mapped[16..48] {
        bail!("artifact digest mismatch for {}", path.display());
    }
    postcard::from_bytes(payload).context("decode postcard artifact")
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn temporary_sibling(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("artifact path has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("artifact filename is not UTF-8")?;
    Ok(parent.join(format!(".{name}.tmp-{}", std::process::id())))
}

fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let parent = path.parent().context("artifact path has no parent")?;
        File::open(parent)
            .with_context(|| format!("open parent directory {}", parent.display()))?
            .sync_all()
            .context("sync artifact parent directory")
    }
}
