use super::drawer::{DrawerTab, ACCENT, ACCENT_DIM};
use super::{PhoenixShell, BORDER, BORDER_BRIGHT, SURFACE, TEXT, TEXT_MUTED};
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgb, Context, IntoElement, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Sizable, StyledExt};
use phoenix_app_core::{
    AtlasBuildState, AtlasControlSnapshot, AtlasPrimaryAction, AtlasStage, AtlasStageState,
};

const PAGE_BG: u32 = 0x101413;
const CARD_BG: u32 = 0x181d1b;
const CARD_RAISED: u32 = 0x1d2421;
const READY: u32 = 0x57e2bb;
const WAITING: u32 = 0x77817e;
const ATTENTION: u32 = 0xf0b45d;
const BLOCKED: u32 = 0xeb776f;
const BLUE: u32 = 0x7fb7ff;

impl PhoenixShell {
    pub(super) fn render_atlas_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let control = self.kernel.atlas_control_snapshot();
        match control {
            Ok(control) => self
                .render_atlas_control_ready(control, cx)
                .into_any_element(),
            Err(error) => atlas_control_error(&error.to_string()).into_any_element(),
        }
    }

    fn render_atlas_control_ready(
        &self,
        control: AtlasControlSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let action = control.primary_action;
        let action_enabled = action != AtlasPrimaryAction::Wait;
        let build_tone = build_tone(control.build_state);
        let mut stage_row = div().w_full().flex().gap_2();
        for (index, stage) in control.stages.iter().enumerate() {
            stage_row = stage_row.child(stage_card(index + 1, stage));
        }

        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
            .bg(linear_gradient(
                145.,
                linear_color_stop(rgb(0x08231d), 0.),
                linear_color_stop(rgb(PAGE_BG), 1.),
            ))
            .p_4()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(rgb(ACCENT))
                                    .child("ATLAS CONTROL"),
                            )
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
                                    .max_w(px(720.))
                                    .text_sm()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(control.guidance.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(rgb(build_tone))
                                    .bg(rgb(0x151a18))
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(rgb(build_tone))
                                    .child(build_state_label(control.build_state)),
                            )
                            .child(
                                Button::new("atlas-primary-action")
                                    .label(primary_action_label(action))
                                    .small()
                                    .primary()
                                    .disabled(!action_enabled)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.dispatch_atlas_action(action, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(div().mt_4().child(stage_row))
            .child(
                div()
                    .mt_4()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(next_move_card(&control))
                    .child(current_truth_card(&control)),
            )
            .child(div().mt_3().w_full().child(review_card(&control)))
            .when_some(control.last_error.as_ref(), |page, error| {
                page.child(
                    div()
                        .mt_3()
                        .w_full()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(0x683e39))
                        .bg(rgb(0x251a18))
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(rgb(BLOCKED))
                                .child("WHY THE BUILD STOPPED"),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_sm()
                                .text_color(rgb(0xd7aaa5))
                                .child(error.to_string()),
                        ),
                )
            })
    }

    fn dispatch_atlas_action(
        &mut self,
        action: AtlasPrimaryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            AtlasPrimaryAction::BuildGraph | AtlasPrimaryAction::RebuildGraph => {
                self.start_native_scene_rebuild(window, cx);
            }
            AtlasPrimaryAction::OpenGraph => {
                self.select_drawer_tab(DrawerTab::Graph, cx);
            }
            AtlasPrimaryAction::OpenDocument | AtlasPrimaryAction::TagEntities => {
                if self.drawer_layout.is_open() {
                    self.toggle_drawer(cx);
                }
                self.status = if action == AtlasPrimaryAction::TagEntities {
                    "ATLAS / SELECT TEXT / TAG ITS ENTITY TYPE".into()
                } else {
                    "ATLAS / SELECT A NOTE".into()
                };
                cx.notify();
            }
            AtlasPrimaryAction::Wait => {}
        }
    }
}

fn stage_card(index: usize, stage: &phoenix_app_core::AtlasStageSummary) -> impl IntoElement {
    let tone = stage_tone(stage.state);
    div()
        .min_w_0()
        .flex_1()
        .px_2()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if stage.state == AtlasStageState::Complete {
            0x2f584c
        } else {
            BORDER
        }))
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
                        .bg(rgb(if stage.state == AtlasStageState::Complete {
                            ACCENT_DIM
                        } else {
                            0x292e2c
                        }))
                        .text_xs()
                        .font_semibold()
                        .text_color(rgb(tone))
                        .child(index.to_string()),
                )
                .child(div().w_2().h_2().rounded_full().bg(rgb(tone))),
        )
        .child(
            div()
                .mt_1()
                .truncate()
                .text_xs()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(stage_label(stage.stage)),
        )
}

fn next_move_card(control: &AtlasControlSnapshot) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER_BRIGHT))
        .bg(rgb(CARD_RAISED))
        .child(kicker("NEXT MOVE", ACCENT))
        .child(
            div()
                .mt_2()
                .text_lg()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(primary_action_label(control.primary_action)),
        )
        .child(
            div()
                .mt_1()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child(action_explanation(control.primary_action)),
        )
        .child(
            div()
                .mt_3()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(0x9aa6a2))
                .child(format!("{} verified mentions", control.verified_mentions))
                .child("/")
                .child(format!("{} canonical entities", control.canonical_entities)),
        )
}

fn current_truth_card(control: &AtlasControlSnapshot) -> impl IntoElement {
    let generation = control
        .generation_id
        .map(|generation| format!("G{generation}"))
        .unwrap_or_else(|| "NONE".to_owned());
    div()
        .min_w_0()
        .flex_1()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CARD_BG))
        .child(kicker("CURRENT TRUTH", BLUE))
        .child(truth_row("SOURCE", document_label(control)))
        .child(truth_row(
            "IDENTITIES",
            format!(
                "{} user / {} NER",
                control.user_entities, control.ner_entities
            ),
        ))
        .child(truth_row(
            "GRAPH",
            format!(
                "{} nodes / {} edges",
                control.node_count, control.edge_count
            ),
        ))
        .child(truth_row("LIVE", generation))
}

fn review_card(control: &AtlasControlSnapshot) -> impl IntoElement {
    let needs_attention = control.proposed_rows > 0;
    div()
        .min_w_0()
        .flex_1()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(if needs_attention { 0x654b29 } else { BORDER }))
        .bg(rgb(CARD_BG))
        .child(kicker(
            "NEEDS YOU",
            if needs_attention { ATTENTION } else { READY },
        ))
        .child(
            div()
                .mt_2()
                .text_lg()
                .font_semibold()
                .text_color(rgb(TEXT))
                .child(if needs_attention {
                    format!("{} proposals", control.proposed_rows)
                } else {
                    "Nothing waiting".to_owned()
                }),
        )
        .child(
            div()
                .mt_1()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child(if needs_attention {
                    "Accepting or rejecting will require a durable decision receipt."
                } else {
                    "Phoenix will put proposed relationships here instead of silently promoting them."
                }),
        )
        .child(
            div()
                .mt_3()
                .flex()
                .gap_3()
                .text_xs()
                .text_color(rgb(0x9aa6a2))
                .child(format!("{} accepted", control.accepted_rows))
                .child(format!("{} rejected", control.rejected_rows)),
        )
}

fn truth_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .mt_2()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .text_xs()
        .child(div().text_color(rgb(TEXT_MUTED)).child(label))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(rgb(TEXT))
                .child(value),
        )
}

fn kicker(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(rgb(color))
        .child(label)
}

fn document_label(control: &AtlasControlSnapshot) -> String {
    match (control.document_id, control.document_revision) {
        (Some(document), Some(revision)) => format!(
            "Note {document} · rev {revision} · {}",
            format_bytes(control.content_bytes)
        ),
        _ => "No active note".to_owned(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

fn build_tone(state: AtlasBuildState) -> u32 {
    match state {
        AtlasBuildState::Published => READY,
        AtlasBuildState::Ready | AtlasBuildState::VerificationRequired => BLUE,
        AtlasBuildState::Building => ATTENTION,
        AtlasBuildState::Failed => BLOCKED,
        AtlasBuildState::WaitingForDocument | AtlasBuildState::WaitingForEntities => WAITING,
    }
}

fn build_state_label(state: AtlasBuildState) -> &'static str {
    match state {
        AtlasBuildState::WaitingForDocument => "CHOOSE NOTE",
        AtlasBuildState::WaitingForEntities => "NEEDS ANCHOR",
        AtlasBuildState::Ready => "READY",
        AtlasBuildState::Building => "BUILDING",
        AtlasBuildState::Published => "LIVE",
        AtlasBuildState::VerificationRequired => "VERIFY",
        AtlasBuildState::Failed => "STOPPED SAFELY",
    }
}

fn primary_action_label(action: AtlasPrimaryAction) -> &'static str {
    match action {
        AtlasPrimaryAction::OpenDocument => "CHOOSE A NOTE",
        AtlasPrimaryAction::TagEntities => "TAG AN ENTITY",
        AtlasPrimaryAction::BuildGraph => "BUILD GRAPH",
        AtlasPrimaryAction::RebuildGraph => "REBUILD SAFELY",
        AtlasPrimaryAction::OpenGraph => "OPEN GRAPH",
        AtlasPrimaryAction::Wait => "BUILDING",
    }
}

fn action_explanation(action: AtlasPrimaryAction) -> &'static str {
    match action {
        AtlasPrimaryAction::OpenDocument => "Pick the note whose graph you want to build.",
        AtlasPrimaryAction::TagEntities => {
            "Highlight a name in the editor, click Tag, and choose its type."
        }
        AtlasPrimaryAction::BuildGraph => {
            "Compile the current verified evidence and publish one atomic generation."
        }
        AtlasPrimaryAction::RebuildGraph => {
            "Build from the current note. The existing generation remains live until success."
        }
        AtlasPrimaryAction::OpenGraph => "Return to the canvas without rebuilding or copying it.",
        AtlasPrimaryAction::Wait => "The current build owns the publication lane.",
    }
}

fn stage_label(stage: AtlasStage) -> &'static str {
    match stage {
        AtlasStage::Source => "NOTE",
        AtlasStage::Entities => "ENTITIES",
        AtlasStage::Connections => "GRAPH",
        AtlasStage::Review => "REVIEW",
        AtlasStage::Live => "LIVE",
    }
}

fn stage_tone(state: AtlasStageState) -> u32 {
    match state {
        AtlasStageState::Complete | AtlasStageState::Ready => READY,
        AtlasStageState::Waiting => WAITING,
        AtlasStageState::NeedsAttention => ATTENTION,
        AtlasStageState::Blocked => BLOCKED,
    }
}

fn atlas_control_error(error: &str) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(PAGE_BG))
        .child(
            div()
                .max_w(px(560.))
                .p_5()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x683e39))
                .bg(rgb(SURFACE))
                .child(kicker("ATLAS CONTROL BLOCKED", BLOCKED))
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(error.to_owned()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_use_plain_language() {
        assert_eq!(stage_label(AtlasStage::Source), "NOTE");
        assert_eq!(stage_label(AtlasStage::Entities), "ENTITIES");
        assert_eq!(
            primary_action_label(AtlasPrimaryAction::BuildGraph),
            "BUILD GRAPH"
        );
    }

    #[test]
    fn byte_labels_are_bounded_and_readable() {
        assert_eq!(format_bytes(54), "54 B");
        assert_eq!(format_bytes(2_048), "2.0 KiB");
    }
}
