use super::{enrollment, worker, PhoenixShell};
use gpui::{div, prelude::*, px, rgb, Context, Entity, IntoElement, PathPromptOptions, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    Disableable, Sizable,
};
use phoenix_reader_session::{VoiceChoice, VoiceLibrary, VoiceProfile};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StudioMode {
    Design,
    Clone,
}

pub(super) struct StudioInputs {
    name: Entity<InputState>,
    description: Entity<InputState>,
    transcript: Entity<InputState>,
    direction: Entity<InputState>,
    reference_path: Option<PathBuf>,
    mode: StudioMode,
    character: Entity<InputState>,
    segment: u32,
    excerpt: String,
    casting: bool,
}
impl PhoenixShell {
    pub(super) fn reader_book_key(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(self.kernel.workspace_path().to_string_lossy().as_bytes());
        hash.update(
            &self
                .editor_lease
                .as_ref()
                .map_or(0, |l| l.entry_id.0)
                .to_le_bytes(),
        );
        *hash.finalize().as_bytes()
    }
    fn reader_library(&self) -> anyhow::Result<VoiceLibrary> {
        let path = self.kernel.workspace_path().with_extension("reader.json");
        anyhow::ensure!(
            std::fs::metadata(&path)?.len() <= 1_048_576,
            "Reader configuration bounds"
        );
        let config: worker::Config = serde_json::from_slice(&std::fs::read(path)?)?;
        Ok(VoiceLibrary::open(config.storage.join("voices"))?)
    }
    pub(super) fn persist_reader_voice(&self, choice: VoiceChoice) -> anyhow::Result<()> {
        self.reader_library()?
            .select(self.reader_book_key(), choice)?;
        Ok(())
    }
    pub(super) fn open_voice_studio(
        &mut self,
        casting: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if casting {
            self.reader_command(super::Command::Pause, cx);
        }
        self.reader.notice.clear();
        let segment = self.reader.status.segment;
        let excerpt = self
            .reader
            .lease
            .as_ref()
            .map(|lease| {
                self.reader
                    .status
                    .source_ranges
                    .iter()
                    .filter_map(|r| lease.content.get(r.start as usize..r.end as usize))
                    .flat_map(str::chars)
                    .take(320)
                    .collect::<String>()
            })
            .unwrap_or_else(|| "Start listening to choose a passage.".into());
        self.reader.studio = Some(StudioInputs {
            casting,
            mode: StudioMode::Design,
            reference_path: None,
            segment,
            excerpt,
            character: cx
                .new(|cx| InputState::new(window, cx).placeholder("Character for current passage")),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Narrator name")),
            description: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Describe voice, accent and delivery")
            }),
            transcript: cx
                .new(|cx| InputState::new(window, cx).placeholder("Exact words spoken in the WAV")),
            direction: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Optional tone, emotion, pace, or delivery")
            }),
        });
        cx.notify();
    }
    fn save_designed_narrator(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let inputs = self
            .reader
            .studio
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Open Voice Studio"))?;
        let name = inputs.name.read(cx).value().trim().to_owned();
        let description = inputs.description.read(cx).value().trim().to_owned();
        anyhow::ensure!(
            !name.is_empty() && name.len() <= 128,
            "Enter a narrator name (up to 128 bytes)."
        );
        anyhow::ensure!(
            !description.is_empty() && description.len() <= 2048,
            "Describe the voice (up to 2048 bytes)."
        );
        let id =
            *blake3::hash(format!("phoenix.designed-voice/v1\0{name}\0{description}").as_bytes())
                .as_bytes();
        let profile = VoiceProfile {
            id,
            revision: 1,
            name,
            description,
            reference: None,
            default_delivery: String::new(),
            seed: 42,
        };
        let library = self.reader_library()?;
        let choice = library.save(&profile)?;
        library.select(self.reader_book_key(), choice)?;
        drop(library);
        self.reader.restart_segment = (self.reader.status.segments > 0
            && !self.reader.selection_mode)
            .then_some(self.reader.status.segment);
        self.invalidate_reader_document(cx);
        self.reader.selected_voice = Some(choice);
        self.load_reader_voice_choices()?;
        self.reader.studio = None;
        self.reader.status.phase = super::worker::presentation::Phase::Stopped;
        self.reader.notice =
            "Voice saved and selected. Preview a sample or listen to your book.".into();
        Ok(())
    }
    fn choose_reference_audio(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a 3–30 second WAV reference".into()),
        });
        cx.spawn(async move |shell, cx| {
            let chosen = prompt.await;
            let _ = shell.update(cx, |this, cx| {
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(studio) = this.reader.studio.as_mut() {
                            studio.reference_path = paths.into_iter().next();
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => this.reader.notice = "Could not open the WAV picker.".into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn start_reference_clone(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.reader.enrollment.is_none(),
            "A voice is already being enrolled"
        );
        let inputs = self
            .reader
            .studio
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Open Voice Studio"))?;
        let name = inputs.name.read(cx).value().trim().to_owned();
        let transcript = inputs.transcript.read(cx).value().trim().to_owned();
        let direction = inputs.direction.read(cx).value().trim().to_owned();
        let audio = inputs
            .reference_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Choose a WAV reference"))?;
        anyhow::ensure!(
            !name.is_empty() && name.len() <= 128,
            "Enter a voice name (up to 128 bytes)"
        );
        anyhow::ensure!(
            (10..=16_384).contains(&transcript.len()) && !transcript.contains('\0'),
            "Enter the exact spoken words (10–16,384 bytes)"
        );
        anyhow::ensure!(
            direction.len() <= 2048 && !direction.contains('\0'),
            "Direction is too long"
        );
        let config_path = self.kernel.workspace_path().with_extension("reader.json");
        anyhow::ensure!(
            std::fs::metadata(&config_path)?.len() <= 1_048_576,
            "Reader configuration bounds"
        );
        let config: worker::Config = serde_json::from_slice(&std::fs::read(config_path)?)?;
        let request = enrollment::Request {
            config,
            audio,
            name,
            transcript,
            direction,
        };
        let previous = self.reader.bridge.take();
        let audition = self.reader.audition.take();
        self.reader.lease = None;
        self.reader.painted = None;
        self.reader.status.phase = super::worker::presentation::Phase::Preparing;
        self.reader.status.requested = false;
        self.reader.status.playing = false;
        self.reader.status.finished = true;
        self.reader.enrollment = Some(enrollment::Enrollment::start(request, previous, audition));
        self.reader.notice = "Encoding reference voice… Playback position is saved; the GPU model will be released when encoding finishes.".into();
        Ok(())
    }
    pub(super) fn poll_voice_enrollment(&mut self, cx: &mut Context<Self>) {
        if !self
            .reader
            .enrollment
            .as_ref()
            .is_some_and(enrollment::Enrollment::finished)
        {
            return;
        }
        let result = self.reader.enrollment.take().unwrap().finish();
        self.reader.status.phase = super::worker::presentation::Phase::Stopped;
        match result {
            Ok((name, _choice)) => match self.load_reader_voice_choices() {
                Ok(()) => {
                    self.reader.studio = None;
                    self.reader.notice = format!(
                        "{name} added to your voices. Preview it or select Use to hear your book."
                    );
                }
                Err(error) => {
                    self.reader.notice = format!("Voice saved, but list refresh failed: {error:#}")
                }
            },
            Err(error) => self.reader.notice = format!("Voice enrollment: {error:#}"),
        }
        cx.notify();
    }
    pub(super) fn render_voice_studio(
        &self,
        inputs: &StudioInputs,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .w_full().min_w_0().flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .pt_4()
            .border_t_1()
            .border_color(rgb(0x303633))
            .when(!inputs.casting, |view| view
            .child(div().text_lg().text_color(rgb(0xf1f3ef)).child("Create a Breeze voice"))
            .child(div().flex().gap_2()
                .child(Button::new("reader-design-tab").label("Design").small()
                    .disabled(self.reader.enrollment.is_some())
                    .when(inputs.mode == StudioMode::Design, |b| b.primary())
                    .when(inputs.mode != StudioMode::Design, |b| b.ghost())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.reader.studio.as_mut() { studio.mode = StudioMode::Design; }
                        cx.notify();
                    })))
                .child(Button::new("reader-clone-tab").label("Clone recording").small()
                    .disabled(self.reader.enrollment.is_some())
                    .when(inputs.mode == StudioMode::Clone, |b| b.primary())
                    .when(inputs.mode != StudioMode::Clone, |b| b.ghost())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.reader.studio.as_mut() { studio.mode = StudioMode::Clone; }
                        cx.notify();
                    }))))
            .child(div().text_sm().text_color(rgb(0x9ca8a2)).child(
                if inputs.mode == StudioMode::Clone {
                    "Use a clean WAV and its exact spoken words. The clone is saved locally and appears in this voice list."
                } else {
                    "Describe its sound and delivery. Designs can vary between passages."
                }))
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child("VOICE NAME"))
            .child(Input::new(&inputs.name).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            )
            .when(!inputs.casting && inputs.mode == StudioMode::Design, |view| view
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child("DESCRIPTION"))
            .child(Input::new(&inputs.description).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child(div().text_xs().text_color(rgb(0x9ca8a2))
                .child("Example: Warm, low English voice; measured pace and gentle delivery."))
            .child(
                Button::new("reader-save-designed")
                    .label("Save narrator")
                    .small().primary()
                    .disabled(self.reader.enrollment.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Err(error) = this.save_designed_narrator(cx) {
                            this.reader.notice = format!("{error:#}");
                        }
                        cx.notify();
                    })),
            ))
            .when(!inputs.casting && inputs.mode == StudioMode::Clone, |view| view
            .child(Button::new("reader-choose-reference").label("Choose WAV recording…").small().ghost()
                .disabled(self.reader.enrollment.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.choose_reference_audio(cx))))
            .child(div().text_xs().text_color(rgb(0x9ca8a2)).child(
                inputs.reference_path.as_ref().map_or("No recording chosen".to_owned(), |p| p.display().to_string())))
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child("EXACT SPOKEN WORDS"))
            .child(Input::new(&inputs.transcript).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child(div().text_xs().text_color(rgb(0x9ca8a2)).child("Include punctuation. A clean 3–30 second excerpt works best."))
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child("VOICE DIRECTION · OPTIONAL"))
            .child(Input::new(&inputs.direction).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child(Button::new("reader-save-clone").label("Save reference voice").small().primary()
                .disabled(self.reader.enrollment.is_some())
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Err(error) = this.start_reference_clone(cx) {
                        this.reader.notice = format!("{error:#}");
                    }
                    cx.notify();
                })))
            .when(self.reader.enrollment.is_some(), |view| view.child(
                Button::new("reader-cancel-clone").label("Cancel encoding").small().ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(enrollment) = &this.reader.enrollment { enrollment.cancel(); }
                        this.reader.notice = "Stopping voice encoder…".into();
                        cx.notify();
                    }))))
            )
            .when(inputs.casting, |view| view
            .child(div().text_lg().text_color(rgb(0xf1f3ef)).child("Cast a passage"))
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child(format!(
                "PASSAGE {} · SAVE, THEN LISTEN", inputs.segment + 1)))
            .child(div().p_3().rounded_md().bg(rgb(0x1b201e))
                .text_sm().text_color(rgb(0xc2cbc5)).child(inputs.excerpt.clone()))
            .child(div().text_xs().text_color(rgb(0x72d6b3)).child("CHARACTER NAME"))
            .child(Input::new(&inputs.character).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child(
                div().flex().flex_wrap().gap_2().children(
                    self.reader
                        .voice_choices
                        .iter()
                        .enumerate()
                        .map(|(index, (name, choice))| {
                            let choice = *choice;
                            Button::new(("reader-assign", index))
                                .label(format!("Assign {name}"))
                                .tooltip("Assign this voice to the named character for the passage shown above")
                                .disabled(self.reader.lease.is_none() || self.reader.status.finished)
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(inputs) = &this.reader.studio {
                                        let character =
                                            inputs.character.read(cx).value().to_string();
                                        let segment = inputs.segment;
                                        this.reader.cast_restart_segment = Some(segment);
                                        this.reader.details = true;
                                        this.reader_command(
                                            super::Command::Assign {
                                                segment,
                                                character,
                                                voice: choice,
                                            },
                                            cx,
                                        );
                                    }
                                }))
                        }),
                ),
            ))
    }
}
