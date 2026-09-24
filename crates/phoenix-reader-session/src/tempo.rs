use crate::{CachedAudio, Error, Result};

/// A disposable playback view. The cache remains the original verified PCM;
/// speed never changes its identity or the source-frame checkpoint units.
pub(crate) struct PlaybackAudio {
    cached: CachedAudio,
    stretched: Option<Box<[i16]>>,
    output_frames: u64,
}

impl PlaybackAudio {
    pub(crate) fn new(cached: CachedAudio, speed_milli: u16) -> Result<Self> {
        let source_frames = cached.manifest().frames;
        if speed_milli == 1000 {
            return Ok(Self {
                cached,
                stretched: None,
                output_frames: source_frames,
            });
        }
        let stretched = stretch_pcm(cached.pcm(), speed_milli)?;
        let output_frames = stretched.len() as u64;
        Ok(Self {
            cached,
            stretched: Some(stretched),
            output_frames,
        })
    }

    pub(crate) fn cached(&self) -> &CachedAudio {
        &self.cached
    }

    pub(crate) fn into_cached(self) -> CachedAudio {
        self.cached
    }

    pub(crate) fn output_frames(&self) -> u64 {
        self.output_frames
    }

    pub(crate) fn source_at(&self, output_frame: u64) -> u64 {
        ((output_frame as u128 * self.cached.manifest().frames as u128)
            / self.output_frames as u128) as u64
    }

    pub(crate) fn output_at(&self, source_frame: u64) -> u64 {
        ((source_frame as u128 * self.output_frames as u128)
            / self.cached.manifest().frames as u128) as u64
    }

    pub(crate) fn copy_samples(&self, start: u64, output: &mut [i16]) {
        if let Some(stretched) = &self.stretched {
            output.copy_from_slice(&stretched[start as usize..start as usize + output.len()]);
        } else {
            let offset = start as usize * 2;
            let bytes_end = offset + output.len() * 2;
            for (out, bytes) in output
                .iter_mut()
                .zip(self.cached.pcm()[offset..bytes_end].chunks_exact(2))
            {
                *out = i16::from_le_bytes([bytes[0], bytes[1]]);
            }
        }
    }
}

fn stretch_pcm(pcm: &[u8], speed_milli: u16) -> Result<Box<[i16]>> {
    let input: Vec<f32> = pcm
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
        .collect();
    let rendered = wsola::stretch(&input, 24_000, 1, speed_milli as f32 / 1000.0)
        .map_err(|_| Error::Invalid("time stretch failed"))?;
    if rendered.is_empty() {
        return Err(Error::Invalid("time stretch returned empty audio"));
    }
    Ok(rendered
        .into_iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::stretch_pcm;

    #[test]
    fn faster_speech_keeps_sine_pitch_and_shortens_duration() {
        let pcm: Vec<u8> = (0..48_000)
            .flat_map(|n| {
                let phase = n as f32 * std::f32::consts::TAU * 240.0 / 24_000.0;
                ((phase.sin() * 16000.0) as i16).to_le_bytes()
            })
            .collect();
        let rendered = stretch_pcm(&pcm, 1300).unwrap();
        assert!((35_000..39_000).contains(&rendered.len()));
        let middle = &rendered[6_000..30_000];
        let crossings = middle
            .windows(2)
            .filter(|pair| pair[0] <= 0 && pair[1] > 0)
            .count();
        assert!((225..255).contains(&crossings));
    }
}
