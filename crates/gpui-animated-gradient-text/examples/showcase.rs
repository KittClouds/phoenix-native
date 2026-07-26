use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, FontWeight, Render, Window,
    WindowBounds, WindowOptions,
};
use gpui_animated_gradient_text::{GradientPalette, GradientText, Motion};
use std::time::Duration;

struct Showcase {
    title: GradientText,
    status: GradientText,
}

impl Showcase {
    fn new() -> Self {
        let aurora =
            GradientPalette::new([rgb(0x57e2bb), rgb(0x78a8ff), rgb(0xd779ff), rgb(0xff7ab8)])
                .expect("showcase palette is valid");

        Self {
            title: GradientText::new("PHOENIX NATIVE", aurora)
                .cycles(0.8)
                .max_color_runs(48),
            status: GradientText::phoenix("26,198 WORDS  ·  151,990 CHARS  ·  SAVED")
                .cycles(1.3)
                .motion(Motion::Reverse)
                .max_color_runs(32),
        }
    }
}

impl Render for Showcase {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(rgb(0x080b0f))
            .child(
                div()
                    .text_size(px(52.0))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .child(
                        self.title
                            .clone()
                            .animated("showcase-title", Duration::from_millis(3_800)),
                    ),
            )
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(
                        self.status
                            .clone()
                            .animated("showcase-status", Duration::from_millis(5_200)),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(920.0), px(480.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_| Showcase::new()),
        )
        .expect("opening showcase window");
    });
}
