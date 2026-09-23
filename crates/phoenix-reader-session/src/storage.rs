use crate::{Digest, Error, Result};
use memmap2::{Mmap, MmapOptions};
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use windows::{
    core::PCWSTR,
    Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
};

pub(crate) fn hex(hash: Digest) -> String {
    blake3::Hash::from(hash).to_hex().to_string()
}
pub(crate) fn owner(root: &Path) -> Result<File> {
    fs::create_dir_all(root)?;
    // Exclusive OS handle, released on process death; never a stale sentinel.
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root.join(".owner"))?)
}
pub(crate) fn sync_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
pub(crate) fn publish(from: &Path, to: &Path, replace: bool) -> Result<()> {
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = if replace {
        MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING
    } else {
        MOVEFILE_WRITE_THROUGH
    };
    // SAFETY: NUL-terminated buffers live through the synchronous call.
    unsafe { MoveFileExW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr()), flags) }
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}
pub(crate) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.writing", uuid::Uuid::new_v4()));
    let result = (|| {
        sync_new(&tmp, bytes)?;
        publish(&tmp, path, true)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Immutable mmap with an OS handle denying concurrent writers and deletion.
/// Only committed artifacts may be opened; never maps an active writer.
pub struct VerifiedBytes {
    map: Mmap,
    _file: File,
}
impl VerifiedBytes {
    pub(crate) fn open(path: &Path, max: u64, expected: Digest) -> Result<Self> {
        let file = OpenOptions::new().read(true).share_mode(1).open(path)?;
        let len = file.metadata()?.len();
        if len == 0 || len > max {
            return Err(Error::Invalid("artifact size"));
        }
        // SAFETY: the Windows file handle permits only shared reads, preventing
        // mutation/truncation by normal filesystem handles for the map lifetime.
        let map = unsafe { MmapOptions::new().map(&file)? };
        if blake3::hash(&map).as_bytes() != &expected {
            return Err(Error::Invalid("artifact checksum"));
        }
        Ok(Self { map, _file: file })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.map
    }
}

pub struct SnapshotStore {
    root: PathBuf,
    _owner: File,
}
impl SnapshotStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let lock = owner(&root)?;
        Ok(Self { root, _owner: lock })
    }
    pub fn retain(&self, lease: &phoenix_workspace::DocumentLease) -> Result<Digest> {
        if lease.revision.0 == 0
            || lease.content.is_empty()
            || lease.content.len() > phoenix_workspace::MAX_DOCUMENT_BYTES
            || blake3::hash(lease.content.as_bytes()).as_bytes() != &lease.content_hash.0
        {
            return Err(Error::Invalid("snapshot lease"));
        }
        let hash = lease.content_hash.0;
        let path = self.root.join(hex(hash));
        if path.exists() {
            self.load(hash)?;
            return Ok(hash);
        }
        let temp = self.root.join(format!("{}.writing", uuid::Uuid::new_v4()));
        let result = (|| {
            sync_new(&temp, lease.content.as_bytes())?;
            publish(&temp, &path, false)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        Ok(hash)
    }
    pub fn load(&self, hash: Digest) -> Result<VerifiedBytes> {
        let bytes = VerifiedBytes::open(
            &self.root.join(hex(hash)),
            phoenix_workspace::MAX_DOCUMENT_BYTES as u64,
            hash,
        )?;
        std::str::from_utf8(bytes.bytes()).map_err(|_| Error::Invalid("snapshot UTF-8"))?;
        Ok(bytes)
    }
    pub fn retain_plan(&self, plan: &crate::NarrationPlan) -> Result<()> {
        let source = self.load(plan.spec().document.content)?;
        let text =
            std::str::from_utf8(source.bytes()).map_err(|_| Error::Invalid("snapshot UTF-8"))?;
        let bytes = plan.encode()?;
        crate::NarrationPlan::decode(text, &bytes, plan.id())?;
        let path = self.root.join(format!("{}.plan", hex(plan.id())));
        if path.exists() {
            self.load_plan(plan.id(), plan.spec().document.content)?;
            return Ok(());
        }
        let temp = self.root.join(format!("{}.writing", uuid::Uuid::new_v4()));
        let result = (|| {
            sync_new(&temp, &bytes)?;
            publish(&temp, &path, false)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn load_plan(&self, id: Digest, source_hash: Digest) -> Result<crate::NarrationPlan> {
        let source = self.load(source_hash)?;
        let path = self.root.join(format!("{}.plan", hex(id)));
        if fs::metadata(&path)?.len() > 128 * 1024 * 1024 {
            return Err(Error::Invalid("plan size"));
        }
        crate::NarrationPlan::decode(
            std::str::from_utf8(source.bytes()).map_err(|_| Error::Invalid("snapshot UTF-8"))?,
            &fs::read(path)?,
            id,
        )
    }
}
