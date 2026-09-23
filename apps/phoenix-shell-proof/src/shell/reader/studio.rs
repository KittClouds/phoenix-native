use super::{worker, PhoenixShell};
use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Window};
use gpui_component::{
    button::Button,
    input::{Input, InputState},
    Disableable, Sizable,
};
use phoenix_reader_session::{VoiceChoice, VoiceLibrary, VoiceProfile};

pub(super) struct StudioInputs {
    name: Entity<InputState>,
    description: Entity<InputState>,
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
            segment,
            excerpt,
            character: cx
                .new(|cx| InputState::new(window, cx).placeholder("Character for current passage")),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Narrator name")),
            description: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Describe voice, accent and delivery")
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
        self.invalidate_reader_document(cx);
        self.reader.selected_voice = Some(choice);
        self.load_reader_voice_choices()?;
        self.reader.studio = None;
        self.reader.status.phase = super::worker::presentation::Phase::Stopped;
        self.reader.notice =
            "Voice saved and selected. Preview a sample or listen to your book.".into();
        Ok(())
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
            .gap_2()
            .p_3()
            .rounded_lg()
            .bg(gpui::rgb(0x222824))
            .when(!inputs.casting, |view| view
            .child("Create a Breeze voice")
            .child("Describe how it should sound. This creates a voice design; it does not clone a speaker.")
            .child("Voice name")
            .child(Input::new(&inputs.name).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child("Voice description")
            .child(Input::new(&inputs.description).w(px((self.reader.panel_width - 64.).max(100.))).h(px(36.)).flex_shrink_0())
            .child(div().text_sm().child("Example: Warm, low-pitched English voice; measured pace and gentle delivery."))
            .child(
                Button::new("reader-save-designed")
                    .label("Save narrator")
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Err(error) = this.save_designed_narrator(cx) {
                            this.reader.notice = format!("{error:#}");
                        }
                        cx.notify();
                    })),
            ))
            .when(inputs.casting, |view| view
            .child("Cast a passage")
            .child(format!(
                "Whole passage {} · save, then Listen to apply",
                inputs.segment + 1
            ))
            .child(inputs.excerpt.clone())
            .child("Character name")
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
