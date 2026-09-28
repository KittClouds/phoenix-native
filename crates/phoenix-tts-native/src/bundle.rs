use crate::{Cancellation, Error, Result};
use memmap2::MmapOptions;
use phoenix_tts_contract::{digest, AudioFormat, Digest, SynthesisIdentity};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    sync::Mutex,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// Which GPU voice engine a bundle runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    /// Breeze TTS 2 (PBN1 worker, BRZV voices, design and clone).
    Breeze,
    /// Qwen3-TTS Base via qwentts.cpp (PBN1 worker, QWNV clones only).
    Qwen,
}

/// Enrollment hashes the actual model, executable and DLLs. Read-only Windows
/// handles prevent replacement while this bundle can launch workers. Hashes are
/// cache identity, not a claim of third-party model or source qualification.
pub struct Bundle {
    pub(crate) engine: Engine,
    pub(crate) executable: PathBuf,
    pub(crate) model_path: PathBuf,
    /// Qwen's separate codec GGUF; Breeze keeps its codec in the model.
    pub(crate) codec_path: Option<PathBuf>,
    pub(crate) dll_directory: PathBuf,
    runtime: Digest,
    model: Digest,
    codec: Digest,
    _files: Vec<File>,
}

/// Qwen worker memory settings, part of its runtime identity: a 1,280
/// position talker context (Reader requests stay under ~1,050) and 4 s codec
/// decode chunks. Together they cut resident GPU memory by ~1.4 GB.
pub(crate) const QWEN_MAX_CTX: &str = "1280";

impl Bundle {
    pub fn open(executable: &Path, model: &Path, dll_directory: &Path) -> Result<Self> {
        Self::open_cancellable(executable, model, dll_directory, &Cancellation::default())
    }
    pub fn open_cancellable(
        executable: &Path,
        model: &Path,
        dll_directory: &Path,
        cancel: &Cancellation,
    ) -> Result<Self> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let executable = executable.canonicalize()?;
        let model_path = model.canonicalize()?;
        let dll_directory = dll_directory.canonicalize()?;
        let mut files = Vec::new();
        let model = pin(&model_path, 16 * 1024 * 1024 * 1024, &mut files, cancel)?;
        let (exe_hash, hashes) = pin_runtime(&executable, &dll_directory, &mut files, cancel)?;
        let runtime = digest(
            b"phoenix.native-breeze/v1",
            &(
                exe_hash,
                hashes,
                "PBN1;host-visible=off;coopmat2=off;cfg=1;stateless",
            ),
        )?;
        Ok(Self {
            engine: Engine::Breeze,
            executable,
            model_path,
            codec_path: None,
            dll_directory,
            runtime,
            model,
            codec: model,
            _files: files,
        })
    }
    /// Pins the Qwen worker, its DLLs (next to the executable), the talker
    /// GGUF and the codec GGUF.
    pub fn open_qwen(
        executable: &Path,
        talker: &Path,
        codec: &Path,
        cancel: &Cancellation,
    ) -> Result<Self> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let executable = executable.canonicalize()?;
        let model_path = talker.canonicalize()?;
        let codec_path = codec.canonicalize()?;
        let dll_directory = executable
            .parent()
            .ok_or(Error::Invalid("executable parent"))?
            .to_path_buf();
        let mut files = Vec::new();
        let model = pin(&model_path, 8 * 1024 * 1024 * 1024, &mut files, cancel)?;
        let codec = pin(&codec_path, 2 * 1024 * 1024 * 1024, &mut files, cancel)?;
        let (exe_hash, hashes) = pin_runtime(&executable, &dll_directory, &mut files, cancel)?;
        let runtime = digest(
            b"phoenix.native-qwen/v1",
            &(
                exe_hash,
                hashes,
                format!("PBN1;qwen3-tts-base;ctx={QWEN_MAX_CTX};codec-chunk=4;fa=on"),
            ),
        )?;
        Ok(Self {
            engine: Engine::Qwen,
            executable,
            model_path,
            codec_path: Some(codec_path),
            dll_directory,
            runtime,
            model,
            codec,
            _files: files,
        })
    }
    pub fn engine(&self) -> Engine {
        self.engine
    }
    /// True when both bundles pin the same worker, model and runtime files.
    pub fn same_files(&self, other: &Bundle) -> bool {
        self.engine == other.engine
            && self.executable == other.executable
            && self.model_path == other.model_path
            && self.codec_path == other.codec_path
            && self.dll_directory == other.dll_directory
            && self.runtime == other.runtime
            && self.model == other.model
            && self.codec == other.codec
    }
    pub fn identity(
        &self,
        instruction: &str,
        seed: u32,
        max_frames: u64,
    ) -> Result<SynthesisIdentity> {
        let (provider, direction_domain, generation_domain, generation): (
            &[u8],
            &[u8],
            &[u8],
            &str,
        ) = match self.engine {
            Engine::Breeze => (
                b"phoenix.native-breeze/PBN1",
                b"phoenix.breeze-direction/v1",
                b"phoenix.breeze-generation/v1",
                "cfg1;split0;chunk4/25;context2048",
            ),
            Engine::Qwen => (
                b"phoenix.native-qwen/PBN1",
                b"phoenix.qwen-direction/v1",
                b"phoenix.qwen-generation/v1",
                "icl;lang=english;sampling-defaults",
            ),
        };
        let direction = digest(direction_domain, &instruction)?;
        let identity = SynthesisIdentity {
            provider: *blake3::hash(provider).as_bytes(),
            runtime: self.runtime,
            model: self.model,
            tokenizer: self.model,
            codec: self.codec,
            voice: direction,
            reference_audio: None,
            reference_transcript: None,
            direction,
            generation_config: digest(generation_domain, &(seed, max_frames, generation))?,
            transformations: *blake3::hash(b"source-exact/no-pronunciation/v1").as_bytes(),
            postprocessing: *blake3::hash(b"pcm16-round-clamp/24k/v1").as_bytes(),
            seed: seed.into(),
            format: AudioFormat::PCM24,
        };
        identity.validate()?;
        Ok(identity)
    }
}

/// Pins the worker executable and the DLLs beside it (and in `dll_directory`).
fn pin_runtime(
    executable: &Path,
    dll_directory: &Path,
    files: &mut Vec<File>,
    cancel: &Cancellation,
) -> Result<(Digest, Vec<((bool, String), Digest)>)> {
    let exe_hash = pin(executable, 256 * 1024 * 1024, files, cancel)?;
    let mut dlls = std::fs::read_dir(dll_directory)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let parent = executable
        .parent()
        .ok_or(Error::Invalid("executable parent"))?;
    if parent != dll_directory {
        dlls.extend(
            std::fs::read_dir(parent)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?,
        );
    }
    dlls.retain(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")));
    dlls.sort();
    if dlls.len() > 32 {
        return Err(Error::Invalid("DLL bundle bound"));
    }
    let mut hashes = Vec::with_capacity(dlls.len());
    for path in dlls {
        hashes.push((
            (
                path.parent() == Some(parent),
                path.file_name().unwrap().to_string_lossy().into_owned(),
            ),
            pin(&path, 512 * 1024 * 1024, files, cancel)?,
        ));
    }
    Ok((exe_hash, hashes))
}
/// Remembers digests of pinned files across launches so a multi-gigabyte model
/// is hashed once, not on every Reader start. An entry matches only when the
/// canonical path, length, creation time and last-write time are unchanged;
/// any change re-hashes. Files stay pinned read-only while leased either way.
static DIGEST_MEMO: Mutex<Option<DigestMemo>> = Mutex::new(None);

type MemoKey = (String, u64, u64, u64);

struct DigestMemo {
    path: PathBuf,
    entries: HashMap<MemoKey, Digest>,
}

/// Enables the persistent digest memo at `path` (a small text file).
pub fn use_digest_memo(path: PathBuf) {
    let mut memo = DIGEST_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    if memo.as_ref().is_some_and(|memo| memo.path == path) {
        return;
    }
    let mut entries = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text.lines() {
            let mut fields = line.splitn(5, '\t');
            let (Some(hex), Some(len), Some(created), Some(modified), Some(file)) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            ) else {
                continue;
            };
            let (Some(digest), Ok(len), Ok(created), Ok(modified)) = (
                parse_hex(hex),
                len.parse(),
                created.parse(),
                modified.parse(),
            ) else {
                continue;
            };
            entries.insert((file.to_owned(), len, created, modified), digest);
        }
    }
    *memo = Some(DigestMemo { path, entries });
}

fn parse_hex(hex: &str) -> Option<Digest> {
    if hex.len() != 64 {
        return None;
    }
    let mut digest = [0u8; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(digest)
}

fn memo_key(path: &Path, metadata: &std::fs::Metadata) -> MemoKey {
    use std::os::windows::fs::MetadataExt;
    (
        path.to_string_lossy().into_owned(),
        metadata.len(),
        metadata.creation_time(),
        metadata.last_write_time(),
    )
}

fn memo_lookup(key: &MemoKey) -> Option<Digest> {
    let memo = DIGEST_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    memo.as_ref()?.entries.get(key).copied()
}

fn memo_record(key: MemoKey, digest: Digest) {
    let mut memo = DIGEST_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    let Some(memo) = memo.as_mut() else {
        return;
    };
    // Drop stale entries for the same file so the memo does not grow.
    memo.entries.retain(|entry, _| entry.0 != key.0);
    memo.entries.insert(key, digest);
    let mut text = String::new();
    for ((file, len, created, modified), digest) in &memo.entries {
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        text.push_str(&format!("{hex}\t{len}\t{created}\t{modified}\t{file}\n"));
    }
    let temp = memo.path.with_extension("tmp");
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, &memo.path);
    }
}

pub(crate) fn pin(
    path: &Path,
    max: u64,
    files: &mut Vec<File>,
    cancel: &Cancellation,
) -> Result<Digest> {
    let file = OpenOptions::new().read(true).share_mode(1).open(path)?;
    let metadata = file.metadata()?;
    let length = metadata.len();
    if length == 0 || length > max {
        return Err(Error::Invalid("pinned file size"));
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let key = memo_key(path, &metadata);
    if let Some(digest) = memo_lookup(&key) {
        files.push(file);
        return Ok(digest);
    }
    // SAFETY: the retained file denies writes and deletion for the map lifetime.
    let map = unsafe { MmapOptions::new().map(&file)? };
    let mut hash = blake3::Hasher::new();
    for chunk in map.chunks(1024 * 1024) {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        hash.update(chunk);
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let hash = *hash.finalize().as_bytes();
    drop(map);
    memo_record(key, hash);
    files.push(file);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_bundle_never_reads_missing_paths() {
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(matches!(
            Bundle::open_cancellable(
                Path::new("missing"),
                Path::new("missing"),
                Path::new("missing"),
                &cancel
            ),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn chunked_pin_keeps_original_digest_and_honors_cancel() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model");
        let bytes = vec![73; 2 * 1024 * 1024 + 3];
        std::fs::write(&path, &bytes).unwrap();
        let mut files = Vec::new();
        let cancel = Cancellation::default();
        assert_eq!(
            pin(&path, 4 * 1024 * 1024, &mut files, &cancel).unwrap(),
            *blake3::hash(&bytes).as_bytes()
        );
        cancel.cancel();
        assert!(matches!(
            pin(&path, 4 * 1024 * 1024, &mut files, &cancel),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn digest_memo_round_trips_and_rehashes_changed_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model");
        std::fs::write(&path, b"first").unwrap();
        use_digest_memo(root.path().join("pins.memo"));
        let cancel = Cancellation::default();
        let first = pin(&path, 64, &mut Vec::new(), &cancel).unwrap();
        assert_eq!(first, *blake3::hash(b"first").as_bytes());
        // A fresh load from disk still returns the recorded digest.
        *DIGEST_MEMO.lock().unwrap() = None;
        use_digest_memo(root.path().join("pins.memo"));
        let key = memo_key(&path, &std::fs::metadata(&path).unwrap());
        assert_eq!(memo_lookup(&key), Some(first));
        // A changed length misses the memo and re-hashes.
        std::fs::write(&path, b"second!").unwrap();
        assert_eq!(
            pin(&path, 64, &mut Vec::new(), &cancel).unwrap(),
            *blake3::hash(b"second!").as_bytes()
        );
        *DIGEST_MEMO.lock().unwrap() = None;
    }
}
