use super::Editor;
use crate::components::{Block, SemanticHighlight, SemanticHighlightMode};
use gpui::{Context, Entity};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticHighlightError {
    StaleDocumentRevision { expected: u64, actual: u64 },
    InvalidRevision,
    InvalidRange,
    OverlappingRanges,
    UnmappedRange,
}

impl fmt::Display for SemanticHighlightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleDocumentRevision { expected, actual } => write!(
                formatter,
                "semantic highlights target editor revision {expected}, current revision is {actual}"
            ),
            Self::InvalidRevision => {
                formatter.write_str("semantic highlight revision must be nonzero")
            }
            Self::InvalidRange => {
                formatter.write_str("semantic highlight range or color is invalid")
            }
            Self::OverlappingRanges => formatter.write_str("semantic highlight ranges overlap"),
            Self::UnmappedRange => formatter.write_str(
                "semantic highlight range cannot be mapped exactly into one rendered block",
            ),
        }
    }
}

impl std::error::Error for SemanticHighlightError {}

impl Editor {
    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }

    pub fn semantic_highlight_revision(&self) -> u64 {
        self.semantic_highlight_revision
    }

    pub fn set_semantic_highlights(
        &mut self,
        revision: u64,
        expected_document_revision: u64,
        mode: SemanticHighlightMode,
        mut spans: Vec<SemanticHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<(), SemanticHighlightError> {
        if expected_document_revision != self.document_revision {
            return Err(SemanticHighlightError::StaleDocumentRevision {
                expected: expected_document_revision,
                actual: self.document_revision,
            });
        }
        if revision == 0 {
            return Err(SemanticHighlightError::InvalidRevision);
        }

        let source = self.current_document_source(cx);
        spans.sort_unstable_by_key(|span| (span.range.start, span.range.end));
        let mut previous_end = 0usize;
        for (index, span) in spans.iter().enumerate() {
            if !span.is_valid_for(&source) {
                return Err(SemanticHighlightError::InvalidRange);
            }
            if index > 0 && span.range.start < previous_end {
                return Err(SemanticHighlightError::OverlappingRanges);
            }
            previous_end = span.range.end;
        }

        let mappings = self.build_source_target_mappings(cx);
        let mut projected: Vec<(Entity<Block>, Vec<SemanticHighlight>)> = Vec::new();
        let mut mapping_index = 0usize;
        for span in spans {
            while mapping_index < mappings.len()
                && mappings[mapping_index].full_source_range.end <= span.range.start
            {
                mapping_index += 1;
            }
            let mapping = mappings
                .get(mapping_index)
                .ok_or(SemanticHighlightError::UnmappedRange)?;
            if span.range.start < mapping.full_source_range.start
                || span.range.end > mapping.full_source_range.end
            {
                return Err(SemanticHighlightError::UnmappedRange);
            }
            let local_start = span.range.start - mapping.full_source_range.start;
            let local_end = span.range.end - mapping.full_source_range.start;
            let content_start = exact_content_offset(mapping, local_start)?;
            let content_end = exact_content_offset(mapping, local_end)?;
            if content_start >= content_end {
                return Err(SemanticHighlightError::UnmappedRange);
            }
            let local =
                SemanticHighlight::new(content_start..content_end, span.primary, span.secondary);
            if let Some((entity, entity_spans)) = projected.last_mut()
                && entity.entity_id() == mapping.entity.entity_id()
            {
                entity_spans.push(local);
            } else {
                projected.push((mapping.entity.clone(), vec![local]));
            }
        }

        self.clear_semantic_highlights(cx);
        self.semantic_highlight_blocks.reserve(projected.len());
        for (block, block_spans) in projected {
            block.update(cx, |block, _cx| {
                block.set_semantic_highlights(revision, mode, block_spans);
            });
            self.semantic_highlight_blocks.push(block);
        }
        self.semantic_highlight_revision = revision;
        self.semantic_highlight_mode = mode;
        cx.notify();
        Ok(())
    }

    pub fn set_semantic_highlight_mode(
        &mut self,
        mode: SemanticHighlightMode,
        cx: &mut Context<Self>,
    ) {
        if self.semantic_highlight_mode == mode {
            return;
        }
        self.semantic_highlight_mode = mode;
        for block in &self.semantic_highlight_blocks {
            block.update(cx, |block, _cx| {
                block.set_semantic_highlight_mode(mode);
            });
        }
        cx.notify();
    }

    pub fn clear_semantic_highlights(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for block in self.semantic_highlight_blocks.drain(..) {
            block.update(cx, |block, _cx| {
                changed |= block.clear_semantic_highlights();
            });
        }
        self.semantic_highlight_revision = 0;
        if changed {
            cx.notify();
        }
    }
}

fn exact_content_offset(
    mapping: &super::SourceTargetMapping,
    source_offset: usize,
) -> Result<usize, SemanticHighlightError> {
    let content_offset = mapping
        .source_to_content
        .get(source_offset)
        .copied()
        .ok_or(SemanticHighlightError::UnmappedRange)?;
    if mapping.content_to_source.get(content_offset).copied() != Some(source_offset) {
        return Err(SemanticHighlightError::UnmappedRange);
    }
    Ok(content_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};

    fn span(text: &str, needle: &str) -> SemanticHighlight {
        let start = text.find(needle).expect("fixture needle");
        SemanticHighlight::new(
            start..start + needle.len(),
            [0.1, 0.9, 0.5, 1.0],
            [0.2, 0.6, 1.0, 1.0],
        )
    }

    #[gpui::test]
    fn projection_maps_markdown_source_without_mutating_document(cx: &mut TestAppContext) {
        let markdown = "# **Phoenix**\n\nRyan entered New Rome.";
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, markdown.into()));
        editor.update(cx, |editor, cx| {
            let before = editor.host_document_text(cx);
            editor
                .set_semantic_highlights(
                    7,
                    editor.document_revision(),
                    SemanticHighlightMode::Subtle,
                    vec![span(markdown, "Phoenix"), span(markdown, "New Rome")],
                    cx,
                )
                .expect("projection");
            assert_eq!(editor.host_document_text(cx), before);
            assert!(!editor.is_dirty());
            assert_eq!(editor.semantic_highlight_revision(), 7);
        });
    }

    #[gpui::test]
    fn edit_invalidation_clears_semantic_projection(cx: &mut TestAppContext) {
        let markdown = "Ryan entered.";
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, markdown.into()));
        editor.update(cx, |editor, cx| {
            editor
                .set_semantic_highlights(
                    2,
                    editor.document_revision(),
                    SemanticHighlightMode::Vivid,
                    vec![span(markdown, "Ryan")],
                    cx,
                )
                .expect("projection");
            editor.mark_dirty(cx);
            assert_eq!(editor.semantic_highlight_revision(), 0);
        });
    }
}
