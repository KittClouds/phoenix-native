mod context_inspector;
mod history;
mod overview;
mod pipeline;
mod review;

use super::drawer::{ACCENT, ACCENT_DIM};
use super::{PhoenixShell, BORDER, BORDER_BRIGHT, SURFACE, TEXT, TEXT_MUTED};
use crate::lifecycle;
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgb, Context, FocusHandle,
    IntoElement, KeyDownEvent, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Sizable, StyledExt};
use phoenix_app_core::{
    AtlasBuildState, AtlasCandidateId, AtlasControlSnapshot, AtlasDecisionAction,
    AtlasDecisionCommand, AtlasPrimaryAction, AtlasReviewSnapshot, KernelCommand, KernelOutcome,
};
use std::sync::Arc;

pub(super) const PAGE_BG: u32 = 0x101413;
pub(super) const CARD_BG: u32 = 0x181d1b;
pub(super) const CARD_RAISED: u32 = 0x1d2421;
pub(super) const READY: u32 = 0x57e2bb;
pub(super) const WAITING: u32 = 0x77817e;
pub(super) const ATTENTION: u32 = 0xf0b45d;
pub(super) const BLOCKED: u32 = 0xeb776f;
pub(super) const BLUE: u32 = 0x7fb7ff;
const INSPECTOR_BREAKPOINT: f32 = 1_120.;
const NAVIGATION_RAIL_BREAKPOINT: f32 = 840.;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AtlasControlSection {
    #[default]
    Overview,
    Pipeline,
    Review,
    History,
}

impl AtlasControlSection {
    const ALL: [Self; 4] = [Self::Overview, Self::Pipeline, Self::Review, Self::History];

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "OVERVIEW",
            Self::Pipeline => "PIPELINE",
            Self::Review => "REVIEW",
            Self::History => "HISTORY",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Pipeline => 1,
            Self::Review => 2,
            Self::History => 3,
        }
    }
}

impl PhoenixShell {
    pub(super) fn render_atlas_control(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let control = self.kernel.atlas_control_snapshot();
        match control {
            Ok(control) if self.drawer_layout.is_full_page() => {
                self.render_atlas_control_full(control, window, cx)
            }
            Ok(control) => overview::render_compact(self, &control, cx),
            Err(error) => atlas_control_error(&error.to_string()).into_any_element(),
        }
    }

    fn render_atlas_control_full(
        &self,
        control: AtlasControlSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let available_width = f32::from(window.viewport_size().width)
            - if self.left_open {
                self.left_sidebar_width
            } else {
                0.
            }
            - if self.right_open {
                self.right_sidebar_width
            } else {
                0.
            };
        let show_inspector = available_width >= INSPECTOR_BREAKPOINT;
        let compact_navigation = available_width < NAVIGATION_RAIL_BREAKPOINT;
        let review = matches!(self.atlas_control_section, AtlasControlSection::Review)
            .then(|| self.kernel.atlas_review_snapshot())
            .transpose();
        let review = match review {
            Ok(value) => value.flatten(),
            Err(error) => {
                return atlas_control_error(&error.to_string()).into_any_element();
            }
        };
        let decisions = if matches!(self.atlas_control_section, AtlasControlSection::History) {
            match self.kernel.atlas_decision_receipts() {
                Ok(receipts) => Some(receipts),
                Err(error) => {
                    return atlas_control_error(&error.to_string()).into_any_element();
                }
            }
        } else {
            None
        };
        let active = match self.atlas_control_section {
            AtlasControlSection::Overview => overview::render_full(self, &control, cx),
            AtlasControlSection::Pipeline => pipeline::render(self, &control, cx),
            AtlasControlSection::Review => review::render(self, &control, review.as_ref(), cx),
            AtlasControlSection::History => {
                history::render(&control, decisions.as_deref().unwrap_or_default())
            }
        };
        let content = div()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .child(active);
        let layout = div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .key_context("PhoenixAtlasControl")
            .track_focus(&self.atlas_control_focus)
            .on_key_down(cx.listener(Self::on_atlas_key_down))
            .bg(linear_gradient(
                145.,
                linear_color_stop(rgb(0x08231d), 0.),
                linear_color_stop(rgb(PAGE_BG), 1.),
            ));
        if compact_navigation {
            layout
                .flex()
                .flex_col()
                .child(self.render_atlas_navigation(true, cx))
                .child(content)
                .into_any_element()
        } else {
            layout
                .flex()
                .child(self.render_atlas_navigation(false, cx))
                .child(content)
                .when(show_inspector, |layout| {
                    layout.child(context_inspector::render(
                        &control,
                        review.as_ref(),
                        self.atlas_selected_candidate,
                        cx.entity().clone(),
                    ))
                })
                .into_any_element()
        }
    }

    fn render_atlas_navigation(&self, compact: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        if compact {
            let mut nav = div()
                .w_full()
                .h(px(48.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .border_b_1()
                .border_color(rgb(BORDER))
                .bg(rgb(0x101816));
            for section in AtlasControlSection::ALL {
                let selected = section == self.atlas_control_section;
                nav = nav.child(
                    div()
                        .id(("atlas-section-compact", section.index()))
                        .min_w_0()
                        .flex_1()
                        .px_2()
                        .py_2()
                        .rounded_md()
                        .cursor_pointer()
                        .text_center()
                        .text_xs()
                        .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                        .when(selected, |item| {
                            item.bg(rgb(ACCENT_DIM))
                                .border_1()
                                .border_color(rgb(0x2f6758))
                        })
                        .hover(|item| item.bg(rgb(0x1b2421)).text_color(rgb(TEXT)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_atlas_section(section, window, cx);
                        }))
                        .child(section.label()),
                );
            }
            return nav.into_any_element();
        }

        let mut nav = div()
            .w(px(184.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x101816))
            .child(kicker("ATLAS CONTROL", ACCENT))
            .child(
                div()
                    .mt_1()
                    .mb_3()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("Native authority"),
            );
        for section in AtlasControlSection::ALL {
            let selected = section == self.atlas_control_section;
            nav = nav.child(
                div()
                    .id(("atlas-section", section.index()))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                    .when(selected, |item| {
                        item.bg(rgb(ACCENT_DIM))
                            .border_1()
                            .border_color(rgb(0x2f6758))
                    })
                    .hover(|item| item.bg(rgb(0x1b2421)).text_color(rgb(TEXT)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_atlas_section(section, window, cx);
                    }))
                    .child(section.label()),
            );
        }
        nav.child(
            div()
                .mt_auto()
                .text_xs()
                .text_color(rgb(0x66716d))
                .child("1–4 navigate  ·  ←→ sections"),
        )
        .into_any_element()
    }

    pub(super) fn focus_atlas_control(&self, window: &mut Window) {
        self.atlas_control_focus.focus(window);
    }

    fn select_atlas_section(
        &mut self,
        section: AtlasControlSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.atlas_control_section = section;
        self.focus_atlas_control(window);
        cx.notify();
    }

    fn on_atlas_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.is_held {
            return;
        }
        let key = event.keystroke.unparse();
        let section = match key.as_str() {
            "1" => Some(AtlasControlSection::Overview),
            "2" => Some(AtlasControlSection::Pipeline),
            "3" => Some(AtlasControlSection::Review),
            "4" => Some(AtlasControlSection::History),
            "left" => {
                Some(AtlasControlSection::ALL[self.atlas_control_section.index().saturating_sub(1)])
            }
            "right" => Some(
                AtlasControlSection::ALL[(self.atlas_control_section.index() + 1)
                    .min(AtlasControlSection::ALL.len() - 1)],
            ),
            _ => None,
        };
        if let Some(section) = section {
            cx.stop_propagation();
            self.select_atlas_section(section, window, cx);
            return;
        }
        if self.atlas_control_section == AtlasControlSection::Review {
            self.on_review_key(&key, window, cx);
        } else if self.atlas_control_section == AtlasControlSection::Pipeline && key == "r" {
            cx.stop_propagation();
            self.start_native_scene_rebuild(window, cx);
        }
    }

    fn on_review_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(Some(review)) = self.kernel.atlas_review_snapshot() else {
            return;
        };
        match key {
            "up" | "down" => {
                cx.stop_propagation();
                self.move_review_selection(&review, key == "down", window, cx);
            }
            "a" => self.dispatch_selected_review(&review, AtlasDecisionAction::Accept, window, cx),
            "r" => self.dispatch_selected_review(&review, AtlasDecisionAction::Reject, window, cx),
            "d" => self.dispatch_selected_review(&review, AtlasDecisionAction::Defer, window, cx),
            "u" => self.dispatch_selected_review(&review, AtlasDecisionAction::Undo, window, cx),
            "p" => {
                cx.stop_propagation();
                self.dispatch_atlas_kernel_command(
                    KernelCommand::PublishReviewedDecisions,
                    "PUBLISHING REVIEWED DECISIONS",
                    window,
                    cx,
                );
            }
            "escape" => {
                cx.stop_propagation();
                self.atlas_selected_candidate = None;
                cx.notify();
            }
            _ => {}
        }
    }

    fn move_review_selection(
        &mut self,
        review: &AtlasReviewSnapshot,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if review.candidates.is_empty() {
            self.atlas_selected_candidate = None;
            return;
        }
        let current = self
            .atlas_selected_candidate
            .and_then(|id| {
                review
                    .candidates
                    .iter()
                    .position(|candidate| candidate.candidate_id == id)
            })
            .unwrap_or(if forward {
                0
            } else {
                review.candidates.len() - 1
            });
        let next = if forward {
            (current + 1).min(review.candidates.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        self.select_atlas_candidate(review.candidates[next].candidate_id, window, cx);
    }

    pub(super) fn select_atlas_candidate(
        &mut self,
        candidate_id: AtlasCandidateId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self
            .kernel
            .execute(KernelCommand::SelectAtlasCandidate(candidate_id))
        {
            self.status = format!("ATLAS CANDIDATE BLOCKED / {error}").into();
            cx.notify();
            return;
        }
        let Ok(snapshot) = self.kernel.snapshot() else {
            self.status = "ATLAS CANDIDATE BLOCKED / SNAPSHOT".into();
            cx.notify();
            return;
        };
        let Some(evidence) = snapshot.graph_selection.evidence else {
            self.status = "ATLAS CANDIDATE BLOCKED / EVIDENCE MISSING".into();
            cx.notify();
            return;
        };
        let Some(lease) = self.editor_lease.as_ref() else {
            self.status = "ATLAS CANDIDATE BLOCKED / DOCUMENT LEASE MISSING".into();
            cx.notify();
            return;
        };
        if lease.entry_id.0 != evidence.document_id
            || lease.revision.0 != evidence.document_revision
            || lease.content_hash.0 != evidence.content_hash
        {
            self.status = "ATLAS CANDIDATE BLOCKED / DOCUMENT AUTHORITY CHANGED".into();
            cx.notify();
            return;
        }
        let Ok(start) = usize::try_from(evidence.start) else {
            self.status = "ATLAS CANDIDATE BLOCKED / EVIDENCE RANGE".into();
            cx.notify();
            return;
        };
        let Ok(end) = usize::try_from(evidence.end) else {
            self.status = "ATLAS CANDIDATE BLOCKED / EVIDENCE RANGE".into();
            cx.notify();
            return;
        };
        if !self
            .editor
            .update(cx, |editor, cx| editor.focus_source_range(start..end, cx))
        {
            self.status = "ATLAS CANDIDATE BLOCKED / EVIDENCE RANGE".into();
            cx.notify();
            return;
        }
        self.atlas_selected_candidate = Some(candidate_id);
        if let Some(host) = self.graph.borrow().as_ref() {
            if let Err(error) = host.sync_kernel_state() {
                lifecycle::mark_proof_failed();
                self.status = format!("ATLAS CANDIDATE / GRAPH SYNC {error:#}").into();
                cx.notify();
                return;
            }
        }
        self.status = "ATLAS CANDIDATE / EXACT EVIDENCE FOCUSED".into();
        cx.notify();
    }

    fn dispatch_selected_review(
        &mut self,
        review: &AtlasReviewSnapshot,
        action: AtlasDecisionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let candidate = self
            .atlas_selected_candidate
            .and_then(|id| {
                review
                    .candidates
                    .iter()
                    .find(|item| item.candidate_id == id)
            })
            .or_else(|| review.candidates.first());
        let Some(candidate) = candidate else {
            return;
        };
        cx.stop_propagation();
        self.atlas_selected_candidate = Some(candidate.candidate_id);
        self.dispatch_review_action(
            candidate.candidate_id,
            candidate.expected_receipt_id,
            action,
            window,
            cx,
        );
    }

    pub(super) fn dispatch_review_action(
        &mut self,
        candidate_id: AtlasCandidateId,
        expected_receipt_id: Option<[u8; 32]>,
        action: AtlasDecisionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reason = match action {
            AtlasDecisionAction::Accept => "accepted in Atlas Control",
            AtlasDecisionAction::Reject => "rejected in Atlas Control",
            AtlasDecisionAction::Defer => "deferred in Atlas Control",
            AtlasDecisionAction::Undo => "undone in Atlas Control",
        };
        self.atlas_selected_candidate = Some(candidate_id);
        self.dispatch_atlas_kernel_command(
            KernelCommand::ReviewAtlasCandidate(Box::new(AtlasDecisionCommand {
                candidate_id,
                action,
                expected_receipt_id,
                reason: reason.to_owned(),
            })),
            "SAVING DURABLE REVIEW",
            window,
            cx,
        );
    }

    pub(super) fn dispatch_atlas_action(
        &mut self,
        action: AtlasPrimaryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            AtlasPrimaryAction::RunPipeline => self.start_native_scene_rebuild(window, cx),
            AtlasPrimaryAction::OpenDocument => {
                if self.drawer_layout.is_open() {
                    self.toggle_drawer(cx);
                }
                self.status = "ATLAS / SELECT A NOTE".into();
                cx.notify();
            }
            AtlasPrimaryAction::ConfigurePipeline | AtlasPrimaryAction::Wait => {}
        }
    }

    pub(super) fn dispatch_atlas_kernel_command(
        &mut self,
        command: KernelCommand,
        pending: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = pending.into();
        cx.notify();
        let kernel = Arc::clone(&self.kernel);
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |shell, async_cx| {
            let result = background
                .spawn(async move { kernel.execute(command) })
                .await;
            if let Err(error) = shell.update(async_cx, |this, cx| {
                match result {
                    Ok(receipt) => {
                        if let KernelOutcome::GraphRebuilt(graph) = receipt.outcome {
                            if let Some(host) = this.graph.borrow().as_ref() {
                                if let Err(error) = host.sync_kernel_state() {
                                    lifecycle::mark_proof_failed();
                                    this.status =
                                        format!("ATLAS PUBLISHED / PRESENTATION {error:#}").into();
                                    cx.notify();
                                    return;
                                }
                            }
                            this.status =
                                format!("ATLAS PUBLISHED / G{}", graph.publication.generation_id)
                                    .into();
                        } else {
                            this.status =
                                format!("ATLAS COMMAND COMPLETE / S{}", receipt.sequence).into();
                        }
                    }
                    Err(error) => {
                        this.status = format!("ATLAS COMMAND BLOCKED / {error}").into();
                    }
                }
                cx.notify();
            }) {
                lifecycle::mark_proof_failed();
                eprintln!("PHOENIX_ATLAS_COMMAND_DELIVERY_FAILED {error:#}");
            }
        })
        .detach();
    }
}

pub(super) fn build_tone(state: AtlasBuildState) -> u32 {
    match state {
        AtlasBuildState::Published => READY,
        AtlasBuildState::Ready | AtlasBuildState::VerificationRequired => BLUE,
        AtlasBuildState::Building => ATTENTION,
        AtlasBuildState::Cancelled => WAITING,
        AtlasBuildState::Failed => BLOCKED,
        AtlasBuildState::WaitingForDocument | AtlasBuildState::RuntimeUnavailable => WAITING,
    }
}

pub(super) fn build_state_label(state: AtlasBuildState) -> &'static str {
    match state {
        AtlasBuildState::WaitingForDocument => "EMPTY",
        AtlasBuildState::RuntimeUnavailable => "UNSUPPORTED",
        AtlasBuildState::Ready => "READY",
        AtlasBuildState::Building => "RUNNING",
        AtlasBuildState::Published => "CURRENT",
        AtlasBuildState::VerificationRequired => "STALE",
        AtlasBuildState::Cancelled => "CANCELLED",
        AtlasBuildState::Failed => "FAILED",
    }
}

pub(super) fn primary_action_label(action: AtlasPrimaryAction) -> &'static str {
    match action {
        AtlasPrimaryAction::OpenDocument => "CHOOSE A NOTE",
        AtlasPrimaryAction::ConfigurePipeline => "CONNECT RUNTIME",
        AtlasPrimaryAction::RunPipeline => "RUN PIPELINE",
        AtlasPrimaryAction::Wait => "PIPELINE RUNNING",
    }
}

pub(super) fn kicker(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(rgb(color))
        .child(label)
}

pub(super) fn short_hash(hash: [u8; 32]) -> String {
    hash[..6].iter().map(|byte| format!("{byte:02x}")).collect()
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

pub(super) fn status_badge(state: AtlasBuildState) -> impl IntoElement {
    let tone = build_tone(state);
    div()
        .px_2()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(rgb(tone))
        .bg(rgb(0x151a18))
        .text_xs()
        .font_semibold()
        .text_color(rgb(tone))
        .child(build_state_label(state))
}

pub(super) fn action_button(
    shell: &PhoenixShell,
    action: AtlasPrimaryAction,
    cx: &mut Context<PhoenixShell>,
) -> impl IntoElement {
    let enabled = !matches!(
        action,
        AtlasPrimaryAction::Wait | AtlasPrimaryAction::ConfigurePipeline
    ) && !shell.graph_rebuild_pending;
    Button::new("atlas-primary-action")
        .label(primary_action_label(action))
        .small()
        .primary()
        .disabled(!enabled)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.dispatch_atlas_action(action, window, cx);
        }))
}

pub(super) fn focus_handle(cx: &mut Context<PhoenixShell>) -> FocusHandle {
    cx.focus_handle()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_order_is_stable() {
        assert_eq!(AtlasControlSection::ALL[0], AtlasControlSection::Overview);
        assert_eq!(AtlasControlSection::ALL[3], AtlasControlSection::History);
    }

    #[test]
    fn every_build_state_has_distinct_copy() {
        let states = [
            AtlasBuildState::WaitingForDocument,
            AtlasBuildState::RuntimeUnavailable,
            AtlasBuildState::Ready,
            AtlasBuildState::Building,
            AtlasBuildState::Published,
            AtlasBuildState::VerificationRequired,
            AtlasBuildState::Cancelled,
            AtlasBuildState::Failed,
        ];
        let mut labels = states.map(build_state_label);
        labels.sort_unstable();
        assert!(labels.windows(2).all(|pair| pair[0] != pair[1]));
    }
}
