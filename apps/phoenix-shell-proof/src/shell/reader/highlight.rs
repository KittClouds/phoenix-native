use super::super::PhoenixShell;
use super::Command;
use gpui::Context;
use velotype::SemanticHighlight;

impl PhoenixShell {
    pub(crate) fn invalidate_reader_document(&mut self, cx: &mut Context<Self>) {
        self.reader.pending_listen = false;
        if self.reader.lease.take().is_some() {
            if let Some(bridge) = &self.reader.bridge {
                bridge.send(Command::Stop);
            }
            self.reader.status.message = "Text changed — save to continue.".into();
            self.reader.status.phase = super::worker::presentation::Phase::Changed;
        }
        self.reader.painted = None;
        self.editor
            .update(cx, |editor, cx| editor.clear_narration_highlights(cx));
    }

    pub(super) fn refresh_reader_highlight(&mut self, cx: &mut Context<Self>) {
        let Some(lease) = self.reader.lease.clone() else {
            return;
        };
        let matches = self
            .editor_lease
            .as_ref()
            .is_some_and(|current| current.token() == lease.token())
            && self.editor.read(cx).document_revision() == self.reader.editor_revision;
        if !matches {
            self.invalidate_reader_document(cx);
            return;
        }
        let s = &self.reader.status;
        if !s.playing || s.finished {
            if self.reader.painted.take().is_some() {
                self.editor
                    .update(cx, |editor, cx| editor.clear_narration_highlights(cx));
            }
            return;
        }
        if self.reader.painted == Some(s.segment) {
            return;
        }
        let spans = s
            .source_ranges
            .iter()
            .map(|r| {
                SemanticHighlight::new(
                    r.start as usize..r.end as usize,
                    [0.20, 0.70, 0.65, 1.0],
                    [0.20, 0.70, 0.65, 1.0],
                )
            })
            .collect();
        let revision = self.reader.editor_revision;
        let result = self.editor.update(cx, |editor, cx| {
            editor.project_narration_highlights(1, revision, &lease.content, spans, cx)
        });
        match result {
            Ok(_) => self.reader.painted = Some(s.segment),
            Err(_) => self.invalidate_reader_document(cx),
        }
    }
}
