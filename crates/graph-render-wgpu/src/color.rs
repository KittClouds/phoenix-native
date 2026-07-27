#[inline]
pub(crate) fn srgb_rgba_to_linear(mut color: [f32; 4]) -> [f32; 4] {
    color[0] = srgb_channel_to_linear(color[0]);
    color[1] = srgb_channel_to_linear(color[1]);
    color[2] = srgb_channel_to_linear(color[2]);
    color
}

#[inline]
pub(crate) fn rich_edge_rgba(color: [f32; 4]) -> [f32; 4] {
    const MAX_LINEAR_CHANNEL: f32 = 0.48;
    const NEUTRAL_FALLBACK: [f32; 3] = [0.135, 0.392, 0.479];
    let mut linear = srgb_rgba_to_linear(color);
    let minimum = linear[0].min(linear[1]).min(linear[2]);
    let maximum = linear[0].max(linear[1]).max(linear[2]);
    if maximum - minimum < 0.025 {
        linear[..3].copy_from_slice(&NEUTRAL_FALLBACK);
        return linear;
    }

    for channel in &mut linear[..3] {
        *channel = (*channel - minimum * 0.86).max(0.0);
    }
    let rich_peak = linear[0].max(linear[1]).max(linear[2]);
    if rich_peak > MAX_LINEAR_CHANNEL {
        let scale = MAX_LINEAR_CHANNEL / rich_peak;
        for channel in &mut linear[..3] {
            *channel *= scale;
        }
    }
    linear
}

#[inline]
fn srgb_channel_to_linear(channel: f32) -> f32 {
    let channel = channel.clamp(0.0, 1.0);
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[must_use]
pub(crate) fn dense_edge_opacity(edge_count: usize) -> f32 {
    (0.216 / ((edge_count.max(1) as f32 / 1_500.0).max(1.0)).sqrt()).clamp(0.019, 0.19)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_alpha_and_linearizes_srgb_channels() {
        let converted = srgb_rgba_to_linear([0.5, 0.040_45, 1.0, 0.37]);
        assert!((converted[0] - 0.214_041_14).abs() < 0.000_001);
        assert!((converted[1] - 0.003_130_805).abs() < 0.000_001);
        assert_eq!(converted[2], 1.0);
        assert_eq!(converted[3], 0.37);
    }

    #[test]
    fn edge_density_matches_the_v3_visibility_curve() {
        assert_eq!(dense_edge_opacity(1_500), 0.19);
        assert!((dense_edge_opacity(50_000) - 0.037_412_297).abs() < 0.000_001);
        assert_eq!(dense_edge_opacity(1_000_000), 0.019);
    }

    #[test]
    fn neutral_edges_become_bounded_v3_teal_without_losing_alpha() {
        let rich = rich_edge_rgba([1.0, 1.0, 1.0, 0.37]);
        assert_eq!(&rich[..3], &[0.135, 0.392, 0.479]);
        assert_eq!(rich[3], 0.37);
    }

    #[test]
    fn pastel_edges_lose_their_white_component_and_stay_bounded() {
        let rich = rich_edge_rgba([1.0, 0.72, 0.82, 0.28]);
        let minimum = rich[0].min(rich[1]).min(rich[2]);
        let maximum = rich[0].max(rich[1]).max(rich[2]);
        assert!(maximum <= 0.48);
        assert!(maximum - minimum > 0.25);
        assert_eq!(rich[3], 0.28);
    }
}
