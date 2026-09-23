//! Preallocated PCM handoff and device-clock accounting.
pub mod device;
use crossbeam_queue::ArrayQueue;
use phoenix_tts_contract::{Error, Result, BLOCK_FRAMES, POOL_BLOCKS};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

struct Block {
    samples: [i16; BLOCK_FRAMES],
    frames: usize,
    epoch: u64,
    segment: u32,
    first_frame: u64,
}
pub struct PcmBlock(Box<Block>);
impl PcmBlock {
    pub fn samples_mut(&mut self) -> &mut [i16; BLOCK_FRAMES] {
        &mut self.0.samples
    }
    pub fn prepare(
        &mut self,
        epoch: u64,
        segment: u32,
        first_frame: u64,
        frames: usize,
    ) -> Result<()> {
        if epoch == 0
            || frames == 0
            || frames > BLOCK_FRAMES
            || first_frame.checked_add(frames as u64).is_none()
        {
            return Err(Error::Invalid("PCM block bounds"));
        }
        self.0.epoch = epoch;
        self.0.segment = segment;
        self.0.first_frame = first_frame;
        self.0.frames = frames;
        Ok(())
    }
}
struct Shared {
    free: ArrayQueue<PcmBlock>,
    ready: ArrayQueue<PcmBlock>,
    epoch: AtomicU64,
    paused: AtomicBool,
}
pub struct AudioProducer {
    shared: Arc<Shared>,
}
pub struct AudioConsumer {
    shared: Arc<Shared>,
    current: Option<PcmBlock>,
    cursor: usize,
}
pub struct AudioControl {
    shared: Arc<Shared>,
}

pub fn pcm_ring() -> (AudioProducer, AudioConsumer, AudioControl) {
    let shared = Arc::new(Shared {
        free: ArrayQueue::new(POOL_BLOCKS),
        ready: ArrayQueue::new(POOL_BLOCKS),
        epoch: AtomicU64::new(1),
        paused: AtomicBool::new(false),
    });
    for _ in 0..POOL_BLOCKS {
        let block = PcmBlock(Box::new(Block {
            samples: [0; BLOCK_FRAMES],
            frames: 0,
            epoch: 0,
            segment: 0,
            first_frame: 0,
        }));
        assert!(shared.free.push(block).is_ok());
    }
    (
        AudioProducer {
            shared: Arc::clone(&shared),
        },
        AudioConsumer {
            shared: Arc::clone(&shared),
            current: None,
            cursor: 0,
        },
        AudioControl { shared },
    )
}
impl AudioControl {
    pub fn pause(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
    }
    pub fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Acquire)
    }
    pub fn invalidate(&self, new_epoch: u64) -> Result<()> {
        let previous = self.shared.epoch.fetch_max(new_epoch, Ordering::AcqRel);
        if new_epoch <= previous {
            return Err(Error::Invalid("playback epoch must increase"));
        }
        Ok(())
    }
}
impl AudioProducer {
    pub fn acquire(&self) -> Option<PcmBlock> {
        self.shared.free.pop().map(|mut block| {
            block.0.frames = 0;
            block
        })
    }
    pub fn submit(&self, block: PcmBlock) -> std::result::Result<(), PcmBlock> {
        if block.0.epoch != self.shared.epoch.load(Ordering::Acquire) || block.0.frames == 0 {
            return Err(block);
        }
        self.shared.ready.push(block)
    }
    pub fn recycle(&self, block: PcmBlock) {
        // Ownership makes overflow impossible: only POOL_BLOCKS tokens exist.
        assert!(self.shared.free.push(block).is_ok());
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentedSpan {
    pub epoch: u64,
    pub segment: u32,
    pub first_source_frame: u64,
    pub end_source_frame: u64,
    pub first_output_frame: u64,
    pub end_output_frame: u64,
}
pub struct RenderReport {
    spans: [PresentedSpan; POOL_BLOCKS],
    count: usize,
    pub silence_frames: usize,
}
impl RenderReport {
    pub fn spans(&self) -> &[PresentedSpan] {
        &self.spans[..self.count]
    }
}
impl AudioConsumer {
    /// Bounded, allocation-free 1x render kernel. Device presentation must be
    /// acknowledged separately; filling this buffer does NOT advance bookmarks.
    pub fn render(&mut self, output: &mut [i16], first_output_frame: u64) -> Result<RenderReport> {
        if output.len() > BLOCK_FRAMES
            || first_output_frame
                .checked_add(output.len() as u64)
                .is_none()
        {
            return Err(Error::Invalid("callback frame bound"));
        }
        output.fill(0);
        let mut report = RenderReport {
            spans: [PresentedSpan::default(); POOL_BLOCKS],
            count: 0,
            silence_frames: output.len(),
        };
        if self.shared.paused.load(Ordering::Acquire) {
            return Ok(report);
        }
        let epoch = self.shared.epoch.load(Ordering::Acquire);
        let mut written = 0;
        for _ in 0..POOL_BLOCKS {
            if written == output.len() {
                break;
            }
            if self.current.is_none() {
                self.current = self.shared.ready.pop();
                self.cursor = 0;
            }
            let Some(block) = self.current.as_ref() else {
                break;
            };
            if block.0.epoch != epoch {
                self.recycle_current();
                continue;
            }
            let frames = (block.0.frames - self.cursor).min(output.len() - written);
            output[written..written + frames]
                .copy_from_slice(&block.0.samples[self.cursor..self.cursor + frames]);
            report.spans[report.count] = PresentedSpan {
                epoch,
                segment: block.0.segment,
                first_source_frame: block.0.first_frame + self.cursor as u64,
                end_source_frame: block.0.first_frame + (self.cursor + frames) as u64,
                first_output_frame: first_output_frame + written as u64,
                end_output_frame: first_output_frame + (written + frames) as u64,
            };
            report.count += 1;
            self.cursor += frames;
            written += frames;
            if self.cursor == block.0.frames {
                self.recycle_current();
            }
        }
        // Pause takes effect at the next callback boundary. Discarding a buffer
        // on a mid-callback pause would skip samples already consumed here.
        if self.shared.epoch.load(Ordering::Acquire) != epoch {
            output.fill(0);
            report.count = 0;
            written = 0;
        }
        report.silence_frames -= written;
        Ok(report)
    }
    fn recycle_current(&mut self) {
        if let Some(block) = self.current.take() {
            assert!(self.shared.free.push(block).is_ok());
        }
    }
}

/// Mapping supplied by the resampler/time-stretcher or the 1x render report.
/// Device output frame positions and source frame positions are distinct clocks.
#[derive(Default)]
pub struct PresentationClock {
    epoch: u64,
    last_output: u64,
    position: Option<(u32, u64)>,
}
impl PresentationClock {
    pub fn reset(&mut self, epoch: u64, output_frame: u64) -> Result<()> {
        if epoch == 0 || epoch <= self.epoch {
            return Err(Error::Invalid("clock epoch must increase"));
        }
        self.epoch = epoch;
        self.last_output = output_frame;
        self.position = None;
        Ok(())
    }
    pub fn position(&self) -> Option<(u32, u64)> {
        self.position
    }
    /// Call with actual presented device frames, after compensating backend DSP
    /// delay. Silence has no span and leaves position unchanged.
    pub fn acknowledge(
        &mut self,
        span: PresentedSpan,
        presented_output: u64,
    ) -> Result<Option<u32>> {
        if span.epoch != self.epoch
            || span.end_output_frame <= span.first_output_frame
            || span.end_source_frame <= span.first_source_frame
            || presented_output < self.last_output
        {
            return Err(Error::Invalid("invalid presentation span/clock"));
        }
        if presented_output <= span.first_output_frame {
            return Ok(None);
        }
        let at = presented_output.min(span.end_output_frame);
        if at < self.last_output {
            return Err(Error::Invalid("old presentation span"));
        }
        let source = span.first_source_frame
            + (((at - span.first_output_frame) as u128
                * (span.end_source_frame - span.first_source_frame) as u128)
                / (span.end_output_frame - span.first_output_frame) as u128) as u64;
        if self.position.is_some_and(|(segment, frame)| {
            (segment == span.segment && source < frame)
                || (segment != span.segment && span.first_output_frame < self.last_output)
        }) {
            return Err(Error::Invalid("overlapping or backward presentation"));
        }
        let boundary = (self.position.map(|p| p.0) != Some(span.segment)).then_some(span.segment);
        self.last_output = at;
        self.position = Some((span.segment, source));
        Ok(boundary)
    }
}
