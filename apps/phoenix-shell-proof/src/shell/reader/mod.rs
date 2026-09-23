mod worker;
use super::PhoenixShell;
use gpui::{Context, Task, Timer};
use std::time::Duration;
use worker::{Bridge, Command, Status};

#[derive(Default)]
pub(super) struct ReaderPanel {
    pub(in crate::shell) panel_width: f32,
    pub open: bool,
    pub(super) sidebar: bool,
    previous_right_open: bool,
    details: bool,
    plain: bool,
    retiring: bool,
    pending_listen: bool,
    bridge: Option<Bridge>,
    status: Status,
    task: Option<Task<()>>,
    lease: Option<std::sync::Arc<phoenix_workspace::DocumentLease>>,
    editor_revision: u64,
    painted: Option<u32>,
    voice_choices: Vec<(String, phoenix_reader_session::VoiceChoice)>,
    selected_voice: Option<phoenix_reader_session::VoiceChoice>,
    studio: Option<studio::StudioInputs>,
    voice_book: Option<[u8; 32]>,
    voice_details: Vec<(String, bool, bool)>,
    cpu_voices: bool,
    notice: String,
    audition: Option<audition::Audition>,
    audition_then_listen: bool,
}
impl PhoenixShell {
    pub(super) fn take_reader_for_shutdown(&mut self) -> Option<Bridge> {
        let bridge = self.reader.bridge.take();
        if let Some(b) = &bridge {
            b.send(Command::Stop);
        }
        bridge
    }
    pub(super) fn open_reader(&mut self, cx: &mut Context<Self>) {
        if let Some(mut geometry) = self.graph_geometry.get() {
            geometry.visible = false;
            self.graph_geometry.set(Some(geometry));
        }
        if let Some(graph) = self.graph.borrow_mut().as_mut() {
            if let Err(error) = graph.hide_viewport() {
                self.status = format!("Reader viewport: {error:#}").into();
            }
        }
        if !self.reader.open {
            self.reader.previous_right_open = self.right_open;
            self.right_open = false;
            self.reader.sidebar = true;
        }
        self.reader.open = true;
        self.editor.update(cx, |editor, cx| {
            editor.set_selection_toolbar_requires_selection(true, cx)
        });
        if let Err(error) = self.load_reader_voice_choices() {
            self.reader.status.phase = worker::presentation::Phase::Failed;
            self.reader.status.message = format!("Voice library: {error:#}");
        }
        if self.reader.task.is_none() {
            self.reader.task = Some(cx.spawn(async move |shell, cx| loop {
                Timer::after(Duration::from_millis(100)).await;
                if shell
                    .update(cx, |this, cx| {
                        this.poll_voice_audition(cx);
                        if let Some(bridge) = &this.reader.bridge {
                            let status = bridge.status.lock().unwrap().clone();
                            if status == this.reader.status {
                                return;
                            }
                            let message = this.reader.status.message.clone();
                            let phase = this.reader.status.phase;
                            this.reader.status = status;
                            if this.reader.status.finished
                                && this.reader.status.message.starts_with("Cast saved")
                            {
                                this.reader.notice = this.reader.status.message.clone();
                                this.reader.studio = None;
                            }
                            if this.reader.lease.is_none() {
                                this.reader.status.message = message;
                                this.reader.status.phase = phase;
                            }
                            this.refresh_reader_highlight(cx);
                            if this.reader.open {
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }));
        }
        cx.notify();
    }
    pub(super) fn close_reader(&mut self, cx: &mut Context<Self>) {
        self.reader.open = false;
        self.editor.update(cx, |editor, cx| {
            editor.set_selection_toolbar_requires_selection(false, cx)
        });
        self.reader.sidebar = false;
        self.right_open = self.reader.previous_right_open;
        cx.notify();
    }
    fn start_reader_document(&mut self, plain: bool, cx: &mut Context<Self>) {
        if self
            .reader
            .bridge
            .as_ref()
            .is_some_and(|b| !b.status.lock().unwrap().finished)
        {
            self.reader.status.message =
                "Stop the current Reader before loading another saved revision.".into();
            cx.notify();
            return;
        }
        if self.editor.read(cx).is_dirty() {
            self.reader.status.message = "Save the edited note before starting narration.".into();
            cx.notify();
            return;
        }
        let Some(lease) = self.editor_lease.clone() else {
            self.reader.status.phase = worker::presentation::Phase::Failed;
            self.reader.status.message = "Open and save a note first.".into();
            cx.notify();
            return;
        };
        self.reader.status = worker::Status {
            phase: worker::presentation::Phase::Preparing,
            ..Default::default()
        };
        self.reader.editor_revision = self.editor.read(cx).document_revision();
        self.reader.lease = Some(lease.clone());
        self.reader.painted = None;
        self.reader.bridge = Some(worker::start_with_voice(
            self.kernel.workspace_path().to_path_buf(),
            lease,
            plain,
            self.reader.selected_voice,
        ));
        cx.notify();
    }
    fn reader_command(&mut self, command: Command, cx: &mut Context<Self>) {
        if matches!(command, Command::Assign { .. })
            && (self.reader.lease.is_none() || self.reader.status.finished)
        {
            self.reader.status.message = "Load the saved document before casting a passage.".into();
            cx.notify();
            return;
        }
        if let Some(bridge) = &self.reader.bridge {
            bridge.send(command);
        }
        cx.notify();
    }
}
mod highlight;

impl PhoenixShell {
    fn load_reader_voice_choices(&mut self) -> anyhow::Result<()> {
        let path = self.kernel.workspace_path().with_extension("reader.json");
        anyhow::ensure!(
            std::fs::metadata(&path)?.len() <= 1_048_576,
            "Reader configuration too large"
        );
        let mut config: worker::Config = serde_json::from_slice(&std::fs::read(path)?)?;
        config.load_voices()?;
        anyhow::ensure!(config.voices.len() <= 257, "Too many voice profiles");
        let choices = config
            .voices
            .iter()
            .map(|v| {
                Ok((
                    v.profile.name.clone(),
                    phoenix_reader_session::VoiceChoice::of(&v.profile)?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let book = self.reader_book_key();
        if self.reader.voice_book != Some(book) || self.reader.selected_voice.is_none() {
            let library =
                phoenix_reader_session::VoiceLibrary::open(config.storage.join("voices"))?;
            self.reader.selected_voice = library
                .selected(self.reader_book_key())?
                .or_else(|| choices.first().map(|(_, c)| *c));
        }
        self.reader.voice_book = Some(book);
        self.reader.voice_details = config
            .voices
            .iter()
            .map(|v| {
                (
                    v.profile.description.clone(),
                    v.profile.reference.is_some(),
                    v.supertonic_style.is_some(),
                )
            })
            .collect();
        self.reader.voice_choices = choices;
        Ok(())
    }
}

mod audition;
mod cast_panel;
mod studio;
mod transport;
