use super::drawer::ACCENT;
use super::{PhoenixShell, BORDER, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{Sizable, StyledExt};
use phoenix_app_core::{KernelCommand, KernelOutcome};
use phoenix_scene_contract::EntityKind;
use phoenix_workspace::RegistryEntityDraft;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RegistryEditorMode {
    Create,
    Edit(u64),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RegistryEditorState {
    mode: RegistryEditorMode,
    kind: EntityKind,
    delete_armed: bool,
}

impl PhoenixShell {
    pub(super) fn open_registry_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.entity_name_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.entity_editor = Some(RegistryEditorState {
            mode: RegistryEditorMode::Create,
            kind: EntityKind::Character,
            delete_armed: false,
        });
        cx.notify();
    }

    pub(super) fn open_registry_edit(
        &mut self,
        entity_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entity) = self.kernel_snapshot().and_then(|snapshot| {
            snapshot
                .atlas_registry
                .entities
                .iter()
                .find(|entity| entity.stable_id == entity_id)
                .cloned()
        }) else {
            self.status = "REGISTRY EDIT BLOCKED / ENTITY NOT FOUND".into();
            cx.notify();
            return;
        };
        self.entity_name_input.update(cx, |input, cx| {
            input.set_value(entity.label.to_string(), window, cx)
        });
        self.entity_editor = Some(RegistryEditorState {
            mode: RegistryEditorMode::Edit(entity_id),
            kind: entity.kind,
            delete_armed: false,
        });
        cx.notify();
    }

    pub(super) fn render_registry_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.entity_editor.expect("editor is open");
        let title = match state.mode {
            RegistryEditorMode::Create => "ADD ENTITY",
            RegistryEditorMode::Edit(_) => "EDIT ENTITY",
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000aa))
            .child(
                div()
                    .w(px(560.))
                    .max_w_full()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(0x3c4542))
                    .bg(rgb(0x1a1d1c))
                    .shadow_lg()
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .text_color(rgb(TEXT))
                                    .child(title),
                            )
                            .child(
                                Button::new("registry-editor-close")
                                    .label("X")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.entity_editor = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .child(div().text_sm().text_color(rgb(TEXT_MUTED)).child("NAME"))
                            .child(div().mt_2().child(Input::new(&self.entity_name_input)))
                            .child(
                                div()
                                    .mt_4()
                                    .text_sm()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("ENTITY KIND"),
                            )
                            .child(kind_grid(state.kind, cx)),
                    )
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .when(
                                matches!(state.mode, RegistryEditorMode::Edit(_)),
                                |footer| {
                                    footer.child(
                                        Button::new("registry-editor-delete")
                                            .label(if state.delete_armed {
                                                "CONFIRM DELETE"
                                            } else {
                                                "DELETE"
                                            })
                                            .small()
                                            .danger()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.delete_registry_entity(window, cx);
                                            })),
                                    )
                                },
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new("registry-editor-cancel")
                                            .label("CANCEL")
                                            .small()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.entity_editor = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("registry-editor-save")
                                            .label("SAVE CHANGES")
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_registry_entity(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
    }

    fn save_registry_entity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.entity_editor else {
            return;
        };
        let label = self.entity_name_input.read(cx).value().trim().to_owned();
        if label.is_empty() {
            self.status = "REGISTRY EDIT BLOCKED / NAME REQUIRED".into();
            cx.notify();
            return;
        }
        let draft = RegistryEntityDraft {
            label,
            kind: state.kind,
            custom_kind: (state.kind == EntityKind::Custom).then(|| "ENTITY".to_owned()),
            origin_document: None,
        };
        let command = match state.mode {
            RegistryEditorMode::Create => KernelCommand::CreateRegistryEntity(draft),
            RegistryEditorMode::Edit(entity_id) => {
                KernelCommand::UpdateRegistryEntity { entity_id, draft }
            }
        };
        self.finish_registry_edit(command, window, cx);
    }

    fn delete_registry_entity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut state) = self.entity_editor else {
            return;
        };
        let RegistryEditorMode::Edit(entity_id) = state.mode else {
            return;
        };
        if !state.delete_armed {
            state.delete_armed = true;
            self.entity_editor = Some(state);
            self.status = "REGISTRY DELETE / CONFIRM".into();
            cx.notify();
            return;
        }
        self.finish_registry_edit(KernelCommand::DeleteRegistryEntity(entity_id), window, cx);
    }

    fn finish_registry_edit(
        &mut self,
        command: KernelCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.kernel.execute(command) {
            Ok(receipt) => match receipt.outcome {
                KernelOutcome::RegistryEntityEdited(result) => {
                    self.entity_editor = None;
                    self.apply_kernel_highlights(cx);
                    self.status = format!(
                        "REGISTRY REV {} / GRAPH REFRESH QUEUED",
                        result.registry_revision
                    )
                    .into();
                    self.start_registry_graph_refresh(window, cx);
                }
                _ => self.status = "REGISTRY EDIT BLOCKED / RECEIPT MISMATCH".into(),
            },
            Err(error) => self.status = format!("REGISTRY EDIT BLOCKED / {error}").into(),
        }
        cx.notify();
    }
}

fn kind_grid(selected: EntityKind, cx: &mut Context<PhoenixShell>) -> impl IntoElement {
    let mut grid = div().mt_2().grid().grid_cols(3).gap_2();
    for kind in EntityKind::TOOLBAR {
        grid = grid.child(
            div()
                .id(("registry-kind", kind as usize))
                .px_3()
                .py_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(if kind == selected { ACCENT } else { BORDER }))
                .bg(rgb(if kind == selected { 0x143b32 } else { 0x131716 }))
                .cursor_pointer()
                .text_center()
                .text_sm()
                .text_color(rgb(if kind == selected { TEXT } else { TEXT_MUTED }))
                .hover(|tile| tile.bg(rgb(0x1b2622)).text_color(rgb(TEXT)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.entity_editor.as_mut() {
                        state.kind = kind;
                        state.delete_armed = false;
                    }
                    cx.notify();
                }))
                .child(kind.label().to_uppercase()),
        );
    }
    grid
}
