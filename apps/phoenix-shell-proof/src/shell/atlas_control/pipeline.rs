use super::{
    action_button, kicker, status_badge, AtlasControlSnapshot, PhoenixShell, ATTENTION, BLOCKED,
    BORDER, CARD_BG, CARD_RAISED, READY, TEXT, TEXT_MUTED,
};
use gpui::{div, prelude::*, rgb, Context, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Sizable, StyledExt};

pub(super) fn render(
    shell: &PhoenixShell,
    control: &AtlasControlSnapshot,
    cx: &mut Context<PhoenixShell>,
) -> gpui::AnyElement {
    let mut stages = div().mt_4().w_full().flex().gap_2();
    for (index, stage) in control.stages.iter().enumerate() {
        stages = stages.child(stage_card(index + 1, stage));
    }
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scrollbar()
        .p_5()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .child(kicker("VERIFIED NATIVE ROUTE", READY))
                        .child(
                            div()
                                .mt_1()
                                .text_2xl()
                                .font_semibold()
                                .text_color(rgb(TEXT))
                                .child("Build this note"),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_sm()
                                .text_color(rgb(TEXT_MUTED))
                                .child("One bounded run: structure, Dynamic NER, candidate-only NLI, compiler, atomic publication."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(status_badge(control.build_state))
                        .child(action_button(shell, control.primary_action, cx))
                        .when(shell.graph_rebuild_pending, |row| {
                            row.child(
                                Button::new("atlas-cancel-run")
                                    .label("CANCEL")
                                    .small()
                                    .danger()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dispatch_atlas_kernel_command(
                                            phoenix_app_core::KernelCommand::CancelAtlasRun,
                                            "ATLAS / CANCELLATION REQUESTED",
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                        }),
                ),
        )
        .child(stages)
        .child(
            div()
                .mt_4()
                .w_full()
                .grid()
                .grid_cols(3)
                .gap_3()
                .child(runtime_lane(
                    "DYNAMIC NER",
                    control
                        .analysis
                        .as_ref()
                        .map(|summary| summary.dynamic_ner_model.to_string())
                        .unwrap_or_else(|| control.analysis_runtime.dynamic_ner.to_string()),
                    control.analysis_runtime.ready,
                ))
                .child(runtime_lane(
                    "NLI · CANDIDATE ONLY",
                    control
                        .analysis
                        .as_ref()
                        .map(|summary| summary.nli_model.to_string())
                        .unwrap_or_else(|| control.analysis_runtime.nli.to_string()),
                    control.analysis_runtime.ready,
                ))
                .child(runtime_lane(
                    "NATIVE SCENE",
                    "PhoenixGraphGenerationV1 → PSA/PSPI".to_owned(),
                    true,
                )),
        )
        .when_some(control.last_run.as_ref(), |page, receipt| {
            page.child(
                div()
                    .mt_4()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(CARD_BG))
                    .child(kicker("LAST VERIFIED RECEIPT", READY))
                    .child(
                        div()
                            .mt_2()
                            .grid()
                            .grid_cols(4)
                            .gap_2()
                            .child(receipt_metric("TOTAL", receipt.timings.total_micros))
                            .child(receipt_metric(
                                "ANALYSIS",
                                receipt
                                    .timings
                                    .analysis_total_micros
                                    .count
                                    .unwrap_or_default(),
                            ))
                            .child(receipt_metric("COMPILER", receipt.timings.compiler_micros))
                            .child(receipt_metric("PUBLISH", receipt.timings.publisher_micros)),
                    ),
            )
        })
        .when_some(control.last_error.as_ref(), |page, error| {
            page.child(
                div()
                    .mt_3()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x683e39))
                    .bg(rgb(0x251a18))
                    .child(kicker("FAILED · PREVIOUS GENERATION PRESERVED", BLOCKED))
                    .child(div().mt_2().text_sm().text_color(rgb(0xd7aaa5)).child(error.to_string())),
            )
        })
        .into_any_element()
}

fn stage_card(index: usize, stage: &phoenix_app_core::AtlasStageSummary) -> impl IntoElement {
    let tone = match stage.state {
        phoenix_app_core::AtlasStageState::Complete | phoenix_app_core::AtlasStageState::Ready => {
            READY
        }
        phoenix_app_core::AtlasStageState::NeedsAttention => ATTENTION,
        phoenix_app_core::AtlasStageState::Blocked => BLOCKED,
        phoenix_app_core::AtlasStageState::Waiting => super::WAITING,
    };
    div()
        .min_w_0()
        .flex_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_BG))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .w_5()
                        .h_5()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(0x26302d))
                        .text_xs()
                        .text_color(rgb(tone))
                        .child(index.to_string()),
                )
                .child(div().w_2().h_2().rounded_full().bg(rgb(tone))),
        )
        .child(
            div()
                .mt_2()
                .truncate()
                .text_xs()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(stage_name(stage.stage)),
        )
        .child(
            div()
                .mt_1()
                .truncate()
                .text_sm()
                .text_color(rgb(tone))
                .child(stage.value.to_string()),
        )
        .child(
            div()
                .mt_1()
                .truncate()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(stage.detail.to_string()),
        )
}

fn runtime_lane(label: &'static str, value: String, ready: bool) -> impl IntoElement {
    div()
        .min_w_0()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_RAISED))
        .child(kicker(label, if ready { READY } else { ATTENTION }))
        .child(
            div()
                .mt_2()
                .truncate()
                .text_sm()
                .text_color(rgb(TEXT))
                .child(value),
        )
        .child(
            div()
                .mt_1()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(if ready { "available" } else { "unsupported" }),
        )
}

fn receipt_metric(label: &'static str, micros: u64) -> impl IntoElement {
    div()
        .p_3()
        .rounded_md()
        .bg(rgb(CARD_RAISED))
        .child(kicker(label, READY))
        .child(
            div()
                .mt_1()
                .text_sm()
                .text_color(rgb(TEXT))
                .child(format!("{:.1} ms", micros as f64 / 1_000.)),
        )
}

const fn stage_name(stage: phoenix_app_core::AtlasStage) -> &'static str {
    match stage {
        phoenix_app_core::AtlasStage::Source => "NOTE",
        phoenix_app_core::AtlasStage::Entities => "ENTITIES",
        phoenix_app_core::AtlasStage::Connections => "GRAPH",
        phoenix_app_core::AtlasStage::Review => "REVIEW",
        phoenix_app_core::AtlasStage::Live => "LIVE",
    }
}
