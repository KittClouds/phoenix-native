//! Reader glow for [`GraphRenderer`]: while the Reader narrates, the objects
//! bound to the spoken segment glow and recent segments fade behind it.
//!
//! The glow only adds light; it never dims or hides anything. It has its own
//! overlay bits, so it composes under the route walk and document flow (which
//! take precedence) and above source-local ghosting.
//!
//! The trail steps once per segment instead of animating over time, so the
//! glow costs one redraw per segment change; only the optional Follow camera
//! eases across frames.

use super::GraphRenderer;
use crate::{READER_GLOW, READER_INTENSITY_SHIFT};
use graph_model::NodeId;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Segments kept in the fading trail, including the current one.
const TRAIL_SEGMENTS: usize = 4;
/// Intensity for the current segment (full bloom), then each older one.
const TRAIL_INTENSITY: [f32; TRAIL_SEGMENTS] = [1.0, 0.5, 0.26, 0.12];

/// One Reader segment resolved to graph objects by stored bindings.
#[derive(Clone, Debug)]
pub struct ReaderGlowFrame {
    /// Reader segment index; consecutive segments extend the trail and any
    /// jump clears it.
    pub segment: u32,
    /// Sorted node ids bound to the spoken ranges.
    pub members: Arc<[u64]>,
    /// Passage node ids, used as the Follow target.
    pub passages: Arc<[u64]>,
    /// False while the Reader is paused: the glow freezes.
    pub playing: bool,
    pub follow: bool,
    /// When the shell observed the segment, for the latency receipt.
    pub observed_at: Instant,
}

#[derive(Default)]
pub(crate) struct ReaderGlow {
    trail: VecDeque<(u32, Arc<[u64]>)>,
    playing: bool,
    follow_goal: Option<[f32; 3]>,
    pending_latency: Option<(u32, Instant)>,
}

impl GraphRenderer {
    /// Installs the Reader's current segment, or clears the glow exactly when
    /// the Reader stops (`None`).
    pub fn set_reader_glow(&mut self, frame: Option<ReaderGlowFrame>) {
        let Some(frame) = frame else {
            if !self.reader_glow.trail.is_empty() {
                self.reader_glow = ReaderGlow::default();
                self.refresh_reader_glow();
            }
            return;
        };
        let goal = if frame.follow {
            self.passage_centroid(&frame.passages)
        } else {
            None
        };
        let glow = &mut self.reader_glow;
        glow.playing = frame.playing;
        let current = glow.trail.front().map(|(segment, _)| *segment);
        if current != Some(frame.segment) {
            if current.is_none_or(|segment| segment.wrapping_add(1) != frame.segment) {
                // A jump (Next, Previous, bookmark) starts a fresh trail.
                glow.trail.clear();
            }
            glow.trail.push_front((frame.segment, frame.members));
            glow.trail.truncate(TRAIL_SEGMENTS);
            glow.pending_latency = Some((frame.segment, frame.observed_at));
            glow.follow_goal = goal;
        } else if !frame.follow {
            glow.follow_goal = None;
        }
        self.refresh_reader_glow();
    }

    /// The segment and delay from Reader observation to the first presented
    /// frame that shows it; taken once per segment.
    pub fn take_reader_glow_latency(&mut self) -> Option<(u32, Duration)> {
        self.reader_glow
            .pending_latency
            .take()
            .map(|(segment, at)| (segment, at.elapsed()))
    }

    pub(super) fn reader_glow_animating(&self) -> bool {
        let glow = &self.reader_glow;
        glow.playing && glow.follow_goal.is_some()
    }

    pub(super) fn advance_reader_glow(&mut self, elapsed: f32) {
        if !self.reader_glow_animating() {
            return;
        }
        let dt = elapsed.clamp(0.0, 0.1);
        if let Some(goal) = self.reader_glow.follow_goal {
            let amount = 1.0 - (-dt * 7.0).exp();
            if self.camera.ease_focus(goal, amount) {
                self.reader_glow.follow_goal = None;
            }
            self.write_camera();
            self.picking.invalidate();
            self.redraw_requested = true;
        }
    }

    fn passage_centroid(&self, passages: &[u64]) -> Option<[f32; 3]> {
        let state = self.scene.state();
        let mut sum = [0.0f32; 3];
        let mut count = 0.0f32;
        for &id in passages {
            if let Some(node) = state.node_slot(NodeId(id)).and_then(|slot| state.node_at_slot(slot)) {
                for axis in 0..3 {
                    sum[axis] += node.position[axis];
                }
                count += 1.0;
            }
        }
        (count > 0.0).then(|| sum.map(|value| value / count))
    }

    pub(super) fn refresh_reader_glow(&mut self) {
        let glow = &self.reader_glow;
        let state = self.scene.state();
        let mut intensity: Vec<(u32, f32)> = Vec::new();
        for (age, (_, members)) in glow.trail.iter().enumerate() {
            let level = TRAIL_INTENSITY[age];
            for &id in members.iter() {
                if let Some(slot) = state.node_slot(NodeId(id)) {
                    intensity.push((slot, level));
                }
            }
        }
        intensity.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
        intensity.dedup_by_key(|entry| entry.0);
        let entries = intensity
            .into_iter()
            .map(|(slot, level)| {
                let byte = (level.clamp(0.0, 1.0) * 255.0).round() as u32;
                (slot, READER_GLOW | (byte.max(1) << READER_INTENSITY_SHIFT))
            })
            .collect();
        self.scene.set_reader_overlay(entries, &self.queue);
        self.redraw_requested = true;
    }
}
