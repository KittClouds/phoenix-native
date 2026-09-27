//! Story timeline (4C) for [`GraphRenderer`]: the atlas replayed in reading
//! order.
//!
//! The story drives the source-local bits: objects introduced by the current
//! position are in scope, the rest ghost at source-local strength, and edges
//! follow once both ends are in. Untimed objects (no stored span) keep a
//! neutral look instead of a guessed time. Newly introduced objects bloom
//! while playing. Paused, the story draws no frames; exit restores the atlas.

use super::GraphRenderer;
use crate::{SOURCE_SCOPE_MEMBER, STORY_BLOOM_SHIFT, STORY_UNTIMED};
use graph_model::NodeId;
use std::sync::Arc;

/// Reading pace in bytes of source per second (about narration speed).
const READING_BYTES_PER_SEC: f32 = 16.0;
/// Seconds an introduced object blooms.
const BLOOM_SECS: f32 = 0.9;
/// Published positions move in steps of this fraction of the story so the
/// shell wakes a bounded number of times per playthrough.
const STATUS_STEPS: f32 = 200.0;

/// Stored first-appearance data for one verified generation.
#[derive(Clone, Debug)]
pub struct StoryData {
    /// Node id and first-appearance byte offset, sorted by offset.
    pub appearances: Arc<[(u64, u32)]>,
    pub chapters: Arc<[u32]>,
    pub end: u32,
    pub document_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryCommand {
    Seek(u32),
    Play,
    Pause,
    /// Multiple of reading pace.
    Speed(u16),
    PreviousChapter,
    NextChapter,
}

/// What the shell's story strip shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryStatus {
    pub position: u32,
    pub end: u32,
    pub playing: bool,
    pub speed: u16,
    pub chapters: Arc<[u32]>,
    pub introduced: usize,
    pub timed: usize,
    pub untimed: usize,
    pub document_revision: u64,
}

pub(crate) struct Story {
    data: StoryData,
    /// Resident slot per appearance, aligned with `data.appearances`.
    slots: Vec<Option<u32>>,
    untimed: Vec<u32>,
    position: f32,
    introduced: usize,
    playing: bool,
    speed: u16,
    /// Slot, remaining bloom (1..0) and the level last written.
    blooms: Vec<(u32, f32, u32)>,
    /// The word currently written for each node slot.
    words: Vec<u32>,
}

impl Story {
    /// Words for every slot at the current position, without blooms:
    /// introduced and untimed slots are in scope, the rest ghost.
    fn full_words(&self) -> Vec<u32> {
        let mut words = vec![0u32; self.words.len()];
        for slot in self.slots[..self.introduced].iter().flatten() {
            if let Some(word) = words.get_mut(*slot as usize) {
                *word = SOURCE_SCOPE_MEMBER;
            }
        }
        for &slot in &self.untimed {
            if let Some(word) = words.get_mut(slot as usize) {
                *word = SOURCE_SCOPE_MEMBER | STORY_UNTIMED;
            }
        }
        words
    }

    fn status(&self) -> StoryStatus {
        // Everything the strip shows moves in coarse steps, so the shell
        // wakes a bounded number of times rather than every frame.
        let step = (self.data.end as f32 / STATUS_STEPS).max(1.0);
        let position = (self.position / step).floor() * step;
        StoryStatus {
            position: position as u32,
            end: self.data.end,
            playing: self.playing,
            speed: self.speed,
            chapters: Arc::clone(&self.data.chapters),
            introduced: self.slots[..self.introduced_at(position)]
                .iter()
                .filter(|slot| slot.is_some())
                .count(),
            timed: self.slots.iter().filter(|slot| slot.is_some()).count(),
            untimed: self.untimed.len(),
            document_revision: self.data.document_revision,
        }
    }

    fn introduced_at(&self, position: f32) -> usize {
        self.data
            .appearances
            .partition_point(|&(_, at)| at as f32 <= position)
    }
}

impl GraphRenderer {
    /// Enters the story at the beginning, paused. Replaces any route walk,
    /// document flow or source-local scope; all are display-only.
    pub fn start_story(&mut self, data: StoryData) -> &crate::RouteWalkStatus {
        self.end_route_walk(None);
        self.end_document_flow();
        let state = self.scene.state();
        let slots: Vec<Option<u32>> = data
            .appearances
            .iter()
            .map(|&(id, _)| state.node_slot(NodeId(id)))
            .collect();
        let mut timed: Vec<u32> = slots.iter().flatten().copied().collect();
        timed.sort_unstable();
        let untimed: Vec<u32> = state
            .nodes_with_slots()
            .map(|(slot, _)| slot as u32)
            .filter(|slot| timed.binary_search(slot).is_err())
            .collect();
        let capacity = state.node_capacity_slots();
        self.story = Some(Story {
            data,
            slots,
            untimed,
            position: 0.0,
            introduced: 0,
            playing: false,
            speed: 1,
            blooms: Vec::new(),
            words: vec![0; capacity],
        });
        let story = self.story.as_mut().unwrap();
        story.introduced = story.introduced_at(0.0);
        let words = story.full_words();
        story.words = words.clone();
        self.scene.set_story_overlay(true, words, &self.queue);
        self.labels.mark_dirty();
        self.redraw_requested = true;
        self.publish_walk_status(None)
    }

    pub fn story_command(&mut self, command: StoryCommand) -> &crate::RouteWalkStatus {
        if let Some(story) = self.story.as_mut() {
            match command {
                StoryCommand::Seek(at) => {
                    story.position = at.min(story.data.end) as f32;
                    story.playing = false;
                }
                StoryCommand::Play => {
                    if story.position >= story.data.end as f32 {
                        story.position = 0.0;
                    }
                    story.playing = true;
                }
                StoryCommand::Pause => story.playing = false,
                StoryCommand::Speed(speed) => story.speed = speed.clamp(1, 1000),
                StoryCommand::PreviousChapter | StoryCommand::NextChapter => {
                    let here = story.position as u32;
                    let chapters = &story.data.chapters;
                    let target = if command == StoryCommand::NextChapter {
                        chapters.iter().copied().find(|&start| start > here)
                    } else {
                        // Back to this chapter's start, or the one before
                        // when already at a start.
                        chapters.iter().copied().rev().find(|&start| start + 1 < here.max(1))
                    };
                    story.position = target.unwrap_or(if command == StoryCommand::NextChapter {
                        story.data.end
                    } else {
                        0
                    }) as f32;
                }
            }
            self.settle_story(!matches!(command, StoryCommand::Play));
        }
        self.publish_walk_status(None)
    }

    /// Exits the story; scope bits return to whatever source-local shows.
    pub fn exit_story(&mut self) -> &crate::RouteWalkStatus {
        self.end_story();
        self.publish_walk_status(None)
    }

    pub(super) fn end_story(&mut self) {
        if self.story.take().is_some() {
            self.scene.set_story_overlay(false, Vec::new(), &self.queue);
            self.labels.mark_dirty();
            self.redraw_requested = true;
        }
    }

    pub(super) fn story_status(&self) -> Option<StoryStatus> {
        self.story.as_ref().map(Story::status)
    }

    pub(super) fn story_animating(&self) -> bool {
        self.story
            .as_ref()
            .is_some_and(|story| story.playing || !story.blooms.is_empty())
    }

    pub(super) fn advance_story(&mut self, elapsed: f32) {
        let Some(story) = self.story.as_mut() else {
            return;
        };
        if !story.playing && story.blooms.is_empty() {
            return;
        }
        let dt = elapsed.clamp(0.0, 0.1);
        for bloom in &mut story.blooms {
            bloom.1 -= dt / BLOOM_SECS;
        }
        if story.playing {
            story.position += READING_BYTES_PER_SEC * story.speed as f32 * dt;
            if story.position >= story.data.end as f32 {
                story.position = story.data.end as f32;
                story.playing = false;
            }
        }
        self.settle_story(false);
        self.publish_walk_status(None);
    }

    /// Recomputes what the position introduces. Moving backwards (or a jump)
    /// un-introduces immediately and clears blooms; playing forward blooms
    /// each newly introduced object.
    fn settle_story(&mut self, jump: bool) {
        let Some(story) = self.story.as_mut() else {
            return;
        };
        let next = story.introduced_at(story.position);
        let mut changes: Vec<(u32, u32)> = Vec::new();
        if jump || next < story.introduced {
            // Rare (seek, chapter step, rewind): recompute and diff all slots.
            story.blooms.clear();
            story.introduced = next;
            let words = story.full_words();
            for (slot, (&old, &new)) in story.words.iter().zip(&words).enumerate() {
                if old != new {
                    changes.push((slot as u32, new));
                }
            }
            story.words = words;
        } else {
            // Forward: only new arrivals and bloom steps change.
            for index in story.introduced..next {
                if let Some(slot) = story.slots[index] {
                    if story.playing {
                        story.blooms.push((slot, 1.0, 0));
                    }
                    let word = story.words[slot as usize] | SOURCE_SCOPE_MEMBER;
                    story.words[slot as usize] = word;
                    changes.push((slot, word));
                }
            }
            story.introduced = next;
            for bloom in &mut story.blooms {
                let level = if bloom.1 > 0.0 {
                    ((bloom.1.clamp(0.0, 1.0) * 15.0).round() as u32).max(1)
                } else {
                    0
                };
                if level != bloom.2 {
                    bloom.2 = level;
                    let slot = bloom.0 as usize;
                    let word = (story.words[slot] & !(15 << STORY_BLOOM_SHIFT))
                        | (level << STORY_BLOOM_SHIFT);
                    story.words[slot] = word;
                    changes.push((bloom.0, word));
                }
            }
            story.blooms.retain(|bloom| bloom.1 > 0.0);
        }
        changes.sort_unstable_by_key(|change| change.0);
        changes.dedup_by(|later, earlier| {
            // Keep the last write for a slot.
            if later.0 == earlier.0 {
                earlier.1 = later.1;
                true
            } else {
                false
            }
        });
        if self.scene.update_story_words(&changes, &self.queue) > 0 {
            self.redraw_requested = true;
        }
    }

    /// Re-resolves slots after the resident scene changed, keeping the
    /// position.
    pub(super) fn revalidate_story(&mut self) {
        let Some(story) = self.story.take() else {
            return;
        };
        let position = story.position;
        let playing = story.playing;
        let speed = story.speed;
        self.start_story(story.data);
        if let Some(story) = self.story.as_mut() {
            story.position = position;
            story.playing = playing;
            story.speed = speed;
        }
        self.settle_story(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(appearances: &[(u64, u32)]) -> Story {
        Story {
            data: StoryData {
                appearances: appearances.into(),
                chapters: Arc::from([0u32, 50]),
                end: 100,
                document_revision: 2,
            },
            slots: (0..appearances.len() as u32).map(Some).collect(),
            untimed: vec![9],
            position: 0.0,
            introduced: 0,
            playing: false,
            speed: 1,
            blooms: Vec::new(),
            words: vec![0; 16],
        }
    }

    #[test]
    fn position_introduces_by_first_appearance() {
        let s = story(&[(1, 0), (2, 10), (3, 10), (4, 60)]);
        assert_eq!(s.introduced_at(0.0), 1);
        assert_eq!(s.introduced_at(9.9), 1);
        assert_eq!(s.introduced_at(10.0), 3);
        assert_eq!(s.introduced_at(100.0), 4);
        let mut s = s;
        s.position = 55.0;
        s.introduced = 3;
        let status = s.status();
        assert_eq!((status.introduced, status.timed, status.untimed), (3, 4, 1));
        assert_eq!(status.end, 100);
        // Introduced and untimed slots are in scope; untimed is marked.
        let words = s.full_words();
        assert_eq!(words[0] & SOURCE_SCOPE_MEMBER, SOURCE_SCOPE_MEMBER);
        assert_eq!(words[3], 0);
        assert_eq!(words[9], SOURCE_SCOPE_MEMBER | STORY_UNTIMED);
    }
}
