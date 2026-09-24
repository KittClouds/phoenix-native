use super::{worker::presentation::Phase, Command, PhoenixShell};
use gpui::{div, prelude::*, px, relative, rgb, Context, IntoElement};
use gpui_component::{
    button::{Button, ButtonVariants},
    Disableable, IconName, Sizable,
};

impl PhoenixShell {
    pub(in crate::shell) fn reader_primary(&mut self, cx: &mut Context<Self>) {
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
        self.reader.details = details;
        self.right_open = true;
        cx.notify();
    }

    pub(in crate::shell) fn render_reader(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.reader.status;
        let phase = s.phase;
        let available = self.reader.lease.is_some() && !s.finished && s.segments > 0;
        let location = if s.segments == 0 {
            "LOCAL VOICE · PREVIEW".to_owned()
        } else {
            format!(
                "CHAPTER {} / {} · PASSAGE {} / {}",
                s.chapter + 1,
                s.chapters,
                s.segment + 1,
                s.segments
            )
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
        let progress = if s.segments == 0 {
            0.
        } else {
            ((s.segment + 1) as f32 / s.segments as f32).clamp(0., 1.)
        };
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
                                Button::new("reader-play")
                                    .label(
                                        phase.primary(s.requested, self.editor.read(cx).is_dirty()),
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
                            .flex_shrink_0()
                            .child(
                                Button::new("reader-voice-picker")
                                    .icon(IconName::User)
                                    .label(voice.to_owned())
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_reader_sidebar(true, cx)
                                    })),
                            ),
                    ),
            )
    }
}
