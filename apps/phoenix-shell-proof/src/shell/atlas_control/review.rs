use super::{
    kicker, AtlasControlSnapshot, AtlasReviewSnapshot, PhoenixShell, ATTENTION, BLOCKED, BORDER,
    CARD_BG, CARD_RAISED, READY, TEXT, TEXT_MUTED,
};
use gpui::{div, prelude::*, px, rgb, uniform_list, Context, Entity, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Sizable, StyledExt};
use phoenix_app_core::{AtlasDecisionAction, AtlasReviewCandidateState};
use std::sync::Arc;

pub(super) fn render(
    shell: &PhoenixShell,
    control: &AtlasControlSnapshot,
    review: Option<&AtlasReviewSnapshot>,
    cx: &mut Context<PhoenixShell>,
) -> gpui::AnyElement {
    let Some(review) = review else {
        return empty_review(control).into_any_element();
    };
    let open_count = review
        .candidates
        .iter()
        .filter(|candidate| candidate.state == AtlasReviewCandidateState::Open)
        .count();
    let selected = shell.atlas_selected_candidate.or_else(|| {
        review
            .candidates
            .first()
            .map(|candidate| candidate.candidate_id)
    });
    let list_candidates = Arc::clone(&review.candidates);
    let list_shell = cx.entity().clone();
    let list = uniform_list(
        "atlas-review-candidates",
        list_candidates.len(),
        move |range, _, _| {
            range
                .map(|index| {
                    candidate_row(
                        index,
                        &list_candidates[index],
                        selected == Some(list_candidates[index].candidate_id),
                        list_shell.clone(),
                    )
                })
                .collect::<Vec<_>>()
        },
    )
    .h_full();
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .p_5()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .child(kicker("DURABLE REVIEW", ATTENTION))
                        .child(
                            div()
                                .mt_1()
                                .text_2xl()
                                .font_semibold()
                                .text_color(rgb(TEXT))
                                .child(if open_count == 0 {
                                    "Review is clear".to_owned()
                                } else {
                                    format!("{open_count} decisions waiting")
                                }),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_sm()
                                .text_color(rgb(TEXT_MUTED))
                                .child("Candidates remain outside accepted topology until you publish durable decisions."),
                        ),
                )
                .child(
                    Button::new("publish-reviewed-decisions")
                        .label("PUBLISH REVIEWED")
                        .small()
                        .primary()
                        .disabled(review.candidates.is_empty())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.dispatch_atlas_kernel_command(
                                phoenix_app_core::KernelCommand::PublishReviewedDecisions,
                                "PUBLISHING REVIEWED DECISIONS",
                                window,
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .mt_3()
                .flex()
                .gap_3()
                .child(review_metric("OPEN", open_count, ATTENTION))
                .child(review_metric(
                    "ACCEPTED",
                    count_state(review, AtlasReviewCandidateState::Accepted),
                    READY,
                ))
                .child(review_metric(
                    "REJECTED",
                    count_state(review, AtlasReviewCandidateState::Rejected),
                    BLOCKED,
                ))
                .child(review_metric(
                    "DEFERRED",
                    count_state(review, AtlasReviewCandidateState::Deferred),
                    super::WAITING,
                )),
        )
        .child(
            div()
                .min_h_0()
                .flex_1()
                .overflow_y_scrollbar()
                .child(if review.candidates.is_empty() {
                    div()
                        .mt_6()
                        .p_6()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CARD_BG))
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child("No candidate relations were produced for this exact note.")
                        .into_any_element()
                } else {
                    div().mt_3().size_full().child(list).into_any_element()
                }),
        )
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(rgb(0x68736f))
                .child("↑↓ select · A accept · R reject · D defer · U undo · P publish"),
        )
        .into_any_element()
}

fn empty_review(control: &AtlasControlSnapshot) -> impl IntoElement {
    let unsupported = !control.analysis_runtime.ready;
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(
            div()
                .max_w(px(672.))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(if unsupported { 0x654b29 } else { BORDER }))
                .bg(rgb(CARD_BG))
                .child(kicker(
                    if unsupported {
                        "REVIEW UNSUPPORTED"
                    } else {
                        "REVIEW EMPTY"
                    },
                    if unsupported { ATTENTION } else { READY },
                ))
                .child(
                    div()
                        .mt_2()
                        .text_lg()
                        .font_semibold()
                        .text_color(rgb(TEXT))
                        .child(if unsupported {
                            "Connect the verified NLI runtime"
                        } else {
                            "Run the pipeline to create evidence-bound candidates"
                        }),
                )
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child("Unsupported capability is not reported as zero output."),
                ),
        )
}

fn candidate_row(
    index: usize,
    candidate: &phoenix_app_core::AtlasReviewCandidateSummary,
    selected: bool,
    shell: Entity<PhoenixShell>,
) -> impl IntoElement {
    let candidate_id = candidate.candidate_id;
    let select_shell = shell.clone();
    div()
        .id(("atlas-review-row", index))
        .w_full()
        .min_w_0()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(if selected { 0x438a76 } else { BORDER }))
        .bg(rgb(if selected { 0x173029 } else { CARD_BG }))
        .cursor_pointer()
        .hover(|row| row.bg(rgb(0x1c2925)))
        .on_click(move |_, window, cx| {
            select_shell.update(cx, |this, cx| {
                this.select_atlas_candidate(candidate_id, window, cx);
            });
        })
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_semibold()
                        .text_color(rgb(TEXT))
                        .child(format!(
                            "{}  →  {}",
                            candidate.left_label, candidate.right_label
                        )),
                )
                .child(state_badge(candidate.state, candidate.applicability)),
        )
        .child(
            div()
                .mt_1()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(candidate.kind.to_string())
                .child("·")
                .child(candidate.adjudication.to_string())
                .child("·")
                .child(format!(
                    "{:.1}% confidence",
                    candidate.confidence_millis as f32 / 10.
                )),
        )
        .when(selected, |row| {
            row.child(
                div()
                    .mt_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(decision_button(
                        index,
                        "ACCEPT",
                        AtlasDecisionAction::Accept,
                        candidate,
                        shell.clone(),
                    ))
                    .child(decision_button(
                        index,
                        "REJECT",
                        AtlasDecisionAction::Reject,
                        candidate,
                        shell.clone(),
                    ))
                    .child(decision_button(
                        index,
                        "DEFER",
                        AtlasDecisionAction::Defer,
                        candidate,
                        shell.clone(),
                    ))
                    .child(decision_button(
                        index,
                        "UNDO",
                        AtlasDecisionAction::Undo,
                        candidate,
                        shell,
                    )),
            )
        })
}

fn decision_button(
    index: usize,
    label: &'static str,
    action: AtlasDecisionAction,
    candidate: &phoenix_app_core::AtlasReviewCandidateSummary,
    shell: Entity<PhoenixShell>,
) -> impl IntoElement {
    let id = match action {
        AtlasDecisionAction::Accept => ("accept-review", index),
        AtlasDecisionAction::Reject => ("reject-review", index),
        AtlasDecisionAction::Defer => ("defer-review", index),
        AtlasDecisionAction::Undo => ("undo-review-action", index),
    };
    let candidate_id = candidate.candidate_id;
    let head = candidate.expected_receipt_id;
    Button::new(id)
        .label(label)
        .small()
        .ghost()
        .disabled(action == AtlasDecisionAction::Undo && head.is_none())
        .on_click(move |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.dispatch_review_action(candidate_id, head, action, window, cx);
            });
        })
}

fn state_badge(
    state: AtlasReviewCandidateState,
    applicability: phoenix_app_core::AtlasDecisionApplicability,
) -> impl IntoElement {
    let (label, tone) = match applicability {
        phoenix_app_core::AtlasDecisionApplicability::Superseded => ("SUPERSEDED", ATTENTION),
        phoenix_app_core::AtlasDecisionApplicability::Preserved => match state {
            AtlasReviewCandidateState::Accepted => ("ACCEPTED / PRESERVED", READY),
            AtlasReviewCandidateState::Rejected => ("REJECTED / PRESERVED", BLOCKED),
            AtlasReviewCandidateState::Deferred => ("DEFERRED / PRESERVED", super::WAITING),
            AtlasReviewCandidateState::Open => ("OPEN", ATTENTION),
        },
        _ => match state {
            AtlasReviewCandidateState::Open => ("OPEN", ATTENTION),
            AtlasReviewCandidateState::Accepted => ("ACCEPTED", READY),
            AtlasReviewCandidateState::Rejected => ("REJECTED", BLOCKED),
            AtlasReviewCandidateState::Deferred => ("DEFERRED", super::WAITING),
        },
    };
    div()
        .px_2()
        .py_1()
        .rounded_full()
        .bg(rgb(CARD_RAISED))
        .text_xs()
        .font_semibold()
        .text_color(rgb(tone))
        .child(label)
}

fn review_metric(label: &'static str, value: usize, tone: u32) -> impl IntoElement {
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
                .text_color(rgb(TEXT))
                .child(value.to_string()),
        )
}

fn count_state(review: &AtlasReviewSnapshot, state: AtlasReviewCandidateState) -> usize {
    review
        .candidates
        .iter()
        .filter(|candidate| candidate.state == state)
        .count()
}
