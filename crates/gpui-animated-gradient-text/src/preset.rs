use crate::{ColorSpace, GradientPalette, GradientPaletteError, GradientStop, Motion};
use gpui::Hsla;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

/// Current portable preset schema.
pub const GRADIENT_PRESET_VERSION: u16 = 1;

/// One serializable positioned stop.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct PresetStop {
    /// Cyclic position.
    pub position: f32,
    /// Stop color.
    pub color: Hsla,
}

/// Versioned, text-independent animated gradient preset.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct GradientPresetV1 {
    /// Must equal [`GRADIENT_PRESET_VERSION`].
    pub version: u16,
    /// Human-readable preset name.
    pub name: String,
    /// Positioned colors.
    pub stops: Vec<PresetStop>,
    /// Color interpolation space.
    pub color_space: ColorSpace,
    /// Palette travel direction.
    pub motion: Motion,
    /// Spatial cycles across the text.
    pub cycles: f32,
    /// Animation period in milliseconds.
    pub duration_ms: u64,
}

impl GradientPresetV1 {
    /// Captures a reusable preset.
    pub fn new(
        name: impl Into<String>,
        palette: &GradientPalette,
        motion: Motion,
        cycles: f32,
        duration: Duration,
    ) -> Self {
        Self {
            version: GRADIENT_PRESET_VERSION,
            name: name.into(),
            stops: palette
                .stops()
                .map(|stop| PresetStop {
                    position: stop.position,
                    color: stop.color,
                })
                .collect(),
            color_space: palette.color_space(),
            motion,
            cycles,
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        }
    }

    /// Validates and reconstructs the immutable palette.
    pub fn to_palette(&self) -> Result<GradientPalette, GradientPresetError> {
        self.validate()?;
        GradientPalette::from_stops(
            self.stops
                .iter()
                .map(|stop| GradientStop::new(stop.position, stop.color)),
        )
        .map(|palette| palette.with_color_space(self.color_space))
        .map_err(GradientPresetError::Palette)
    }

    /// Validates portable animation metadata.
    pub fn validate(&self) -> Result<(), GradientPresetError> {
        if self.version != GRADIENT_PRESET_VERSION {
            return Err(GradientPresetError::UnsupportedVersion(self.version));
        }
        if !self.cycles.is_finite() || self.cycles <= 0.0 {
            return Err(GradientPresetError::InvalidCycles(self.cycles));
        }
        if self.duration_ms == 0 {
            return Err(GradientPresetError::ZeroDuration);
        }
        Ok(())
    }

    /// Emits a pasteable Rust builder expression.
    pub fn to_rust_builder(&self) -> Result<String, GradientPresetError> {
        self.to_palette()?;
        let mut output = String::from("GradientPalette::from_stops([\n");
        for stop in &self.stops {
            output.push_str(&format!(
                "    GradientStop::new({:.6}, gpui::hsla({:.8}, {:.8}, {:.8}, {:.8})),\n",
                stop.position, stop.color.h, stop.color.s, stop.color.l, stop.color.a,
            ));
        }
        output.push_str(&format!(
            "]).unwrap().with_color_space(ColorSpace::{:?})",
            self.color_space
        ));
        Ok(output)
    }

    /// Built-in Phoenix preset.
    pub fn phoenix() -> Self {
        Self::new(
            "Phoenix",
            &GradientPalette::phoenix(),
            Motion::Forward,
            0.85,
            Duration::from_millis(3_800),
        )
    }

    /// Built-in blue-green aurora.
    pub fn aurora() -> Self {
        let palette = GradientPalette::from_hex([0x42f5b3, 0x58a6ff, 0x9b7bff])
            .expect("built-in palette")
            .with_color_space(ColorSpace::Oklab);
        Self::new(
            "Aurora",
            &palette,
            Motion::Forward,
            1.0,
            Duration::from_millis(4_600),
        )
    }

    /// Built-in amber-rose sunset.
    pub fn sunset() -> Self {
        let palette = GradientPalette::from_hex([0xffc857, 0xff7b72, 0xd779ff])
            .expect("built-in palette")
            .with_color_space(ColorSpace::Oklab);
        Self::new(
            "Sunset",
            &palette,
            Motion::Reverse,
            0.9,
            Duration::from_millis(5_000),
        )
    }

    /// All named built-ins.
    pub fn builtins() -> [Self; 3] {
        [Self::phoenix(), Self::aurora(), Self::sunset()]
    }

    /// Serializes pretty JSON when the `serde` feature is enabled.
    #[cfg(feature = "serde")]
    pub fn to_json_pretty(&self) -> Result<String, GradientPresetError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(GradientPresetError::Json)
    }

    /// Parses and validates JSON when the `serde` feature is enabled.
    #[cfg(feature = "serde")]
    pub fn from_json(json: &str) -> Result<Self, GradientPresetError> {
        let preset: Self = serde_json::from_str(json).map_err(GradientPresetError::Json)?;
        preset.to_palette()?;
        Ok(preset)
    }
}

/// Why a portable preset was rejected.
#[derive(Debug)]
pub enum GradientPresetError {
    /// Unknown schema version.
    UnsupportedVersion(u16),
    /// Invalid palette data.
    Palette(GradientPaletteError),
    /// Spatial cycle count must be finite and positive.
    InvalidCycles(f32),
    /// Animation periods must be non-zero.
    ZeroDuration,
    /// Malformed JSON.
    #[cfg(feature = "serde")]
    Json(serde_json::Error),
}

impl Display for GradientPresetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported gradient preset version {version}")
            }
            Self::Palette(error) => Display::fmt(error, formatter),
            Self::InvalidCycles(cycles) => {
                write!(
                    formatter,
                    "preset cycles must be positive and finite, received {cycles}"
                )
            }
            Self::ZeroDuration => formatter.write_str("preset duration must be non-zero"),
            #[cfg(feature = "serde")]
            Self::Json(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for GradientPresetError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_exportable() {
        for preset in GradientPresetV1::builtins() {
            assert!(preset.to_palette().is_ok());
            assert!(preset
                .to_rust_builder()
                .unwrap()
                .contains("GradientStop::new"));
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn json_round_trip_is_exact() {
        let preset = GradientPresetV1::phoenix();
        let json = preset.to_json_pretty().unwrap();
        assert_eq!(GradientPresetV1::from_json(&json).unwrap(), preset);
    }
}
