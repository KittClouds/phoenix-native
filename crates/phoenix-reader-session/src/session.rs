use crate::{
    storage::{atomic_replace, hex, owner},
    Digest, DocumentBinding, Error, NarrationPlan, Result,
};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub segment: u32,
    pub source_frame: u64,
    /// Zero only for the initial, not-yet-generated position at frame zero.
    pub audio_key: Digest,
    pub artifact_hash: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReaderSession {
    id: Digest,
    document: DocumentBinding,
    plan: Digest,
    voice: Digest,
    pronunciation: Digest,
    position: Position,
    speed_milli: u16,
    sequence: u64,
    bookmarks: HashMap<u64, Position>,
}
impl ReaderSession {
    pub fn new(id: Digest, plan: &NarrationPlan, voice: Digest) -> Result<Self> {
        if id == [0; 32] || voice == [0; 32] {
            return Err(Error::Invalid("session identity"));
        }
        Ok(Self {
            id,
            document: plan.spec().document,
            plan: plan.id(),
            voice,
            pronunciation: plan.spec().pronunciation,
            position: Position {
                segment: 0,
                source_frame: 0,
                audio_key: [0; 32],
                artifact_hash: [0; 32],
            },
            speed_milli: 1000,
            sequence: 1,
            bookmarks: HashMap::new(),
        })
    }
    pub fn id(&self) -> Digest {
        self.id
    }
    pub fn document(&self) -> DocumentBinding {
        self.document
    }
    pub fn plan_id(&self) -> Digest {
        self.plan
    }
    pub fn voice_binding(&self) -> Digest {
        self.voice
    }
    pub fn position(&self) -> Position {
        self.position
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn speed_milli(&self) -> u16 {
        self.speed_milli
    }
    pub fn bookmarks(&self) -> &HashMap<u64, Position> {
        &self.bookmarks
    }
    /// Exact resume is permitted only against the original realized artifact.
    /// A cache miss or different regeneration requires an explicit segment restart.
    pub fn resume_position(
        &self,
        plan: &NarrationPlan,
        audio: &crate::CachedAudio,
    ) -> Result<Position> {
        self.resume_with_voices(plan, audio, None)
    }
    pub(crate) fn resume_with_voices(
        &self,
        plan: &NarrationPlan,
        audio: &crate::CachedAudio,
        voices: Option<&crate::UtteranceVoices>,
    ) -> Result<Position> {
        self.validate_audio_voice(plan, self.position.segment, audio, voices)?;
        self.validate(plan)?;
        let m = audio.manifest();
        let segment = plan.segment(self.position.segment)?;
        if m.key != self.position.audio_key
            || m.audio_hash != self.position.artifact_hash
            || self.position.source_frame > m.frames
            || segment.spoken.slice(&plan.spec().spoken)? != m.spoken
        {
            return Err(Error::Invalid("resume artifact differs from checkpoint"));
        }
        Ok(self.position)
    }
    fn bump(&mut self) -> Result<()> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::Invalid("checkpoint sequence overflow"))?;
        Ok(())
    }
    pub fn set_speed(&mut self, milli: u16) -> Result<()> {
        if !(500..=3000).contains(&milli) {
            return Err(Error::Invalid("speed outside contract range"));
        }
        self.bump()?;
        self.speed_milli = milli;
        Ok(())
    }
    /// Only verified committed audio can establish an exact durable position.
    /// Live, incomplete playback may keep an ephemeral clock until completion.
    pub fn set_position(
        &mut self,
        plan: &NarrationPlan,
        segment: u32,
        source_frame: u64,
        audio: &crate::CachedAudio,
    ) -> Result<()> {
        self.set_position_with_voices(plan, segment, source_frame, audio, None)
    }
    pub(crate) fn set_position_with_voices(
        &mut self,
        plan: &NarrationPlan,
        segment: u32,
        source_frame: u64,
        audio: &crate::CachedAudio,
        voices: Option<&crate::UtteranceVoices>,
    ) -> Result<()> {
        self.validate_audio_voice(plan, segment, audio, voices)?;
        self.validate(plan)?;
        let planned = plan.segment(segment)?;
        let manifest = audio.manifest();
        if source_frame > manifest.frames
            || planned.spoken.slice(&plan.spec().spoken)? != manifest.spoken
        {
            return Err(Error::Invalid("position audio/voice/segment mismatch"));
        }
        let position = Position {
            segment,
            source_frame,
            audio_key: manifest.key,
            artifact_hash: manifest.audio_hash,
        };
        self.bump()?;
        self.position = position;
        Ok(())
    }
    fn validate_audio_voice(
        &self,
        plan: &NarrationPlan,
        segment: u32,
        audio: &crate::CachedAudio,
        voices: Option<&crate::UtteranceVoices>,
    ) -> Result<()> {
        let valid = match voices {
            Some(voices) => {
                voices.plan_id() == plan.id()
                    && voices.fingerprint() == self.voice
                    && voices.identity(segment)? == &audio.manifest().identity
            }
            None => audio.manifest().identity.voice == self.voice,
        };
        if !valid {
            return Err(Error::Invalid("session audio voice binding"));
        }
        Ok(())
    }
    pub fn bookmark(&mut self, id: u64) -> Result<()> {
        if id == 0 || (!self.bookmarks.contains_key(&id) && self.bookmarks.len() >= 4096) {
            return Err(Error::Invalid("bookmark bound"));
        }
        self.bump()?;
        self.bookmarks.insert(id, self.position);
        Ok(())
    }
    pub fn validate(&self, plan: &NarrationPlan) -> Result<()> {
        if self.id == [0; 32]
            || self.voice == [0; 32]
            || self.sequence == 0
            || self.document != plan.spec().document
            || self.plan != plan.id()
            || self.pronunciation != plan.spec().pronunciation
            || !(500..=3000).contains(&self.speed_milli)
            || self.bookmarks.len() > 4096
        {
            return Err(Error::Invalid("session plan binding"));
        }
        plan.segment(self.position.segment)?;
        validate_position(self.position)?;
        for (&id, p) in &self.bookmarks {
            if id == 0 {
                return Err(Error::Invalid("bookmark identity"));
            }
            plan.segment(p.segment)?;
            validate_position(*p)?;
        }
        Ok(())
    }
}
fn validate_position(p: Position) -> Result<()> {
    if (p.audio_key == [0; 32]) != (p.artifact_hash == [0; 32])
        || (p.audio_key == [0; 32] && (p.source_frame != 0 || p.segment != 0))
    {
        return Err(Error::Invalid("position artifact binding"));
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u32,
    hash: Digest,
    payload: Vec<u8>,
}
pub struct SessionStore {
    root: PathBuf,
    _owner: File,
    last_write_ms: HashMap<Digest, u64>,
}
impl SessionStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let lock = owner(&root)?;
        Ok(Self {
            root,
            _owner: lock,
            last_write_ms: HashMap::new(),
        })
    }
    pub fn load(&self, id: Digest, plan: &NarrationPlan) -> Result<ReaderSession> {
        let path = self.root.join(hex(id));
        if fs::metadata(&path)?.len() > 1024 * 1024 {
            return Err(Error::Invalid("checkpoint size"));
        }
        let checkpoint: Checkpoint = postcard::from_bytes(&fs::read(path)?)?;
        if checkpoint.version != 1
            || *blake3::hash(&checkpoint.payload).as_bytes() != checkpoint.hash
        {
            return Err(Error::Invalid("checkpoint checksum/version"));
        }
        let session: ReaderSession = postcard::from_bytes(&checkpoint.payload)?;
        if session.id != id {
            return Err(Error::Invalid("checkpoint identity"));
        }
        session.validate(plan)?;
        Ok(session)
    }
    /// force=true for pause, seek, bookmark and close; otherwise <=1 write/sec.
    pub fn checkpoint(
        &mut self,
        session: &ReaderSession,
        plan: &NarrationPlan,
        now_ms: u64,
        force: bool,
    ) -> Result<bool> {
        session.validate(plan)?;
        let path = self.root.join(hex(session.id));
        if path.exists() && self.load(session.id, plan)?.sequence >= session.sequence {
            return Err(Error::Invalid("stale checkpoint"));
        }
        if !force
            && self
                .last_write_ms
                .get(&session.id)
                .is_some_and(|last| now_ms.saturating_sub(*last) < 1000)
        {
            return Ok(false);
        }
        let payload = postcard::to_allocvec(session)?;
        let checkpoint = Checkpoint {
            version: 1,
            hash: *blake3::hash(&payload).as_bytes(),
            payload,
        };
        atomic_replace(&path, &postcard::to_allocvec(&checkpoint)?)?;
        self.last_write_ms.insert(session.id, now_ms);
        Ok(true)
    }
}
