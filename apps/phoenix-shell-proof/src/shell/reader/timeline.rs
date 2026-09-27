//! Listening timeline for the Reader scrubber.
//!
//! A passage lasts its real audio length once its clip has been seen, and
//! otherwise an estimate from its spoken bytes at the voice's measured pace.
//! Either way the shaped pause after it (see `phoenix_reader_session::Join`)
//! replaces the clip's own tail, as playback does. Times are at 1x; callers
//! divide by speed for display.
use phoenix_reader_session::{Join, NarrationPlan};

/// Typical tail silence Breeze leaves on a clip, which shaping replaces.
const TYPICAL_TAIL: f32 = 0.16;
/// Seconds per spoken byte before any clip has been measured (about 16 B/s).
const DEFAULT_PACE: f32 = 1.0 / 16.0;

pub struct Timeline {
    bytes: Vec<u32>,
    pause: Vec<f32>,
    known: Vec<Option<f32>>,
    pace: f32,
    measured: u32,
}

impl Timeline {
    pub fn new(plan: &NarrationPlan) -> Self {
        let segments = &plan.spec().segments;
        Self {
            bytes: segments
                .iter()
                .map(|s| s.spoken.end - s.spoken.start)
                .collect(),
            pause: (0..segments.len() as u32)
                .map(|i| Join::after(plan, i).pause_ms().unwrap_or(0) as f32 / 1000.0)
                .collect(),
            known: vec![None; segments.len()],
            pace: DEFAULT_PACE,
            measured: 0,
        }
    }

    /// Records a passage's cached clip length and refines the pace estimate.
    pub fn learn(&mut self, segment: u32, clip_seconds: f32) {
        let Some(slot) = self.known.get_mut(segment as usize) else {
            return;
        };
        if slot.is_none() && self.bytes[segment as usize] > 0 {
            let pace = clip_seconds / self.bytes[segment as usize] as f32;
            self.measured += 1;
            // A running mean settles quickly and then stays steady.
            self.pace += (pace - self.pace) / self.measured.min(64) as f32;
        }
        *slot = Some(clip_seconds);
    }

    pub fn seconds(&self, segment: usize) -> f32 {
        let speech = match self.known[segment] {
            Some(clip) => (clip - TYPICAL_TAIL).max(0.0),
            None => self.bytes[segment] as f32 * self.pace,
        };
        speech + self.pause[segment]
    }

    pub fn start_of(&self, segment: u32) -> f32 {
        (0..(segment as usize).min(self.known.len()))
            .map(|i| self.seconds(i))
            .sum()
    }

    pub fn total(&self) -> f32 {
        (0..self.known.len()).map(|i| self.seconds(i)).sum()
    }

    /// The passage playing at `at` seconds and the offset into it.
    pub fn locate(&self, at: f32) -> (u32, f32) {
        let mut start = 0.0;
        for i in 0..self.known.len() {
            let length = self.seconds(i);
            if at < start + length || i + 1 == self.known.len() {
                return (i as u32, (at - start).clamp(0.0, length));
            }
            start += length;
        }
        (0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_reader_session::{plan_markdown, PlannerConfig};

    fn plan(text: &str) -> NarrationPlan {
        let lease = phoenix_workspace::DocumentLease {
            entry_id: phoenix_workspace::EntryId(3),
            revision: phoenix_workspace::DocumentRevision(1),
            content_hash: phoenix_workspace::ContentHash::of(text.as_bytes()),
            content: std::sync::Arc::from(text),
        };
        plan_markdown([1; 32], &lease, PlannerConfig::default())
            .unwrap()
            .plan
    }

    #[test]
    fn timeline_estimates_learns_and_locates() {
        let plan = plan("One two three four. Five six seven eight.\n\nNew paragraph here.");
        let mut timeline = Timeline::new(&plan);
        assert_eq!(timeline.known.len(), 3);
        let before = timeline.total();
        assert!(before > 0.0);
        // A measured clip replaces the estimate and refines later ones.
        timeline.learn(0, 4.0);
        assert!((timeline.seconds(0) - (4.0 - TYPICAL_TAIL + 0.3)).abs() < 1e-4);
        let (segment, offset) = timeline.locate(timeline.start_of(1) + 0.25);
        assert_eq!(segment, 1);
        assert!((offset - 0.25).abs() < 1e-4);
        let (last, _) = timeline.locate(timeline.total() + 10.0);
        assert_eq!(last, 2);
        assert_eq!(timeline.locate(0.0), (0, 0.0));
    }
}
