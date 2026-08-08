use super::PhoenixShell;
use crate::lifecycle;
use gpui::{Context, Hsla, Rgba};
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent};
use gpui_component::Sizable;
use phoenix_app_core::KernelCommand;
use phoenix_scene_contract::{GraphColorKey, HighlightPalette};

impl PhoenixShell {
    pub(super) fn graph_color_picker(&self, key: GraphColorKey) -> ColorPicker {
        ColorPicker::new(&self.graph_color_pickers[key.slot()])
            .small()
            .featured_colors(self.graph_featured_colors(key))
    }

    pub(super) fn on_graph_color_change(
        &mut self,
        key: GraphColorKey,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        let Some(mut palette) = self
            .kernel_snapshot()
            .map(|snapshot| *snapshot.highlight_palette)
        else {
            self.status = "PALETTE BLOCKED / KERNEL SNAPSHOT".into();
            cx.notify();
            return;
        };
        palette.set_graph_color(key, hsla_to_rgba(*color));
        match self
            .kernel
            .execute(KernelCommand::SetHighlightPalette(Box::new(palette)))
        {
            Ok(_) => match self
                .graph
                .borrow()
                .as_ref()
                .map(|graph| graph.sync_kernel_state())
            {
                Some(Ok(())) => {
                    self.status = format!("PALETTE / {key:?} / LIVE").into();
                }
                Some(Err(error)) => {
                    lifecycle::mark_proof_failed();
                    self.status = format!("PALETTE BLOCKED / {error:#}").into();
                }
                None => self.status = format!("PALETTE / {key:?} / STORED").into(),
            },
            Err(error) => self.status = format!("PALETTE BLOCKED / {error}").into(),
        }
        cx.notify();
    }

    fn graph_featured_colors(&self, active: GraphColorKey) -> Vec<Hsla> {
        self.kernel_snapshot()
            .map(|snapshot| {
                std::iter::once(active)
                    .chain(
                        GraphColorKey::ALL
                            .into_iter()
                            .filter(move |key| *key != active),
                    )
                    .take(10)
                    .map(|key| rgba_to_hsla(snapshot.highlight_palette.graph.color(key)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub(super) fn initial_picker_color(palette: HighlightPalette, key: GraphColorKey) -> Hsla {
    rgba_to_hsla(palette.graph.color(key))
}

pub(super) fn rgba_u32(color: [f32; 4]) -> u32 {
    let [red, green, blue, _] = color;
    ((red * 255.).round() as u32) << 16
        | ((green * 255.).round() as u32) << 8
        | (blue * 255.).round() as u32
}

fn rgba_to_hsla(color: [f32; 4]) -> Hsla {
    Rgba {
        r: color[0],
        g: color[1],
        b: color[2],
        a: color[3],
    }
    .into()
}

fn hsla_to_rgba(color: Hsla) -> [f32; 4] {
    let color: Rgba = color.into();
    [color.r, color.g, color.b, color.a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_round_trip_preserves_render_rgb() {
        let source = [0.18, 0.50, 1.0, 1.0];
        let actual = hsla_to_rgba(rgba_to_hsla(source));
        for (left, right) in source.into_iter().zip(actual) {
            assert!((left - right).abs() < 0.0001);
        }
        assert_eq!(rgba_u32(source), 0x2e80ff);
    }
}
