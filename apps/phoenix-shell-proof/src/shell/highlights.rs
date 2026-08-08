use super::analytics::AnalyticsHighlight;
use super::PhoenixShell;
use gpui::{Context, Entity, Timer};
use phoenix_app_core::KernelCommand;
use phoenix_scene_contract::HighlightMode;
use std::time::Duration;
use velotype::{SemanticHighlight, SemanticHighlightMode};

const EDIT_HIGHLIGHT_REPROJECTION_DELAY: Duration = Duration::from_millis(75);

impl PhoenixShell {
    pub(super) fn initialize_highlights(&mut self, cx: &mut Context<Self>) {
        self.apply_kernel_highlights(cx);
        self.schedule_highlight_reprojection(self.editor.clone(), cx);
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

    /// Reprojects immutable, evidence-bound anchors after the editor settles.
    ///
    /// Velotype clears block-local paint on every edit so offsets can never
    /// silently drift. The shell restores that paint from the kernel's exact
    /// source spans once the current edit burst stops. Replacing the task
    /// cancels the prior timer; the revision guard also prevents an older task
    /// from painting a newer document.
    pub(super) fn schedule_highlight_reprojection(
        &mut self,
        editor: Entity<velotype::Editor>,
        cx: &mut Context<Self>,
    ) {
        let scheduled_revision = editor.read_with(cx, |editor, _cx| editor.document_revision());
        self.highlight_reprojection_task = Some(cx.spawn(async move |shell, async_cx| {
            Timer::after(EDIT_HIGHLIGHT_REPROJECTION_DELAY).await;
            let _ = shell.update(async_cx, |this, cx| {
                let current_revision =
                    editor.read_with(cx, |editor, _cx| editor.document_revision());
                if current_revision != scheduled_revision {
                    return;
                }
                this.apply_kernel_highlights(cx);
            });
        }));
    }

    pub(super) fn apply_kernel_highlights(&mut self, cx: &mut Context<Self>) {
        if self.analytics_highlight.is_some() {
            self.apply_analytics_highlights(cx);
            return;
        }
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

    fn apply_analytics_highlights(&mut self, cx: &mut Context<Self>) {
        let source = self
            .editor
            .read_with(cx, |editor, cx| editor.host_document_text(cx));
        if self.text_analytics.source_hash != phoenix_text_analytics::source_fingerprint(&source) {
            self.editor
                .update(cx, |editor, cx| editor.clear_semantic_highlights(cx));
            return;
        }
        let Some(highlight) = self.analytics_highlight else {
            return;
        };
        let mut ranges = match highlight {
            AnalyticsHighlight::SentenceBand(band) => self
                .text_analytics
                .sentence_spans
                .iter()
                .filter(|sentence| sentence.band == band)
                .map(|sentence| sentence.span)
                .collect::<Vec<_>>(),
            AnalyticsHighlight::Lens(kind) => self
                .text_analytics
                .lens(kind)
                .into_iter()
                .flat_map(|lens| lens.items.iter())
                .flat_map(|item| item.spans.iter().copied())
                .collect::<Vec<_>>(),
            AnalyticsHighlight::Item(kind, index) => self
                .text_analytics
                .lens(kind)
                .and_then(|lens| lens.items.get(index))
                .map_or_else(Vec::new, |item| item.spans.to_vec()),
        };
        ranges.sort_unstable_by_key(|span| (span.start, span.end));
        let mut merged: Vec<phoenix_text_analytics::SourceSpan> = Vec::with_capacity(ranges.len());
        for span in ranges {
            if span.start >= span.end || span.end as usize > source.len() {
                continue;
            }
            if let Some(previous) = merged.last_mut() {
                if span.start <= previous.end {
                    previous.end = previous.end.max(span.end);
                    continue;
                }
            }
            merged.push(span);
        }
        let (primary, secondary) = analytics_colors(highlight);
        let mut spans = merged
            .iter()
            .map(|span| {
                SemanticHighlight::new(span.start as usize..span.end as usize, primary, secondary)
            })
            .collect::<Vec<_>>();

        // Preserve graph paint outside the active analytics evidence. Graph
        // anchors are safe to merge only while both systems target the exact
        // same saved source frame; analytics wins on an overlap.
        if let Ok(snapshot) = self.kernel.snapshot() {
            if let (Some(anchors), Some(lease)) =
                (snapshot.document_anchors, snapshot.active_document_lease)
            {
                if lease.content.as_ref() == source {
                    let palette = *snapshot.highlight_palette;
                    for anchor in anchors.anchors() {
                        let graph_range = anchor.start..anchor.end;
                        let overlaps_analytics = merged.iter().any(|span| {
                            graph_range.start < span.end && span.start < graph_range.end
                        });
                        if overlaps_analytics {
                            continue;
                        }
                        let family = palette.for_family(anchor.family);
                        spans.push(SemanticHighlight::new(
                            graph_range.start as usize..graph_range.end as usize,
                            family.primary,
                            family.secondary,
                        ));
                    }
                }
            }
        }
        spans.sort_unstable_by_key(|span| (span.range.start, span.range.end));
        let editor_revision = self
            .editor
            .read_with(cx, |editor, _| editor.document_revision());
        let highlight_revision = editor_revision.wrapping_add(1).max(1);
        let result = self.editor.update(cx, |editor, cx| {
            editor.project_semantic_highlights_from_source(
                highlight_revision,
                editor_revision,
                &source,
                SemanticHighlightMode::Vivid,
                spans,
                cx,
            )
        });
        match result {
            Ok(receipt) => eprintln!(
                "PHOENIX_ANALYTICS_HIGHLIGHTS_ACTIVE selection={highlight:?} requested={} applied={} unmapped={}",
                receipt.requested, receipt.applied, receipt.unmapped
            ),
            Err(error) => {
                self.status = format!("ANALYTICS HIGHLIGHTS BLOCKED / {error}").into();
            }
        }
    }
}

fn analytics_colors(highlight: AnalyticsHighlight) -> ([f32; 4], [f32; 4]) {
    let (primary, secondary) = match highlight {
        AnalyticsHighlight::SentenceBand(band) => {
            let color = super::analytics::BAND_COLORS[usize::from(band).min(5)];
            (color, 0x48d9c2)
        }
        AnalyticsHighlight::Lens(_) => (0x24d6b2, 0x438cff),
        AnalyticsHighlight::Item(_, _) => (0x438cff, 0x24d6b2),
    };
    (color_to_rgba(primary), color_to_rgba(secondary))
}

fn color_to_rgba(color: u32) -> [f32; 4] {
    [
        ((color >> 16) & 0xff) as f32 / 255.0,
        ((color >> 8) & 0xff) as f32 / 255.0,
        (color & 0xff) as f32 / 255.0,
        1.0,
    ]
}

fn to_velotype_mode(mode: HighlightMode) -> SemanticHighlightMode {
    match mode {
        HighlightMode::Off => SemanticHighlightMode::Off,
        HighlightMode::Subtle => SemanticHighlightMode::Subtle,
        HighlightMode::Vivid => SemanticHighlightMode::Vivid,
    }
}
