use super::{worker::presentation::Phase, Command, PhoenixShell};
use gpui::{
    canvas, div, prelude::*, px, relative, rgb, Bounds, Context, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    Disableable, Icon, IconName, Selectable, Sizable,
};

/// `m:ss`, or `h:mm:ss` from an hour.
fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn track_fraction(bounds: Option<Bounds<Pixels>>, position: Point<Pixels>) -> Option<f32> {
    let bounds = bounds?;
    let width = f32::from(bounds.size.width);
    (width > 0.0).then(|| (f32::from(position.x - bounds.left()) / width).clamp(0.0, 1.0))
}

impl PhoenixShell {
    pub(in crate::shell) fn reader_primary(&mut self, cx: &mut Context<Self>) {
        if self.reader.selection_mode {
            if self.reader.status.phase == Phase::Preparing {
                self.reader_command(Command::Stop, cx);
                return;
            }
            if self.reader.status.requested {
                self.reader_command(Command::Pause, cx);
                return;
            }
            if self.reader.status.phase != Phase::Completed && !self.reader.status.finished {
                self.reader_command(Command::Play, cx);
                return;
            }
            self.reader.selection_request = self.reader.selection_request.wrapping_add(1);
            self.reader.selection_mode = false;
            self.reader.lease = None;
            self.reader.status.phase = Phase::Stopped;
            self.reader.status.finished = true;
        }
        if let Some(audition) = &self.reader.audition {
            audition.cancel();
            self.reader.audition_then_listen = true;
            self.reader.notice = "Starting your book after the sample stops…".into();
            cx.notify();
            return;
        }
        if self.reader.retiring {
            self.reader.pending_listen = false;
            self.reader.status.phase = Phase::Stopped;
            cx.notify();
            return;
        }
        if self.reader.status.phase == Phase::Preparing {
            self.reader_command(Command::Stop, cx);
            return;
        }
        if self.editor.read(cx).is_dirty() {
            self.on_editor_event(
                self.editor.clone(),
                &velotype::EditorEvent::SaveRequested,
                cx,
            );
            if self.editor.read(cx).is_dirty() {
                self.reader.status.phase = Phase::Failed;
                self.reader.status.message =
                    "The document could not be saved. Resolve the save error before listening."
                        .into();
                return;
            }
        }
        if self.reader.lease.is_none() || self.reader.status.finished {
            // Retire the owned controller before opening its exclusive stores again.
            if let Some(bridge) = self.reader.bridge.take() {
                self.reader.retiring = true;
                self.reader.pending_listen = true;
                self.reader.status.phase = Phase::Preparing;
                let revision = self.editor.read(cx).document_revision();
                let retired = cx.background_executor().spawn(async move {
                    bridge.shutdown();
                });
                cx.spawn(async move |shell, cx| {
                    retired.await;
                    let _ = shell.update(cx, |this, cx| {
                        this.reader.retiring = false;
                        let wanted = std::mem::take(&mut this.reader.pending_listen);
                        if wanted
                            && this.editor.read(cx).document_revision() == revision
                            && !this.editor.read(cx).is_dirty()
                        {
                            this.start_reader_document(this.reader.plain, cx);
                            this.reader_command(Command::Play, cx);
                        } else if wanted {
                            this.reader.status.phase = Phase::Changed;
                        }
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
                return;
            }
            self.start_reader_document(self.reader.plain, cx);
        } else if self.reader.status.requested {
            self.reader_command(Command::Pause, cx);
            return;
        } else if self.reader.status.phase == Phase::Completed {
            self.reader_command(Command::Next, cx);
        }
        self.reader_command(Command::Play, cx);
    }

    pub(super) fn show_reader_sidebar(&mut self, details: bool, cx: &mut Context<Self>) {
        self.right_sidebar_width = self.right_sidebar_width.max(440.);
        self.reader.sidebar = true;
        self.reader.settings = false;
        self.reader.details = details;
        self.right_open = true;
        cx.notify();
    }

    pub(super) fn show_reader_settings(&mut self, cx: &mut Context<Self>) {
        if self.right_open && self.reader.settings {
            self.right_open = false;
        } else {
            self.right_sidebar_width = self.right_sidebar_width.max(440.);
            self.reader.sidebar = true;
            self.reader.settings = true;
            self.right_open = true;
        }
        cx.notify();
    }

    /// Sends the dragged scrubber position as a seek.
    fn commit_scrub(&mut self, cx: &mut Context<Self>) {
        if let Some(fraction) = self.reader.scrub_drag.take() {
            let target = (fraction as f64 * self.reader.status.total_ms as f64) as u64;
            self.reader_command(Command::SeekMs(target), cx);
        }
        cx.notify();
    }

    fn skip_reader(&mut self, seconds: i64, cx: &mut Context<Self>) {
        let s = &self.reader.status;
        let target = (s.elapsed_ms as i64 + seconds * 1000).clamp(0, s.total_ms as i64);
        self.reader_command(Command::SeekMs(target as u64), cx);
    }

    pub(in crate::shell) fn render_reader(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.reader.status;
        let phase = s.phase;
        let available = self.reader.lease.is_some()
            && !self.reader.selection_mode
            && !s.finished
            && s.segments > 0;
        let location = if s.segments == 0 {
            if self.reader.selection_mode {
                "SELECTED TEXT · PREPARING".to_owned()
            } else {
                "LOCAL VOICE · PREVIEW".to_owned()
            }
        } else if self.reader.selection_mode {
            format!("SELECTED TEXT · PASSAGE {} / {}", s.segment + 1, s.segments)
        } else {
            format!(
                "CHAPTER {} / {} · PASSAGE {} / {}",
                s.chapter + 1,
                s.chapters,
                s.segment + 1,
                s.segments
            )
        };
        // The right panel (Reader voices, settings or the inspector) narrows the dock.
        let narrow_dock = self.right_open;
        let location = if self.reader.glow_stale {
            format!("{location} \u{b7} Graph is behind the note \u{b7} glow paused")
        } else {
            location
        };
        let voice = if s.voice_name.is_empty() || self.reader.lease.is_none() {
            self.reader
                .voice_choices
                .iter()
                .find(|(_, choice)| Some(*choice) == self.reader.selected_voice)
                .map(|(name, _)| name.as_str())
                .unwrap_or("Choose a voice")
        } else {
            &s.voice_name
        };
        let timed = available && s.total_ms > 0;
        let played = if timed {
            (s.elapsed_ms as f32 / s.total_ms as f32).clamp(0., 1.)
        } else if s.segments == 0 {
            0.
        } else {
            ((s.segment + 1) as f32 / s.segments as f32).clamp(0., 1.)
        };
        // While dragging, the thumb and time follow the pointer.
        let progress = self.reader.scrub_drag.filter(|_| timed).unwrap_or(played);
        let shown_ms = (progress as f64 * s.total_ms as f64) as u64;
        let scrub_bounds = self.reader.scrub_bounds.clone();
        div()
            .id("reader-dock")
            .h(px(92.))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .pt_2()
            .pb_2()
            .border_t_1()
            .border_color(rgb(0x303633))
            .bg(rgb(0x101312))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .text_xs()
                    .text_color(rgb(0x9ca8a2))
                    .child(
                        div()
                            .w(px(52.))
                            .flex_shrink_0()
                            .child(if timed { clock(shown_ms) } else { String::new() }),
                    )
                    .child(
                        div()
                            .id("reader-scrubber")
                            .flex_1()
                            .min_w_0()
                            .h(px(14.))
                            .relative()
                            .flex()
                            .items_center()
                            .when(timed, |track| track.cursor_pointer())
                            .child(
                                canvas(
                                    move |bounds, _, _| scrub_bounds.set(Some(bounds)),
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(
                                div()
                                    .h(px(3.))
                                    .w_full()
                                    .rounded_full()
                                    .bg(rgb(0x303633))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(progress))
                                            .rounded_full()
                                            .bg(rgb(0xe7eee9)),
                                    ),
                            )
                            .when(timed, |track| {
                                track
                                    .child(
                                        div()
                                            .absolute()
                                            .left(relative(progress))
                                            .ml(px(-5.))
                                            .size(px(10.))
                                            .rounded_full()
                                            .bg(rgb(0xe7eee9)),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                            this.reader.scrub_drag = track_fraction(
                                                this.reader.scrub_bounds.get(),
                                                event.position,
                                            );
                                            cx.notify();
                                        }),
                                    )
                                    .on_mouse_move(cx.listener(
                                        |this, event: &MouseMoveEvent, _, cx| {
                                            if this.reader.scrub_drag.is_some()
                                                && event.pressed_button == Some(MouseButton::Left)
                                            {
                                                this.reader.scrub_drag = track_fraction(
                                                    this.reader.scrub_bounds.get(),
                                                    event.position,
                                                );
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                            this.commit_scrub(cx)
                                        }),
                                    )
                                    .on_mouse_up_out(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                            this.commit_scrub(cx)
                                        }),
                                    )
                            }),
                    )
                    .child(
                        div()
                            .w(px(52.))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(if timed {
                                format!("-{}", clock(s.total_ms.saturating_sub(shown_ms)))
                            } else {
                                String::new()
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex_basis(px(0.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x72d6b3))
                                    .truncate()
                                    .child(phase.headline()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x9ca8a2))
                                    .truncate()
                                    .child(location),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                Button::new("reader-prev")
                                    .icon(IconName::ChevronLeft)
                                    .ghost()
                                    .tooltip("Previous chapter")
                                    .disabled(!available)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reader_command(Command::Previous, cx)
                                    })),
                            )
                            .child(
                                Button::new("reader-back-15")
                                    .icon(Icon::empty().path("icons/rotate-ccw.svg"))
                                    .when(!narrow_dock, |button| button.label("15"))
                                    .ghost()
                                    .small()
                                    .tooltip("Back 15 seconds")
                                    .disabled(!timed)
                                    .on_click(cx.listener(|this, _, _, cx| this.skip_reader(-15, cx))),
                            )
                            .child(
                                Button::new("reader-play")
                                    .label(
                                        if self.reader.selection_mode
                                            && (phase == Phase::Completed || s.finished)
                                        {
                                            "Read note"
                                        } else {
                                            phase.primary(
                                                s.requested,
                                                self.editor.read(cx).is_dirty(),
                                            )
                                        },
                                    )
                                    .primary()
                                    .rounded(px(24.))
                                    .min_w(px(92.))
                                    .h(px(44.))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reader_primary(cx)),
                                    ),
                            )
                            .child(
                                Button::new("reader-forward-15")
                                    .icon(Icon::empty().path("icons/rotate-cw.svg"))
                                    .when(!narrow_dock, |button| button.label("15"))
                                    .ghost()
                                    .small()
                                    .tooltip("Forward 15 seconds")
                                    .disabled(!timed)
                                    .on_click(cx.listener(|this, _, _, cx| this.skip_reader(15, cx))),
                            )
                            .child(
                                Button::new("reader-next")
                                    .icon(IconName::ChevronRight)
                                    .ghost()
                                    .tooltip("Next chapter")
                                    .disabled(!available)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reader_command(Command::Next, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .flex_1()
                            .flex_basis(px(0.))
                            .justify_end()
                            // Clip rather than spill over the play controls.
                            .overflow_hidden()
                            // A side panel narrows the dock; the voice picker
                            // opens the same panel, so CAST steps aside.
                            .when(!narrow_dock, |row| {
                                row.child(
                                    Button::new("reader-cast-badge")
                                        .label("CAST")
                                        .small()
                                        .ghost()
                                        .tooltip("Open voices and passage casting")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_reader_sidebar(false, cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("reader-glow-follow")
                                    .icon(IconName::Eye)
                                    .ghost()
                                    .selected(self.reader.glow_follow)
                                    .tooltip(if self.reader.glow_follow {
                                        "Atlas follows the passage · click to stop"
                                    } else {
                                        "Follow the passage on the atlas"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reader.glow_follow = !this.reader.glow_follow;
                                        this.sync_reader_glow();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("reader-voice-picker")
                                    .icon(IconName::User)
                                    .when(!narrow_dock, |button| button.label(voice.to_owned()))
                                    .small()
                                    .ghost()
                                    .max_w(px(150.))
                                    .overflow_hidden()
                                    .tooltip("Voices and passage casting")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_reader_sidebar(false, cx)
                                    })),
                            )
                            .child(
                                Button::new("reader-bookmark")
                                    .icon(IconName::Star)
                                    .ghost()
                                    .tooltip("Save listening bookmark")
                                    .disabled(!available)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reader_command(Command::Bookmark, cx)
                                    })),
                            )
                            .child(
                                Button::new("reader-details")
                                    .icon(IconName::Ellipsis)
                                    .ghost()
                                    .tooltip("Playback details and options")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.show_reader_settings(cx)),
                                    ),
                            ),
                    ),
            )
    }
}
