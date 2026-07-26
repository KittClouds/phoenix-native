//! Fast animated gradient text for [GPUI](https://crates.io/crates/gpui).
//!
//! `GradientText` colors one shaped `StyledText` with a bounded number of text
//! runs. Unicode grapheme boundaries are indexed once when the value is built,
//! so rendering never splits a user-perceived character.
//!
//! ```
//! use gpui::{ElementId, rgb};
//! use gpui_animated_gradient_text::{GradientPalette, GradientText};
//! use std::time::Duration;
//!
//! let palette = GradientPalette::new([
//!     rgb(0x57e2bb),
//!     rgb(0x78a8ff),
//!     rgb(0xd779ff),
//! ]).unwrap();
//!
//! let text = GradientText::new("PHOENIX", palette)
//!     .cycles(0.8)
//!     .max_color_runs(32);
//!
//! // In a GPUI render method:
//! // div().child(text.clone().animated(
//! //     ElementId::Name("phoenix-gradient".into()),
//! //     Duration::from_secs(4),
//! // ))
//! # let _ = (text, ElementId::Name("unused".into()), Duration::ZERO);
//! ```

use gpui::{
    Animation, AnimationElement, AnimationExt, App, ElementId, Hsla, IntoElement, RenderOnce, Rgba,
    SharedString, StyledText, TextRun, TextStyle, Window,
};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

const DEFAULT_MAX_COLOR_RUNS: usize = 64;
const MIN_DURATION: Duration = Duration::from_millis(1);
const MIN_CYCLES: f32 = 0.01;
const MAX_CYCLES: f32 = 64.0;

static PHOENIX_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| {
    GradientPalette::from_hex([0x57e2bb, 0x78a8ff, 0xd779ff, 0xff7ab8])
        .expect("the built-in palette is valid")
});

/// The direction in which palette colors travel through the text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Motion {
    /// Advance through the palette as animation time increases.
    #[default]
    Forward,
    /// Travel through the palette in the opposite direction.
    Reverse,
}

/// A validated, cheaply cloned cyclic palette.
///
/// Colors are stored as compact RGBA values behind one `Arc`. Sampling treats
/// the palette as a loop, including the transition from the last color back to
/// the first, so repeating animations never jump at the cycle boundary.
#[derive(Clone, Debug)]
pub struct GradientPalette {
    colors: Arc<[Rgba]>,
}

impl GradientPalette {
    /// Builds a palette from two or more GPUI colors.
    pub fn new<C>(colors: impl IntoIterator<Item = C>) -> Result<Self, GradientPaletteError>
    where
        C: Into<Hsla>,
    {
        let colors = colors
            .into_iter()
            .enumerate()
            .map(|(index, color)| {
                let color = color.into();
                if [color.h, color.s, color.l, color.a]
                    .into_iter()
                    .all(f32::is_finite)
                {
                    Ok(Rgba::from(color))
                } else {
                    Err(GradientPaletteError::NonFiniteColor { index })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        if colors.len() < 2 {
            return Err(GradientPaletteError::TooFewColors {
                provided: colors.len(),
            });
        }

        Ok(Self {
            colors: colors.into(),
        })
    }

    /// Builds a palette from `0xRRGGBB` color values.
    pub fn from_hex(colors: impl IntoIterator<Item = u32>) -> Result<Self, GradientPaletteError> {
        Self::new(colors.into_iter().map(gpui::rgb))
    }

    /// Phoenix's teal, blue, violet, and rose palette.
    pub fn phoenix() -> Self {
        PHOENIX_PALETTE.clone()
    }

    /// Number of colors in this palette.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Palettes always contain at least two colors.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Samples the cyclic palette at any finite position.
    ///
    /// Integer boundaries wrap, so `sample(0.0) == sample(1.0)`.
    pub fn sample(&self, position: f32) -> Hsla {
        let position = if position.is_finite() { position } else { 0.0 };
        let scaled = position.rem_euclid(1.0) * self.colors.len() as f32;
        let lower = scaled.floor() as usize % self.colors.len();
        let upper = (lower + 1) % self.colors.len();
        let amount = scaled.fract();
        let a = self.colors[lower];
        let b = self.colors[upper];
        Rgba {
            r: a.r + (b.r - a.r) * amount,
            g: a.g + (b.g - a.g) * amount,
            b: a.b + (b.b - a.b) * amount,
            a: a.a + (b.a - a.a) * amount,
        }
        .into()
    }
}

impl Default for GradientPalette {
    fn default() -> Self {
        Self::phoenix()
    }
}

/// Why a palette could not be created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GradientPaletteError {
    /// A cyclic gradient needs at least two colors.
    TooFewColors { provided: usize },
    /// GPUI color components must be finite.
    NonFiniteColor { index: usize },
}

impl Display for GradientPaletteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewColors { provided } => {
                write!(
                    formatter,
                    "a gradient palette needs at least 2 colors; received {provided}"
                )
            }
            Self::NonFiniteColor { index } => {
                write!(
                    formatter,
                    "palette color {index} contains a non-finite component"
                )
            }
        }
    }
}

impl Error for GradientPaletteError {}

/// Unicode-aware gradient text that inherits the surrounding GPUI text style.
///
/// Clone and reuse this value from view state. The text and grapheme index are
/// immutable shared storage, so clones do not rescan or copy the string.
#[derive(Clone, IntoElement)]
pub struct GradientText {
    text: SharedString,
    grapheme_offsets: Arc<[usize]>,
    palette: GradientPalette,
    phase: f32,
    cycles: f32,
    max_color_runs: usize,
    motion: Motion,
    text_style: Option<TextStyle>,
}

impl GradientText {
    /// Indexes `text` once and uses `palette` as a seamless color loop.
    pub fn new(text: impl Into<SharedString>, palette: GradientPalette) -> Self {
        let text = text.into();
        let mut offsets = Vec::with_capacity(text.len().min(64) + 1);
        offsets.push(0);
        offsets.extend(
            text.grapheme_indices(true)
                .skip(1)
                .map(|(byte_offset, _)| byte_offset),
        );
        if !text.is_empty() {
            offsets.push(text.len());
        }

        Self {
            text,
            grapheme_offsets: offsets.into(),
            palette,
            phase: 0.0,
            cycles: 1.0,
            max_color_runs: DEFAULT_MAX_COLOR_RUNS,
            motion: Motion::Forward,
            text_style: None,
        }
    }

    /// Uses the built-in Phoenix palette.
    pub fn phoenix(text: impl Into<SharedString>) -> Self {
        Self::new(text, GradientPalette::phoenix())
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

    /// Sets how many palette cycles span the text.
    pub fn cycles(mut self, cycles: f32) -> Self {
        self.cycles = if cycles.is_finite() {
            cycles.abs().clamp(MIN_CYCLES, MAX_CYCLES)
        } else {
            1.0
        };
        self
    }

    /// Bounds shaped color runs and the single per-frame `Vec` allocation.
    ///
    /// Long text is divided into at most this many contiguous bands. `0`
    /// becomes `1`. The default is 64.
    pub fn max_color_runs(mut self, max_color_runs: usize) -> Self {
        self.max_color_runs = max_color_runs.max(1);
        self
    }

    /// Sets the palette's travel direction.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Overrides inherited GPUI text styling except for the animated color.
    pub fn text_style(mut self, text_style: TextStyle) -> Self {
        self.text_style = Some(text_style);
        self
    }

    /// Returns the shared source text.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// Returns the number of user-perceived characters indexed at creation.
    pub fn grapheme_count(&self) -> usize {
        self.grapheme_offsets.len().saturating_sub(1)
    }

    /// Produces the bounded GPUI text runs for the current phase.
    ///
    /// This is public for deterministic testing, snapshotting, and benchmarks
    /// without opening a native window.
    pub fn color_runs(&self, inherited_style: &TextStyle) -> Vec<TextRun> {
        let grapheme_count = self.grapheme_count();
        if grapheme_count == 0 {
            return Vec::new();
        }

        let style = self.text_style.as_ref().unwrap_or(inherited_style);
        let run_count = grapheme_count.min(self.max_color_runs);
        let mut runs = Vec::with_capacity(run_count);
        let motion = match self.motion {
            Motion::Forward => self.phase,
            Motion::Reverse => -self.phase,
        };

        for run_index in 0..run_count {
            let start = run_index * grapheme_count / run_count;
            let end = (run_index + 1) * grapheme_count / run_count;
            let midpoint = (start + end) as f32 * 0.5 / grapheme_count as f32;
            let color = self.palette.sample(midpoint * self.cycles + motion);
            let byte_len = self.grapheme_offsets[end] - self.grapheme_offsets[start];
            let mut run = style.to_run(byte_len);
            run.color = color;
            runs.push(run);
        }

        runs
    }

    /// Builds a non-animated `StyledText` at the configured phase.
    pub fn styled_text(&self, inherited_style: &TextStyle) -> StyledText {
        StyledText::new(self.text.clone()).with_runs(self.color_runs(inherited_style))
    }

    /// Repeats this gradient using GPUI's display-linked frame scheduler.
    ///
    /// `id` must remain stable between renders. Durations below one
    /// millisecond are clamped to one millisecond.
    pub fn animated(self, id: impl Into<ElementId>, duration: Duration) -> AnimationElement<Self> {
        self.with_animation(
            id,
            Animation::new(duration.max(MIN_DURATION)).repeat(),
            |text, phase| text.phase(phase),
        )
    }
}

impl RenderOnce for GradientText {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let inherited_style = window.text_style();
        self.styled_text(&inherited_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{hsla, rgb, FontWeight};

    #[test]
    fn palette_rejects_invalid_inputs() {
        assert_eq!(
            GradientPalette::new([rgb(0xff00ff)]).unwrap_err(),
            GradientPaletteError::TooFewColors { provided: 1 }
        );
        assert_eq!(
            GradientPalette::new([hsla(f32::NAN, 1.0, 0.5, 1.0), hsla(0.5, 1.0, 0.5, 1.0),])
                .unwrap_err(),
            GradientPaletteError::NonFiniteColor { index: 0 }
        );
    }

    #[test]
    fn cyclic_palette_has_no_phase_seam() {
        let palette = GradientPalette::from_hex([0xff0000, 0x00ff00, 0x0000ff]).unwrap();
        assert_eq!(palette.sample(0.0), palette.sample(1.0));
        assert_eq!(palette.sample(-0.25), palette.sample(0.75));
    }

    #[test]
    fn grapheme_index_preserves_composed_characters_and_emoji() {
        let text = GradientText::phoenix("A\u{301} 👩🏽‍🚀 Z").max_color_runs(128);
        assert_eq!(text.grapheme_count(), 5);
        let runs = text.color_runs(&TextStyle::default());
        assert_eq!(runs.len(), 5);
        assert_eq!(
            runs.iter().map(|run| run.len).sum::<usize>(),
            text.text().len()
        );
        assert_eq!(runs[0].len, "A\u{301}".len());
        assert_eq!(runs[2].len, "👩🏽‍🚀".len());
    }

    #[test]
    fn long_text_respects_the_run_budget_and_covers_every_byte() {
        let source = "gradient ".repeat(2_000);
        let text = GradientText::phoenix(source.clone()).max_color_runs(24);
        let runs = text.color_runs(&TextStyle::default());
        assert_eq!(runs.len(), 24);
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), source.len());
    }

    #[test]
    fn inherited_font_style_is_preserved() {
        let inherited = TextStyle {
            font_weight: FontWeight::BOLD,
            ..TextStyle::default()
        };
        let runs = GradientText::phoenix("Phoenix").color_runs(&inherited);
        assert!(runs.iter().all(|run| run.font.weight == FontWeight::BOLD));
    }

    #[test]
    fn configuration_sanitizes_non_finite_values() {
        let inherited = TextStyle::default();
        let text = GradientText::phoenix("Phoenix")
            .phase(f32::NAN)
            .cycles(f32::INFINITY)
            .max_color_runs(0);
        assert_eq!(text.color_runs(&inherited).len(), 1);
    }
}
