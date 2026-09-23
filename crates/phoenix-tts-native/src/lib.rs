//! Single-owner, authenticated loopback worker; completed-cache publication only
//! after EOS and a quiescence barrier. Run on a control worker, never the UI thread.
mod bundle;
pub mod supertonic;
mod supervisor;
mod voice;
pub use voice::{VoiceAsset, MAX_VOICE_BYTES};
pub mod wire;
pub use bundle::Bundle;
pub use supervisor::{Cancellation, NativeProvider, PcmChunk, Request};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native provider: {0}")]
    Invalid(&'static str),
    #[error("native provider cancelled")]
    Cancelled,
    #[error("native provider deadline expired")]
    Timeout,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reader(#[from] phoenix_reader_session::Error),
    #[error(transparent)]
    Contract(#[from] phoenix_tts_contract::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
