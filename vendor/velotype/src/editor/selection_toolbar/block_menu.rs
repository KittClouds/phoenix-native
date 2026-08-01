use super::*;

const BLOCK_CONTROL_WIDTH: f32 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlockControlState {
    Paragraph,
    Heading(u8),
    BulletedList,
    NumberedList,
    TaskList,
    Quote,
    Callout,
    Mixed,
    Incompatible,
}

impl BlockControlState {
    fn from_kind(kind: &BlockKind) -> Self {
        match kind {
            BlockKind::Paragraph => Self::Paragraph,
            BlockKind::Heading { level } if (1..=6).contains(level) => Self::Heading(*level),
            BlockKind::BulletedListItem => Self::BulletedList,
            BlockKind::NumberedListItem => Self::NumberedList,
            BlockKind::TaskListItem { .. } => Self::TaskList,
            BlockKind::Quote => Self::Quote,
            BlockKind::Callout(_) => Self::Callout,
            _ => Self::Incompatible,
        }
    }

    fn label(self) -> SharedString {
        match self {
            Self::Paragraph => "Paragraph".into(),
            Self::Heading(level) => format!("Heading {level}").into(),
            Self::BulletedList => "Bulleted list".into(),
            Self::NumberedList => "Numbered list".into(),
            Self::TaskList => "Task list".into(),
            Self::Quote => "Quote".into(),
            Self::Callout => "Callout".into(),
            Self::Mixed => "Mixed blocks".into(),
            Self::Incompatible => "Unavailable".into(),
        }
    }

    fn can_open(self) -> bool {
        !matches!(self, Self::Mixed | Self::Incompatible)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BlockControl {
    pub(super) state: BlockControlState,
    pub(super) target: Option<EntityId>,
}

impl Editor {
    pub(super) fn block_control_for_slices(
        &self,
        slices: &[SelectionSlice],
        cx: &App,
    ) -> BlockControl {
        let mut target = None;
        for slice in slices {
            let root = self.root_ancestor_entity_id(slice.block.entity_id());
            if target.is_some_and(|current| current != root) {
                return BlockControl {
                    state: BlockControlState::Mixed,
                    target: None,
                };
            }
            target = Some(root);
        }

        let Some(target) = target else {
            return BlockControl {
                state: BlockControlState::Incompatible,
                target: None,
            };
        };
        let state = self
            .document
            .block_entity_by_id(target)
            .map(|block| BlockControlState::from_kind(&block.read(cx).kind()))
            .unwrap_or(BlockControlState::Incompatible);
        BlockControl {
            target: state.can_open().then_some(target),
            state,
        }
    }

    pub(super) fn toggle_block_menu(&mut self, cx: &mut Context<Self>) {
        let can_open = self
            .selection_toolbar
            .lease
            .as_ref()
            .is_some_and(|lease| lease.block_control.target.is_some());
        if !can_open {
            self.selection_toolbar.block_menu_open = false;
            return;
        }
        self.clear_link_editor(cx);
        self.clear_entity_editor(cx);
        self.selection_toolbar.block_menu_open ^= true;
        cx.notify();
    }

    fn apply_toolbar_block_transform(&mut self, transform: BlockTransform, cx: &mut Context<Self>) {
        let Some((identity, target)) = self.selection_toolbar.lease.as_ref().and_then(|lease| {
            lease
                .block_control
                .target
                .map(|target| (lease.identity.clone(), target))
        }) else {
            return;
        };
        if self.current_selection_identity(cx) != identity {
            self.selection_toolbar.block_menu_open = false;
            cx.notify();
            return;
        }

        let _ = self.execute_block_command(BlockCommand::Transform { target, transform }, cx);
        self.selection_toolbar.block_menu_open = false;
        self.selection_toolbar.dismissed = None;
        cx.notify();
    }

    pub(super) fn render_block_control(
        &self,
        lease: &SelectionLease,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = lease.block_control.target.is_some();
        div()
            .id("selection-block-control")
            .flex_shrink_0()
            .w(px(BLOCK_CONTROL_WIDTH))
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(5.0))
            .border(px(1.0))
            .border_color(rgba(TOOLBAR_BORDER))
            .bg(if self.selection_toolbar.block_menu_open {
                rgba(TOOLBAR_ACTIVE_BG)
            } else {
                rgba(TOOLBAR_BG)
            })
            .text_size(px(11.0))
            .text_color(if enabled {
                rgba(TOOLBAR_TEXT_ACTIVE)
            } else {
                rgba(TOOLBAR_TEXT)
            })
            .opacity(if enabled { 1.0 } else { 0.52 })
            .when(enabled, |control| {
                control
                    .cursor_pointer()
                    .hover(|control| control.bg(rgba(TOOLBAR_HOVER_BG)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, _, _, cx| {
                            editor.toggle_block_menu(cx);
                            cx.stop_propagation();
                        }),
                    )
            })
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(lease.block_control.state.label()),
            )
            .child("v")
            .into_any_element()
    }

    fn render_block_menu_row(
        &self,
        id: &'static str,
        label: &'static str,
        transform: BlockTransform,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_shrink_0()
            .h(px(28.0))
            .px(px(9.0))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(5.0))
            .cursor_pointer()
            .bg(if active {
                rgba(TOOLBAR_ACTIVE_BG)
            } else {
                rgba(TOOLBAR_BG)
            })
            .text_size(px(11.0))
            .text_color(if active {
                rgba(TOOLBAR_TEXT_ACTIVE)
            } else {
                rgba(TOOLBAR_TEXT)
            })
            .hover(|row| {
                row.bg(rgba(TOOLBAR_HOVER_BG))
                    .text_color(rgba(TOOLBAR_TEXT_ACTIVE))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, _, _, cx| {
                    editor.apply_toolbar_block_transform(transform, cx);
                    cx.stop_propagation();
                }),
            )
            .child(label)
            .when(active, |row| row.child("*"))
            .into_any_element()
    }

    pub(super) fn render_block_menu(
        &self,
        toolbar_position: Point<Pixels>,
        viewport: Bounds<Pixels>,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.selection_toolbar.block_menu_open {
            return None;
        }
        let lease = self.selection_toolbar.lease.as_ref()?;
        let current = lease.block_control.state;
        let available_height = (f32::from(viewport.size.height) - VIEWPORT_MARGIN * 2.0).max(1.0);
        let menu_height = BLOCK_MENU_NATURAL_HEIGHT.min(available_height);
        let toolbar_top = if embedded {
            f32::from(toolbar_position.y + viewport.top())
        } else {
            f32::from(toolbar_position.y)
        };
        let below = toolbar_top + TOOLBAR_HEIGHT + 5.0;
        let global_y = if below + menu_height <= f32::from(viewport.bottom()) - VIEWPORT_MARGIN {
            below
        } else {
            (toolbar_top - menu_height - 5.0).max(f32::from(viewport.top()) + VIEWPORT_MARGIN)
        };
        let y = if embedded {
            px(global_y) - viewport.top()
        } else {
            px(global_y)
        };

        let rows = [
            (
                "selection-block-paragraph",
                "Paragraph",
                BlockTransform::Paragraph,
                current == BlockControlState::Paragraph,
            ),
            (
                "selection-block-h1",
                "Heading 1",
                BlockTransform::Heading(1),
                current == BlockControlState::Heading(1),
            ),
            (
                "selection-block-h2",
                "Heading 2",
                BlockTransform::Heading(2),
                current == BlockControlState::Heading(2),
            ),
            (
                "selection-block-h3",
                "Heading 3",
                BlockTransform::Heading(3),
                current == BlockControlState::Heading(3),
            ),
            (
                "selection-block-h4",
                "Heading 4",
                BlockTransform::Heading(4),
                current == BlockControlState::Heading(4),
            ),
            (
                "selection-block-h5",
                "Heading 5",
                BlockTransform::Heading(5),
                current == BlockControlState::Heading(5),
            ),
            (
                "selection-block-h6",
                "Heading 6",
                BlockTransform::Heading(6),
                current == BlockControlState::Heading(6),
            ),
            (
                "selection-block-bulleted",
                "Bulleted list",
                BlockTransform::BulletedList,
                current == BlockControlState::BulletedList,
            ),
            (
                "selection-block-numbered",
                "Numbered list",
                BlockTransform::NumberedList,
                current == BlockControlState::NumberedList,
            ),
            (
                "selection-block-task",
                "Task list",
                BlockTransform::TaskList,
                current == BlockControlState::TaskList,
            ),
            (
                "selection-block-quote",
                "Quote",
                BlockTransform::Quote,
                current == BlockControlState::Quote,
            ),
        ];

        Some(
            div()
                .id("selection-block-menu")
                .absolute()
                .left(toolbar_position.x)
                .top(y)
                .w(px(BLOCK_MENU_WIDTH))
                .h(px(menu_height))
                .p(px(6.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .overflow_y_scroll()
                .occlude()
                .rounded(px(7.0))
                .border(px(1.0))
                .border_color(rgba(TOOLBAR_BORDER))
                .bg(rgba(TOOLBAR_BG))
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(rows.into_iter().map(|(id, label, transform, active)| {
                    self.render_block_menu_row(id, label, transform, active, cx)
                }))
                .into_any_element(),
        )
    }
}
