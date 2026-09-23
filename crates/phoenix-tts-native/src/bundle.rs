use crate::{Cancellation, Error, Result};
use memmap2::MmapOptions;
use phoenix_tts_contract::{digest, AudioFormat, Digest, SynthesisIdentity};
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// Enrollment hashes the actual model, executable and DLLs. Read-only Windows
/// handles prevent replacement while this bundle can launch workers. Hashes are
/// cache identity, not a claim of third-party model or source qualification.
pub struct Bundle {
    pub(crate) executable: PathBuf,
    pub(crate) model_path: PathBuf,
    pub(crate) dll_directory: PathBuf,
    runtime: Digest,
    model: Digest,
    _files: Vec<File>,
}
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
        let exe_hash = pin(&executable, 256 * 1024 * 1024, &mut files, cancel)?;
        let mut dlls = std::fs::read_dir(&dll_directory)?
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
                pin(&path, 512 * 1024 * 1024, &mut files, cancel)?,
            ));
        }
        let runtime = digest(
            b"phoenix.native-breeze/v1",
            &(
                exe_hash,
                hashes,
                "PBN1;host-visible=off;coopmat2=off;cfg=1;stateless",
            ),
        )?;
        Ok(Self {
            executable,
            model_path,
            dll_directory,
            runtime,
            model,
            _files: files,
        })
    }
    pub fn identity(
        &self,
        instruction: &str,
        seed: u32,
        max_frames: u64,
    ) -> Result<SynthesisIdentity> {
        let direction = digest(b"phoenix.breeze-direction/v1", &instruction)?;
        let identity = SynthesisIdentity {
            provider: *blake3::hash(b"phoenix.native-breeze/PBN1").as_bytes(),
            runtime: self.runtime,
            model: self.model,
            tokenizer: self.model,
            codec: self.model,
            voice: direction,
            reference_audio: None,
            reference_transcript: None,
            direction,
            generation_config: digest(
                b"phoenix.breeze-generation/v1",
                &(seed, max_frames, "cfg1;split0;chunk4/25;context2048"),
            )?,
            transformations: *blake3::hash(b"source-exact/no-pronunciation/v1").as_bytes(),
            postprocessing: *blake3::hash(b"pcm16-round-clamp/24k/v1").as_bytes(),
            seed: seed.into(),
            format: AudioFormat::PCM24,
        };
        identity.validate()?;
        Ok(identity)
    }
}
pub(crate) fn pin(
    path: &Path,
    max: u64,
    files: &mut Vec<File>,
    cancel: &Cancellation,
) -> Result<Digest> {
    let file = OpenOptions::new().read(true).share_mode(1).open(path)?;
    let length = file.metadata()?.len();
    if length == 0 || length > max {
        return Err(Error::Invalid("pinned file size"));
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
}
