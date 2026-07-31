use super::Editor;
use crate::components::{Block, SemanticHighlight, SemanticHighlightMode};
use gpui::{Context, Entity};
use std::fmt;
use std::ops::Range;

const UNMAPPED_SOURCE_OFFSET: u32 = u32::MAX;
const SOURCE_ALIGNMENT_LOOKAHEAD: usize = 16 * 1024;
const SOURCE_ALIGNMENT_ANCHOR: usize = 16;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticHighlightProjectionReceipt {
    pub requested: usize,
    pub applied: usize,
    pub unmapped: usize,
    pub first_unmapped: Option<Range<usize>>,
}

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
        spans: Vec<SemanticHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<(), SemanticHighlightError> {
        self.project_semantic_highlights_inner(
            revision,
            expected_document_revision,
            mode,
            spans,
            false,
            None,
            cx,
        )
        .map(|_| ())
    }

    /// Projects verified source spans into rendered blocks without allowing one
    /// non-rendered Markdown range to erase every other visible highlight.
    ///
    /// Invalid, stale, or overlapping source contracts still fail closed.
    /// Ranges that target Markdown syntax or cross block boundaries are omitted
    /// from paint only and reported explicitly in the returned receipt.
    pub fn project_semantic_highlights(
        &mut self,
        revision: u64,
        expected_document_revision: u64,
        mode: SemanticHighlightMode,
        spans: Vec<SemanticHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<SemanticHighlightProjectionReceipt, SemanticHighlightError> {
        self.project_semantic_highlights_inner(
            revision,
            expected_document_revision,
            mode,
            spans,
            true,
            None,
            cx,
        )
    }

    /// Projects spans verified against the host's authoritative source into
    /// Velotype's rendered Markdown representation.
    ///
    /// The editor can losslessly display source whose Markdown serializer uses
    /// a different spelling (for example, indented code becoming fenced code).
    /// This method aligns byte coordinates once without discovering entities
    /// from labels or mutating either source.
    pub fn project_semantic_highlights_from_source(
        &mut self,
        revision: u64,
        expected_document_revision: u64,
        authoritative_source: &str,
        mode: SemanticHighlightMode,
        spans: Vec<SemanticHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<SemanticHighlightProjectionReceipt, SemanticHighlightError> {
        self.project_semantic_highlights_inner(
            revision,
            expected_document_revision,
            mode,
            spans,
            true,
            Some(authoritative_source),
            cx,
        )
    }

    fn project_semantic_highlights_inner(
        &mut self,
        revision: u64,
        expected_document_revision: u64,
        mode: SemanticHighlightMode,
        mut spans: Vec<SemanticHighlight>,
        allow_unmapped: bool,
        authoritative_source: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<SemanticHighlightProjectionReceipt, SemanticHighlightError> {
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
        let verification_source = authoritative_source.unwrap_or(&source);
        spans.sort_unstable_by_key(|span| (span.range.start, span.range.end));
        let requested = spans.len();
        let mut previous_end = 0usize;
        for (index, span) in spans.iter().enumerate() {
            if !span.is_valid_for(verification_source) {
                return Err(SemanticHighlightError::InvalidRange);
            }
            if index > 0 && span.range.start < previous_end {
                return Err(SemanticHighlightError::OverlappingRanges);
            }
            previous_end = span.range.end;
        }

        let source_alignment = (verification_source != source)
            .then(|| align_source_offsets(verification_source.as_bytes(), source.as_bytes()));
        let mut aligned = Vec::with_capacity(spans.len());
        let mut unmapped = 0usize;
        let mut first_unmapped = None;
        for span in spans {
            let original_range = span.range.clone();
            let Some(range) = aligned_range(
                &original_range,
                verification_source,
                &source,
                source_alignment.as_deref(),
            ) else {
                record_unmapped(
                    allow_unmapped,
                    &original_range,
                    &mut unmapped,
                    &mut first_unmapped,
                )?;
                continue;
            };
            aligned.push((
                original_range,
                SemanticHighlight::new(range, span.primary, span.secondary),
            ));
        }

        let mappings = self.build_source_target_mappings(cx);
        let mut projected: Vec<(Entity<Block>, Vec<SemanticHighlight>)> = Vec::new();
        let mut mapping_index = 0usize;
        for (original_range, span) in aligned {
            while mapping_index < mappings.len()
                && mappings[mapping_index].full_source_range.end <= span.range.start
            {
                mapping_index += 1;
            }
            let Some(mapping) = mappings.get(mapping_index) else {
                record_unmapped(
                    allow_unmapped,
                    &original_range,
                    &mut unmapped,
                    &mut first_unmapped,
                )?;
                continue;
            };
            if span.range.start < mapping.full_source_range.start
                || span.range.end > mapping.full_source_range.end
            {
                record_unmapped(
                    allow_unmapped,
                    &original_range,
                    &mut unmapped,
                    &mut first_unmapped,
                )?;
                continue;
            }
            let local_start = span.range.start - mapping.full_source_range.start;
            let local_end = span.range.end - mapping.full_source_range.start;
            let (Ok(content_start), Ok(content_end)) = (
                exact_content_offset(mapping, local_start),
                exact_content_offset(mapping, local_end),
            ) else {
                record_unmapped(
                    allow_unmapped,
                    &original_range,
                    &mut unmapped,
                    &mut first_unmapped,
                )?;
                continue;
            };
            if content_start >= content_end {
                record_unmapped(
                    allow_unmapped,
                    &original_range,
                    &mut unmapped,
                    &mut first_unmapped,
                )?;
                continue;
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
        Ok(SemanticHighlightProjectionReceipt {
            requested,
            applied: requested.saturating_sub(unmapped),
            unmapped,
            first_unmapped,
        })
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

fn aligned_range(
    range: &Range<usize>,
    authoritative_source: &str,
    rendered_source: &str,
    alignment: Option<&[u32]>,
) -> Option<Range<usize>> {
    let Some(alignment) = alignment else {
        return Some(range.clone());
    };
    let start = *alignment.get(range.start)?;
    let end = *alignment.get(range.end)?;
    if start == UNMAPPED_SOURCE_OFFSET || end == UNMAPPED_SOURCE_OFFSET {
        return None;
    }
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    if start >= end
        || rendered_source.get(start..end)? != authoritative_source.get(range.clone())?
    {
        return None;
    }
    Some(start..end)
}

fn align_source_offsets(authoritative: &[u8], rendered: &[u8]) -> Vec<u32> {
    let mut offsets = vec![UNMAPPED_SOURCE_OFFSET; authoritative.len().saturating_add(1)];
    let mut source_index = 0usize;
    let mut rendered_index = 0usize;
    offsets[0] = 0;

    while source_index < authoritative.len() && rendered_index < rendered.len() {
        if authoritative[source_index] == rendered[rendered_index] {
            source_index += 1;
            rendered_index += 1;
            offsets[source_index] = u32::try_from(rendered_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            continue;
        }

        let rendered_skip =
            find_alignment_anchor(&rendered[rendered_index..], &authoritative[source_index..]);
        let source_skip =
            find_alignment_anchor(&authoritative[source_index..], &rendered[rendered_index..]);
        match (rendered_skip, source_skip) {
            (Some(inserted), Some(deleted)) if inserted <= deleted => {
                rendered_index += inserted;
                offsets[source_index] =
                    u32::try_from(rendered_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (Some(inserted), None) => {
                rendered_index += inserted;
                offsets[source_index] =
                    u32::try_from(rendered_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (_, Some(deleted)) => {
                source_index += deleted;
                offsets[source_index] =
                    u32::try_from(rendered_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
            (None, None) => {
                source_index += 1;
                rendered_index += 1;
                offsets[source_index] =
                    u32::try_from(rendered_index).unwrap_or(UNMAPPED_SOURCE_OFFSET);
            }
        }
    }
    if source_index == authoritative.len() {
        offsets[source_index] = u32::try_from(rendered.len()).unwrap_or(UNMAPPED_SOURCE_OFFSET);
    }
    offsets
}

fn find_alignment_anchor(haystack: &[u8], authority: &[u8]) -> Option<usize> {
    let anchor_len = SOURCE_ALIGNMENT_ANCHOR.min(authority.len());
    if anchor_len == 0 {
        return None;
    }
    let search_len = haystack
        .len()
        .min(SOURCE_ALIGNMENT_LOOKAHEAD.saturating_add(anchor_len));
    haystack[..search_len]
        .windows(anchor_len)
        .position(|window| window == &authority[..anchor_len])
        .filter(|offset| *offset > 0)
}

fn record_unmapped(
    allow_unmapped: bool,
    range: &Range<usize>,
    unmapped: &mut usize,
    first_unmapped: &mut Option<Range<usize>>,
) -> Result<(), SemanticHighlightError> {
    if !allow_unmapped {
        return Err(SemanticHighlightError::UnmappedRange);
    }
    *unmapped = unmapped.saturating_add(1);
    if first_unmapped.is_none() {
        *first_unmapped = Some(range.clone());
    }
    Ok(())
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

    #[gpui::test]
    fn verified_projection_can_be_restored_after_an_unsaved_edit(cx: &mut TestAppContext) {
        let authoritative = "Ryan entered New Rome.";
        let edited = "Today, Ryan entered New Rome.";
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, authoritative.into()));
        editor.update(cx, |editor, cx| {
            editor.replace_embedded_document(edited.to_owned(), cx);
            editor.mark_dirty(cx);
            let receipt = editor
                .project_semantic_highlights_from_source(
                    12,
                    editor.document_revision(),
                    authoritative,
                    SemanticHighlightMode::Vivid,
                    vec![span(authoritative, "Ryan"), span(authoritative, "New Rome")],
                    cx,
                )
                .expect("verified anchors should align across an insertion");
            assert_eq!(receipt.requested, 2);
            assert_eq!(receipt.applied, 2);
            assert_eq!(receipt.unmapped, 0);
            assert_eq!(editor.semantic_highlight_revision(), 12);
            assert_eq!(editor.host_document_text(cx), edited);
            assert!(editor.is_dirty());
        });
    }

    #[gpui::test]
    fn verified_projection_reports_unrendered_markdown_without_blanket_failure(
        cx: &mut TestAppContext,
    ) {
        let markdown = "# Phoenix\n\nRyan entered New Rome.";
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, markdown.into()));
        editor.update(cx, |editor, cx| {
            let receipt = editor
                .project_semantic_highlights(
                    9,
                    editor.document_revision(),
                    SemanticHighlightMode::Subtle,
                    vec![
                        SemanticHighlight::new(0..1, [0.1, 0.9, 0.5, 1.0], [0.2, 0.6, 1.0, 1.0]),
                        span(markdown, "Ryan"),
                    ],
                    cx,
                )
                .expect("verified projection");
            assert_eq!(
                receipt,
                SemanticHighlightProjectionReceipt {
                    requested: 2,
                    applied: 1,
                    unmapped: 1,
                    first_unmapped: Some(0..1),
                }
            );
            assert_eq!(editor.semantic_highlight_revision(), 9);
        });
    }

    #[gpui::test]
    fn verified_projection_aligns_lossless_host_source(cx: &mut TestAppContext) {
        let markdown = "Chapter 1\n\n\t\t Ryan entered New Rome.\n\nAfter.";
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, markdown.into()));
        editor.update(cx, |editor, cx| {
            assert_ne!(editor.host_document_text(cx), markdown);
            let receipt = editor
                .project_semantic_highlights_from_source(
                    11,
                    editor.document_revision(),
                    markdown,
                    SemanticHighlightMode::Subtle,
                    vec![span(markdown, "Ryan"), span(markdown, "New Rome")],
                    cx,
                )
                .expect("verified host-source projection");
            assert_eq!(
                receipt,
                SemanticHighlightProjectionReceipt {
                    requested: 2,
                    applied: 2,
                    unmapped: 0,
                    first_unmapped: None,
                }
            );
            assert_eq!(editor.semantic_highlight_revision(), 11);
        });
    }
}
