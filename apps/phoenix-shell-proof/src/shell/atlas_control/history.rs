use super::{
    kicker, short_hash, AtlasControlSnapshot, ATTENTION, BLUE, BORDER, CARD_BG, CARD_RAISED, READY,
    TEXT, TEXT_MUTED,
};
use gpui::{div, prelude::*, rgb, IntoElement};
use gpui_component::scroll::ScrollableElement;
use gpui_component::StyledExt;
use phoenix_app_core::{
    AtlasCapabilityCount, AtlasCapabilityState, AtlasDecisionAction, AtlasDecisionReceiptV1,
};

pub(super) fn render(
    control: &AtlasControlSnapshot,
    decisions: &[AtlasDecisionReceiptV1],
) -> gpui::AnyElement {
    let Some(run) = control.last_run.as_ref() else {
        return empty_history().into_any_element();
    };
    let mut decision_rows = div().mt_3().flex().flex_col().gap_2();
    for receipt in decisions.iter().rev().take(100) {
        decision_rows = decision_rows.child(decision_row(receipt));
    }
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scrollbar()
        .p_5()
        .child(kicker("DURABLE HISTORY", BLUE))
        .child(
            div()
                .mt_1()
                .text_2xl()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(format!("Generation {}", run.authority.published_generation)),
        )
        .child(div().mt_1().text_sm().text_color(rgb(TEXT_MUTED)).child(
            if control.last_run_restored {
                "Reopened and verified from the immutable run receipt."
            } else {
                "Published by this process and persisted for restart."
            },
        ))
        .child(
            div()
                .mt_4()
                .grid()
                .grid_cols(3)
                .gap_3()
                .child(history_card(
                    "SOURCE",
                    format!(
                        "Note {} · r{}",
                        run.authority.document_id, run.authority.document_revision
                    ),
                    short_hash(run.authority.content_hash),
                    READY,
                ))
                .child(history_card(
                    "GRAPH",
                    format!(
                        "{} nodes · {} edges",
                        run.resources.graph_nodes, run.resources.graph_edges
                    ),
                    format!(
                        "{} candidates",
                        capability_label(run.resources.nli_candidates)
                    ),
                    BLUE,
                ))
                .child(history_card(
                    "WALL CLOCK",
                    format!("{:.1} ms", run.timings.total_micros as f64 / 1_000.),
                    format!(
                        "queue high-water {} / {}",
                        run.queues.command_high_water_after, run.queues.command_capacity
                    ),
                    ATTENTION,
                )),
        )
        .child(
            div()
                .mt_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CARD_BG))
                .child(kicker("LINEAGE", BLUE))
                .child(history_line(
                    "PREVIOUS",
                    run.authority
                        .previous_generation
                        .map(|generation| format!("G{generation}"))
                        .unwrap_or_else(|| "NONE".to_owned()),
                ))
                .child(history_line(
                    "ARCHIVE",
                    short_hash(run.authority.archive_cohort_hash),
                ))
                .child(history_line(
                    "PRODUCT INDEX",
                    short_hash(run.authority.product_index_hash),
                ))
                .child(history_line(
                    "RECEIPT",
                    control
                        .last_run_hash
                        .map(short_hash)
                        .unwrap_or_else(|| "UNAVAILABLE".to_owned()),
                )),
        )
        .child(
            div()
                .mt_4()
                .child(kicker("DECISION LEDGER", READY))
                .child(
                    div()
                        .mt_1()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(format!(
                            "{} durable actions · newest first",
                            decisions.len()
                        )),
                )
                .child(if decisions.is_empty() {
                    div()
                        .mt_3()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CARD_BG))
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child("No review decisions have been written.")
                        .into_any_element()
                } else {
                    decision_rows.into_any_element()
                }),
        )
        .into_any_element()
}

fn empty_history() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(
            div()
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CARD_BG))
                .child(kicker("HISTORY EMPTY", READY))
                .child(
                    div()
                        .mt_2()
                        .text_lg()
                        .text_color(rgb(TEXT))
                        .child("No verified run has been published yet"),
                ),
        )
}

fn history_card(label: &'static str, value: String, detail: String, tone: u32) -> impl IntoElement {
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
                .text_lg()
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

fn history_line(label: &'static str, value: String) -> impl IntoElement {
    div()
        .mt_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .text_xs()
        .child(div().text_color(rgb(TEXT_MUTED)).child(label))
        .child(div().text_color(rgb(TEXT)).child(value))
}

fn decision_row(receipt: &AtlasDecisionReceiptV1) -> impl IntoElement {
    div()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_RAISED))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(rgb(TEXT))
                        .child(action_label(receipt.action)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(READY))
                        .child(format!("S{}", receipt.sequence)),
                ),
        )
        .child(
            div()
                .mt_1()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(format!(
                    "{} · candidate {}",
                    receipt.reason,
                    short_hash(receipt.candidate_id.0)
                )),
        )
}

fn capability_label(value: AtlasCapabilityCount) -> String {
    match value.state {
        AtlasCapabilityState::Produced => value.count.unwrap_or_default().to_string(),
        AtlasCapabilityState::Unsupported => "unsupported".to_owned(),
        AtlasCapabilityState::NotRun => "not run".to_owned(),
    }
}

const fn action_label(action: AtlasDecisionAction) -> &'static str {
    match action {
        AtlasDecisionAction::Accept => "Accepted",
        AtlasDecisionAction::Reject => "Rejected",
        AtlasDecisionAction::Defer => "Deferred",
        AtlasDecisionAction::Undo => "Undid decision",
    }
}
