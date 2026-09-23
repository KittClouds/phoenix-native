//! Single-owner device boundary. All positions are mono 24 kHz frames since reset.
pub trait PlaybackDevice {
    fn available(&mut self) -> std::io::Result<bool>;
    fn submit(&mut self, samples: &[i16]) -> std::io::Result<()>;
    fn position(&mut self) -> std::io::Result<u64>;
    fn pause(&mut self, paused: bool) -> std::io::Result<()>;
    /// Synchronously discard queued audio and reset the presentation clock.
    fn reset(&mut self) -> std::io::Result<()>;
}

#[cfg(windows)]
mod wave;
#[cfg(windows)]
pub use wave::WaveOutput;
