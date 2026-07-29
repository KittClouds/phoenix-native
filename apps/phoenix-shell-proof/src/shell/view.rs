use super::drawer::DRAWER_MIN_HEIGHT;
use super::{
    EditMode, PhoenixShell, BORDER, CANVAS, DANGER, LEFT_SIDEBAR_MAX_WIDTH, LEFT_SIDEBAR_MIN_WIDTH,
    RIGHT_SIDEBAR_MAX_WIDTH, RIGHT_SIDEBAR_MIN_WIDTH, SURFACE, TEXT, TEXT_MUTED,
};
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgb, Context, IntoElement,
    ParentElement, Render, SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, PixelsExt, Sizable};
use phoenix_scene_contract::HighlightMode;
use phoenix_workspace::{EntryKind, ROOT_ID};

const ACCENT_BRIGHT: u32 = 0x57e2bb;
const SHELL_HEADER_HEIGHT: f32 = 44.;
const CENTER_MIN_WIDTH: f32 = 480.;

impl PhoenixShell {
    fn render_nav_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(56.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .py_3()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x151918))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        Button::new("rail-files")
                            .label("F")
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.left_open = true;
                                cx.notify();
                            })),
                    )
                    .child(rail_button("rail-graph", "G", false))
                    .child(rail_button("rail-search", "S", false)),
            )
            .child(rail_button("rail-settings", "*", false))
    }

    fn render_workspace_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tree = div().flex_1().min_h_0().overflow_y_scrollbar().py_2();
        if let Some(snapshot) = self.kernel_snapshot() {
            let workspace = &snapshot.workspace;
            for row in workspace.visible_rows(&self.expanded) {
                let Some(entry) = workspace.entry(row.id) else {
                    continue;
                };
                let id = entry.id;
                let selected = id == snapshot.active_entry;
                let is_folder = entry.kind == EntryKind::Folder;
                let marker = if is_folder {
                    if self.expanded.contains(&id) {
                        "v"
                    } else {
                        ">"
                    }
                } else {
                    "-"
                };
                tree = tree.child(
                    div()
                        .id(("workspace-entry", id.0))
                        .h_8()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pl(px(10. + row.depth as f32 * 15.))
                        .pr_2()
                        .cursor_pointer()
                        .text_sm()
                        .text_color(if selected { rgb(TEXT) } else { rgb(0xa3b0ac) })
                        .when(selected, |item| {
                            item.bg(rgb(0x10332a))
                                .border_l_2()
                                .border_color(rgb(ACCENT_BRIGHT))
                        })
                        .hover(|item| item.bg(rgb(0x172522)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_entry(id, cx);
                        }))
                        .child(
                            div()
                                .w_3()
                                .text_xs()
                                .text_color(rgb(if is_folder { ACCENT_BRIGHT } else { TEXT_MUTED }))
                                .child(marker),
                        )
                        .child(entry.name.clone()),
                );
            }
        } else {
            tree = tree.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(rgb(DANGER))
                    .child("Workspace unavailable. Inspect the drawer for the exact error."),
            );
        }
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(linear_gradient(
                145.,
                linear_color_stop(rgb(0x0b2922), 0.),
                linear_color_stop(rgb(0x191b1b), 1.),
            ))
            .child(
                div()
                    .h(px(SHELL_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child("WORKSPACE"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new("new-note")
                                    .label("+ Note")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_edit_mode(EditMode::CreateNote, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("new-folder")
                                    .label("+ Folder")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_edit_mode(EditMode::CreateFolder, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(tree)
    }

    fn render_editor_surface(&self) -> impl IntoElement {
        let selected = self.selected_entry();
        let selected_name = selected
            .as_ref()
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "No selection".into());
        let is_note = selected.is_some_and(|entry| entry.kind == EntryKind::Note);
        let lease_label = self
            .editor_lease
            .as_ref()
            .map(|lease| {
                format!(
                    "KERNEL LEASE / REV {} / {}",
                    lease.revision.0,
                    &lease.content_hash.to_hex()[..12]
                )
            })
            .unwrap_or_else(|| "NO DOCUMENT LEASE".into());
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .h(px(SHELL_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ACCENT_BRIGHT))
                            .child(format!(
                                "{} / {selected_name}",
                                if is_note { "NOTE" } else { "FOLDER" }
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT_MUTED))
                            .child(lease_label),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(self.editor_lease.is_some(), |surface| {
                        surface.child(self.editor.clone())
                    })
                    .when(self.editor_lease.is_none(), |surface| {
                        surface.flex().items_center().justify_center().child(
                            div()
                                .text_sm()
                                .text_color(rgb(TEXT_MUTED))
                                .child("Select a note to acquire its kernel document lease."),
                        )
                    }),
            )
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.kernel_snapshot();
        let selected = self.selected_entry();
        let selected_name = selected
            .as_ref()
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let selected_kind = selected
            .as_ref()
            .map(|entry| entry.kind.label())
            .unwrap_or("unavailable");
        let active = self.active_entry();
        let selected_path = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.workspace.path_for(active).ok())
            .unwrap_or_else(|| "Unavailable".into());
        let delete_label = if self.delete_armed == Some(active) {
            "Confirm delete"
        } else {
            "Delete"
        };
        let writable = snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.shutting_down);
        let highlight_mode = snapshot
            .as_ref()
            .map(|snapshot| snapshot.style.highlight_mode)
            .unwrap_or(HighlightMode::Off);
        let workspace_revision = snapshot
            .as_ref()
            .map(|snapshot| snapshot.workspace.revision())
            .unwrap_or(0);
        let workspace_counts = snapshot
            .as_ref()
            .map(|snapshot| snapshot.workspace.counts());
        let workspace_composition = workspace_counts
            .map(|counts| format!("{} folders / {} notes", counts.folders, counts.notes))
            .unwrap_or_else(|| "unavailable".into());
        let document_revision = self
            .editor_lease
            .as_ref()
            .map(|lease| lease.revision.0)
            .unwrap_or(0);
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .h(px(SHELL_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("INSPECTOR"),
            )
            .child(
                div()
                    .p_4()
                    .child(inspector_value("NAME", selected_name))
                    .child(inspector_value("KIND", selected_kind))
                    .child(inspector_value("PATH", selected_path))
                    .child(inspector_value("STABLE ID", active.0.to_string()))
                    .child(inspector_value(
                        "AUTHORITY REVISIONS",
                        format!("workspace {workspace_revision} / document {document_revision}"),
                    ))
                    .child(inspector_value("WORKSPACE CONTENT", workspace_composition))
                    .child(
                        div()
                            .mt_6()
                            .pt_4()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ACCENT_BRIGHT))
                                    .child("GRAPH ANCHORS"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Paint-only semantic projection"),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("highlight-subtle")
                                            .label("Subtle")
                                            .small()
                                            .when(
                                                highlight_mode != HighlightMode::Subtle,
                                                |button| button.ghost(),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_highlight_mode(HighlightMode::Subtle, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("highlight-vivid")
                                            .label("Vivid")
                                            .small()
                                            .when(
                                                highlight_mode != HighlightMode::Vivid,
                                                |button| button.ghost(),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_highlight_mode(HighlightMode::Vivid, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("highlight-off")
                                            .label("Off")
                                            .small()
                                            .when(highlight_mode != HighlightMode::Off, |button| {
                                                button.ghost()
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_highlight_mode(HighlightMode::Off, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .mt_6()
                            .pt_4()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ACCENT_BRIGHT))
                                    .child(self.edit_mode.label()),
                            )
                            .child(div().mt_3().child(Input::new(&self.name_input).small()))
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("apply-edit")
                                            .label("Apply")
                                            .small()
                                            .disabled(!writable)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.commit_edit(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("rename-mode")
                                            .label("Rename")
                                            .small()
                                            .ghost()
                                            .disabled(!writable || active == ROOT_ID)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.set_edit_mode(EditMode::Rename, window, cx);
                                            })),
                                    ),
                            )
                            .child(
                                Button::new("delete-entry")
                                    .label(delete_label)
                                    .small()
                                    .danger()
                                    .mt_3()
                                    .disabled(!writable || active == ROOT_ID)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.delete_selected(cx);
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for PhoenixShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.schedule_proof(window, cx);
        let center = if self.drawer_layout.is_full_page() {
            self.render_drawer_surface(true, window, cx)
                .into_any_element()
        } else if self.drawer_layout.is_open() {
            let shell = cx.entity().clone();
            let drawer_height = self.drawer_layout.height();
            v_resizable("editor-drawer-split")
                .with_state(&self.drawer_resize_state)
                .child(
                    resizable_panel()
                        .size_range(px(0.)..gpui::Pixels::MAX)
                        .child(self.render_editor_surface()),
                )
                .child(
                    resizable_panel()
                        .size(px(drawer_height))
                        .size_range(px(DRAWER_MIN_HEIGHT)..gpui::Pixels::MAX)
                        .child(self.render_drawer_surface(false, window, cx)),
                )
                .on_resize(move |state, _, cx| {
                    let height = state.read(cx).sizes().get(1).map(|height| height.as_f32());
                    if let Some(height) = height {
                        shell.update(cx, |shell, _| {
                            shell.drawer_layout.set_height(height);
                        });
                    }
                })
                .into_any_element()
        } else {
            self.render_editor_surface().into_any_element()
        };
        let panel_group_id = match (self.left_open, self.right_open) {
            (true, true) => "shell-panels-both",
            (true, false) => "shell-panels-left",
            (false, true) => "shell-panels-right",
            (false, false) => "shell-panels-center",
        };
        let mut panels = h_resizable(panel_group_id);
        if self.left_open {
            panels = panels.child(
                resizable_panel()
                    .size(px(self.left_sidebar_width))
                    .size_range(px(LEFT_SIDEBAR_MIN_WIDTH)..px(LEFT_SIDEBAR_MAX_WIDTH))
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(self.render_workspace_sidebar(cx))
                            .child(self.render_left_footer(cx)),
                    ),
            );
        }
        panels = panels.child(
            resizable_panel()
                .size_range(px(CENTER_MIN_WIDTH)..gpui::Pixels::MAX)
                .child(
                    div()
                        .size_full()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .w_full()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .overflow_hidden()
                                .child(center),
                        )
                        .child(self.render_center_footer(cx)),
                ),
        );
        if self.right_open {
            panels = panels.child(
                resizable_panel()
                    .size(px(self.right_sidebar_width))
                    .size_range(px(RIGHT_SIDEBAR_MIN_WIDTH)..px(RIGHT_SIDEBAR_MAX_WIDTH))
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(self.render_inspector(cx))
                            .child(self.render_right_footer(cx)),
                    ),
            );
        }
        let shell = cx.entity().clone();
        let left_index = self.left_open.then_some(0);
        let right_index = self
            .right_open
            .then_some(if self.left_open { 2 } else { 1 });
        panels = panels.on_resize(move |state, _, cx| {
            let sizes = state.read(cx).sizes().clone();
            let left_width = left_index
                .and_then(|index| sizes.get(index))
                .map(|width| width.as_f32());
            let right_width = right_index
                .and_then(|index| sizes.get(index))
                .map(|width| width.as_f32());
            shell.update(cx, |shell, _| {
                if let Some(width) = left_width {
                    shell.left_sidebar_width =
                        width.clamp(LEFT_SIDEBAR_MIN_WIDTH, LEFT_SIDEBAR_MAX_WIDTH);
                }
                if let Some(width) = right_width {
                    shell.right_sidebar_width =
                        width.clamp(RIGHT_SIDEBAR_MIN_WIDTH, RIGHT_SIDEBAR_MAX_WIDTH);
                }
            });
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(!self.left_open, |layout| {
                        layout.child(self.render_nav_rail(cx))
                    })
                    .child(panels),
            )
    }
}

fn rail_button(id: &'static str, label: &'static str, selected: bool) -> impl IntoElement {
    Button::new(id)
        .label(label)
        .small()
        .ghost()
        .disabled(!selected)
}

fn inspector_value(label: &'static str, value: impl Into<SharedString>) -> impl IntoElement {
    div()
        .mt_4()
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(label))
        .child(
            div()
                .mt_1()
                .text_sm()
                .text_color(rgb(0xb8cbc5))
                .child(value.into()),
        )
}
