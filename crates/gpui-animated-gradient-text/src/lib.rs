//! Animated, editable gradient text for [GPUI](https://crates.io/crates/gpui).
//!
//! The default feature set contains the immutable renderer, Unicode-safe range
//! fills, palette authoring model, selection element, and built-in presets.
//! Enable `picker-ui` for the optional `gpui-component` color-picker bridge,
//! and `serde` to import and export versioned JSON presets.
//!
//! ```
//! use gpui::rgb;
//! use gpui_animated_gradient_text::{
//!     ColorSpace, GradientPalette, GradientStop, GradientText,
//! };
//!
//! let palette = GradientPalette::from_stops([
//!     GradientStop::new(0.0, rgb(0x57e2bb).into()),
//!     GradientStop::new(0.42, rgb(0x78a8ff).into()),
//!     GradientStop::new(0.78, rgb(0xd779ff).into()),
//! ])
//! .unwrap()
//! .with_color_space(ColorSpace::Oklab);
//!
//! let text = GradientText::new("PHOENIX", palette)
//!     .cycles(0.8)
//!     .max_color_runs(32);
//! # let _ = text;
//! ```

mod color;
mod fills;
mod palette;
mod preset;
mod selectable;
mod text;

#[cfg(feature = "picker-ui")]
mod picker;

pub use color::ColorSpace;
pub use fills::{FillSpan, TextFill, TextFillMap, TextRangeError};
pub use palette::{
    EditableGradientStop, GradientDraft, GradientPalette, GradientPaletteError, GradientStop,
    GradientStopId,
};
pub use preset::{GradientPresetError, GradientPresetV1, PresetStop, GRADIENT_PRESET_VERSION};
pub use selectable::SelectableGradientText;
pub use text::{GradientText, Motion};

#[cfg(feature = "picker-ui")]
pub use picker::{GradientPicker, GradientPickerEvent, GradientPickerState};

#[cfg(all(test, feature = "serde"))]
mod v02_e2e_tests {
    use super::*;
    use gpui::{rgb, TextStyle};
    use std::time::Duration;

    #[test]
    fn authoring_range_render_and_preset_round_trip() {
        let source = "alpha βeta gamma";
        let mut draft = GradientDraft::new();
        assert_eq!(draft.resolved_fill(), TextFill::Inherit);

        let teal = draft.insert_color(None, rgb(0x57e2bb).into());
        assert!(matches!(draft.resolved_fill(), TextFill::Solid(_)));
        let violet = draft.insert_color(Some(teal), rgb(0xd779ff).into());
        draft.insert_color(Some(violet), rgb(0xff7ab8).into());
        draft.set_position(violet, 0.72);
        draft.set_color_space(ColorSpace::Oklab);

        let TextFill::Gradient(palette) = draft.resolved_fill() else {
            panic!("three stops must resolve to a gradient");
        };
        assert_eq!(palette.color_space(), ColorSpace::Oklab);

        let start = source.find('β').unwrap();
        let end = start + "βeta".len();
        let mut fills = TextFillMap::new();
        fills
            .assign_bytes(source, start..end, TextFill::Gradient(palette.clone()))
            .unwrap();
        let text = GradientText::phoenix(source)
            .with_fill_map(fills)
            .selection_bytes(start..end)
            .unwrap();
        let runs = text.color_runs(&TextStyle::default());
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), source.len());
        assert!(runs.iter().any(|run| run.background_color.is_some()));

        let preset = GradientPresetV1::new(
            "E2E",
            &palette,
            Motion::Forward,
            1.25,
            Duration::from_millis(3_600),
        );
        let json = preset.to_json_pretty().unwrap();
        let restored = GradientPresetV1::from_json(&json).unwrap();
        assert_eq!(restored, preset);
        assert!(restored
            .to_rust_builder()
            .unwrap()
            .contains("ColorSpace::Oklab"));
    }
}
