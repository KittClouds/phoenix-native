use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Entity, FontWeight,
    IntoElement, Render, Window, WindowBounds, WindowOptions,
};
use gpui_animated_gradient_text::{
    GradientDraft, GradientPalette, GradientPicker, GradientPickerEvent, GradientPickerState,
    GradientPresetV1, GradientText, Motion, SelectableGradientText, TextFill, TextFillMap,
};
use gpui_component::Root;
use std::ops::Range;
use std::time::Duration;

struct AuthoringShowcase {
    source: &'static str,
    picker: Entity<GradientPickerState>,
    draft: GradientDraft,
    fills: TextFillMap,
    selection: Option<Range<usize>>,
}

impl AuthoringShowcase {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = GradientDraft::from_palette(&GradientPalette::phoenix());
        let picker = cx.new(|cx| GradientPickerState::new(draft.clone(), window, cx));
        cx.subscribe(&picker, |this, _, event: &GradientPickerEvent, cx| {
            let GradientPickerEvent::Change(draft) = event;
            this.draft = draft.clone();
            cx.notify();
        })
        .detach();
        Self {
            source: "Select any words, then apply the live gradient.",
            picker,
            draft,
            fills: TextFillMap::new(),
            selection: None,
        }
    }

    fn apply_fill_to_selection(&mut self, fill: TextFill, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let _ = self.fills.assign_bytes(self.source, selection, fill);
        cx.notify();
    }

    fn apply_palette_to_selection(&mut self, cx: &mut Context<Self>) {
        self.apply_fill_to_selection(self.draft.resolved_fill(), cx);
    }

    fn apply_active_color_to_selection(&mut self, cx: &mut Context<Self>) {
        let active = self.picker.read(cx).active_stop();
        let color = active.and_then(|id| {
            self.draft
                .stops()
                .iter()
                .find(|stop| stop.id == id)
                .map(|stop| stop.color)
        });
        if let Some(color) = color {
            self.apply_fill_to_selection(TextFill::Solid(color), cx);
        }
    }

    fn select(&mut self, selection: Range<usize>, cx: &mut Context<Self>) {
        self.selection = Some(selection);
        cx.notify();
    }
}

impl Render for AuthoringShowcase {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self
            .draft
            .to_palette()
            .unwrap_or_else(|_| GradientPalette::phoenix());
        let mut gradient = GradientText::new(self.source, palette.clone())
            .with_fill_map(self.fills.clone())
            .max_color_runs(64);
        if let Some(selection) = self.selection.clone() {
            gradient = gradient
                .selection_bytes(selection)
                .expect("selection callback returns aligned ranges");
        }
        let preset_json = GradientPresetV1::new(
            "Current",
            &palette,
            Motion::Forward,
            1.0,
            Duration::from_millis(4_800),
        )
        .to_json_pretty()
        .unwrap_or_else(|error| error.to_string());

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_5()
            .p_8()
            .bg(rgb(0x080b0f))
            .text_color(rgb(0xe8eef5))
            .child(
                div()
                    .text_size(px(36.0))
                    .font_weight(FontWeight::BOLD)
                    .child(
                        GradientText::new("GRADIENT TEXT v0.2", GradientPalette::phoenix())
                            .animated("v02-title", Duration::from_millis(3_800)),
                    ),
            )
            .child(
                div().text_size(px(24.0)).child(
                    SelectableGradientText::new("editable-gradient", gradient)
                        .on_select(cx.listener(
                            |this: &mut Self, selection: &Range<usize>, _, cx| {
                                this.select(selection.clone(), cx);
                            },
                        ))
                        .animated("editable-gradient-animation", Duration::from_millis(4_800)),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("apply-selection-gradient")
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x155e54))
                            .cursor_pointer()
                            .child("Apply palette to selection")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_palette_to_selection(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("apply-selection-color")
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x3730a3))
                            .cursor_pointer()
                            .child("Apply selected stop color")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_active_color_to_selection(cx);
                            })),
                    ),
            )
            .child(div().child(GradientPicker::new(&self.picker)))
            .child(
                div()
                    .max_h(px(180.0))
                    .overflow_hidden()
                    .font_family("monospace")
                    .text_size(px(12.0))
                    .child(preset_json),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(980.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| AuthoringShowcase::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("opening authoring showcase");
    });
}
