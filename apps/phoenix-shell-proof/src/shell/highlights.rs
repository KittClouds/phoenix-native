use super::PhoenixShell;
use gpui::Context;
use phoenix_app_core::KernelCommand;
use phoenix_scene_contract::HighlightMode;
use velotype::{SemanticHighlight, SemanticHighlightMode};

impl PhoenixShell {
    pub(super) fn initialize_highlights(&mut self, cx: &mut Context<Self>) {
        self.apply_kernel_highlights(cx);
    }

    pub(super) fn set_highlight_mode(&mut self, mode: HighlightMode, cx: &mut Context<Self>) {
        let Ok(snapshot) = self.kernel.snapshot() else {
            self.status = "HIGHLIGHTS BLOCKED / KERNEL SNAPSHOT UNAVAILABLE".into();
            cx.notify();
            return;
        };
        let mut style = snapshot.style;
        if style.highlight_mode == mode {
            return;
        }
        let Some(revision) = style.revision.checked_add(1) else {
            self.status = "HIGHLIGHTS BLOCKED / STYLE REVISION EXHAUSTED".into();
            cx.notify();
            return;
        };
        style.revision = revision;
        style.highlight_mode = mode;
        match self.kernel.execute(KernelCommand::SetStyle(style)) {
            Ok(receipt) => {
                self.editor.update(cx, |editor, cx| {
                    editor.set_semantic_highlight_mode(to_velotype_mode(mode), cx);
                });
                self.status = format!(
                    "HIGHLIGHTS / {} / STYLE REV {} / SEQUENCE {}",
                    mode.label(),
                    style.revision,
                    receipt.sequence
                )
                .into();
                let _ = self.kernel.drain_events();
            }
            Err(error) => self.status = format!("HIGHLIGHTS BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    pub(super) fn apply_kernel_highlights(&mut self, cx: &mut Context<Self>) {
        let Ok(snapshot) = self.kernel.snapshot() else {
            self.editor
                .update(cx, |editor, cx| editor.clear_semantic_highlights(cx));
            return;
        };
        let Some(anchors) = snapshot.document_anchors else {
            self.editor
                .update(cx, |editor, cx| editor.clear_semantic_highlights(cx));
            return;
        };
        let Some(lease) = snapshot.active_document_lease else {
            self.status = "HIGHLIGHTS BLOCKED / DOCUMENT LEASE UNAVAILABLE".into();
            cx.notify();
            return;
        };
        let palette = *snapshot.highlight_palette;
        let spans = anchors
            .anchors()
            .iter()
            .map(|anchor| {
                let family = palette.for_family(anchor.family);
                SemanticHighlight::new(
                    anchor.start as usize..anchor.end as usize,
                    family.primary,
                    family.secondary,
                )
            })
            .collect::<Vec<_>>();
        let editor_revision = self
            .editor
            .read_with(cx, |editor, _cx| editor.document_revision());
        let result = self.editor.update(cx, |editor, cx| {
            editor.project_semantic_highlights_from_source(
                snapshot.revision,
                editor_revision,
                &lease.content,
                to_velotype_mode(snapshot.style.highlight_mode),
                spans,
                cx,
            )
        });
        match result {
            Ok(receipt) => {
                eprintln!(
                    "PHOENIX_HIGHLIGHTS_ACTIVE document={} document_revision={} source={:?} \
                     requested={} applied={} unmapped={} first_unmapped={:?}",
                    anchors.document().0,
                    anchors.document_revision(),
                    anchors.source(),
                    receipt.requested,
                    receipt.applied,
                    receipt.unmapped,
                    receipt.first_unmapped
                );
                if receipt.unmapped > 0 {
                    self.status = format!(
                        "HIGHLIGHTS / {} ACTIVE / {} SOURCE-ONLY SPANS OMITTED",
                        receipt.applied, receipt.unmapped
                    )
                    .into();
                }
            }
            Err(error) => {
                eprintln!(
                    "PHOENIX_HIGHLIGHTS_BLOCKED document={} document_revision={} source={:?} \
                     anchors={} error={error}",
                    anchors.document().0,
                    anchors.document_revision(),
                    anchors.source(),
                    anchors.anchors().len()
                );
                self.status = format!("HIGHLIGHTS BLOCKED / {error}").into();
            }
        }
    }
}

fn to_velotype_mode(mode: HighlightMode) -> SemanticHighlightMode {
    match mode {
        HighlightMode::Off => SemanticHighlightMode::Off,
        HighlightMode::Subtle => SemanticHighlightMode::Subtle,
        HighlightMode::Vivid => SemanticHighlightMode::Vivid,
    }
}
