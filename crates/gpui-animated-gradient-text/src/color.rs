use gpui::{Hsla, Rgba};

/// Color space used between gradient stops.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ColorSpace {
    /// Interpolate encoded sRGB channels. Fast and compatible with v0.1.
    #[default]
    Srgb,
    /// Interpolate linear-light sRGB channels.
    LinearSrgb,
    /// Interpolate perceptually uniform Oklab components.
    Oklab,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreparedColor {
    srgb: Rgba,
    linear: [f32; 3],
    oklab: [f32; 3],
}

impl PreparedColor {
    pub(crate) fn new(color: Hsla) -> Self {
        let srgb = Rgba::from(color);
        let linear = [
            srgb_to_linear(srgb.r),
            srgb_to_linear(srgb.g),
            srgb_to_linear(srgb.b),
        ];
        let oklab = linear_to_oklab(linear);
        Self {
            srgb,
            linear,
            oklab,
        }
    }

    pub(crate) fn interpolate(self, other: Self, amount: f32, space: ColorSpace) -> Hsla {
        let amount = amount.clamp(0.0, 1.0);
        let alpha = lerp(self.srgb.a, other.srgb.a, amount);
        let channels = match space {
            ColorSpace::Srgb => [
                lerp(self.srgb.r, other.srgb.r, amount),
                lerp(self.srgb.g, other.srgb.g, amount),
                lerp(self.srgb.b, other.srgb.b, amount),
            ],
            ColorSpace::LinearSrgb => linear_to_srgb([
                lerp(self.linear[0], other.linear[0], amount),
                lerp(self.linear[1], other.linear[1], amount),
                lerp(self.linear[2], other.linear[2], amount),
            ]),
            ColorSpace::Oklab => {
                let lab = [
                    lerp(self.oklab[0], other.oklab[0], amount),
                    lerp(self.oklab[1], other.oklab[1], amount),
                    lerp(self.oklab[2], other.oklab[2], amount),
                ];
                linear_to_srgb(oklab_to_linear(lab))
            }
        };
        Rgba {
            r: channels[0].clamp(0.0, 1.0),
            g: channels[1].clamp(0.0, 1.0),
            b: channels[2].clamp(0.0, 1.0),
            a: alpha.clamp(0.0, 1.0),
        }
        .into()
    }
}

#[inline]
fn lerp(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount
}

#[inline]
fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb_channel(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        12.92 * channel
    } else {
        1.055 * channel.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

fn linear_to_srgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        linear_to_srgb_channel(rgb[0]),
        linear_to_srgb_channel(rgb[1]),
        linear_to_srgb_channel(rgb[2]),
    ]
}

fn linear_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * rgb[0] + 0.536_332_55 * rgb[1] + 0.051_445_995 * rgb[2];
    let m = 0.211_903_5 * rgb[0] + 0.680_699_5 * rgb[1] + 0.107_396_96 * rgb[2];
    let s = 0.088_302_46 * rgb[0] + 0.281_718_85 * rgb[1] + 0.629_978_7 * rgb[2];
    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::rgb;

    #[test]
    fn interpolation_preserves_endpoints_in_every_space() {
        let a = PreparedColor::new(rgb(0x57e2bb).into());
        let b = PreparedColor::new(rgb(0xd779ff).into());
        for space in [ColorSpace::Srgb, ColorSpace::LinearSrgb, ColorSpace::Oklab] {
            let a_rgb = Rgba::from(a.interpolate(b, 0.0, space));
            let b_rgb = Rgba::from(a.interpolate(b, 1.0, space));
            assert!((a_rgb.r - a.srgb.r).abs() < 0.0001);
            assert!((b_rgb.b - b.srgb.b).abs() < 0.0001);
        }
    }
}
