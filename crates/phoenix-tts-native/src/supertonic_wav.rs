use crate::{Error, Result};
pub(super) struct PcmWave<'a> {
    data: &'a [u8],
    rate: u32,
}
impl<'a> PcmWave<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let bad = || Error::Invalid("Incomplete or unsupported Supertonic WAV");
        if bytes.len() < 44
            || &bytes[..4] != b"RIFF"
            || &bytes[8..12] != b"WAVE"
            || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 != bytes.len()
        {
            return Err(bad());
        }
        let mut offset = 12usize;
        let mut rate = None;
        let mut data = None;
        while offset < bytes.len() {
            let header = bytes.get(offset..offset + 8).ok_or_else(bad)?;
            let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let end = (offset + 8).checked_add(size).ok_or_else(bad)?;
            let chunk = bytes.get(offset + 8..end).ok_or_else(bad)?;
            match &header[..4] {
                b"fmt " => {
                    if rate.is_some()
                        || size != 16
                        || chunk[..4] != [1, 0, 1, 0]
                        || chunk[12..16] != [2, 0, 16, 0]
                    {
                        return Err(bad());
                    }
                    let value = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
                    if ![24_000, 44_100].contains(&value)
                        || u32::from_le_bytes(chunk[8..12].try_into().unwrap()) != value * 2
                    {
                        return Err(bad());
                    }
                    rate = Some(value);
                }
                b"data" => {
                    if data.is_some() || size == 0 || size % 2 != 0 {
                        return Err(bad());
                    }
                    data = Some(chunk);
                }
                _ => {}
            }
            offset = end.checked_add(size % 2).ok_or_else(bad)?;
        }
        if offset != bytes.len() {
            return Err(bad());
        }
        Ok(Self {
            data: data.ok_or_else(bad)?,
            rate: rate.ok_or_else(bad)?,
        })
    }
    pub fn output_frames(&self) -> u64 {
        self.data.len() as u64 / 2 * 24_000 / self.rate as u64
    }
    fn sample(&self, index: i64) -> f64 {
        let index = index.clamp(0, self.data.len() as i64 / 2 - 1) as usize * 2;
        i16::from_le_bytes([self.data[index], self.data[index + 1]]) as f64
    }
}
// 44,100 / 24,000 = 147 / 80. Precomputed rational phases avoid trigonometry
// and heap allocation per sample. Low-pass filtering precedes downsampling.
pub(super) struct Resampler {
    taps: [[f64; 64]; 80],
}
impl Resampler {
    pub fn new() -> Self {
        let mut taps = [[0.; 64]; 80];
        let cutoff = 0.94 * 24_000. / 44_100.;
        for (phase, row) in taps.iter_mut().enumerate() {
            for (tap, weight) in row.iter_mut().enumerate() {
                let x = tap as f64 - 31. - phase as f64 / 80.;
                let a = std::f64::consts::PI * x * cutoff;
                let sinc = if a.abs() < 1e-10 {
                    cutoff
                } else {
                    cutoff * a.sin() / a
                };
                let window = 0.42 - 0.5 * (2. * std::f64::consts::PI * tap as f64 / 63.).cos()
                    + 0.08 * (4. * std::f64::consts::PI * tap as f64 / 63.).cos();
                *weight = sinc * window;
            }
            let sum: f64 = row.iter().sum();
            for weight in row {
                *weight /= sum;
            }
        }
        Self { taps }
    }
    pub fn render(&self, audio: &PcmWave<'_>, first: u64, output: &mut [u8]) {
        for (i, pair) in output.chunks_exact_mut(2).enumerate() {
            let frame = first + i as u64;
            let value = if audio.rate == 24_000 {
                audio.sample(frame as i64)
            } else {
                let position = frame * 147;
                let center = (position / 80) as i64;
                self.taps[(position % 80) as usize]
                    .iter()
                    .enumerate()
                    .map(|(tap, weight)| audio.sample(center + tap as i64 - 31) * weight)
                    .sum::<f64>()
            };
            pair.copy_from_slice(
                &(value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16).to_le_bytes(),
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wave(data: &[u8], rate: u32) -> Vec<u8> {
        let mut b = b"RIFF".to_vec();
        b.extend((36 + data.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend([2, 0, 16, 0]);
        b.extend(b"data");
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
        b
    }
    #[test]
    fn truncated_and_duplicate_chunks_fail() {
        let b = wave(&[1, 0, 2, 0], 24_000);
        for end in 0..b.len() {
            assert!(PcmWave::parse(&b[..end]).is_err());
        }
        let mut b = b;
        b.extend(b"data\x02\0\0\0\x01\0");
        let len = b.len() as u32 - 8;
        b[4..8].copy_from_slice(&len.to_le_bytes());
        assert!(PcmWave::parse(&b).is_err());
    }
    #[test]
    fn resampling_preserves_duration_dc_and_block_continuity() {
        let data: Vec<_> = (0..4410).flat_map(|_| 1234i16.to_le_bytes()).collect();
        let b = wave(&data, 44_100);
        let audio = PcmWave::parse(&b).unwrap();
        assert_eq!(audio.output_frames(), 2400);
        let resampler = Resampler::new();
        let mut full = vec![0; 4800];
        resampler.render(&audio, 0, &mut full);
        assert!(full
            .chunks_exact(2)
            .all(|p| i16::from_le_bytes([p[0], p[1]]) == 1234));
        let mut split = vec![0; 4800];
        resampler.render(&audio, 0, &mut split[..2048]);
        resampler.render(&audio, 1024, &mut split[2048..]);
        assert_eq!(full, split);
    }
    #[test]
    fn rejects_alias_band_energy() {
        let data: Vec<_> = (0..4410)
            .flat_map(|i| {
                ((2. * std::f64::consts::PI * 16_000. * i as f64 / 44_100.)
                    .sin()
                    .mul_add(10_000., 0.) as i16)
                    .to_le_bytes()
            })
            .collect();
        let b = wave(&data, 44_100);
        let audio = PcmWave::parse(&b).unwrap();
        let mut out = vec![0; 4800];
        Resampler::new().render(&audio, 0, &mut out);
        assert!(out[200..4600]
            .chunks_exact(2)
            .all(|p| i16::from_le_bytes([p[0], p[1]]).abs() < 30));
    }
}
