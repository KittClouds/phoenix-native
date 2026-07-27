use crate::{GradientPalette, TextFill, TextFillMap, TextRangeError};
use gpui::{
    Animation, AnimationElement, AnimationExt, App, ElementId, Hsla, IntoElement, RenderOnce,
    SharedString, StyledText, TextRun, TextStyle, Window,
};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

const DEFAULT_MAX_COLOR_RUNS: usize = 64;
const MIN_DURATION: Duration = Duration::from_millis(1);
const MIN_CYCLES: f32 = 0.01;
const MAX_CYCLES: f32 = 64.0;

/// Direction in which palette colors travel through the text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Motion {
    /// Advance through the palette as animation time increases.
    #[default]
    Forward,
    /// Travel through the palette in the opposite direction.
    Reverse,
}

/// Unicode-aware gradient text with optional normalized range fills.
#[derive(Clone, IntoElement)]
pub struct GradientText {
    text: SharedString,
    pub(crate) grapheme_offsets: Arc<[usize]>,
    palette: GradientPalette,
    fills: TextFillMap,
    phase: f32,
    cycles: f32,
    max_color_runs: usize,
    motion: Motion,
    text_style: Option<TextStyle>,
    selection: Option<Range<usize>>,
    selection_background: Hsla,
}

impl GradientText {
    /// Indexes `text` once and uses `palette` as the global fill.
    pub fn new(text: impl Into<SharedString>, palette: GradientPalette) -> Self {
        let text = text.into();
        let mut offsets = Vec::with_capacity(text.len().min(64) + 1);
        offsets.extend(
            text.grapheme_indices(true)
                .map(|(byte_offset, _)| byte_offset),
        );
        if offsets.last().copied() != Some(text.len()) {
            offsets.push(text.len());
        }
        Self {
            text,
            grapheme_offsets: offsets.into(),
            palette,
            fills: TextFillMap::new(),
            phase: 0.0,
            cycles: 1.0,
            max_color_runs: DEFAULT_MAX_COLOR_RUNS,
            motion: Motion::Forward,
            text_style: None,
            selection: None,
            selection_background: gpui::rgba(0x3b82f666).into(),
        }
    }

    /// Uses the built-in Phoenix palette.
    pub fn phoenix(text: impl Into<SharedString>) -> Self {
        Self::new(text, GradientPalette::phoenix())
    }

    /// Replaces the global gradient.
    pub fn palette(mut self, palette: GradientPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Returns the global gradient.
    pub fn global_palette(&self) -> &GradientPalette {
        &self.palette
    }

    /// Replaces normalized range fills.
    pub fn with_fill_map(mut self, fills: TextFillMap) -> Self {
        self.fills = fills;
        self
    }

    /// Returns normalized range fills.
    pub fn fill_map(&self) -> &TextFillMap {
        &self.fills
    }

    /// Sets the animation phase. Values wrap into the cyclic palette.
    pub fn phase(mut self, phase: f32) -> Self {
        self.phase = if phase.is_finite() {
            phase.rem_euclid(1.0)
        } else {
            0.0
        };
        self
    }

    /// Sets how many palette cycles span each gradient anchor.
    pub fn cycles(mut self, cycles: f32) -> Self {
        self.cycles = if cycles.is_finite() {
            cycles.abs().clamp(MIN_CYCLES, MAX_CYCLES)
        } else {
            1.0
        };
        self
    }

    /// Bounds gradient sampling runs while preserving every explicit range
    /// and selection boundary. The default is 64.
    pub fn max_color_runs(mut self, max_color_runs: usize) -> Self {
        self.max_color_runs = max_color_runs.max(1);
        self
    }

    /// Sets palette travel direction.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Overrides inherited GPUI text styling except fill color.
    pub fn text_style(mut self, text_style: TextStyle) -> Self {
        self.text_style = Some(text_style);
        self
    }

    /// Adds a validated read-only selection highlight.
    pub fn selection_bytes(mut self, range: Range<usize>) -> Result<Self, TextRangeError> {
        let mut validator = TextFillMap::new();
        validator.assign_bytes(&self.text, range.clone(), TextFill::Solid(Hsla::default()))?;
        self.selection = Some(range);
        Ok(self)
    }

    /// Changes the selection highlight color.
    pub fn selection_background(mut self, color: impl Into<Hsla>) -> Self {
        self.selection_background = color.into();
        self
    }

    /// Returns the shared source text.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// Number of user-perceived characters indexed at creation.
    pub fn grapheme_count(&self) -> usize {
        self.grapheme_offsets.len().saturating_sub(1)
    }

    /// Produces bounded GPUI text runs for the current phase.
    pub fn color_runs(&self, inherited_style: &TextStyle) -> Vec<TextRun> {
        let grapheme_count = self.grapheme_count();
        if grapheme_count == 0 {
            return Vec::new();
        }
        let style = self.text_style.as_ref().unwrap_or(inherited_style);
        let regions = self.regions();
        let allocations = allocate_runs(&regions, self.max_color_runs);
        let motion_phase = match self.motion {
            Motion::Forward => self.phase,
            Motion::Reverse => -self.phase,
        };
        let mut runs = Vec::with_capacity(allocations.iter().sum());

        for (region, run_count) in regions.iter().zip(allocations) {
            let region_len = region.end - region.start;
            for run_index in 0..run_count {
                let start = region.start + run_index * region_len / run_count;
                let end = region.start + (run_index + 1) * region_len / run_count;
                let midpoint = (start + end) as f32 * 0.5;
                let color = match &region.fill {
                    RegionFill::Global => {
                        let position = midpoint / grapheme_count as f32;
                        self.palette.sample(position * self.cycles + motion_phase)
                    }
                    RegionFill::Solid(color) => *color,
                    RegionFill::Gradient {
                        palette,
                        anchor_start,
                        anchor_end,
                    } => {
                        let anchor_len = anchor_end - anchor_start;
                        let position = (midpoint - *anchor_start as f32) / anchor_len as f32;
                        palette.sample(position * self.cycles + motion_phase)
                    }
                };
                let byte_len = self.grapheme_offsets[end] - self.grapheme_offsets[start];
                let mut run = style.to_run(byte_len);
                run.color = color;
                if region.selected {
                    run.background_color = Some(self.selection_background);
                }
                runs.push(run);
            }
        }
        runs
    }

    /// Builds non-animated `StyledText` at the configured phase.
    pub fn styled_text(&self, inherited_style: &TextStyle) -> StyledText {
        StyledText::new(self.text.clone()).with_runs(self.color_runs(inherited_style))
    }

    /// Repeats this gradient using GPUI's display-linked scheduler.
    pub fn animated(self, id: impl Into<ElementId>, duration: Duration) -> AnimationElement<Self> {
        self.with_animation(
            id,
            Animation::new(duration.max(MIN_DURATION)).repeat(),
            |text, phase| text.phase(phase),
        )
    }

    fn regions(&self) -> Vec<RunRegion<'_>> {
        let count = self.grapheme_count();
        let mut boundaries = Vec::with_capacity(self.fills.spans().len() * 2 + 4);
        boundaries.extend([0, count]);
        for span in self.fills.spans() {
            boundaries.push(self.grapheme_index(span.range.start));
            boundaries.push(self.grapheme_index(span.range.end));
        }
        if let Some(selection) = &self.selection {
            boundaries.push(self.grapheme_index(selection.start));
            boundaries.push(self.grapheme_index(selection.end));
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        boundaries
            .windows(2)
            .filter_map(|pair| {
                let start = pair[0];
                let end = pair[1];
                (start < end).then(|| {
                    let byte_start = self.grapheme_offsets[start];
                    let span = self.fills.fill_at(byte_start);
                    let fill = match span.map(|span| &span.fill) {
                        None | Some(TextFill::Inherit) => RegionFill::Global,
                        Some(TextFill::Solid(color)) => RegionFill::Solid(*color),
                        Some(TextFill::Gradient(palette)) => {
                            let span = span.expect("gradient fill has a span");
                            RegionFill::Gradient {
                                palette,
                                anchor_start: self.grapheme_index(span.range.start),
                                anchor_end: self.grapheme_index(span.range.end),
                            }
                        }
                    };
                    let selected = self
                        .selection
                        .as_ref()
                        .is_some_and(|selection| selection.contains(&byte_start));
                    RunRegion {
                        start,
                        end,
                        fill,
                        selected,
                    }
                })
            })
            .collect()
    }

    #[cfg(test)]
    fn selection_from_points(&self, a: usize, b: usize) -> Range<usize> {
        let lower = a.min(b).min(self.text.len());
        let upper = a.max(b).min(self.text.len());
        let start_index = self
            .grapheme_offsets
            .partition_point(|offset| *offset <= lower)
            .saturating_sub(1)
            .min(self.grapheme_count().saturating_sub(1));
        let mut end_index = self
            .grapheme_offsets
            .partition_point(|offset| *offset < upper);
        if end_index <= start_index {
            end_index = start_index + 1;
        }
        self.grapheme_offsets[start_index]
            ..self.grapheme_offsets[end_index.min(self.grapheme_count())]
    }

    fn grapheme_index(&self, byte_offset: usize) -> usize {
        self.grapheme_offsets
            .binary_search(&byte_offset)
            .expect("validated fills are grapheme aligned")
    }
}

impl RenderOnce for GradientText {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.styled_text(&window.text_style())
    }
}

enum RegionFill<'a> {
    Global,
    Solid(Hsla),
    Gradient {
        palette: &'a GradientPalette,
        anchor_start: usize,
        anchor_end: usize,
    },
}

struct RunRegion<'a> {
    start: usize,
    end: usize,
    fill: RegionFill<'a>,
    selected: bool,
}

fn allocate_runs(regions: &[RunRegion<'_>], budget: usize) -> Vec<usize> {
    let minimum = regions.len();
    let budget = budget.max(minimum);
    let capacities = regions
        .iter()
        .map(|region| match &region.fill {
            RegionFill::Solid(_) => 0,
            RegionFill::Global | RegionFill::Gradient { .. } => region.end - region.start - 1,
        })
        .collect::<Vec<_>>();
    let total_capacity: usize = capacities.iter().sum();
    let mut allocations = vec![1; regions.len()];
    if total_capacity == 0 {
        return allocations;
    }
    let extra_budget = (budget - minimum).min(total_capacity);
    let mut assigned = 0;
    for (allocation, capacity) in allocations.iter_mut().zip(&capacities) {
        let extra = extra_budget * *capacity / total_capacity;
        *allocation += extra;
        assigned += extra;
    }
    let mut remaining = extra_budget - assigned;
    while remaining > 0 {
        let mut progressed = false;
        for ((allocation, capacity), region) in allocations.iter_mut().zip(&capacities).zip(regions)
        {
            let max_runs = region.end - region.start;
            if *capacity > 0 && *allocation < max_runs {
                *allocation += 1;
                remaining -= 1;
                progressed = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    allocations
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb, FontWeight};

    #[test]
    fn explicit_boundaries_survive_a_tiny_run_budget() {
        let source = "abcdefghij";
        let mut fills = TextFillMap::new();
        fills
            .assign_bytes(source, 2..4, TextFill::Solid(rgb(0xff0000).into()))
            .unwrap();
        fills
            .assign_bytes(source, 6..8, TextFill::Solid(rgb(0x0000ff).into()))
            .unwrap();
        let text = GradientText::phoenix(source)
            .with_fill_map(fills)
            .max_color_runs(1);
        let runs = text.color_runs(&TextStyle::default());
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), source.len());
        assert!(runs.len() >= 5);
    }

    #[test]
    fn inherited_font_style_and_selection_background_are_preserved() {
        let inherited = TextStyle {
            font_weight: FontWeight::BOLD,
            ..TextStyle::default()
        };
        let text = GradientText::phoenix("Phoenix")
            .selection_bytes(0..3)
            .unwrap();
        let runs = text.color_runs(&inherited);
        assert!(runs.iter().all(|run| run.font.weight == FontWeight::BOLD));
        assert!(runs.iter().any(|run| run.background_color.is_some()));
    }

    #[test]
    fn pointer_indexes_snap_to_whole_graphemes() {
        let text = GradientText::phoenix("A\u{301}👩🏽‍🚀Z");
        let astronaut_start = "A\u{301}".len();
        let astronaut_end = astronaut_start + "👩🏽‍🚀".len();
        assert_eq!(
            text.selection_from_points(astronaut_start + 1, astronaut_end - 1),
            astronaut_start..astronaut_end
        );
    }
}
