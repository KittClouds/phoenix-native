//! Story timeline strip (4C). The graph thread owns the story position and
//! playback; the shell sends commands and renders the status it publishes.

use super::{drawer::ACCENT, PhoenixShell, TEXT, TEXT_MUTED};
use crate::graph_window::RouteWalkRequest;
use gpui::{
    canvas, div, prelude::*, px, relative, rgb, Bounds, Context, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable};
use graph_render_wgpu::{StoryCommand, StoryStatus};
use std::{cell::Cell, rc::Rc};

const STRIP_BG: u32 = 0x0f1720;
const TRACK: u32 = 0x2a3a38;
const TICK: u32 = 0x5f7a73;
/// Playback speeds as multiples of reading pace.
const SPEEDS: [(u16, &str); 4] = [(1, "1\u{d7}"), (10, "10\u{d7}"), (60, "60\u{d7}"), (300, "300\u{d7}")];

/// Shell-side scrub state; the position itself lives on the graph thread.
#[derive(Default)]
pub(super) struct StoryStrip {
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    drag: Option<f32>,
    /// Drive the story from the Reader's spoken position (4B).
    pub(super) follow_reader: bool,
    pub(super) follow_sent: Option<u32>,
}

impl PhoenixShell {
    pub(super) fn send_story(&mut self, command: StoryCommand, cx: &mut Context<Self>) {
        self.send_route_walk(RouteWalkRequest::Story(command), cx);
    }

    fn story_fraction(&self, x: Pixels) -> Option<f32> {
        let bounds = self.story_strip.bounds.get()?;
        let width = f32::from(bounds.size.width);
        (width > 0.0).then(|| (f32::from(x - bounds.left()) / width).clamp(0.0, 1.0))
    }

    fn commit_story_scrub(&mut self, end: u32, cx: &mut Context<Self>) {
        if let Some(fraction) = self.story_strip.drag.take() {
            self.send_story(StoryCommand::Seek((fraction * end as f32) as u32), cx);
        }
        cx.notify();
    }

    /// Follow Reader: seek the story to the start of the passage being
    /// spoken, only when the Reader reads the graph's own revision.
    pub(in crate::shell) fn sync_story_follow(&mut self, spoken_start: Option<u32>, cx: &mut Context<Self>) {
        if !self.story_strip.follow_reader {
            return;
        }
        let Some(start) = spoken_start else {
            return;
        };
        if self.story_strip.follow_sent == Some(start) {
            return;
        }
        self.story_strip.follow_sent = Some(start);
        self.send_story(StoryCommand::Seek(start), cx);
    }

    pub(super) fn render_story_strip(
        &self,
        status: &StoryStatus,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let end = status.end.max(1);
        let played = status.position as f32 / end as f32;
        let progress = self.story_strip.drag.unwrap_or(played).clamp(0.0, 1.0);
        let chapter = status
            .chapters
            .iter()
            .filter(|&&start| start <= (progress * end as f32) as u32)
            .count()
            .max(1);
        let bounds = self.story_strip.bounds.clone();
        let track = div()
            .id("story-scrubber")
            .flex_1()
            .min_w(px(160.))
            .h(px(16.))
            .relative()
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(rgb(TRACK))
                    .child(
                        div()
                            .h_full()
                            .w(relative(progress))
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    ),
            )
            .children(status.chapters.iter().skip(1).map(|&start| {
                div()
                    .absolute()
                    .left(relative(start as f32 / end as f32))
                    .top(px(3.))
                    .w(px(1.))
                    .h(px(10.))
                    .bg(rgb(TICK))
            }))
            .child(
                div()
                    .absolute()
                    .left(relative(progress))
                    .ml(px(-5.))
                    .size(px(10.))
                    .rounded_full()
                    .bg(rgb(TEXT)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.story_strip.follow_reader = false;
                    this.story_strip.drag = this.story_fraction(event.position.x);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.story_strip.drag.is_some() && event.pressed_button == Some(MouseButton::Left)
                {
                    this.story_strip.drag = this.story_fraction(event.position.x);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| this.commit_story_scrub(end, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| this.commit_story_scrub(end, cx)),
            );
        let playing = status.playing;
        super::graph_toolbar::mode_strip(STRIP_BG)
            .child(super::graph_toolbar::strip_mark("Story", ACCENT))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child(format!("rev {}", status.document_revision)),
            )
            .child(
                Button::new("story-previous-chapter")
                    .label("\u{2039}")
                    .small()
                    .ghost()
                    .tooltip("Previous chapter")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_story(StoryCommand::PreviousChapter, cx)
                    })),
            )
            .child(
                Button::new("story-play")
                    .label(if playing { "Pause" } else { "Play" })
                    .small()
                    .ghost()
                    .selected(playing)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.story_strip.follow_reader = false;
                        this.send_story(
                            if playing {
                                StoryCommand::Pause
                            } else {
                                StoryCommand::Play
                            },
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("story-next-chapter")
                    .label("\u{203a}")
                    .small()
                    .ghost()
                    .tooltip("Next chapter")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_story(StoryCommand::NextChapter, cx)
                    })),
            )
            .child(track)
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT))
                    .flex_shrink_0()
                    .child(format!(
                        "Ch {chapter}/{} \u{b7} {}%",
                        status.chapters.len().max(1),
                        (progress * 100.0).round()
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .flex_shrink_0()
                    .child(format!(
                        "{}/{} \u{b7} {} untimed",
                        status.introduced, status.timed, status.untimed
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .children(SPEEDS.iter().map(|&(speed, label)| {
                        Button::new(("story-speed", speed as usize))
                            .label(label)
                            .xsmall()
                            .ghost()
                            .selected(status.speed == speed)
                            .tooltip("Playback speed, as a multiple of reading pace")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send_story(StoryCommand::Speed(speed), cx)
                            }))
                    })),
            )
            .child(
                Button::new("story-follow-reader")
                    .label("Follow")
                    .small()
                    .ghost()
                    .selected(self.story_strip.follow_reader)
                    .tooltip("Drive the story from the passage the Reader is speaking")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.story_strip.follow_reader = !this.story_strip.follow_reader;
                        this.story_strip.follow_sent = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("story-exit")
                    .label("Exit")
                    .tooltip("Leave the story. The atlas returns to its prior view.")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.story_strip.follow_reader = false;
                        this.send_route_walk(RouteWalkRequest::StoryExit, cx);
                    })),
            )
    }
}
