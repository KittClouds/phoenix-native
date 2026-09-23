use super::{PhoenixShell, BORDER, TEXT_MUTED};
use gpui::{div, prelude::*, rgb, Context, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable};

impl PhoenixShell {
    pub(super) fn render_caps_space(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (space, depth, cutaway) = self.caps_inspection;
        let mut row = div().flex().flex_wrap().items_center().gap_2().child(
            Button::new("caps-space-mode")
                .label(if space { "Show graph" } else { "Inspect space" })
                .small()
                .ghost()
                .tooltip("Inspect containment shells with nodes and edges hidden")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.caps_inspection.0 = !this.caps_inspection.0;
                    this.sync_caps_inspection(cx);
                })),
        );
        if space {
            for (level, label, color) in [
                (1_u8, "Document", 0xeb2e40),
                (2, "Chapter", 0x57a6f2),
                (3, "Paragraph", 0x6bc2f5),
                (4, "Sentence", 0x80d6cc),
            ] {
                row = row.child(
                    Button::new(("caps-depth", level as usize))
                        .label(label)
                        .small()
                        .ghost()
                        .selected(depth == level)
                        .text_color(rgb(color))
                        .tooltip("Peel away shells beyond this level")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.caps_inspection.1 = level;
                            this.sync_caps_inspection(cx);
                        })),
                );
            }
            row = row.child(
                Button::new("caps-cutaway")
                    .label(if cutaway {
                        "Close cutaway"
                    } else {
                        "Open cutaway"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.caps_inspection.2 = !this.caps_inspection.2;
                        this.sync_caps_inspection(cx);
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(row)
            .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(if space {
                "CONTAINMENT ATLAS  ·  broad context inside, finer text outside  ·  drag to orbit"
            } else {
                "CAPS  ·  radius = text granularity  ·  angular caps = containment regions"
            }))
    }

    fn sync_caps_inspection(&self, cx: &mut Context<Self>) {
        let (space, depth, cutaway) = self.caps_inspection;
        if let Some(graph) = self.graph.borrow().as_ref() {
            if let Err(error) = graph.inspect_caps(space, depth, cutaway) {
                eprintln!("CAPS inspection: {error}");
            }
            if let Err(error) = graph.fit_graph() {
                eprintln!("CAPS inspection fit: {error}");
            }
        }
        cx.notify();
    }
}
