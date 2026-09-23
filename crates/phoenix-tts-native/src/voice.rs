//! Immutable, bounded upstream BRZV voice asset. Keep the original WAV separately.
use crate::{Bundle, Error, Result};
use memmap2::{Mmap, MmapOptions};
use phoenix_tts_contract::{Digest, SynthesisIdentity};
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::Path,
};

pub const MAX_VOICE_BYTES: u64 = 512 * 1024;
pub struct VoiceAsset {
    map: Mmap,
    _file: File,
    transcript_end: usize,
    hash: Digest,
    transcript_hash: Digest,
    model: Digest,
    codec: Digest,
}
impl VoiceAsset {
    /// Model and codec hashes are enrollment authority, not inferred from BRZV.
    pub fn open(path: &Path, expected: Digest, model: Digest, codec: Digest) -> Result<Self> {
        if expected == [0; 32] || model == [0; 32] || codec == [0; 32] {
            return Err(Error::Invalid("voice enrollment identity"));
        }
        let file = OpenOptions::new().read(true).share_mode(1).open(path)?;
        if !(24..=MAX_VOICE_BYTES).contains(&file.metadata()?.len()) {
            return Err(Error::Invalid("voice file bound"));
        }
        // SAFETY: retained Windows handle denies writes and replacement.
        let map = unsafe { MmapOptions::new().map(&file)? };
        let transcript_end = validate(&map)?;
        let hash = *blake3::hash(&map).as_bytes();
        if hash != expected {
            return Err(Error::Invalid("voice file hash mismatch"));
        }
        let transcript_hash = *blake3::hash(&map[24..transcript_end]).as_bytes();
        Ok(Self {
            map,
            _file: file,
            transcript_end,
            hash,
            transcript_hash,
            model,
            codec,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.map
    }
    pub fn transcript(&self) -> &str {
        std::str::from_utf8(&self.map[24..self.transcript_end]).unwrap()
    }
    pub fn hash(&self) -> Digest {
        self.hash
    }
    pub fn identity(
        &self,
        bundle: &Bundle,
        instruction: &str,
        seed: u32,
        max_frames: u64,
    ) -> Result<SynthesisIdentity> {
        let mut identity = bundle.identity(instruction, seed, max_frames)?;
        if identity.model != self.model || identity.codec != self.codec {
            return Err(Error::Invalid("voice belongs to another model or codec"));
        }
        identity.voice = self.hash;
        identity.reference_audio = Some(self.hash);
        identity.reference_transcript = Some(self.transcript_hash);
        identity.validate()?;
        Ok(identity)
    }
}
fn validate(bytes: &[u8]) -> Result<usize> {
    let invalid = || Error::Invalid("invalid BRZV voice");
    if bytes.len() < 24 || bytes.len() as u64 > MAX_VOICE_BYTES || &bytes[..4] != b"BRZV" {
        return Err(invalid());
    }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (version, rate, books, frames, text) = (word(4), word(8), word(12), word(16), word(20));
    if version != 1
        || rate != 24000
        || books != 16
        || !(1..=375).contains(&frames)
        || !(1..=16384).contains(&text)
    {
        return Err(invalid());
    }
    let end = 24 + text;
    if bytes.len() != end + frames * books * 4 {
        return Err(invalid());
    }
    let transcript = std::str::from_utf8(&bytes[24..end]).map_err(|_| invalid())?;
    if transcript.trim().is_empty() || transcript.contains('\0') {
        return Err(invalid());
    }
    if bytes[end..]
        .chunks_exact(4)
        .any(|c| u32::from_le_bytes(c.try_into().unwrap()) >= 2048)
    {
        return Err(invalid());
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset() -> Vec<u8> {
        let mut b = b"BRZV".to_vec();
        for n in [1u32, 24000, 16, 1, 2] {
            b.extend(n.to_le_bytes());
        }
        b.extend(b"Hi");
        b.extend([0; 64]);
        b
    }
    #[test]
    fn rejects_truncation_trailing_bytes_invalid_codes_and_transcript() {
        let b = asset();
        assert_eq!(validate(&b).unwrap(), 26);
        for n in 0..b.len() {
            assert!(validate(&b[..n]).is_err());
        }
        let mut b = asset();
        b.push(0);
        assert!(validate(&b).is_err());
        let mut b = asset();
        b[26..30].copy_from_slice(&2048u32.to_le_bytes());
        assert!(validate(&b).is_err());
        let mut b = asset();
        b[24] = 255;
        assert!(validate(&b).is_err());
    }
    #[test]
    fn enrollment_pins_bytes_and_rejects_wrong_hash() {
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("voice.breeze");
        let b = asset();
        std::fs::write(&p, &b).unwrap();
        assert!(VoiceAsset::open(&p, [1; 32], [2; 32], [2; 32]).is_err());
        let voice = VoiceAsset::open(&p, *blake3::hash(&b).as_bytes(), [2; 32], [2; 32]).unwrap();
        assert_eq!(voice.transcript(), "Hi");
        assert!(std::fs::write(&p, b"changed").is_err());
    }
}
