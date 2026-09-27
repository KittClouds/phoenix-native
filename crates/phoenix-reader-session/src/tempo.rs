use crate::{CachedAudio, Error, NarrationPlan, Result};

/// Source sample rate of every cached Reader artifact.
const RATE: u64 = 24_000;
/// A 10 ms analysis window; speech ends after the last window above -40 dBFS.
const WINDOW: usize = 240;
const SPEECH_FLOOR: i16 = 328;
/// Natural release kept after the last loud window.
const RELEASE_FRAMES: u64 = RATE * 40 / 1000;
/// Edge fades remove clicks where speech meets inserted silence.
const FADE_IN_FRAMES: usize = 24_000 * 4 / 1000;
const FADE_OUT_FRAMES: usize = 24_000 * 12 / 1000;

/// The pause that follows a passage, by how the next passage begins. Breeze
/// leaves 70-530 ms of arbitrary tail silence per clip; replacing it with a
/// pause that matches the text's structure gives narration an even rhythm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// A long sentence split at the byte budget: barely a breath.
    WithinSentence,
    Sentence,
    Paragraph,
    Chapter,
    /// The last passage keeps its own tail.
    End,
}

impl Join {
    pub fn after(plan: &NarrationPlan, segment: u32) -> Join {
        let segments = &plan.spec().segments;
        let (Some(this), Some(next)) = (
            segments.get(segment as usize),
            segments.get(segment as usize + 1),
        ) else {
            return Join::End;
        };
        let spoken = &plan.spec().spoken;
        let next_text = spoken
            .get(next.spoken.start as usize..next.spoken.end as usize)
            .unwrap_or("");
        if next.chapter != this.chapter {
            Join::Chapter
        } else if next_text
            .chars()
            .take_while(|c| c.is_whitespace())
            .any(|c| c == '\n')
        {
            Join::Paragraph
        } else if next.sentence == this.sentence {
            Join::WithinSentence
        } else {
            Join::Sentence
        }
    }

    /// Silence after the speech release, at 1x.
    pub fn pause_ms(self) -> Option<u64> {
        match self {
            Join::WithinSentence => Some(80),
            Join::Sentence => Some(300),
            Join::Paragraph => Some(620),
            Join::Chapter => Some(1_100),
            Join::End => None,
        }
    }
}

/// A disposable playback view. The cache remains the original verified PCM;
/// speed and join shaping never change its identity or the source-frame
/// checkpoint units. The view is `speech` (the source up to its speech end,
/// time-stretched for speed, with edge fades) followed by `pause` frames of
/// silence; positions map piecewise-linearly between the two timelines.
pub(crate) struct PlaybackAudio {
    cached: CachedAudio,
    rendered: Box<[i16]>,
    /// Source frames covered by the speech part.
    speech_source: u64,
    /// Output frames of the speech part.
    speech_output: u64,
    join: Join,
}

impl PlaybackAudio {
    pub(crate) fn new(cached: CachedAudio, speed_milli: u16, join: Join) -> Result<Self> {
        let source: Vec<i16> = cached
            .pcm()
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let source_frames = source.len() as u64;
        if source_frames == 0 {
            return Err(Error::Invalid("empty playback audio"));
        }
        let (speech_source, pause_source) = match join.pause_ms() {
            Some(ms) => (speech_end(&source), RATE * ms / 1000),
            None => (source_frames, 0),
        };
        let mut speech = if speed_milli == 1000 {
            source[..speech_source as usize].to_vec()
        } else {
            stretch(&source[..speech_source as usize], speed_milli)?
        };
        if join.pause_ms().is_some() {
            fade(&mut speech);
        }
        let speech_output = speech.len() as u64;
        let pause_output = pause_source * 1000 / speed_milli as u64;
        speech.resize(speech.len() + pause_output as usize, 0);
        Ok(Self {
            cached,
            rendered: speech.into_boxed_slice(),
            speech_source,
            speech_output,
            join,
        })
    }

    pub(crate) fn join(&self) -> Join {
        self.join
    }

    pub(crate) fn cached(&self) -> &CachedAudio {
        &self.cached
    }

    pub(crate) fn into_cached(self) -> CachedAudio {
        self.cached
    }

    pub(crate) fn output_frames(&self) -> u64 {
        self.rendered.len() as u64
    }

    pub(crate) fn source_at(&self, output_frame: u64) -> u64 {
        let source_frames = self.cached.manifest().frames;
        let output_frame = output_frame.min(self.output_frames());
        if output_frame <= self.speech_output {
            scale(output_frame, self.speech_source, self.speech_output)
        } else {
            self.speech_source
                + scale(
                    output_frame - self.speech_output,
                    source_frames - self.speech_source,
                    self.output_frames() - self.speech_output,
                )
        }
    }

    pub(crate) fn output_at(&self, source_frame: u64) -> u64 {
        let source_frames = self.cached.manifest().frames;
        let source_frame = source_frame.min(source_frames);
        if source_frame >= source_frames {
            self.output_frames()
        } else if source_frame <= self.speech_source {
            scale(source_frame, self.speech_output, self.speech_source)
        } else {
            self.speech_output
                + scale(
                    source_frame - self.speech_source,
                    self.output_frames() - self.speech_output,
                    source_frames - self.speech_source,
                )
        }
    }

    pub(crate) fn copy_samples(&self, start: u64, output: &mut [i16]) {
        output.copy_from_slice(&self.rendered[start as usize..start as usize + output.len()]);
    }
}

/// `value * numerator / denominator`, with an empty span mapping to its end.
fn scale(value: u64, numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        return numerator;
    }
    ((value as u128 * numerator as u128) / denominator as u128) as u64
}

/// The frame just after the last loud window plus a short release; a clip
/// with no loud window keeps all of its audio.
fn speech_end(samples: &[i16]) -> u64 {
    let loud = samples
        .chunks(WINDOW)
        .rposition(|window| window.iter().any(|s| s.unsigned_abs() > SPEECH_FLOOR as u16));
    match loud {
        Some(window) => ((window as u64 + 1) * WINDOW as u64 + RELEASE_FRAMES)
            .min(samples.len() as u64),
        None => samples.len() as u64,
    }
}

fn fade(speech: &mut [i16]) {
    let len = speech.len();
    let fade_in = FADE_IN_FRAMES.min(len / 4);
    for (i, sample) in speech.iter_mut().take(fade_in).enumerate() {
        *sample = (*sample as f32 * (i as f32 / fade_in as f32)) as i16;
    }
    let fade_out = FADE_OUT_FRAMES.min(len / 4);
    for i in 0..fade_out {
        let sample = &mut speech[len - 1 - i];
        *sample = (*sample as f32 * (i as f32 / fade_out as f32)) as i16;
    }
}

fn stretch(samples: &[i16], speed_milli: u16) -> Result<Vec<i16>> {
    let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    Ok(stretch_pcm(&pcm, speed_milli)?.into_vec())
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
    use super::{fade, speech_end, stretch_pcm, RELEASE_FRAMES, WINDOW};

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

    #[test]
    fn speech_ends_after_the_last_loud_window_plus_release() {
        let mut samples = vec![0i16; 24_000];
        for s in &mut samples[2_400..12_000] {
            *s = 8_000;
        }
        let end = speech_end(&samples);
        assert_eq!(end, 12_000 + RELEASE_FRAMES);
        assert_eq!(speech_end(&[0; 4_800]), 4_800);
        assert!(end as usize % WINDOW == 0 || end > 12_000);
    }

    #[test]
    fn edge_fades_start_and_end_silent() {
        let mut speech = vec![10_000i16; 24_000];
        fade(&mut speech);
        assert_eq!(speech[0], 0);
        assert_eq!(*speech.last().unwrap(), 0);
        assert_eq!(speech[12_000], 10_000);
    }
}
