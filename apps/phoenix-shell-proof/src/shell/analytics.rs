use super::{PhoenixShell, BORDER, CANVAS, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, relative, rgb, Context, FontWeight, IntoElement, Timer};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Sizable, StyledExt};
use phoenix_text_analytics::{LensKind, LensSummary, SentenceBand};
use std::time::Duration;

const ACCENT: u32 = 0x42ddc2;
const ACCENT_DIM: u32 = 0x123a35;
const CARD: u32 = 0x191b1d;
const CARD_DARK: u32 = 0x141617;
const ANALYTICS_DELAY: Duration = Duration::from_millis(180);
const COLLAPSED_ROWS: usize = 6;

pub(super) const BAND_COLORS: [u32; 6] =
    [0x7f42cf, 0x426bd2, 0x28b9a1, 0xcf6c2e, 0xbd3b43, 0xa82a5a];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AnalyticsHighlight {
    SentenceBand(u8),
    Lens(LensKind),
    Item(LensKind, usize),
}

impl PhoenixShell {
    pub(super) fn schedule_text_analytics_refresh(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        let scheduled_revision = editor.read_with(cx, |editor, _| editor.document_revision());
        let background = cx.background_executor().clone();
        self.analytics_refresh_task = Some(cx.spawn(async move |shell, async_cx| {
            Timer::after(ANALYTICS_DELAY).await;
            let source = match shell.update(async_cx, |_this, cx| {
                let current_revision = editor.read_with(cx, |editor, _| editor.document_revision());
                if current_revision != scheduled_revision {
                    return None;
                }
                Some(editor.read_with(cx, |editor, cx| editor.host_document_text(cx)))
            }) {
                Ok(Some(source)) => source,
                Ok(None) | Err(_) => return,
            };
            let analytics = background
                .spawn(async move { phoenix_text_analytics::analyze(&source) })
                .await;
            let _ = shell.update(async_cx, |this, cx| {
                let current_revision = editor.read_with(cx, |editor, _| editor.document_revision());
                if current_revision == scheduled_revision {
                    this.text_analytics = analytics;
                    this.analytics_expanded = false;
                    this.apply_kernel_highlights(cx);
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn render_analytics(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let analytics = &self.text_analytics;
        let selected = analytics
            .lens(self.analytics_selected_lens)
            .or_else(|| analytics.lenses.first());
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CANVAS))
            .child(self.render_right_sidebar_header(cx))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .child(self.analytics_mode_tabs())
                    .child(self.prose_health_card())
                    .child(self.rhythm_map_card(cx))
                    .child(self.editing_lenses_card(cx))
                    .when_some(selected, |panel, lens| {
                        panel.child(self.lens_detail(lens, cx))
                    })
                    .child(
                        div()
                            .mt_3()
                            .mb_2()
                            .text_center()
                            .text_xs()
                            .text_color(rgb(0x59625f))
                            .child("Native analysis · revision-cached · no network"),
                    ),
            )
    }

    fn analytics_mode_tabs(&self) -> impl IntoElement {
        div()
            .mb_3()
            .p_1()
            .grid()
            .grid_cols(2)
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CARD_DARK))
            .child(
                div()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x237d70))
                    .bg(rgb(ACCENT_DIM))
                    .text_center()
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .child("Prose"),
            )
            .child(
                div()
                    .py_2()
                    .text_center()
                    .text_sm()
                    .text_color(rgb(0x6f7775))
                    .child("Meter Analysis"),
            )
    }

    fn prose_health_card(&self) -> impl IntoElement {
        let analytics = &self.text_analytics;
        let rhythm = if analytics.has_monotony {
            "monotony"
        } else {
            "varied"
        };
        let mut chips = div().mt_3().grid().grid_cols(2).gap_2();
        for (slot, label, value, color) in [
            (0, "FLOW", format!("{}%", analytics.flow_score), ACCENT),
            (1, "RHYTHM", rhythm.to_string(), 0xe6b83f),
            (
                2,
                "ECHO",
                lens_count(analytics.lens(LensKind::Echo)),
                0xe36a72,
            ),
            (
                3,
                "NEGATION",
                lens_count(analytics.lens(LensKind::Negation)),
                0xe36a72,
            ),
            (
                4,
                "ORNAMENT",
                lens_count(analytics.lens(LensKind::Ornament)),
                0xe36a72,
            ),
        ] {
            chips = chips.child(metric_chip(("health-chip", slot), label, value, color));
        }
        section_card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(section_title("▥", "Prose Health"))
                    .child(
                        div()
                            .ml_auto()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(0x27292b))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            .child(analytics.reading_grade.to_string()),
                    ),
            )
            .child(chips)
            .child(
                div()
                    .mt_3()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child(vital(duration_label(analytics.reading_seconds), "read"))
                    .child(vital(duration_label(analytics.speaking_seconds), "spoken"))
                    .child(vital(
                        format!("{:.0} words", analytics.average_sentence_length),
                        "avg",
                    )),
            )
    }

    fn rhythm_map_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let analytics = &self.text_analytics;
        let mut bands = div().mt_3().grid().grid_cols(2).gap_2();
        for (index, band) in analytics.sentence_bands.iter().copied().enumerate() {
            let highlighted =
                self.analytics_highlight == Some(AnalyticsHighlight::SentenceBand(index as u8));
            bands = bands.child(
                sentence_band_card(index, band, highlighted)
                    .id(("sentence-band", index))
                    .when(highlighted, |card| {
                        card.border_2().border_color(rgb(0xe9f7f3))
                    })
                    .cursor_pointer()
                    .hover(|card| card.bg(rgb(0x222828)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_analytics_highlight(
                            AnalyticsHighlight::SentenceBand(index as u8),
                            cx,
                        );
                    })),
            );
        }
        let flow_width = (analytics.flow_score as f32 / 100.0).max(0.01);
        section_card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(section_title("●", "Rhythm Map"))
                    .child(
                        div()
                            .ml_auto()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(if analytics.has_monotony {
                                "watch"
                            } else {
                                "strong"
                            }),
                    ),
            )
            .child(
                div()
                    .mt_3()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(rgb(TEXT))
                            .child("↗  Flow Score"),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x237d70))
                            .bg(rgb(ACCENT_DIM))
                            .text_xl()
                            .font_bold()
                            .text_color(rgb(ACCENT))
                            .child(format!("{}%", analytics.flow_score)),
                    ),
            )
            .child(
                div()
                    .mt_3()
                    .h(px(10.))
                    .w_full()
                    .rounded_full()
                    .bg(rgb(0x292c2d))
                    .child(
                        div()
                            .h_full()
                            .w(relative(flow_width))
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    ),
            )
            .child(
                div()
                    .mt_4()
                    .text_xs()
                    .font_semibold()
                    .text_color(rgb(TEXT_MUTED))
                    .child("SENTENCE VARIATION"),
            )
            .child(
                div()
                    .mt_1()
                    .text_xs()
                    .text_color(rgb(0x69726f))
                    .child("Select a band to inspect and paint matching sentences."),
            )
            .child(bands)
            .child(rhythm_notice(
                analytics.has_monotony,
                analytics.longest_monotony_run,
            ))
            .child(
                div()
                    .mt_3()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("Distribution Balance")
                    .child(
                        div()
                            .ml_auto()
                            .font_semibold()
                            .text_color(rgb(TEXT))
                            .child(format!("{}% varied", analytics.variety_score)),
                    ),
            )
    }

    fn editing_lenses_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut grid = div().mt_3().grid().grid_cols(2).gap_2();
        for (index, kind) in LensKind::ALL.into_iter().enumerate() {
            let count = self.text_analytics.lens(kind).map_or(0, |lens| lens.count);
            let selected = kind == self.analytics_selected_lens;
            let highlighted = self.analytics_highlight == Some(AnalyticsHighlight::Lens(kind));
            grid = grid.child(
                div()
                    .id(("analytics-lens", index))
                    .min_h(px(112.))
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(if highlighted {
                        ACCENT
                    } else if selected {
                        0x258b7d
                    } else {
                        0x653036
                    }))
                    .bg(rgb(if selected { 0x173a36 } else { CARD_DARK }))
                    .cursor_pointer()
                    .hover(|card| card.bg(rgb(if selected { 0x1b4741 } else { 0x21191a })))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.analytics_selected_lens = kind;
                        this.analytics_expanded = false;
                        this.toggle_analytics_highlight(AnalyticsHighlight::Lens(kind), cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_lg()
                            .font_bold()
                            .text_color(rgb(TEXT))
                            .child(kind.label()),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_2xl()
                            .font_bold()
                            .text_color(rgb(TEXT))
                            .child(count.to_string()),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(rgb(TEXT_MUTED))
                            .child(kind.description()),
                    ),
            );
        }
        section_card()
            .child(section_title("✦", "Editing Lenses"))
            .child(
                div()
                    .mt_1()
                    .text_xs()
                    .text_color(rgb(0x69726f))
                    .child("Select a lens for its evidence; select a row for exact paint."),
            )
            .child(grid)
    }

    fn lens_detail(&self, lens: &LensSummary, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = if self.analytics_expanded {
            lens.items.len()
        } else {
            lens.items.len().min(COLLAPSED_ROWS)
        };
        let mut rows = div().mt_3();
        for (index, item) in lens.items.iter().take(shown).enumerate() {
            let highlighted =
                self.analytics_highlight == Some(AnalyticsHighlight::Item(lens.kind, index));
            let kind = lens.kind;
            rows = rows.child(
                div()
                    .id(("analytics-detail-row", index))
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(if highlighted { ACCENT } else { BORDER }))
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(0x202524)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_analytics_highlight(AnalyticsHighlight::Item(kind, index), cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .mr_2()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(rgb(if highlighted { ACCENT } else { 0x535957 }))
                                    .bg(rgb(if highlighted { ACCENT_DIM } else { CARD_DARK }))
                                    .when(highlighted, |mark| {
                                        mark.flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(rgb(ACCENT))
                                            .child("•")
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_lg()
                                    .font_bold()
                                    .text_color(rgb(TEXT))
                                    .child(item.label.to_string()),
                            )
                            .child(
                                div()
                                    .ml_2()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .bg(rgb(0x292b2d))
                                    .font_semibold()
                                    .text_color(rgb(TEXT))
                                    .child(item.count.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_sm()
                            .text_color(rgb(TEXT_MUTED))
                            .child(item.detail.to_string()),
                    ),
            );
        }
        section_card()
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(rgb(TEXT))
                                    .child(lens.kind.label()),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(lens.kind.description()),
                            ),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x292b2d))
                            .text_sm()
                            .font_semibold()
                            .text_color(rgb(TEXT))
                            .child(format!("{} rows", lens.items.len())),
                    ),
            )
            .child(rows)
            .when(lens.items.len() > COLLAPSED_ROWS, |card| {
                card.child(
                    Button::new("analytics-expand-rows")
                        .label(if self.analytics_expanded {
                            "Show less".to_string()
                        } else {
                            format!("Show {} more", lens.items.len() - COLLAPSED_ROWS)
                        })
                        .mt_2()
                        .w_full()
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.analytics_expanded = !this.analytics_expanded;
                            cx.notify();
                        })),
                )
            })
            .when(lens.items.is_empty(), |card| {
                card.child(
                    div()
                        .mt_4()
                        .py_4()
                        .text_center()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child("No pressure points found in this lens."),
                )
            })
    }

    fn toggle_analytics_highlight(
        &mut self,
        highlight: AnalyticsHighlight,
        cx: &mut Context<Self>,
    ) {
        self.analytics_highlight =
            (self.analytics_highlight != Some(highlight)).then_some(highlight);
        self.apply_kernel_highlights(cx);
        cx.notify();
    }
}

fn section_card() -> gpui::Div {
    div()
        .mb_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD))
}

fn section_title(icon: &'static str, title: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_lg()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(TEXT))
        .child(div().text_color(rgb(ACCENT)).child(icon))
        .child(title)
}

fn metric_chip(
    id: (&'static str, usize),
    label: &'static str,
    value: String,
    color: u32,
) -> impl IntoElement {
    div()
        .id(id)
        .min_h(px(74.))
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(color))
        .bg(rgb(CARD_DARK))
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(label))
        .child(
            div()
                .mt_1()
                .text_lg()
                .font_bold()
                .text_color(rgb(TEXT))
                .child(value),
        )
}

fn vital(value: String, label: &'static str) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .child(value)
        .child(div().mt_1().child(label))
}

fn sentence_band_card(index: usize, band: SentenceBand, highlighted: bool) -> gpui::Div {
    let color = BAND_COLORS[index];
    div()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(color))
        .bg(rgb(color & 0x3f3f3f))
        .child(
            div()
                .flex()
                .items_center()
                .text_sm()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(band.label)
                .child(
                    div()
                        .ml_auto()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if highlighted { 0xe9f7f3 } else { color }))
                        .text_xs()
                        .child(if highlighted { "●" } else { "○" }),
                ),
        )
        .child(
            div()
                .mt_2()
                .flex()
                .items_end()
                .child(
                    div()
                        .text_2xl()
                        .font_bold()
                        .text_color(rgb(TEXT))
                        .child(band.count.to_string()),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(format!("{}%", band.percent)),
                ),
        )
        .child(
            div()
                .mt_2()
                .h(px(4.))
                .w_full()
                .rounded_full()
                .bg(rgb(0x303234))
                .child(
                    div()
                        .h_full()
                        .w(relative((band.percent as f32 / 100.0).max(0.01)))
                        .rounded_full()
                        .bg(rgb(color)),
                ),
        )
}

fn rhythm_notice(has_monotony: bool, longest_run: u32) -> impl IntoElement {
    let (border, background, color, title, body) = if has_monotony {
        (
            0x8f6515,
            0x382b16,
            0xf0c84e,
            "!  Monotony detected",
            format!(
                "{longest_run} consecutive sentences have similar length. Break the pattern for variety."
            ),
        )
    } else {
        (
            0x267467,
            0x15322e,
            ACCENT,
            "✦  Strong sentence variety",
            "The prose moves across distinct rhythm bands.".to_string(),
        )
    };
    div()
        .mt_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .text_sm()
        .text_color(rgb(color))
        .child(div().font_semibold().child(title))
        .child(div().mt_1().child(body))
}

fn duration_label(seconds: u32) -> String {
    format!("{} min {:02} sec", seconds / 60, seconds % 60)
}

fn lens_count(lens: Option<&LensSummary>) -> String {
    lens.map_or(0, |lens| lens.count).to_string()
}
