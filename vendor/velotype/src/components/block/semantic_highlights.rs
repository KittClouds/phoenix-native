use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SemanticHighlightMode {
    Off,
    #[default]
    Subtle,
    Vivid,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticHighlight {
    pub range: Range<usize>,
    pub primary: [f32; 4],
    pub secondary: [f32; 4],
}

impl SemanticHighlight {
    pub fn new(range: Range<usize>, primary: [f32; 4], secondary: [f32; 4]) -> Self {
        Self {
            range,
            primary,
            secondary,
        }
    }

    pub(crate) fn is_valid_for(&self, text: &str) -> bool {
        self.range.start < self.range.end
            && self.range.end <= text.len()
            && text.is_char_boundary(self.range.start)
            && text.is_char_boundary(self.range.end)
            && valid_color(self.primary)
            && valid_color(self.secondary)
    }
}

fn valid_color(color: [f32; 4]) -> bool {
    color
        .into_iter()
        .all(|channel| channel.is_finite() && (0.0..=1.0).contains(&channel))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BlockSemanticHighlights {
    pub(crate) revision: u64,
    pub(crate) mode: SemanticHighlightMode,
    pub(crate) spans: Vec<SemanticHighlight>,
}

impl BlockSemanticHighlights {
    pub(crate) fn clear(&mut self) -> bool {
        if self.spans.is_empty() {
            return false;
        }
        self.spans.clear();
        self.revision = self.revision.wrapping_add(1);
        true
    }
}
