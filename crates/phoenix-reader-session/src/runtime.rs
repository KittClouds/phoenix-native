//! Completed-cache playback coordinator. A cache miss is explicit; regeneration
//! and incomplete provider audio cannot silently substitute a resume artifact.
use crate::tempo::PlaybackAudio;
use crate::{CachedAudio, Digest, Error, NarrationPlan, ReaderSession, Result, SessionStore};
use phoenix_audio::device::PlaybackDevice;
use phoenix_tts_contract::{SynthesisIdentity, BLOCK_FRAMES};
use std::collections::VecDeque;

pub const PREFETCH_SEGMENTS: usize = 8;
struct Ahead {
    audio: PlaybackAudio,
    queued: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    NeedsAudio(u32),
    Playing,
    Paused,
    Completed,
    Stopped,
    Failed,
}
pub struct ReaderRuntime<D: PlaybackDevice> {
    device: D,
    plan: NarrationPlan,
    session: ReaderSession,
    identity: SynthesisIdentity,
    voices: Option<crate::UtteranceVoices>,
    audio: Option<PlaybackAudio>,
    state: PlaybackState,
    epoch: u64,
    segment: u32,
    start: u64,
    queued: u64,
    presented: u64,
    checkpointed: u64,
    scratch: [i16; BLOCK_FRAMES],
    ahead: VecDeque<Ahead>,
    device_base: u64,
    starvations: u32,
}
impl<D: PlaybackDevice> Drop for ReaderRuntime<D> {
    fn drop(&mut self) {
        let _ = self.device.reset();
    }
}
impl<D: PlaybackDevice> ReaderRuntime<D> {
    /// Restore through the authoritative store and remember the persisted sequence.
    /// An unchanged restored cursor must not be republished as a new checkpoint.
    pub fn restore(
        device: D,
        plan: NarrationPlan,
        session_id: Digest,
        identity: SynthesisIdentity,
        store: &SessionStore,
    ) -> Result<Self> {
        let session = store.load(session_id, &plan)?;
        let sequence = session.sequence();
        let mut runtime = Self::new(device, plan, session, identity)?;
        runtime.checkpointed = sequence;
        Ok(runtime)
    }
    pub fn new(
        mut device: D,
        plan: NarrationPlan,
        session: ReaderSession,
        identity: SynthesisIdentity,
    ) -> Result<Self> {
        session.validate(&plan)?;
        device.reset()?;
        let segment = session.position().segment;
        Ok(Self {
            device,
            plan,
            session,
            identity,
            voices: None,
            audio: None,
            state: PlaybackState::NeedsAudio(segment),
            epoch: 1,
            segment,
            start: 0,
            queued: 0,
            presented: 0,
            checkpointed: 0,
            scratch: [0; BLOCK_FRAMES],
            ahead: VecDeque::with_capacity(PREFETCH_SEGMENTS),
            device_base: 0,
            starvations: 0,
        })
    }
    pub fn state(&self) -> PlaybackState {
        self.state
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn session(&self) -> &ReaderSession {
        &self.session
    }
    pub fn speed_milli(&self) -> u16 {
        self.session.speed_milli()
    }
    pub fn plan(&self) -> &NarrationPlan {
        &self.plan
    }
    pub fn next_prefetch_segment(&self) -> u32 {
        self.segment + 1 + self.ahead.len() as u32
    }
    pub fn ahead_count(&self) -> usize {
        self.ahead.len()
    }
    pub fn device_starvations(&self) -> u32 {
        self.starvations
    }
    pub fn buffered_frames(&self) -> u64 {
        self.audio.as_ref().map_or(0, |a| {
            a.cached().manifest().frames - a.source_at(self.presented)
        }) + self
            .ahead
            .iter()
            .map(|a| a.audio.cached().manifest().frames)
            .sum::<u64>()
    }
    /// Only sequential completed artifacts in the current playback epoch may queue.
    pub fn enqueue(&mut self, epoch: u64, segment: u32, audio: CachedAudio) -> Result<()> {
        if epoch != self.epoch
            || !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
            || self.ahead.len() == PREFETCH_SEGMENTS
            || segment != self.next_prefetch_segment()
            || audio.manifest().key != self.required_key(segment)?
        {
            return Err(Error::Invalid("stale, nonsequential or excessive prefetch"));
        }
        self.ahead.push_back(Ahead {
            audio: PlaybackAudio::new(audio, self.speed_milli())?,
            queued: 0,
        });
        Ok(())
    }
    pub fn required_key(&self, segment: u32) -> Result<Digest> {
        let identity = match &self.voices {
            Some(voices) => voices.identity(segment)?,
            None => &self.identity,
        };
        Ok(identity.audio_key(
            self.plan
                .segment(segment)?
                .spoken
                .slice(&self.plan.spec().spoken)?,
        )?)
    }
    /// Set once before attaching audio. Session IDs must include this table's
    /// fingerprint; changing casting is a new playback session.
    pub fn bind_voices(&mut self, voices: crate::UtteranceVoices) -> Result<()> {
        if self.audio.is_some()
            || self.voices.is_some()
            || !matches!(self.state, PlaybackState::NeedsAudio(_))
            || voices.plan_id() != self.plan.id()
            || self.session.voice_binding() != voices.fingerprint()
        {
            return Err(Error::Invalid("voice table binding state"));
        }
        self.voices = Some(voices);
        Ok(())
    }
    /// The epoch belongs to the asynchronous lookup/request that obtained audio.
    /// Late completions after a seek/cancel cannot attach to the active stream.
    pub fn attach(&mut self, epoch: u64, segment: u32, audio: CachedAudio) -> Result<()> {
        if epoch != self.epoch || self.state != PlaybackState::NeedsAudio(segment) {
            return Err(Error::Invalid("stale or unsolicited audio"));
        }
        let frame = if self.session.position().segment == segment {
            if self.session.position().audio_key != [0; 32] {
                self.session
                    .resume_with_voices(&self.plan, &audio, self.voices.as_ref())?
                    .source_frame
            } else {
                0
            }
        } else {
            0
        };
        self.install(segment, frame, audio)
    }
    fn install(&mut self, segment: u32, frame: u64, audio: CachedAudio) -> Result<()> {
        if audio.manifest().key != self.required_key(segment)? {
            return Err(Error::Invalid("runtime synthesis identity mismatch"));
        }
        let mut candidate = self.session.clone();
        candidate.set_position_with_voices(
            &self.plan,
            segment,
            frame,
            &audio,
            self.voices.as_ref(),
        )?;
        self.flush()?;
        self.session = candidate;
        self.segment = segment;
        let playback = PlaybackAudio::new(audio, self.speed_milli())?;
        let output_frame = playback.output_at(frame);
        self.start = output_frame;
        self.queued = output_frame;
        self.presented = output_frame;
        self.audio = Some(playback);
        self.state = PlaybackState::Playing;
        if let Err(e) = self.device.pause(false) {
            self.state = PlaybackState::Failed;
            return Err(e.into());
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        self.ahead.clear();
        self.device_base = 0;
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(Error::Invalid("runtime epoch exhausted"))?;
        if let Err(e) = self.device.reset() {
            self.state = PlaybackState::Failed;
            return Err(e.into());
        }
        Ok(())
    }
    /// Explicit seeking may select a different completed artifact. Exact resume
    /// uses attach instead and refuses regenerated audio with a different hash.
    pub fn seek(&mut self, segment: u32, frame: u64, audio: CachedAudio) -> Result<()> {
        self.install(segment, frame, audio)
    }
    pub fn tick(&mut self) -> Result<PlaybackState> {
        if let Err(e) = self.advance() {
            self.state = PlaybackState::Failed;
            let _ = self.device.reset();
            return Err(e);
        }
        Ok(self.state)
    }
    fn advance(&mut self) -> Result<()> {
        if self.state != PlaybackState::Playing {
            return Ok(());
        }
        self.observe()?;
        let audio = self
            .audio
            .as_ref()
            .ok_or(Error::Invalid("missing active audio"))?;
        let frames = audio.output_frames();
        if self.presented == self.queued && self.queued > self.start && self.queued < frames {
            self.starvations = self.starvations.saturating_add(1);
        }
        if self.presented == frames {
            self.state = if self.segment as usize + 1 == self.plan.spec().segments.len() {
                PlaybackState::Completed
            } else {
                PlaybackState::NeedsAudio(self.segment + 1)
            };
            self.audio = None;
            return Ok(());
        }
        // Bounded work per control tick; mmap is borrowed, only the device queue copies.
        for _ in 0..8 {
            if !self.device.available()? {
                break;
            }
            let (audio, queued) = if self.queued < frames {
                (audio, &mut self.queued)
            } else if let Some(next) = self
                .ahead
                .iter_mut()
                .find(|a| a.queued < a.audio.output_frames())
            {
                (&next.audio, &mut next.queued)
            } else {
                break;
            };
            let count = (audio.output_frames() - *queued).min(BLOCK_FRAMES as u64) as usize;
            audio.copy_samples(*queued, &mut self.scratch[..count]);
            self.device.submit(&self.scratch[..count])?;
            *queued += count as u64;
        }
        Ok(())
    }
    fn observe(&mut self) -> Result<()> {
        let device_frame = self.device.position()?;
        let mut frame = device_frame
            .checked_sub(self.device_base)
            .and_then(|f| f.checked_add(self.start))
            .ok_or(Error::Invalid("device clock overflow"))?;
        // Advance highlighting only when the device actually crosses the boundary.
        while let Some(audio) = &self.audio {
            let frames = audio.output_frames();
            if frame < frames || self.ahead.is_empty() {
                break;
            }
            if self.queued != frames {
                return Err(Error::Invalid("unsubmitted boundary"));
            }
            self.device_base += frames - self.start;
            frame -= frames;
            self.start = 0;
            let next = self.ahead.pop_front().unwrap();
            self.audio = Some(next.audio);
            self.queued = next.queued;
            self.presented = 0;
            self.segment += 1;
            self.session.set_position_with_voices(
                &self.plan,
                self.segment,
                0,
                self.audio.as_ref().unwrap().cached(),
                self.voices.as_ref(),
            )?;
        }
        if frame < self.presented || frame > self.queued {
            return Err(Error::Invalid("device presentation outside queued range"));
        }
        if frame != self.presented {
            self.session.set_position_with_voices(
                &self.plan,
                self.segment,
                self.audio.as_ref().unwrap().source_at(frame),
                self.audio
                    .as_ref()
                    .ok_or(Error::Invalid("missing active audio"))?
                    .cached(),
                self.voices.as_ref(),
            )?;
            self.presented = frame;
        }
        Ok(())
    }
    pub fn pause(&mut self, paused: bool) -> Result<()> {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return Err(Error::Invalid("pause state"));
        }
        if let Err(error) = self
            .device
            .pause(paused)
            .map_err(Error::from)
            .and_then(|()| self.observe())
        {
            self.state = PlaybackState::Failed;
            let _ = self.device.reset();
            return Err(error);
        }
        self.state = if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        Ok(())
    }
    /// Change tempo at an exact source-frame boundary. Discard queued output,
    /// then render the current completed artifact at the new tempo. The worker
    /// refills ahead audio before resuming, so no old-speed tail can leak through.
    pub fn set_speed(&mut self, speed_milli: u16) -> Result<()> {
        if !matches!(speed_milli, 850 | 1000 | 1150 | 1300) {
            return Err(Error::Invalid("unsupported Reader speed"));
        }
        if self.speed_milli() == speed_milli {
            return Ok(());
        }
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            self.device.pause(true)?;
            self.observe()?;
        }
        let frame = self.session.position().source_frame;
        let audio = self.audio.take().map(PlaybackAudio::into_cached);
        self.flush()?;
        self.session.set_speed(speed_milli)?;
        if let Some(audio) = audio {
            let playback = PlaybackAudio::new(audio, speed_milli)?;
            let output_frame = playback.output_at(frame);
            self.start = output_frame;
            self.queued = output_frame;
            self.presented = output_frame;
            self.audio = Some(playback);
            self.state = PlaybackState::Paused;
            self.device.pause(true)?;
        }
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<()> {
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            // Flush even if the endpoint disconnected before it could report position.
            let observed = self
                .device
                .pause(true)
                .map_err(Error::from)
                .and_then(|()| self.observe());
            if let Err(error) = observed {
                let _ = self.flush();
                self.state = PlaybackState::Failed;
                return Err(error);
            }
        }
        self.flush()?;
        self.audio = None;
        self.state = PlaybackState::Stopped;
        Ok(())
    }
    pub fn bookmark(&mut self, id: u64) -> Result<()> {
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            self.observe()?;
        }
        self.session.bookmark(id)
    }
    pub fn checkpoint(
        &mut self,
        store: &mut SessionStore,
        now_ms: u64,
        force: bool,
    ) -> Result<bool> {
        if self.checkpointed == self.session.sequence() {
            return Ok(false);
        }
        let written = store.checkpoint(&self.session, &self.plan, now_ms, force)?;
        if written {
            self.checkpointed = self.session.sequence();
        }
        Ok(written)
    }
}
