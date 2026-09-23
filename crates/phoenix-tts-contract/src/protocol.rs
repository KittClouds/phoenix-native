use crate::{AlignmentLevel, AudioFormat, Binding, Digest, Error, Result, BLOCK_FRAMES};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Normal,
    TokenLimit,
    TransportEof,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    Timeout,
    Protocol,
    Disconnected,
    GenerationLimit,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlignmentHint {
    pub level: AlignmentLevel,
    pub provenance: Digest,
    pub spoken_start: u32,
    pub spoken_end: u32,
    pub first_frame: u64,
    pub end_frame: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Started {
        provider: Digest,
        format: AudioFormat,
    },
    AudioChunk {
        first_frame: u64,
        frames: u32,
    },
    AlignmentChunk(AlignmentHint),
    Completed {
        frames: u64,
        reason: FinishReason,
    },
    Cancelled,
    Failed(Failure),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub binding: Binding,
    pub sequence: u64,
    pub event: Event,
}

/// Only this validator can construct a completion receipt. EOF cannot do so.
#[derive(Clone, Debug)]
pub struct Completion {
    binding: Binding,
    frames: u64,
    format: AudioFormat,
}
impl Completion {
    pub fn binding(&self) -> Binding {
        self.binding
    }
    pub fn frames(&self) -> u64 {
        self.frames
    }
    pub fn format(&self) -> AudioFormat {
        self.format
    }
}

#[derive(Clone, Debug)]
pub struct StreamValidator {
    binding: Binding,
    provider: Digest,
    next_sequence: u64,
    frames: u64,
    limit: u64,
    started: bool,
    terminal: bool,
    last_alignment: Option<AlignmentHint>,
}
impl StreamValidator {
    pub fn new(binding: Binding, provider: Digest, max_frames: u64) -> Result<Self> {
        if binding.request == 0
            || binding.epoch == 0
            || binding.plan == [0; 32]
            || binding.audio_key == [0; 32]
            || provider == [0; 32]
            || max_frames == 0
        {
            return Err(Error::Invalid("invalid request binding"));
        }
        Ok(Self {
            binding,
            provider,
            next_sequence: 0,
            frames: 0,
            limit: max_frames,
            started: false,
            terminal: false,
            last_alignment: None,
        })
    }
    pub fn accept(&mut self, envelope: Envelope) -> Result<Option<Completion>> {
        let result = self.accept_inner(envelope);
        // Poison on protocol violation; callers cannot recover by replaying a
        // corrected event into a stream whose actual transport is now unknown.
        if result.is_err() {
            self.terminal = true;
        }
        result
    }
    fn accept_inner(&mut self, e: Envelope) -> Result<Option<Completion>> {
        if self.terminal || e.binding != self.binding || e.sequence != self.next_sequence {
            return Err(Error::Invalid("stale, terminal, or out-of-order event"));
        }
        let next = self
            .next_sequence
            .checked_add(1)
            .ok_or(Error::Invalid("sequence overflow"))?;
        let mut receipt = None;
        match e.event {
            Event::Started { provider, format } => {
                format.validate()?;
                if self.started || provider != self.provider {
                    return Err(Error::Invalid("duplicate start or provider mismatch"));
                }
                self.started = true;
            }
            Event::AudioChunk {
                first_frame,
                frames,
            } => {
                if !self.started
                    || first_frame != self.frames
                    || frames == 0
                    || frames as usize > BLOCK_FRAMES
                {
                    return Err(Error::Invalid("noncontiguous or invalid PCM block"));
                }
                let end = self
                    .frames
                    .checked_add(frames as u64)
                    .ok_or(Error::Invalid("frame overflow"))?;
                if end > self.limit {
                    return Err(Error::Invalid("output frame budget exceeded"));
                }
                self.frames = end;
            }
            Event::AlignmentChunk(hint) => {
                if !self.started
                    || hint.provenance == [0; 32]
                    || hint.spoken_start >= hint.spoken_end
                    || hint.first_frame >= hint.end_frame
                    || hint.end_frame > self.frames
                    || self.last_alignment.is_some_and(|last| {
                        last.level != hint.level
                            || last.provenance != hint.provenance
                            || last.spoken_end > hint.spoken_start
                            || last.end_frame > hint.first_frame
                    })
                {
                    return Err(Error::Invalid("invalid or reordered alignment event"));
                }
                self.last_alignment = Some(hint);
            }
            Event::Completed { frames, reason } => {
                if !self.started
                    || frames == 0
                    || frames != self.frames
                    || reason != FinishReason::Normal
                {
                    return Err(Error::Invalid("incomplete or truncated generation"));
                }
                self.terminal = true;
                receipt = Some(Completion {
                    binding: self.binding,
                    frames,
                    format: AudioFormat::PCM24,
                });
            }
            Event::Cancelled | Event::Failed(_) => self.terminal = true,
        }
        self.next_sequence = next;
        Ok(receipt)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerState {
    Ready,
    Running,
    Cancelling { deadline_ms: u64 },
    Terminating,
    Unavailable,
}
/// Control-path state. Milliseconds are from a caller-owned monotonic clock.
pub struct Cancellation {
    epoch: u64,
    state: WorkerState,
    presented: bool,
    active_request: Option<u64>,
    last_request: u64,
}
impl Default for Cancellation {
    fn default() -> Self {
        Self {
            epoch: 1,
            state: WorkerState::Ready,
            presented: false,
            active_request: None,
            last_request: 0,
        }
    }
}
impl Cancellation {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn state(&self) -> WorkerState {
        self.state
    }
    pub fn start(&mut self, request: u64) -> Result<u64> {
        if self.state != WorkerState::Ready || request <= self.last_request {
            return Err(Error::Invalid("worker not quiescent or reused request ID"));
        }
        self.state = WorkerState::Running;
        self.presented = false;
        self.active_request = Some(request);
        self.last_request = request;
        Ok(self.epoch)
    }
    pub fn mark_presented(&mut self, request: u64, epoch: u64) {
        if self.active_request == Some(request)
            && epoch == self.epoch
            && self.state == WorkerState::Running
        {
            self.presented = true;
        }
    }
    pub fn may_retry(&self) -> bool {
        !self.presented && self.state == WorkerState::Ready
    }
    pub fn cancel(&mut self, now_ms: u64) -> Result<u64> {
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(Error::Invalid("epoch overflow"))?;
        let deadline_ms = now_ms
            .checked_add(2000)
            .ok_or(Error::Invalid("deadline overflow"))?;
        self.epoch = epoch;
        if self.state == WorkerState::Running {
            self.state = WorkerState::Cancelling { deadline_ms };
        }
        Ok(epoch)
    }
    pub fn quiescent(&mut self, request: u64) -> Result<()> {
        if self.active_request != Some(request)
            || !matches!(
                self.state,
                WorkerState::Running | WorkerState::Cancelling { .. }
            )
        {
            return Err(Error::Invalid("unexpected quiescence"));
        }
        self.state = WorkerState::Ready;
        self.active_request = None;
        Ok(())
    }
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if matches!(self.state, WorkerState::Cancelling { deadline_ms } if now_ms >= deadline_ms) {
            self.state = WorkerState::Terminating;
            return true;
        }
        false
    }
    pub fn exit_confirmed(&mut self) -> Result<()> {
        if self.state != WorkerState::Terminating {
            return Err(Error::Invalid("worker was not terminating"));
        }
        self.state = WorkerState::Unavailable;
        self.active_request = None;
        Ok(())
    }
    pub fn replacement_ready(&mut self) -> Result<()> {
        if self.state != WorkerState::Unavailable {
            return Err(Error::Invalid("old worker exit not confirmed"));
        }
        self.state = WorkerState::Ready;
        Ok(())
    }
}
