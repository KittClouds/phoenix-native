use super::{
    kicker, short_hash, AtlasControlSnapshot, AtlasReviewSnapshot, BLUE, BORDER, CARD_BG, READY,
    TEXT, TEXT_MUTED,
};
use gpui::{div, prelude::*, px, rgb, Entity, IntoElement};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::Sizable;
use gpui_component::StyledExt;
use phoenix_app_core::AtlasCandidateId;

pub(super) fn render(
    control: &AtlasControlSnapshot,
    review: Option<&AtlasReviewSnapshot>,
    selected: Option<AtlasCandidateId>,
    shell: Entity<super::PhoenixShell>,
) -> impl IntoElement {
    let candidate = review.and_then(|review| {
        selected
            .and_then(|id| {
                review
                    .candidates
                    .iter()
                    .find(|candidate| candidate.candidate_id == id)
            })
            .or_else(|| review.candidates.first())
    });
    div()
        .w(px(320.))
        .h_full()
        .min_h_0()
        .flex_shrink_0()
        .overflow_y_scrollbar()
        .p_4()
        .border_l_1()
        .border_color(rgb(BORDER))
        .bg(rgb(0x111715))
        .child(kicker("CONTEXT", BLUE))
        .child(
            div()
                .mt_1()
                .text_lg()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(if candidate.is_some() {
                    "Candidate evidence"
                } else {
                    "Current authority"
                }),
        )
        .when_some(candidate, |inspector, candidate| {
            inspector
                .child(
                    div()
                        .mt_4()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CARD_BG))
                        .child(kicker("RELATION", READY))
                        .child(
                            div()
                                .mt_2()
                                .text_sm()
                                .font_semibold()
                                .text_color(rgb(TEXT))
                                .child(format!(
                                    "{} → {}",
                                    candidate.left_label, candidate.right_label
                                )),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_xs()
                                .text_color(rgb(TEXT_MUTED))
                                .child(candidate.hypothesis.to_string()),
                        ),
                )
                .child(
                    div()
                        .mt_3()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CARD_BG))
                        .child(kicker("EVIDENCE", BLUE))
                        .child(
                            div()
                                .mt_2()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .child(candidate.premise.to_string()),
                        )
                        .child(context_row(
                            "CANDIDATE",
                            short_hash(candidate.candidate_id.0),
                        ))
                        .child(context_row(
                            "MODEL",
                            format!(
                                "{} · {:.1}%",
                                candidate.adjudication,
                                candidate.confidence_millis as f32 / 10.
                            ),
                        ))
                        .child(context_row(
                            "SOURCE BYTES",
                            format!("{}..{}", candidate.evidence_start, candidate.evidence_end),
                        ))
                        .child(
                            Button::new("focus-candidate-evidence")
                                .label("FOCUS EXACT EVIDENCE")
                                .small()
                                .ghost()
                                .on_click({
                                    let shell = shell.clone();
                                    let candidate_id = candidate.candidate_id;
                                    move |_, window, cx| {
                                        shell.update(cx, |this, cx| {
                                            this.select_atlas_candidate(candidate_id, window, cx);
                                        });
                                    }
                                }),
                        ),
                )
        })
        .when(candidate.is_none(), |inspector| {
            inspector
                .child(
                    div()
                        .mt_4()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CARD_BG))
                        .child(context_row(
                            "DOCUMENT",
                            control
                                .document_id
                                .map(|id| format!("Note {id}"))
                                .unwrap_or_else(|| "NONE".to_owned()),
                        ))
                        .child(context_row(
                            "REVISION",
                            control
                                .document_revision
                                .map(|revision| revision.to_string())
                                .unwrap_or_else(|| "NONE".to_owned()),
                        ))
                        .child(context_row(
                            "SOURCE HASH",
                            control
                                .content_hash
                                .map(short_hash)
                                .unwrap_or_else(|| "NONE".to_owned()),
                        ))
                        .child(context_row(
                            "REGISTRY",
                            format!("r{}", control.registry_revision),
                        ))
                        .child(context_row(
                            "GENERATION",
                            control
                                .generation_id
                                .map(|id| format!("G{id}"))
                                .unwrap_or_else(|| "NONE".to_owned()),
                        )),
                )
                .when_some(control.last_run.as_ref(), |inspector, run| {
                    inspector.child(
                        div()
                            .mt_3()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CARD_BG))
                            .child(kicker("PROVENANCE", READY))
                            .child(context_row(
                                "ARCHIVE",
                                short_hash(run.authority.archive_cohort_hash),
                            ))
                            .child(context_row(
                                "INDEX",
                                short_hash(run.authority.product_index_hash),
                            ))
                            .child(context_row(
                                "COMPILER",
                                run.producers.compiler_contract.clone(),
                            )),
                    )
                })
        })
}

fn context_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .mt_2()
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(label))
        .child(div().mt_1().text_sm().text_color(rgb(TEXT)).child(value))
}
