use super::PhoenixShell;
use gpui::Context;
use phoenix_app_core::{KernelCommand, KernelOutcome};
use phoenix_scene_contract::{
    AnchorCandidate, AnchorSource, DocumentId, EntityFamily, HighlightMode, VerifiedDocumentAnchors,
};
use std::sync::Arc;
use velotype::{SemanticHighlight, SemanticHighlightMode};

const PROOF_DOCUMENT: &str = "# Phoenix Native\n\nKernel-owned lease save proof \u{2014} Cut 5.";

impl PhoenixShell {
    pub(super) fn initialize_highlights(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.publish_proof_highlights() {
            self.status = format!("HIGHLIGHTS BLOCKED / {error}").into();
        }
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
            editor.set_semantic_highlights(
                snapshot.style.revision,
                editor_revision,
                to_velotype_mode(snapshot.style.highlight_mode),
                spans,
                cx,
            )
        });
        if let Err(error) = result {
            self.status = format!("HIGHLIGHTS BLOCKED / {error}").into();
        }
    }

    fn publish_proof_highlights(&self) -> Result<(), String> {
        if self
            .kernel
            .snapshot()
            .map_err(|error| error.to_string())?
            .document_anchors
            .is_some()
        {
            return Ok(());
        }
        let Some(lease) = self.editor_lease.as_ref() else {
            return Ok(());
        };
        if lease.content.as_ref() != PROOF_DOCUMENT {
            return Ok(());
        }
        let candidates = [
            ("Phoenix Native", EntityFamily::Organization, 1u64),
            ("Kernel-owned", EntityFamily::Concept, 2),
            ("lease save proof", EntityFamily::Event, 3),
            ("Cut 5", EntityFamily::Structure, 4),
        ]
        .into_iter()
        .map(|(surface, family, node_id)| {
            let start = PROOF_DOCUMENT
                .find(surface)
                .ok_or_else(|| format!("missing proof surface {surface}"))?;
            Ok(AnchorCandidate {
                start: u32::try_from(start).map_err(|_| "proof start overflow".to_string())?,
                end: u32::try_from(start + surface.len())
                    .map_err(|_| "proof end overflow".to_string())?,
                node_id,
                entity_slot: u32::try_from(node_id)
                    .map_err(|_| "proof entity slot overflow".to_string())?,
                family,
                surface: surface.into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
        let verified = VerifiedDocumentAnchors::verify(
            DocumentId(lease.entry_id.0),
            lease.revision.0,
            lease.content_hash.0,
            None,
            AnchorSource::VerificationFixture,
            &lease.content,
            candidates,
        )
        .map_err(|error| error.to_string())?;
        let receipt = self
            .kernel
            .execute(KernelCommand::PublishDocumentAnchors(Arc::new(verified)))
            .map_err(|error| error.to_string())?;
        if !matches!(receipt.outcome, KernelOutcome::DocumentAnchorsPublished(4)) {
            return Err("unexpected anchor publication receipt".into());
        }
        Ok(())
    }
}

fn to_velotype_mode(mode: HighlightMode) -> SemanticHighlightMode {
    match mode {
        HighlightMode::Off => SemanticHighlightMode::Off,
        HighlightMode::Subtle => SemanticHighlightMode::Subtle,
        HighlightMode::Vivid => SemanticHighlightMode::Vivid,
    }
}
