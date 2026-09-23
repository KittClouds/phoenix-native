use super::PlaybackDevice;
use phoenix_tts_contract::BLOCK_FRAMES;
use std::{io, mem::size_of};
use windows::Win32::Media::{Audio::*, MMTIME, TIME_SAMPLES};

const SLOTS: usize = 8;
const HEADER_SIZE: u32 = size_of::<WAVEHDR>() as u32;
struct Slot {
    samples: [i16; BLOCK_FRAMES],
    header: WAVEHDR,
    prepared: bool,
}
/// Bounded WinMM output, owned and polled on one control thread. No callback,
/// cross-thread mutable Rust references, or allocation during submit/poll.
pub struct WaveOutput {
    handle: HWAVEOUT,
    slots: Box<[Slot; SLOTS]>,
    next: usize,
    submitted: u64,
    last: u64,
}
fn check(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("waveOut error {code}")))
    }
}
impl WaveOutput {
    pub fn open_default() -> io::Result<Self> {
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: 24_000,
            nAvgBytesPerSec: 48_000,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut handle = HWAVEOUT::default();
        // SAFETY: format and output handle are valid for the synchronous call.
        unsafe {
            check(waveOutOpen(
                Some(&mut handle),
                WAVE_MAPPER,
                &format,
                None,
                None,
                MIDI_WAVE_OPEN_TYPE(0),
            ))?;
        }
        Ok(Self {
            handle,
            slots: Box::new(std::array::from_fn(|_| Slot {
                samples: [0; BLOCK_FRAMES],
                header: WAVEHDR::default(),
                prepared: false,
            })),
            next: 0,
            submitted: 0,
            last: 0,
        })
    }
    fn release(&mut self, index: usize) -> io::Result<bool> {
        let slot = &mut self.slots[index];
        if !slot.prepared {
            return Ok(true);
        }
        // SAFETY: the pinned-in-Box header lives until unprepare/reset completes.
        // Use the API, rather than reading driver-mutated header flags.
        let code = unsafe { waveOutUnprepareHeader(self.handle, &mut slot.header, HEADER_SIZE) };
        if code == WAVERR_STILLPLAYING {
            return Ok(false);
        }
        check(code)?;
        slot.prepared = false;
        Ok(true)
    }
}
impl PlaybackDevice for WaveOutput {
    fn available(&mut self) -> io::Result<bool> {
        self.release(self.next)
    }
    fn submit(&mut self, samples: &[i16]) -> io::Result<()> {
        if samples.is_empty() || samples.len() > BLOCK_FRAMES || !self.available()? {
            return Err(io::Error::other("device block bound or backpressure"));
        }
        let slot = &mut self.slots[self.next];
        slot.samples[..samples.len()].copy_from_slice(samples);
        slot.header = WAVEHDR {
            lpData: windows::core::PSTR(slot.samples.as_mut_ptr().cast()),
            dwBufferLength: (samples.len() * 2) as u32,
            ..Default::default()
        };
        // SAFETY: boxed sample/header addresses stay fixed until unprepared.
        unsafe {
            check(waveOutPrepareHeader(
                self.handle,
                &mut slot.header,
                HEADER_SIZE,
            ))?;
            slot.prepared = true;
            check(waveOutWrite(self.handle, &mut slot.header, HEADER_SIZE))?;
        }
        self.submitted += samples.len() as u64;
        self.next = (self.next + 1) % SLOTS;
        Ok(())
    }
    fn position(&mut self) -> io::Result<u64> {
        let mut time = MMTIME {
            wType: TIME_SAMPLES,
            ..Default::default()
        };
        // SAFETY: valid initialized output structure; union read follows tag check.
        unsafe {
            check(waveOutGetPosition(
                self.handle,
                &mut time,
                size_of::<MMTIME>() as u32,
            ))?;
        }
        if time.wType != TIME_SAMPLES {
            return Err(io::Error::other("device lacks sample clock"));
        }
        let low = unsafe { time.u.sample } as u64;
        let mut frame = (self.last & !0xffff_ffff) | low;
        if frame < self.last {
            frame += 1u64 << 32;
        }
        if frame > self.submitted {
            return Err(io::Error::other("invalid device clock"));
        }
        self.last = frame;
        Ok(frame)
    }
    fn pause(&mut self, paused: bool) -> io::Result<()> {
        unsafe {
            check(if paused {
                waveOutPause(self.handle)
            } else {
                waveOutRestart(self.handle)
            })
        }
    }
    fn reset(&mut self) -> io::Result<()> {
        unsafe {
            check(waveOutReset(self.handle))?;
        }
        for i in 0..SLOTS {
            if !self.release(i)? {
                return Err(io::Error::other("reset did not release audio"));
            }
        }
        self.submitted = 0;
        self.last = 0;
        self.next = 0;
        Ok(())
    }
}
impl Drop for WaveOutput {
    fn drop(&mut self) {
        // A failed reset must not free buffers still owned by the driver.
        if self.reset().is_err() {
            let slots = std::mem::replace(
                &mut self.slots,
                Box::new(std::array::from_fn(|_| Slot {
                    samples: [0; BLOCK_FRAMES],
                    header: WAVEHDR::default(),
                    prepared: false,
                })),
            );
            std::mem::forget(slots);
            return;
        }
        unsafe {
            let _ = waveOutClose(self.handle);
        }
    }
}
