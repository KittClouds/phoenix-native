use crate::color::PreparedColor;
use crate::ColorSpace;
use gpui::Hsla;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, LazyLock};

static PHOENIX_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| {
    GradientPalette::from_hex([0x57e2bb, 0x78a8ff, 0xd779ff, 0xff7ab8])
        .expect("the built-in palette is valid")
        .with_color_space(ColorSpace::Oklab)
});

/// One positioned color in an immutable cyclic palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Cyclic position in the range `0.0..=1.0`.
    pub position: f32,
    /// GPUI color at this stop.
    pub color: Hsla,
}

impl GradientStop {
    /// Creates a positioned stop.
    pub const fn new(position: f32, color: Hsla) -> Self {
        Self { position, color }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PreparedStop {
    stop: GradientStop,
    color: PreparedColor,
}

/// A validated, cheaply cloned cyclic gradient palette.
#[derive(Clone, Debug)]
pub struct GradientPalette {
    stops: Arc<[PreparedStop]>,
    color_space: ColorSpace,
}

impl GradientPalette {
    /// Builds an evenly distributed cyclic palette from two or more colors.
    ///
    /// Positions use `index / color_count`, leaving a final interval for a
    /// smooth last-to-first transition.
    pub fn new<C>(colors: impl IntoIterator<Item = C>) -> Result<Self, GradientPaletteError>
    where
        C: Into<Hsla>,
    {
        let colors = colors.into_iter().map(Into::into).collect::<Vec<_>>();
        let count = colors.len();
        if count < 2 {
            return Err(GradientPaletteError::TooFewColors { provided: count });
        }
        Self::from_stops(
            colors
                .into_iter()
                .enumerate()
                .map(|(index, color)| GradientStop::new(index as f32 / count as f32, color)),
        )
    }

    /// Builds a palette from explicit positioned stops.
    pub fn from_stops(
        stops: impl IntoIterator<Item = GradientStop>,
    ) -> Result<Self, GradientPaletteError> {
        let mut stops = stops.into_iter().collect::<Vec<_>>();
        if stops.len() < 2 {
            return Err(GradientPaletteError::TooFewColors {
                provided: stops.len(),
            });
        }
        for (index, stop) in stops.iter().enumerate() {
            if !stop.position.is_finite() {
                return Err(GradientPaletteError::NonFinitePosition { index });
            }
            if !(0.0..=1.0).contains(&stop.position) {
                return Err(GradientPaletteError::PositionOutOfRange {
                    index,
                    position: stop.position,
                });
            }
            if ![stop.color.h, stop.color.s, stop.color.l, stop.color.a]
                .into_iter()
                .all(f32::is_finite)
            {
                return Err(GradientPaletteError::NonFiniteColor { index });
            }
        }
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        Ok(Self {
            stops: stops
                .into_iter()
                .map(|stop| PreparedStop {
                    color: PreparedColor::new(stop.color),
                    stop,
                })
                .collect::<Vec<_>>()
                .into(),
            color_space: ColorSpace::Srgb,
        })
    }

    /// Builds a palette from `0xRRGGBB` values.
    pub fn from_hex(colors: impl IntoIterator<Item = u32>) -> Result<Self, GradientPaletteError> {
        Self::new(colors.into_iter().map(gpui::rgb))
    }

    /// Phoenix's teal, blue, violet, and rose Oklab palette.
    pub fn phoenix() -> Self {
        PHOENIX_PALETTE.clone()
    }

    /// Changes interpolation without rebuilding stop storage.
    pub fn with_color_space(mut self, color_space: ColorSpace) -> Self {
        self.color_space = color_space;
        self
    }

    /// Interpolation space.
    pub const fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    /// Number of stops.
    pub fn len(&self) -> usize {
        self.stops.len()
    }

    /// Valid palettes always have at least two stops.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Iterates the immutable positioned stops.
    pub fn stops(&self) -> impl ExactSizeIterator<Item = GradientStop> + '_ {
        self.stops.iter().map(|stop| stop.stop)
    }

    /// Samples the cyclic palette at any finite position without allocation.
    pub fn sample(&self, position: f32) -> Hsla {
        let position = if position.is_finite() {
            position.rem_euclid(1.0)
        } else {
            0.0
        };
        let upper = self
            .stops
            .partition_point(|stop| stop.stop.position <= position);
        let (lower, upper_stop, sample_position) = if upper == 0 {
            (
                self.stops[self.stops.len() - 1],
                self.stops[0],
                position + 1.0,
            )
        } else if upper == self.stops.len() {
            (self.stops[upper - 1], self.stops[0], position)
        } else {
            (self.stops[upper - 1], self.stops[upper], position)
        };
        let upper_position = if upper == 0 || upper == self.stops.len() {
            upper_stop.stop.position + 1.0
        } else {
            upper_stop.stop.position
        };
        let lower_position = if upper == 0 {
            lower.stop.position - 1.0
        } else {
            lower.stop.position
        };
        let width = upper_position - lower_position;
        let amount = if width <= f32::EPSILON {
            1.0
        } else {
            (sample_position - lower_position) / width
        };
        lower
            .color
            .interpolate(upper_stop.color, amount, self.color_space)
    }
}

impl Default for GradientPalette {
    fn default() -> Self {
        Self::phoenix()
    }
}

impl PartialEq for GradientPalette {
    fn eq(&self, other: &Self) -> bool {
        self.color_space == other.color_space && self.stops == other.stops
    }
}

/// Stable identity of a stop while a palette is being edited.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[repr(transparent)]
pub struct GradientStopId(pub u64);

/// Editable stop with stable identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditableGradientStop {
    /// Stable authoring identity.
    pub id: GradientStopId,
    /// Cyclic stop position.
    pub position: f32,
    /// Stop color.
    pub color: Hsla,
}

/// A zero-, one-, or many-color authoring state.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientDraft {
    stops: Vec<EditableGradientStop>,
    next_id: u64,
    color_space: ColorSpace,
}

impl Default for GradientDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl GradientDraft {
    /// Starts with no fill.
    pub const fn new() -> Self {
        Self {
            stops: Vec::new(),
            next_id: 1,
            color_space: ColorSpace::Srgb,
        }
    }

    /// Creates an editable draft from an immutable palette.
    pub fn from_palette(palette: &GradientPalette) -> Self {
        let mut draft = Self::new();
        draft.color_space = palette.color_space();
        for stop in palette.stops() {
            let id = GradientStopId(draft.next_id);
            draft.next_id += 1;
            draft.stops.push(EditableGradientStop {
                id,
                position: stop.position,
                color: stop.color,
            });
        }
        draft
    }

    /// Current ordered stops.
    pub fn stops(&self) -> &[EditableGradientStop] {
        &self.stops
    }

    /// Current interpolation space.
    pub const fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    /// Changes interpolation for the draft.
    pub fn set_color_space(&mut self, color_space: ColorSpace) {
        self.color_space = color_space;
    }

    /// Adds a confirmed color after `after`, then evenly distributes stops.
    pub fn insert_color(&mut self, after: Option<GradientStopId>, color: Hsla) -> GradientStopId {
        let id = GradientStopId(self.next_id);
        self.next_id += 1;
        let index = after
            .and_then(|id| self.stops.iter().position(|stop| stop.id == id))
            .map_or(self.stops.len(), |index| index + 1);
        self.stops.insert(
            index,
            EditableGradientStop {
                id,
                position: 0.0,
                color,
            },
        );
        self.redistribute();
        id
    }

    /// Updates one stop's color.
    pub fn set_color(&mut self, id: GradientStopId, color: Hsla) -> bool {
        let Some(stop) = self.stops.iter_mut().find(|stop| stop.id == id) else {
            return false;
        };
        stop.color = color;
        true
    }

    /// Sets a stop position and restores stable position ordering.
    pub fn set_position(&mut self, id: GradientStopId, position: f32) -> bool {
        if !position.is_finite() {
            return false;
        }
        let Some(stop) = self.stops.iter_mut().find(|stop| stop.id == id) else {
            return false;
        };
        stop.position = position.clamp(0.0, 1.0);
        self.stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        true
    }

    /// Moves a stop one slot and redistributes positions.
    pub fn move_stop(&mut self, id: GradientStopId, delta: isize) -> bool {
        let Some(index) = self.stops.iter().position(|stop| stop.id == id) else {
            return false;
        };
        let target = index
            .saturating_add_signed(delta)
            .min(self.stops.len().saturating_sub(1));
        if target == index {
            return false;
        }
        self.stops.swap(index, target);
        self.redistribute();
        true
    }

    /// Removes one stop.
    pub fn remove(&mut self, id: GradientStopId) -> bool {
        let Some(index) = self.stops.iter().position(|stop| stop.id == id) else {
            return false;
        };
        self.stops.remove(index);
        self.redistribute();
        true
    }

    /// Resolves two or more stops into an immutable palette.
    pub fn to_palette(&self) -> Result<GradientPalette, GradientPaletteError> {
        GradientPalette::from_stops(
            self.stops
                .iter()
                .map(|stop| GradientStop::new(stop.position, stop.color)),
        )
        .map(|palette| palette.with_color_space(self.color_space))
    }

    fn redistribute(&mut self) {
        let count = self.stops.len();
        if count == 0 {
            return;
        }
        for (index, stop) in self.stops.iter_mut().enumerate() {
            stop.position = index as f32 / count as f32;
        }
    }
}

/// Why an immutable palette could not be created.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientPaletteError {
    /// A gradient needs at least two colors.
    TooFewColors { provided: usize },
    /// Color components must be finite.
    NonFiniteColor { index: usize },
    /// Stop positions must be finite.
    NonFinitePosition { index: usize },
    /// Stop positions must be inside the cyclic unit interval.
    PositionOutOfRange { index: usize, position: f32 },
}

impl Display for GradientPaletteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewColors { provided } => {
                write!(
                    formatter,
                    "a gradient needs at least 2 colors; received {provided}"
                )
            }
            Self::NonFiniteColor { index } => {
                write!(
                    formatter,
                    "palette color {index} contains a non-finite component"
                )
            }
            Self::NonFinitePosition { index } => {
                write!(formatter, "palette stop {index} has a non-finite position")
            }
            Self::PositionOutOfRange { index, position } => {
                write!(
                    formatter,
                    "palette stop {index} position {position} is outside 0..=1"
                )
            }
        }
    }
}

impl Error for GradientPaletteError {}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb, Rgba};

    #[test]
    fn auto_positions_leave_a_cyclic_seam_interval() {
        let palette = GradientPalette::from_hex([0xff0000, 0x00ff00, 0x0000ff]).unwrap();
        let positions = palette
            .stops()
            .map(|stop| stop.position)
            .collect::<Vec<_>>();
        assert_eq!(positions, [0.0, 1.0 / 3.0, 2.0 / 3.0]);
        assert_eq!(palette.sample(0.0), palette.sample(1.0));
    }

    #[test]
    fn explicit_hard_stop_uses_the_last_duplicate() {
        let palette = GradientPalette::from_stops([
            GradientStop::new(0.0, rgb(0xff0000).into()),
            GradientStop::new(0.5, rgb(0xff0000).into()),
            GradientStop::new(0.5, rgb(0x0000ff).into()),
        ])
        .unwrap();
        let at_stop = Rgba::from(palette.sample(0.5));
        let blue = rgb(0x0000ff);
        assert!((at_stop.b - blue.b).abs() < 0.001);
    }

    #[test]
    fn draft_supports_empty_solid_and_gradient_states() {
        let mut draft = GradientDraft::new();
        assert!(draft.to_palette().is_err());
        let first = draft.insert_color(None, rgb(0xff0000).into());
        assert!(draft.to_palette().is_err());
        draft.insert_color(Some(first), rgb(0x0000ff).into());
        assert_eq!(draft.to_palette().unwrap().len(), 2);
    }
}
