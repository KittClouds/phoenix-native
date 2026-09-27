use super::super::PhoenixShell;
use super::Command;
use gpui::Context;
use velotype::SemanticHighlight;

impl PhoenixShell {
    pub(crate) fn invalidate_reader_document(&mut self, cx: &mut Context<Self>) {
        self.reader.selection_request = self.reader.selection_request.wrapping_add(1);
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

    /// Sends the spoken segment to the atlas (4B). The glow runs only when
    /// the Reader's saved revision matches the verified graph, using the same
    /// comparison as the graph revision chip; otherwise it is paused.
    pub(in crate::shell) fn sync_reader_glow(&mut self) {
        use super::worker::presentation::Phase;
        let s = &self.reader.status;
        let speaking = !s.finished
            && s.segments > 0
            && !s.source_ranges.is_empty()
            && matches!(s.phase, Phase::Playing | Phase::Paused | Phase::Buffering);
        let lease = self.reader.lease.clone().filter(|_| speaking);
        let currency = lease.as_ref().map(|lease| {
            let graph = self
                .kernel_snapshot()
                .and_then(|snapshot| snapshot.graph_generation_v2)
                .map(|generation| {
                    let header = generation.header();
                    (
                        header.native_document_id,
                        header.document_revision,
                        header.content_hash,
                    )
                });
            crate::shell::graph_toolbar::classify_currency(
                graph,
                Some((lease.entry_id.0, lease.revision.0, lease.content_hash.0)),
            )
        });
        let current = matches!(
            currency,
            Some(crate::shell::graph_toolbar::GraphCurrency::Current { .. })
        );
        self.reader.glow_stale = currency.is_some_and(|c| c.reader_glow_behind());
        let follow = self.reader.glow_follow;
        let key = lease
            .filter(|_| current)
            .map(|lease| (lease.revision.0, s.segment, s.playing, follow));
        if key == self.reader.glow_sent {
            return;
        }
        let request = key.map(|_| crate::graph_window::ReaderGlowRequest {
            segment: s.segment,
            ranges: s
                .source_ranges
                .iter()
                .map(|range| (range.start, range.end))
                .collect(),
            playing: s.playing,
            follow,
            observed_at: std::time::Instant::now(),
        });
        let sent = self
            .graph
            .borrow()
            .as_ref()
            .map(|graph| graph.reader_glow(request));
        if matches!(sent, Some(Ok(()))) {
            self.reader.glow_sent = key;
        }
    }

    pub(super) fn refresh_reader_highlight(&mut self, cx: &mut Context<Self>) {
        self.sync_reader_glow();
        // Story Follow Reader (4C) uses the same currency gate as the glow.
        let spoken = self
            .reader
            .glow_sent
            .and(self.reader.status.source_ranges.first())
            .map(|range| range.start);
        self.sync_story_follow(spoken, cx);
        if self.reader.selection_mode {
            return;
        }
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
