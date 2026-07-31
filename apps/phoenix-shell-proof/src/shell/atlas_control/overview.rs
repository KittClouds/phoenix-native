use super::{
    action_button, build_tone, kicker, status_badge, AtlasBuildState, AtlasControlSnapshot,
    PhoenixShell, ATTENTION, BLUE, BORDER, BORDER_BRIGHT, CARD_BG, CARD_RAISED, READY, TEXT,
    TEXT_MUTED,
};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Sizable, StyledExt};

pub(super) fn render_compact(
    shell: &PhoenixShell,
    control: &AtlasControlSnapshot,
    cx: &mut Context<PhoenixShell>,
) -> gpui::AnyElement {
    let outstanding = control.graph_reviews.proposed_edges;
    div()
        .size_full()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(0x0f1715))
        .child(
            div()
                .px_4()
                .py_3()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    div().min_w_0().child(kicker("ATLAS CONTROL", READY)).child(
                        div()
                            .mt_1()
                            .truncate()
                            .text_sm()
                            .font_semibold()
                            .text_color(rgb(TEXT))
                            .child(control.headline.to_string()),
                    ),
                )
                .child(status_badge(control.build_state)),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_4()
                .child(compact_progress(control))
                .child(
                    div()
                        .mt_3()
                        .flex()
                        .gap_2()
                        .child(metric("ENTITIES", control.canonical_entities, READY))
                        .child(metric("EDGES", control.edge_count, BLUE))
                        .child(metric("TO REVIEW", outstanding, ATTENTION)),
                )
                .child(
                    div()
                        .mt_3()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER_BRIGHT))
                        .bg(rgb(CARD_RAISED))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .child(control.guidance.to_string()),
                        ),
                )
                .child(
                    div()
                        .mt_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(action_button(shell, control.primary_action, cx))
                        .when(shell.graph_rebuild_pending, |row| {
                            row.child(
                                Button::new("atlas-cancel-compact")
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
        .into_any_element()
}

pub(super) fn render_full(
    shell: &PhoenixShell,
    control: &AtlasControlSnapshot,
    cx: &mut Context<PhoenixShell>,
) -> gpui::AnyElement {
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
                        .min_w_0()
                        .child(kicker("CURRENT WORKSPACE", READY))
                        .child(
                            div()
                                .mt_1()
                                .text_2xl()
                                .font_semibold()
                                .text_color(rgb(TEXT))
                                .child(control.headline.to_string()),
                        )
                        .child(
                            div()
                                .mt_1()
                                .max_w(px(680.))
                                .text_sm()
                                .text_color(rgb(TEXT_MUTED))
                                .child(control.guidance.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(status_badge(control.build_state))
                        .child(action_button(shell, control.primary_action, cx)),
                ),
        )
        .child(
            div()
                .mt_5()
                .grid()
                .grid_cols(3)
                .gap_3()
                .child(summary_card(
                    "SOURCE",
                    document_value(control),
                    format!("{} bytes", control.content_bytes),
                    READY,
                ))
                .child(summary_card(
                    "IDENTITIES",
                    control.canonical_entities.to_string(),
                    format!(
                        "{} user · {} NER",
                        control.user_entities, control.ner_entities
                    ),
                    BLUE,
                ))
                .child(summary_card(
                    "GRAPH",
                    format!("{} / {}", control.node_count, control.edge_count),
                    "nodes / edges".to_owned(),
                    build_tone(control.build_state),
                )),
        )
        .child(
            div()
                .mt_3()
                .grid()
                .grid_cols(3)
                .gap_3()
                .child(summary_card(
                    "REVIEW",
                    control.graph_reviews.proposed_edges.to_string(),
                    "candidate relations waiting".to_owned(),
                    if control.graph_reviews.proposed_edges > 0 {
                        ATTENTION
                    } else {
                        READY
                    },
                ))
                .child(summary_card(
                    "LIVE GENERATION",
                    control
                        .generation_id
                        .map(|generation| format!("G{generation}"))
                        .unwrap_or_else(|| "NONE".to_owned()),
                    if control.last_run_restored {
                        "restored durable receipt"
                    } else {
                        "current process authority"
                    }
                    .to_owned(),
                    BLUE,
                ))
                .child(summary_card(
                    "MEMORY AUTHORITY",
                    memory_hash(control),
                    format!(
                        "{} sources · {} indexed · queue {}",
                        control.memory.source_count,
                        control.memory.indexed_items,
                        control.memory.queue_high_water
                    ),
                    if control.memory.generation_hash.is_some() {
                        READY
                    } else {
                        ATTENTION
                    },
                )),
        )
        .when_some(control.last_error.as_ref(), |page, error| {
            page.child(
                div()
                    .mt_3()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x683e39))
                    .bg(rgb(0x251a18))
                    .child(kicker("WHY IT STOPPED", super::BLOCKED))
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(rgb(0xd7aaa5))
                            .child(error.to_string()),
                    ),
            )
        })
        .into_any_element()
}

fn memory_hash(control: &AtlasControlSnapshot) -> String {
    control
        .memory
        .generation_hash
        .map(|hash| {
            format!(
                "{:02x}{:02x}{:02x}{:02x}",
                hash[0], hash[1], hash[2], hash[3]
            )
        })
        .unwrap_or_else(|| "NONE".to_owned())
}

fn compact_progress(control: &AtlasControlSnapshot) -> impl IntoElement {
    let mut row = div().w_full().flex().gap_1();
    for stage in &control.stages {
        let tone = super::build_tone(match stage.state {
            phoenix_app_core::AtlasStageState::Complete => AtlasBuildState::Published,
            phoenix_app_core::AtlasStageState::Ready => AtlasBuildState::Ready,
            phoenix_app_core::AtlasStageState::Waiting => AtlasBuildState::WaitingForDocument,
            phoenix_app_core::AtlasStageState::NeedsAttention => {
                AtlasBuildState::VerificationRequired
            }
            phoenix_app_core::AtlasStageState::Blocked => AtlasBuildState::Failed,
        });
        row = row.child(div().h(px(5.)).flex_1().rounded_full().bg(rgb(tone)));
    }
    row
}

fn metric(label: &'static str, value: u64, tone: u32) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_BG))
        .child(kicker(label, tone))
        .child(
            div()
                .mt_1()
                .text_lg()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(value.to_string()),
        )
}

fn summary_card(label: &'static str, value: String, detail: String, tone: u32) -> impl IntoElement {
    div()
        .min_w_0()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_BG))
        .child(kicker(label, tone))
        .child(
            div()
                .mt_2()
                .truncate()
                .text_xl()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(value),
        )
        .child(
            div()
                .mt_1()
                .truncate()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(detail),
        )
}

fn document_value(control: &AtlasControlSnapshot) -> String {
    match (control.document_id, control.document_revision) {
        (Some(document), Some(revision)) => format!("Note {document} · r{revision}"),
        _ => "No active note".to_owned(),
    }
}
