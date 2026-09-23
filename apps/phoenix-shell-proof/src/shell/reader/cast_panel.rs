use super::{Command, PhoenixShell};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    Disableable, IconName, Sizable,
};

impl PhoenixShell {
    pub(in crate::shell) fn render_reader_sidebar(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let s = &self.reader.status;
        let available = self.reader.lease.is_some() && !s.finished && s.segments > 0;
        div().w_full().min_w_0().flex_1().min_h_0().flex().flex_col().bg(rgb(0x191e1b))
            .child(div().h(px(44.)).px_4().flex().items_center().justify_between()
                .border_b_1().border_color(rgb(0x303c35))
                .child(div().text_color(rgb(0x72d6b3)).child("Voices"))
                .child(Button::new("reader-panel-close").icon(IconName::Close).small().ghost().tooltip("Close voice panel")
                    .on_click(cx.listener(|this, _, _, cx| { this.right_open = false; cx.notify(); }))))
            .child(div().id("reader-sidebar-scroll").w_full().min_w_0().flex_1().min_h_0().overflow_y_scrollbar().p_4()
                .flex().flex_col().gap_4()
                .child(div().text_sm().text_color(rgb(0xc0c9c3)).child("Preview a sample, then choose a voice for your book."))
                .when(!self.reader.notice.is_empty(), |view| view.child(div().w_full().flex_shrink_0().p_3().rounded_lg().bg(rgb(0x203b30)).child(self.reader.notice.clone())))
                .when(self.reader.audition.is_some(), |view| view.child(Button::new("reader-stop-sample").label("Stop sample").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(sample) = &this.reader.audition { sample.cancel(); }
                    this.reader.audition_then_listen = false;
                    this.reader.notice = "Stopping sample…".into(); cx.notify();
                }))))
                .when(self.reader.studio.is_none(), |view| view
                .child(div().text_xs().text_color(rgb(0xaab9b1)).child("YOUR VOICES"))
                .child(div().flex().flex_wrap().gap_2()
                    .child(Button::new("voices-breeze-tab").label("Breeze · expressive").small().when(!self.reader.cpu_voices, |b|b.primary()).on_click(cx.listener(|this,_,_,cx| {this.reader.cpu_voices=false;cx.notify();})))
                    .child(Button::new("voices-cpu-tab").label("Supertonic · CPU").small().when(self.reader.cpu_voices, |b|b.primary()).on_click(cx.listener(|this,_,_,cx| {this.reader.cpu_voices=true;cx.notify();}))))
                .child(div().w_full().min_w_0().flex_shrink_0().flex().flex_col().gap_2().children(self.reader.voice_choices.iter().enumerate().filter(|(i,_)|self.reader.voice_details.get(*i).is_some_and(|(_,_,cpu)|*cpu==self.reader.cpu_voices)).map(|(index, (name, choice))| {
                    let choice = *choice;
                    let selected = self.reader.selected_voice == Some(choice);
                    let (description, reference, cpu) = self.reader.voice_details.get(index)
                        .map(|(description, reference, cpu)| (description.as_str(), *reference, *cpu)).unwrap_or(("", false, false));
                    div().w_full().min_w_0().flex_shrink_0().p_3().rounded_lg().border_1().border_color(rgb(if selected { 0x72d6b3 } else { 0x35443c }))
                        .bg(rgb(if selected { 0x203b30 } else { 0x222824 })).flex().flex_col().gap_2()
                        .child(div().text_color(rgb(0xe8e6df)).child(name.clone()))
                        .child(div().text_xs().text_color(rgb(0xaab9b1)).child(if cpu { "Supertonic 3 · CPU voice" } else if reference { "Breeze · Reference voice" } else { "Breeze · Voice design" }))
                        .child(div().text_sm().text_color(rgb(0xc0c9c3)).child(description.to_owned()))
                        .when(selected, |view| view.child(div().text_sm().text_color(rgb(0x72d6b3)).child("✓ Selected for this book")))
                        .child(div().w_full().flex().flex_wrap().gap_2()
                        .child(Button::new(("reader-preview", index)).label("Preview sample").small()
                            .disabled(self.reader.audition.is_some() || self.reader.retiring)
                            .on_click(cx.listener(move |this, _, _, cx| this.preview_reader_voice(choice, cx))))
                        .child(Button::new(("reader-voice", index))
                            .label(if selected { "Listen with this voice" } else { "Use voice & listen" })
                            .icon(IconName::User).small().primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Err(error) = this.persist_reader_voice(choice) {
                                    this.reader.status.phase = super::worker::presentation::Phase::Failed;
                                    this.reader.notice = format!("Could not save narrator: {error:#}");
                                    cx.notify();
                                    return;
                                }
                                this.invalidate_reader_document(cx);
                                this.reader.selected_voice = Some(choice);
                                this.reader.status.phase = super::worker::presentation::Phase::Stopped;
                                this.reader.studio = None;
                                this.reader.notice = "Narrator saved for this book. Character assignments stay in place.".into();
                                this.reader_primary(cx);
                                cx.notify();
                            }))))
                })))
                .child(div().text_sm().text_color(rgb(0xaab9b1)).child(if self.reader.cpu_voices { "Ten fixed voices for CPU playback. If no voices appear, the local Supertonic runtime needs configuring." } else { "Breeze designs voices from descriptions. These are your saved designs, not a downloaded speaker catalogue." })))
                .child(Button::new("reader-studio").label(if self.reader.studio.is_some() { "Back to voices" } else { "Create a voice" })
                    .icon(IconName::Plus).small().on_click(cx.listener(|this, _, window, cx| {
                        if this.reader.studio.is_some() { this.reader.studio = None; cx.notify(); }
                        else { this.open_voice_studio(false, window, cx); }
                    })))
                .when_some(self.reader.studio.as_ref(), |view, inputs| view.child(self.render_voice_studio(inputs, cx)))
                .when(self.reader.studio.is_none(), |view| view.child(div().w_full().flex_shrink_0().flex().flex_col().gap_2()
                    .child("Character voices")
                    .child(div().text_sm().text_color(rgb(0xaab9b1)).child(if available { "Give the current passage its own speaker. Opening this pauses playback." } else { "Start listening to your book, then cast the passage you hear." }))
                    .child(Button::new("reader-cast-passage").label(if available { "Cast current passage" } else { "Listen to choose a passage" }).small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.reader.lease.is_some() && !this.reader.status.finished && this.reader.status.segments > 0 { this.open_voice_studio(true, window, cx); }
                            else { this.reader_primary(cx); }
                        })))))
                .when(!s.voice_name.is_empty() && available, |view| view.child(div().text_sm().text_color(rgb(0xaab9b1)).child(format!("Current passage: {}", s.voice_name))))
                .child(div().h(px(1.)).bg(rgb(0x303c35)))
                .child(div().text_xs().text_color(rgb(0xaab9b1)).child("LISTENING"))
                .when(available, |view| view.child(Button::new("reader-return").label("Return to bookmark").icon(IconName::Star).ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.reader_command(Command::ReturnBookmark, cx))))
                .child(Button::new("reader-stop").label("End listening session").ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.reader_command(Command::Stop, cx)))))
                .when(s.phase == super::worker::presentation::Phase::Failed, |view| view.child(div().text_color(rgb(0xf7a49e)).child(s.message.clone())))
                .child(Button::new("reader-details-toggle").label("Playback details").icon(IconName::Info).ghost().small()
                    .on_click(cx.listener(|this, _, _, cx| { this.reader.details = !this.reader.details; cx.notify(); })))
                .when(self.reader.details, |view| view.child(div().p_3().rounded_lg().bg(rgb(0x222824)).flex().flex_col().gap_2().text_sm()
                    .child("Local Reader · provider follows each assigned voice")
                            .child(format!("{} seconds into the current passage", s.seconds))
                    .child("Long-form content qualification is still in progress.")
                    .child(s.message.clone())
                    .child(format!("{} seconds buffered · {} rebufferings · {} device gaps", s.buffered_seconds, s.rebufferings, s.device_starvations))
                    .child(format!("{} segments generated during playback", s.generated_during_playback))
                    .child(Button::new("reader-plain-mode").label(if self.reader.plain { "Text format: plain text" } else { "Text format: Markdown" }).small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.invalidate_reader_document(cx);
                            this.reader.plain = !this.reader.plain;
                            cx.notify();
                        })))))
                .child(Button::new("reader-back-editor").label("Return to editor").icon(IconName::ArrowLeft).small().ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.close_reader(cx)))))
    }
}
