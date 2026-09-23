//! Provider-independent, fail-closed narration protocol. No inference runtime.
mod protocol;
pub use protocol::*;
use serde::{Deserialize, Serialize};

pub const CONTRACT: &str = "phoenix.reader/v1";
pub const BLOCK_FRAMES: usize = 2048;
pub const POOL_BLOCKS: usize = 64;
pub type Digest = [u8; 32];

/// Borrowed request view, validated after the adapter tokenizes the full prompt.
pub struct SynthesisRequest<'a> {
    binding: Binding,
    identity: &'a SynthesisIdentity,
    spoken: &'a str,
    max_frames: u64,
}
impl<'a> SynthesisRequest<'a> {
    pub fn new(
        binding: Binding,
        identity: &'a SynthesisIdentity,
        spoken: &'a str,
        capabilities: Capabilities,
        full_context_tokens: u32,
        max_frames: u64,
    ) -> Result<Self> {
        capabilities.validate_request(identity, full_context_tokens)?;
        if binding.audio_key != identity.audio_key(spoken)?
            || max_frames == 0
            || max_frames > capabilities.max_output_frames
        {
            return Err(Error::Invalid("request key or frame limit"));
        }
        StreamValidator::new(binding, identity.provider, max_frames)?;
        Ok(Self {
            binding,
            identity,
            spoken,
            max_frames,
        })
    }
    pub fn binding(&self) -> Binding {
        self.binding
    }
    pub fn identity(&self) -> &SynthesisIdentity {
        self.identity
    }
    pub fn spoken(&self) -> &str {
        self.spoken
    }
    pub fn validator(&self) -> Result<StreamValidator> {
        StreamValidator::new(self.binding, self.identity.provider, self.max_frames)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("reader contract: {0}")]
    Invalid(&'static str),
    #[error("reader encoding: {0}")]
    Encoding(#[from] postcard::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// Length-delimited, domain-separated serialization; never hashes debug strings.
pub fn digest<T: Serialize>(domain: &[u8], value: &T) -> Result<Digest> {
    let bytes = postcard::to_allocvec(value)?;
    let mut hash = blake3::Hasher::new();
    hash.update(&(domain.len() as u64).to_le_bytes());
    hash.update(domain);
    hash.update(&(bytes.len() as u64).to_le_bytes());
    hash.update(&bytes);
    Ok(*hash.finalize().as_bytes())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Binding {
    pub request: u64,
    pub epoch: u64,
    pub plan: Digest,
    pub segment: u32,
    pub audio_key: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub bits: u8,
}
impl AudioFormat {
    pub const PCM24: Self = Self {
        sample_rate: 24_000,
        channels: 1,
        bits: 16,
    };
    pub fn validate(self) -> Result<()> {
        if self != Self::PCM24 {
            return Err(Error::Invalid("unsupported audio format"));
        }
        Ok(())
    }
}

/// Every field is a content identity, including the canonical generation config.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesisIdentity {
    pub provider: Digest,
    pub runtime: Digest,
    pub model: Digest,
    pub tokenizer: Digest,
    pub codec: Digest,
    pub voice: Digest,
    pub reference_audio: Option<Digest>,
    pub reference_transcript: Option<Digest>,
    pub direction: Digest,
    pub generation_config: Digest,
    pub transformations: Digest,
    pub postprocessing: Digest,
    pub seed: u64,
    pub format: AudioFormat,
}
impl SynthesisIdentity {
    pub fn validate(&self) -> Result<()> {
        self.format.validate()?;
        if [
            self.provider,
            self.runtime,
            self.model,
            self.tokenizer,
            self.codec,
            self.voice,
            self.direction,
            self.generation_config,
            self.transformations,
            self.postprocessing,
        ]
        .contains(&[0; 32])
            || self.reference_audio.is_some() != self.reference_transcript.is_some()
            || self.reference_audio == Some([0; 32])
            || self.reference_transcript == Some([0; 32])
        {
            return Err(Error::Invalid("incomplete synthesis identity"));
        }
        Ok(())
    }
    pub fn audio_key(&self, spoken: &str) -> Result<Digest> {
        self.validate()?;
        if spoken.trim().is_empty() {
            return Err(Error::Invalid("empty speech"));
        }
        digest(b"phoenix.audio-request/v1", &(self, spoken))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignmentLevel {
    Segment,
    Sentence,
    Word,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub provider: Digest,
    pub format: AudioFormat,
    pub max_context_tokens: u32,
    pub max_output_frames: u64,
    pub concurrency: u16,
    pub cancellation: bool,
    pub normal_finish_reason: bool,
    pub alignment: AlignmentLevel,
}
impl Capabilities {
    pub fn validate_request(
        &self,
        identity: &SynthesisIdentity,
        full_context_tokens: u32,
    ) -> Result<()> {
        identity.validate()?;
        if self.provider != identity.provider
            || self.format != identity.format
            || self.concurrency != 1
            || self.max_output_frames == 0
            || full_context_tokens == 0
            || full_context_tokens > self.max_context_tokens
            || !self.normal_finish_reason
        {
            return Err(Error::Invalid(
                "provider capability or token budget mismatch",
            ));
        }
        Ok(())
    }
}
