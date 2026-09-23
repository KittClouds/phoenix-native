use crate::{
    storage::{hex, owner, publish, sync_new},
    AlignedRange, Alignment, ByteRange, Digest, Error, Result, VerifiedBytes,
};
use hashbrown::HashMap;
use phoenix_tts_contract::{
    AlignmentLevel, Binding, Envelope, Event, StreamValidator, SynthesisIdentity,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

const META_MAX: u64 = 1024 * 1024;
const AUDIO_MAX: u64 = 256 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub key: Digest,
    pub identity: SynthesisIdentity,
    pub spoken: String,
    pub frames: u64,
    pub audio_hash: Digest,
    pub alignment: Alignment,
}
#[derive(Serialize, Deserialize)]
struct Commit {
    manifest: CacheManifest,
    hash: Digest,
}
struct Entry {
    bytes: u64,
    used: u64,
    lease: Weak<()>,
}
pub struct AudioCache {
    root: PathBuf,
    _owner: Arc<File>,
    quota: u64,
    bytes: u64,
    clock: u64,
    entries: HashMap<Digest, Entry>,
}
pub struct CachedAudio {
    manifest: CacheManifest,
    pcm: VerifiedBytes,
    _lease: Arc<()>,
    _owner: Arc<File>,
}
impl CachedAudio {
    pub fn manifest(&self) -> &CacheManifest {
        &self.manifest
    }
    pub fn pcm(&self) -> &[u8] {
        self.pcm.bytes()
    }
}
impl AudioCache {
    pub fn open(root: impl AsRef<Path>, quota: u64) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let lock = owner(&root)?;
        let mut cache = Self {
            root,
            _owner: Arc::new(lock),
            quota,
            bytes: 0,
            clock: 0,
            entries: HashMap::new(),
        };
        // One writer owns the root. Incomplete UUID directories are never hits.
        for entry in fs::read_dir(&cache.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".writing")
                && uuid::Uuid::parse_str(name.trim_end_matches(".writing")).is_ok()
            {
                fs::remove_dir_all(entry.path())?;
                continue;
            }
            let Some(key) = parse_key(&name) else {
                continue;
            };
            let bytes = fs::metadata(entry.path().join("audio.pcm"))?
                .len()
                .checked_add(fs::metadata(entry.path().join("commit"))?.len())
                .ok_or(Error::Invalid("cache size overflow"))?;
            cache.bytes = cache
                .bytes
                .checked_add(bytes)
                .ok_or(Error::Invalid("cache size overflow"))?;
            cache.entries.insert(
                key,
                Entry {
                    bytes,
                    used: 0,
                    lease: Weak::new(),
                },
            );
        }
        Ok(cache)
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn contains(&self, key: Digest) -> bool {
        self.entries.contains_key(&key)
    }
    pub fn get(&mut self, key: Digest) -> Result<CachedAudio> {
        let path = self.root.join(hex(key));
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or(Error::Invalid("cache miss"))?;
        if fs::metadata(path.join("commit"))?.len() > META_MAX {
            return Err(Error::Invalid("cache manifest size"));
        }
        let commit: Commit = postcard::from_bytes(&fs::read(path.join("commit"))?)?;
        let m = &commit.manifest;
        if commit.hash != phoenix_tts_contract::digest(b"phoenix.audio-commit/v1", m)?
            || m.version != 1
            || m.key != key
            || m.identity.audio_key(&m.spoken)? != key
            || m.frames == 0
            || m.frames > AUDIO_MAX / 2
        {
            return Err(Error::Invalid("cache commit identity"));
        }
        let pcm = VerifiedBytes::open(&path.join("audio.pcm"), AUDIO_MAX, m.audio_hash)?;
        if pcm.bytes().len() as u64 != m.frames * 2 {
            return Err(Error::Invalid("cache sample count"));
        }
        m.alignment.validate(&m.spoken, m.audio_hash, m.frames)?;
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or(Error::Invalid("cache clock overflow"))?;
        entry.used = self.clock;
        let lease = entry.lease.upgrade().unwrap_or_else(|| Arc::new(()));
        entry.lease = Arc::downgrade(&lease);
        Ok(CachedAudio {
            manifest: commit.manifest,
            pcm,
            _lease: lease,
            _owner: Arc::clone(&self._owner),
        })
    }
    fn reserve(&mut self, bytes: u64) -> Result<()> {
        if bytes > self.quota {
            return Err(Error::Invalid("cache request exceeds quota"));
        }
        while self.bytes > self.quota - bytes {
            let key = self
                .entries
                .iter()
                .filter(|(_, e)| e.lease.strong_count() == 0)
                .min_by_key(|(k, e)| (e.used, **k))
                .map(|(k, _)| *k)
                .ok_or(Error::Invalid("cache quota pinned by playback"))?;
            let entry = self
                .entries
                .get(&key)
                .ok_or(Error::Invalid("cache index"))?;
            fs::remove_dir_all(self.root.join(hex(key)))?;
            self.bytes -= entry.bytes;
            self.entries.remove(&key);
        }
        Ok(())
    }
    pub fn begin(
        &mut self,
        binding: Binding,
        identity: SynthesisIdentity,
        spoken: &str,
        max_frames: u64,
    ) -> Result<CacheWriter<'_>> {
        if identity.audio_key(spoken)? != binding.audio_key
            || spoken.len() > 128 * 1024
            || max_frames == 0
            || max_frames > AUDIO_MAX / 2
            || self.contains(binding.audio_key)
        {
            return Err(Error::Invalid(
                "cache request binding, bounds, or existing entry",
            ));
        }
        let stream = StreamValidator::new(binding, identity.provider, max_frames)?;
        self.reserve(max_frames * 2 + META_MAX)?;
        let temporary = self.root.join(format!("{}.writing", uuid::Uuid::new_v4()));
        fs::create_dir(&temporary)?;
        let file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary.join("audio.pcm"))
        {
            Ok(f) => f,
            Err(e) => {
                let _ = fs::remove_dir(&temporary);
                return Err(e.into());
            }
        };
        Ok(CacheWriter {
            cache: self,
            temporary,
            file: Some(file),
            binding,
            identity,
            spoken: spoken.to_owned(),
            stream,
            hasher: blake3::Hasher::new(),
            frames: 0,
            failed: false,
            committed: false,
            alignment_authority: None,
            alignment_ranges: Vec::new(),
        })
    }
}

pub struct CacheWriter<'a> {
    cache: &'a mut AudioCache,
    temporary: PathBuf,
    file: Option<File>,
    binding: Binding,
    identity: SynthesisIdentity,
    spoken: String,
    stream: StreamValidator,
    hasher: blake3::Hasher,
    frames: u64,
    failed: bool,
    committed: bool,
    alignment_authority: Option<(AlignmentLevel, Digest)>,
    alignment_ranges: Vec<AlignedRange>,
}
impl CacheWriter<'_> {
    pub fn push(&mut self, event: Envelope, pcm_s16le: &[u8]) -> Result<()> {
        let result = self.push_inner(event, pcm_s16le);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn push_inner(&mut self, event: Envelope, pcm: &[u8]) -> Result<()> {
        if self.failed {
            return Err(Error::Invalid("aborted cache writer"));
        }
        match event.event {
            Event::Started { .. } if pcm.is_empty() => {}
            Event::AudioChunk { frames, .. } if pcm.len() == frames as usize * 2 => {}
            Event::AlignmentChunk(hint) if pcm.is_empty() => {
                ByteRange {
                    start: hint.spoken_start,
                    end: hint.spoken_end,
                }
                .slice(&self.spoken)?;
                if self.alignment_ranges.len() >= 4096 {
                    return Err(Error::Invalid("alignment event bound"));
                }
            }
            _ => return Err(Error::Invalid("cache event/payload mismatch")),
        }
        self.stream.accept(event)?;
        if let Event::AlignmentChunk(hint) = event.event {
            self.alignment_authority = Some((hint.level, hint.provenance));
            self.alignment_ranges.push(AlignedRange {
                spoken: ByteRange {
                    start: hint.spoken_start,
                    end: hint.spoken_end,
                },
                first_frame: hint.first_frame,
                end_frame: hint.end_frame,
            });
            return Ok(());
        }
        self.file
            .as_mut()
            .ok_or(Error::Invalid("writer closed"))?
            .write_all(pcm)?;
        self.hasher.update(pcm);
        self.frames += pcm.len() as u64 / 2;
        Ok(())
    }
    pub fn finish(mut self, event: Envelope, alignment: Option<Alignment>) -> Result<Digest> {
        if self.failed {
            return Err(Error::Invalid("aborted cache writer"));
        }
        let receipt = self
            .stream
            .accept(event)?
            .ok_or(Error::Invalid("normal completion required"))?;
        if receipt.binding() != self.binding
            || receipt.frames() != self.frames
            || receipt.format() != self.identity.format
        {
            return Err(Error::Invalid("completion receipt mismatch"));
        }
        let audio_hash = *self.hasher.finalize().as_bytes();
        if alignment.is_some() && self.alignment_authority.is_some() {
            return Err(Error::Invalid("conflicting alignment authorities"));
        }
        let streamed = self
            .alignment_authority
            .take()
            .map(|(level, provenance)| Alignment {
                level,
                provenance,
                audio_hash,
                spoken_hash: *blake3::hash(self.spoken.as_bytes()).as_bytes(),
                ranges: std::mem::take(&mut self.alignment_ranges).into_boxed_slice(),
            });
        let alignment = alignment.or(streamed).unwrap_or_else(|| Alignment {
            level: AlignmentLevel::Segment,
            provenance: *blake3::hash(b"phoenix.segment-alignment/v1").as_bytes(),
            audio_hash,
            spoken_hash: *blake3::hash(self.spoken.as_bytes()).as_bytes(),
            ranges: vec![AlignedRange {
                spoken: ByteRange {
                    start: 0,
                    end: self.spoken.len() as u32,
                },
                first_frame: 0,
                end_frame: self.frames,
            }]
            .into_boxed_slice(),
        });
        alignment.validate(&self.spoken, audio_hash, self.frames)?;
        let manifest = CacheManifest {
            version: 1,
            key: self.binding.audio_key,
            identity: self.identity.clone(),
            spoken: self.spoken.clone(),
            frames: self.frames,
            audio_hash,
            alignment,
        };
        let hash = phoenix_tts_contract::digest(b"phoenix.audio-commit/v1", &manifest)?;
        let bytes = postcard::to_allocvec(&Commit { manifest, hash })?;
        if bytes.len() as u64 > META_MAX {
            return Err(Error::Invalid("manifest exceeds reservation"));
        }
        self.file
            .take()
            .ok_or(Error::Invalid("writer closed"))?
            .sync_all()?;
        // Commit record is written last, then the entire directory is published.
        sync_new(&self.temporary.join("commit"), &bytes)?;
        publish(
            &self.temporary,
            &self.cache.root.join(hex(self.binding.audio_key)),
            false,
        )?;
        self.committed = true;
        let size = self.frames * 2 + bytes.len() as u64;
        self.cache.bytes += size;
        self.cache.entries.insert(
            self.binding.audio_key,
            Entry {
                bytes: size,
                used: self.cache.clock,
                lease: Weak::new(),
            },
        );
        Ok(audio_hash)
    }
}
impl Drop for CacheWriter<'_> {
    fn drop(&mut self) {
        self.file.take();
        if !self.committed {
            let _ = fs::remove_dir_all(&self.temporary);
        }
    }
}
fn parse_key(name: &str) -> Option<Digest> {
    if name.len() != 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&name[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
