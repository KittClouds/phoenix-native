//! Explicit casting authority, separate from source text and delivery instructions.
use crate::{
    storage::{atomic_replace, hex, owner},
    ByteRange, Digest, DocumentBinding, Error, NarrationPlan, Result,
};
use phoenix_tts_contract::{digest, SynthesisIdentity};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceReference {
    pub encoded: Digest,
    pub original_audio: Digest,
    pub transcript: Digest,
    pub model: Digest,
    pub codec: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub id: Digest,
    pub revision: u64,
    pub name: String,
    pub description: String,
    pub reference: Option<VoiceReference>,
    pub default_delivery: String,
    pub seed: u32,
}
impl VoiceProfile {
    pub fn validate(&self) -> Result<()> {
        if self.id == [0; 32]
            || self.revision == 0
            || self.name.trim().is_empty()
            || self.name.len() > 256
            || self.description.len() > 4096
            || self.default_delivery.len() > 4096
            || self.description.contains('\0')
            || self.default_delivery.contains('\0')
        {
            return Err(Error::Invalid("voice profile bounds"));
        }
        if let Some(r) = &self.reference {
            if [r.encoded, r.original_audio, r.transcript, r.model, r.codec].contains(&[0; 32]) {
                return Err(Error::Invalid("incomplete reference enrollment"));
            }
        } else if self.description.trim().is_empty() {
            return Err(Error::Invalid("designed voice needs description"));
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        self.validate()?;
        Ok(digest(b"phoenix.voice-profile/v1", self)?)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceChoice {
    pub id: Digest,
    pub revision: u64,
    pub fingerprint: Digest,
}
impl VoiceChoice {
    pub fn of(profile: &VoiceProfile) -> Result<Self> {
        Ok(Self {
            id: profile.id,
            revision: profile.revision,
            fingerprint: profile.fingerprint()?,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CastMember {
    /// Stable user/registry ID; renaming a character does not change this ID.
    pub character: Digest,
    pub name: String,
    pub voice: VoiceChoice,
    pub delivery: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeakerAssignment {
    pub source: ByteRange,
    pub character: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CastProfile {
    pub revision: u64,
    pub document: DocumentBinding,
    pub narrator: VoiceChoice,
    pub members: Vec<CastMember>,
    pub assignments: Vec<SpeakerAssignment>,
}
impl CastProfile {
    pub fn validate(&self, source: &str, plan: &NarrationPlan) -> Result<()> {
        if self.revision == 0
            || self.document != plan.spec().document
            || *blake3::hash(source.as_bytes()).as_bytes() != self.document.content
            || self.members.len() > 256
            || self.assignments.len() > 262144
        {
            return Err(Error::Invalid("cast revision, source or bounds"));
        }
        let valid_choice =
            |v: VoiceChoice| v.id != [0; 32] && v.revision != 0 && v.fingerprint != [0; 32];
        if !valid_choice(self.narrator) {
            return Err(Error::Invalid("narrator identity"));
        }
        let mut ids = hashbrown::HashSet::with_capacity(self.members.len());
        for m in &self.members {
            if m.character == [0; 32]
                || !ids.insert(m.character)
                || m.name.trim().is_empty()
                || m.name.len() > 256
                || m.delivery.len() > 4096
                || m.delivery.contains('\0')
                || !valid_choice(m.voice)
            {
                return Err(Error::Invalid("cast member identity"));
            }
        }
        let mut end = 0;
        for a in &self.assignments {
            if a.source.slice(source)?.is_empty()
                || a.source.start < end
                || !ids.contains(&a.character)
            {
                return Err(Error::Invalid("overlapping or unknown speaker assignment"));
            }
            end = a.source.end;
        }
        Ok(())
    }
    /// Unassigned prose uses narrator. Partial segment assignment is rejected:
    /// the planner must first split it at the speaker boundary, never guess.
    pub fn resolve(&self, range: ByteRange) -> Result<(VoiceChoice, &str)> {
        self.resolve_with_narrator(range, self.narrator)
    }
    /// A narrator override changes only unassigned prose, even if a character
    /// explicitly uses the old narrator's voice.
    pub fn resolve_with_narrator(
        &self,
        range: ByteRange,
        narrator: VoiceChoice,
    ) -> Result<(VoiceChoice, &str)> {
        let at = self
            .assignments
            .partition_point(|a| a.source.end <= range.start);
        let Some(a) = self
            .assignments
            .get(at)
            .filter(|a| a.source.start < range.end)
        else {
            return Ok((narrator, ""));
        };
        if a.source.start > range.start || a.source.end < range.end {
            return Err(Error::Invalid("utterance crosses speaker boundary"));
        }
        let m = self
            .members
            .iter()
            .find(|m| m.character == a.character)
            .ok_or(Error::Invalid("missing assigned character"))?;
        Ok((m.voice, &m.delivery))
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        Ok(digest(b"phoenix.cast/v1", self)?)
    }
}

/// Dense slots avoid duplicating synthesis identity for every utterance.
pub struct UtteranceVoices {
    identities: Box<[SynthesisIdentity]>,
    slots: Box<[u16]>,
    plan: Digest,
    fingerprint: Digest,
}
impl UtteranceVoices {
    pub fn new(
        plan: &NarrationPlan,
        identities: Vec<SynthesisIdentity>,
        slots: Vec<u16>,
    ) -> Result<Self> {
        if identities.is_empty()
            || identities.len() > 257
            || slots.len() != plan.spec().segments.len()
            || slots.iter().any(|s| usize::from(*s) >= identities.len())
        {
            return Err(Error::Invalid("utterance voice table bounds"));
        }
        for i in &identities {
            i.validate()?;
        }
        let fingerprint = digest(
            b"phoenix.utterance-voices/v1",
            &(plan.id(), &identities, &slots),
        )?;
        Ok(Self {
            identities: identities.into_boxed_slice(),
            slots: slots.into_boxed_slice(),
            plan: plan.id(),
            fingerprint,
        })
    }
    pub fn plan_id(&self) -> Digest {
        self.plan
    }
    pub fn fingerprint(&self) -> Digest {
        self.fingerprint
    }
    pub fn identity(&self, segment: u32) -> Result<&SynthesisIdentity> {
        let slot = self
            .slots
            .get(segment as usize)
            .ok_or(Error::Invalid("voice segment index"))?;
        Ok(&self.identities[usize::from(*slot)])
    }
}

pub struct VoiceLibrary {
    root: PathBuf,
    _owner: File,
}
impl VoiceLibrary {
    pub fn save_cast(&self, cast: &CastProfile, source: &str, plan: &NarrationPlan) -> Result<()> {
        cast.validate(source, plan)?;
        self.load(cast.narrator)?;
        for member in &cast.members {
            self.load(member.voice)?;
        }
        let key = digest(b"phoenix.cast-document/v1", &cast.document)?;
        atomic_replace(
            &self.root.join(format!("cast-{}", hex(key))),
            &postcard::to_allocvec(cast)?,
        )
    }
    pub fn load_cast(&self, source: &str, plan: &NarrationPlan) -> Result<Option<CastProfile>> {
        let key = digest(b"phoenix.cast-document/v1", &plan.spec().document)?;
        let path = self.root.join(format!("cast-{}", hex(key)));
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(m) if m.len() > 32 * 1024 * 1024 => return Err(Error::Invalid("cast file bounds")),
            Ok(_) => {}
        }
        let cast: CastProfile = postcard::from_bytes(&fs::read(path)?)?;
        cast.validate(source, plan)?;
        self.load(cast.narrator)?;
        for member in &cast.members {
            self.load(member.voice)?;
        }
        Ok(Some(cast))
    }
    /// Bounded cold-path catalog. Audio and reference assets are never scanned.
    pub fn list(&self) -> Result<Vec<VoiceProfile>> {
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            if profiles.len() == 257 || entry.metadata()?.len() > 32768 {
                return Err(Error::Invalid("voice catalog bounds"));
            }
            let profile: VoiceProfile = postcard::from_bytes(&fs::read(entry.path())?)?;
            if hex(profile.fingerprint()?) != name {
                return Err(Error::Invalid("voice catalog identity mismatch"));
            }
            profiles.push(profile);
        }
        profiles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.revision.cmp(&b.revision)));
        Ok(profiles)
    }
    pub fn select(&self, book: Digest, choice: VoiceChoice) -> Result<()> {
        if book == [0; 32] {
            return Err(Error::Invalid("voice selection book"));
        }
        self.load(choice)?;
        atomic_replace(
            &self.root.join(format!("selected-{}", hex(book))),
            &postcard::to_allocvec(&choice)?,
        )
    }
    pub fn selected(&self, book: Digest) -> Result<Option<VoiceChoice>> {
        let path = self.root.join(format!("selected-{}", hex(book)));
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(m) if m.len() > 128 => return Err(Error::Invalid("voice selection bounds")),
            Ok(_) => {}
        }
        let choice = postcard::from_bytes(&fs::read(path)?)?;
        self.load(choice)?;
        Ok(Some(choice))
    }
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let lock = owner(&root)?;
        Ok(Self { root, _owner: lock })
    }
    pub fn save(&self, profile: &VoiceProfile) -> Result<VoiceChoice> {
        let choice = VoiceChoice::of(profile)?;
        let path = self.root.join(hex(choice.fingerprint));
        if path.exists() {
            self.load(choice)?;
            return Ok(choice);
        }
        atomic_replace(&path, &postcard::to_allocvec(profile)?)?;
        Ok(choice)
    }
    pub fn load(&self, choice: VoiceChoice) -> Result<VoiceProfile> {
        let path = self.root.join(hex(choice.fingerprint));
        if fs::metadata(&path)?.len() > 32768 {
            return Err(Error::Invalid("voice profile file bound"));
        }
        let profile: VoiceProfile = postcard::from_bytes(&fs::read(path)?)?;
        if VoiceChoice::of(&profile)? != choice {
            return Err(Error::Invalid("voice profile identity mismatch"));
        }
        Ok(profile)
    }
}
