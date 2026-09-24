use super::{Command, PhoenixShell};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    Disableable, IconName, Sizable,
};

const BG: u32 = 0x111413;
const RAISED: u32 = 0x1b201e;
const LINE: u32 = 0x303633;
const TEXT: u32 = 0xf1f3ef;
const MUTED: u32 = 0x9ca8a2;
const ACCENT: u32 = 0x72d6b3;

fn avatar_color(name: &str) -> u32 {
    const COLORS: [u32; 8] = [
        0x536a81, 0x8a645e, 0x77639a, 0x96774f, 0x507c76, 0x886a87, 0x717c55, 0x5e728f,
    ];
    let slot = name.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(16_777_619) ^ u32::from(byte)
    });
    COLORS[slot as usize % COLORS.len()]
}

impl PhoenixShell {
    pub(in crate::shell) fn render_reader_sidebar(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let s = &self.reader.status;
        let available = self.reader.lease.is_some() && !s.finished && s.segments > 0;
        div().w_full().min_w_0().flex_1().min_h_0().flex().flex_col()
            .border_l_1().border_color(rgb(LINE)).bg(rgb(BG))
            .child(div().h(px(62.)).px_5().flex().items_center().justify_between()
                .border_b_1().border_color(rgb(LINE))
                .child(div().flex().flex_col().gap_1()
                    .child(div().text_lg().text_color(rgb(TEXT)).child("Voices"))
                    .child(div().text_xs().text_color(rgb(MUTED)).child("Find the sound of this book")))
                .child(Button::new("reader-panel-close").icon(IconName::Close).small().ghost()
                    .tooltip("Close voice panel")
                    .on_click(cx.listener(|this, _, _, cx| { this.right_open = false; cx.notify(); }))))
            .child(div().id("reader-sidebar-scroll").w_full().min_w_0().flex_1().min_h_0()
                .overflow_y_scrollbar().px_4().py_4().flex().flex_col().gap_3()
                .when(!self.reader.notice.is_empty(), |view| view.child(
                    div().w_full().flex_shrink_0().px_3().py_2().rounded_md()
                        .border_l_2().border_color(rgb(ACCENT)).bg(rgb(RAISED))
                        .text_sm().text_color(rgb(0xc9d5ce)).child(self.reader.notice.clone())))
                .when(self.reader.audition.is_some(), |view| view.child(
                    Button::new("reader-stop-sample").label("Stop sample").small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(sample) = &this.reader.audition { sample.cancel(); }
                            this.reader.audition_then_listen = false;
                            this.reader.notice = "Stopping sample…".into(); cx.notify();
                        }))))
                .when(self.reader.studio.is_none(), |view| view.child(
                    div().flex().items_center().justify_between()
                        .child(div().text_xs().text_color(rgb(MUTED)).child("YOUR VOICES"))
                        .child(div().text_xs().text_color(rgb(ACCENT)).child(format!("{} voices",
                            self.reader.voice_details.iter().filter(|(_, _, cpu)| *cpu == self.reader.cpu_voices).count())))))
                .child(div().flex().flex_wrap().gap_2()
                    .child(Button::new("voices-breeze-tab").label("Breeze").small()
                        .when(!self.reader.cpu_voices, |b| b.primary())
                        .when(self.reader.cpu_voices, |b| b.ghost())
                        .on_click(cx.listener(|this,_,_,cx| { this.reader.cpu_voices = false; cx.notify(); })))
                    .child(Button::new("voices-cpu-tab").label("Supertonic · CPU").small()
                        .when(self.reader.cpu_voices, |b| b.primary())
                        .when(!self.reader.cpu_voices, |b| b.ghost())
                        .on_click(cx.listener(|this,_,_,cx| { this.reader.cpu_voices = true; cx.notify(); }))))
                .child(Button::new("reader-studio")
                    .label(if self.reader.studio.is_some() { "Back to voices" } else { "Create a voice" })
                    .icon(if self.reader.studio.is_some() { IconName::ArrowLeft } else { IconName::Plus })
                    .small().ghost().on_click(cx.listener(|this, _, window, cx| {
                        if this.reader.studio.is_some() { this.reader.studio = None; cx.notify(); }
                        else { this.open_voice_studio(false, window, cx); }
                    })))
                .when(self.reader.studio.is_none(), |view| view.child(
                    div().w_full().min_w_0().flex_shrink_0().flex().flex_col()
                        .children(self.reader.voice_choices.iter().enumerate()
                            .filter(|(i,_)| self.reader.voice_details.get(*i)
                                .is_some_and(|(_,_,cpu)| *cpu == self.reader.cpu_voices))
                            .map(|(index, (name, choice))| {
                                let choice = *choice;
                                let selected = self.reader.selected_voice == Some(choice);
                                let (description, reference, cpu) = self.reader.voice_details.get(index)
                                    .map(|(description, reference, cpu)| (description.as_str(), *reference, *cpu))
                                    .unwrap_or(("", false, false));
                                let subtitle = if cpu { "Supertonic · local".to_owned() }
                                    else if reference { "Breeze · reference".to_owned() }
                                    else if description.is_empty() { "Breeze · voice design".to_owned() }
                                    else {
                                        let mut summary = description.chars().take(32).collect::<String>();
                                        if description.chars().nth(32).is_some() { summary.push('…'); }
                                        summary
                                    };
                                div().w_full().min_w_0().flex_shrink_0().py_2().px_2()
                                    .flex().items_center().gap_3().border_b_1().border_color(rgb(LINE))
                                    .when(selected, |row| row.bg(rgb(0x1a2721)))
                                    .child(div().size(px(38.)).flex_shrink_0().rounded_full()
                                        .bg(rgb(avatar_color(name))).flex().items_center().justify_center()
                                        .text_color(rgb(0xffffff))
                                        .child(name.chars().next().unwrap_or('•').to_uppercase().collect::<String>()))
                                    .child(div().w(px((self.reader.panel_width - 196.).max(80.)))
                                        .min_w_0().flex_shrink().flex().flex_col().gap_1()
                                        .child(div().w_full().overflow_hidden().text_sm()
                                            .text_color(rgb(if selected { ACCENT } else { TEXT }))
                                            .child(name.clone()))
                                        .child(div().w_full().overflow_hidden().text_xs()
                                            .text_color(rgb(MUTED)).child(subtitle)))
                                    .child(div().flex_shrink_0().flex().flex_col().items_end()
                                        .child(Button::new(("reader-preview", index)).label("Preview")
                                            .small().ghost()
                                            .disabled(self.reader.audition.is_some() || self.reader.retiring)
                                            .on_click(cx.listener(move |this, _, _, cx|
                                                this.preview_reader_voice(choice, cx))))
                                        .child(Button::new(("reader-voice", index))
                                            .label(if selected { "Listen" } else { "Use" }).small()
                                            .when(selected, |b| b.primary())
                                            .when(!selected, |b| b.ghost())
                                            .tooltip(if selected { "Listen with this voice" }
                                                else { "Use this voice and listen" })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Err(error) = this.persist_reader_voice(choice) {
                                                    this.reader.status.phase = super::worker::presentation::Phase::Failed;
                                                    this.reader.notice = format!("Could not save narrator: {error:#}");
                                                    cx.notify(); return;
                                                }
                                                this.invalidate_reader_document(cx);
                                                this.reader.selected_voice = Some(choice);
                                                this.reader.status.phase = super::worker::presentation::Phase::Stopped;
                                                this.reader.studio = None;
                                                this.reader.notice = "Narrator saved for this book. Character assignments stay in place.".into();
                                                this.reader_primary(cx); cx.notify();
                                            }))))
                            }))))
                .when_some(self.reader.studio.as_ref(), |view, inputs|
                    view.child(self.render_voice_studio(inputs, cx)))
                .when(self.reader.studio.is_none(), |view| view.child(
                    div().w_full().flex_shrink_0().flex().flex_col().gap_2().pt_4()
                        .border_t_1().border_color(rgb(LINE))
                        .child(div().text_xs().text_color(rgb(MUTED)).child("CHARACTER VOICES"))
                        .child(div().text_sm().text_color(rgb(0xc2cbc5)).child(if available {
                            "Give the current passage its own speaker."
                        } else { "Listen first, then cast the passage you hear." }))
                        .child(Button::new("reader-cast-passage")
                            .label(if available { "Cast current passage" } else { "Listen to choose a passage" })
                            .small().ghost().on_click(cx.listener(|this, _, window, cx| {
                                if this.reader.lease.is_some() && !this.reader.status.finished
                                    && this.reader.status.segments > 0 {
                                    this.open_voice_studio(true, window, cx);
                                } else { this.reader_primary(cx); }
                            })))))
                .when(!s.voice_name.is_empty() && available, |view| view.child(
                    div().text_xs().text_color(rgb(MUTED))
                        .child(format!("Current passage · {}", s.voice_name))))
                .child(div().h(px(1.)).bg(rgb(LINE)))
                .child(div().text_xs().text_color(rgb(MUTED)).child("LISTENING"))
                .when(available, |view| view
                    .child(Button::new("reader-return").label("Return to bookmark")
                        .icon(IconName::Star).ghost().small()
                        .on_click(cx.listener(|this, _, _, cx|
                            this.reader_command(Command::ReturnBookmark, cx))))
                    .child(Button::new("reader-stop").label("End listening session")
                        .ghost().small().on_click(cx.listener(|this, _, _, cx|
                            this.reader_command(Command::Stop, cx)))))
                .when(s.phase == super::worker::presentation::Phase::Failed, |view|
                    view.child(div().text_color(rgb(0xf7a49e)).child(s.message.clone())))
                .child(Button::new("reader-details-toggle").label("Playback details")
                    .icon(IconName::Info).ghost().small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reader.details = !this.reader.details; cx.notify();
                    })))
                .when(self.reader.details, |view| view.child(
                    div().p_3().rounded_md().bg(rgb(RAISED)).flex().flex_col().gap_2()
                        .text_sm().text_color(rgb(0xc2cbc5))
                        .child("Local Reader · provider follows each assigned voice")
                        .child(format!("{} seconds into the current passage", s.seconds))
                        .child("Long-form content qualification is still in progress.")
                        .child(s.message.clone())
                        .child(format!("{} seconds buffered · {} rebufferings · {} device gaps",
                            s.buffered_seconds, s.rebufferings, s.device_starvations))
                        .child(format!("Target {}s · synthesis RTF {:.2}", s.target_seconds, s.synthesis_rtf))
                        .child(format!("{} segments generated during playback", s.generated_during_playback))
                        .child(Button::new("reader-plain-mode")
                            .label(if self.reader.plain { "Text format: plain text" }
                                else { "Text format: Markdown" }).small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.invalidate_reader_document(cx);
                                this.reader.plain = !this.reader.plain; cx.notify();
                            })))))
                .child(Button::new("reader-back-editor").label("Return to editor")
                    .icon(IconName::ArrowLeft).small().ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.close_reader(cx)))))
    }
}
