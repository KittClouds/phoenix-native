use crate::{GradientDraft, GradientPalette};
use gpui::Hsla;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// A fill that can be assigned to a text range.
#[derive(Clone, Debug, PartialEq)]
pub enum TextFill {
    /// Remove the explicit range fill and inherit the global gradient.
    Inherit,
    /// Use one static color.
    Solid(Hsla),
    /// Use a local gradient spanning the assigned range.
    Gradient(GradientPalette),
}

impl GradientDraft {
    /// Resolves authoring state: zero colors inherit, one is solid, and two or
    /// more become a gradient.
    pub fn resolved_fill(&self) -> TextFill {
        match self.stops() {
            [] => TextFill::Inherit,
            [stop] => TextFill::Solid(stop.color),
            _ => TextFill::Gradient(
                self.to_palette()
                    .expect("drafts with at least two validated stops resolve"),
            ),
        }
    }
}

/// One non-overlapping UTF-8 byte range and its fill.
#[derive(Clone, Debug, PartialEq)]
pub struct FillSpan {
    /// Grapheme-aligned byte range.
    pub range: Range<usize>,
    /// Fill applied inside the range.
    pub fill: TextFill,
}

/// Sorted, normalized range fills for immutable text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextFillMap {
    spans: Vec<FillSpan>,
}

impl TextFillMap {
    /// Creates an empty map that inherits the global gradient.
    pub const fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Returns normalized spans in source order.
    pub fn spans(&self) -> &[FillSpan] {
        &self.spans
    }

    /// Assigns a fill to a validated UTF-8 byte range.
    ///
    /// Overlapping assignments are replaced and split deterministically.
    pub fn assign_bytes(
        &mut self,
        text: &str,
        range: Range<usize>,
        fill: TextFill,
    ) -> Result<(), TextRangeError> {
        validate_range(text, &range)?;
        let mut normalized = Vec::with_capacity(self.spans.len() + 1);
        for span in self.spans.drain(..) {
            if span.range.end <= range.start || span.range.start >= range.end {
                normalized.push(span);
                continue;
            }
            if span.range.start < range.start {
                normalized.push(FillSpan {
                    range: span.range.start..range.start,
                    fill: span.fill.clone(),
                });
            }
            if span.range.end > range.end {
                normalized.push(FillSpan {
                    range: range.end..span.range.end,
                    fill: span.fill,
                });
            }
        }
        if fill != TextFill::Inherit {
            normalized.push(FillSpan { range, fill });
        }
        normalized.sort_by_key(|span| span.range.start);
        self.spans = coalesce(normalized);
        Ok(())
    }

    /// Assigns by user-perceived grapheme indexes.
    pub fn assign_graphemes(
        &mut self,
        text: &str,
        range: Range<usize>,
        fill: TextFill,
    ) -> Result<(), TextRangeError> {
        if range.start >= range.end {
            return Err(TextRangeError::EmptyRange);
        }
        let mut offsets = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        offsets.push(text.len());
        let grapheme_count = offsets.len().saturating_sub(1);
        if range.end > grapheme_count {
            return Err(TextRangeError::GraphemeOutOfBounds {
                end: range.end,
                grapheme_count,
            });
        }
        self.assign_bytes(text, offsets[range.start]..offsets[range.end], fill)
    }

    /// Removes every explicit range fill.
    pub fn clear(&mut self) {
        self.spans.clear();
    }

    pub(crate) fn fill_at(&self, byte_index: usize) -> Option<&FillSpan> {
        let index = self
            .spans
            .partition_point(|span| span.range.end <= byte_index);
        self.spans
            .get(index)
            .filter(|span| span.range.contains(&byte_index))
    }
}

fn validate_range(text: &str, range: &Range<usize>) -> Result<(), TextRangeError> {
    if range.start >= range.end {
        return Err(TextRangeError::EmptyRange);
    }
    if range.end > text.len() {
        return Err(TextRangeError::ByteOutOfBounds {
            end: range.end,
            text_len: text.len(),
        });
    }
    if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
        return Err(TextRangeError::NotUtf8Boundary);
    }
    let grapheme_aligned = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .filter(|offset| *offset == range.start || *offset == range.end)
        .count()
        == 2;
    if !grapheme_aligned {
        return Err(TextRangeError::NotGraphemeBoundary);
    }
    Ok(())
}

fn coalesce(spans: Vec<FillSpan>) -> Vec<FillSpan> {
    let mut output: Vec<FillSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(previous) = output.last_mut() {
            if previous.range.end == span.range.start && previous.fill == span.fill {
                previous.range.end = span.range.end;
                continue;
            }
        }
        output.push(span);
    }
    output
}

/// Why a range assignment was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextRangeError {
    /// Styling a collapsed range is ambiguous.
    EmptyRange,
    /// Byte range extends past the immutable text.
    ByteOutOfBounds { end: usize, text_len: usize },
    /// Byte range cuts through a UTF-8 scalar.
    NotUtf8Boundary,
    /// Byte range cuts through a user-perceived grapheme.
    NotGraphemeBoundary,
    /// Grapheme range extends past the immutable text.
    GraphemeOutOfBounds { end: usize, grapheme_count: usize },
}

impl Display for TextRangeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRange => formatter.write_str("text fill range is empty"),
            Self::ByteOutOfBounds { end, text_len } => {
                write!(
                    formatter,
                    "text fill ends at byte {end}, past text length {text_len}"
                )
            }
            Self::NotUtf8Boundary => formatter.write_str("text fill is not UTF-8 aligned"),
            Self::NotGraphemeBoundary => formatter.write_str("text fill splits a Unicode grapheme"),
            Self::GraphemeOutOfBounds {
                end,
                grapheme_count,
            } => write!(
                formatter,
                "text fill ends at grapheme {end}, past grapheme count {grapheme_count}"
            ),
        }
    }
}

impl Error for TextRangeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::rgb;

    #[test]
    fn replacement_splits_overlapping_spans_and_coalesces_neighbors() {
        let text = "abcdefghij";
        let red = TextFill::Solid(rgb(0xff0000).into());
        let blue = TextFill::Solid(rgb(0x0000ff).into());
        let mut map = TextFillMap::new();
        map.assign_bytes(text, 1..9, red.clone()).unwrap();
        map.assign_bytes(text, 3..7, blue.clone()).unwrap();
        assert_eq!(
            map.spans(),
            &[
                FillSpan {
                    range: 1..3,
                    fill: red.clone(),
                },
                FillSpan {
                    range: 3..7,
                    fill: blue,
                },
                FillSpan {
                    range: 7..9,
                    fill: red,
                },
            ]
        );
    }

    #[test]
    fn grapheme_ranges_never_split_composed_text() {
        let text = "A\u{301}👩🏽‍🚀Z";
        let mut map = TextFillMap::new();
        map.assign_graphemes(text, 1..2, TextFill::Solid(rgb(0xff00ff).into()))
            .unwrap();
        assert_eq!(map.spans()[0].range, "A\u{301}".len()..text.len() - 1);
        assert_eq!(
            map.assign_bytes(text, 1..3, TextFill::Inherit),
            Err(TextRangeError::NotGraphemeBoundary)
        );
    }

    #[test]
    fn inherit_clears_only_the_requested_interval() {
        let text = "abcdefgh";
        let mut map = TextFillMap::new();
        map.assign_bytes(text, 1..7, TextFill::Solid(rgb(0xff0000).into()))
            .unwrap();
        map.assign_bytes(text, 3..5, TextFill::Inherit).unwrap();
        assert_eq!(map.spans().len(), 2);
        assert_eq!(map.spans()[0].range, 1..3);
        assert_eq!(map.spans()[1].range, 5..7);
    }
}
