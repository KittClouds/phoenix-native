//! Native, editor-owned toolbar for rendered text selections.

mod block_menu;
mod button;
mod entity_panel;
mod panel;

use std::ops::Range;
use std::time::Duration;

use gpui::{prelude::FluentBuilder as _, *};

use self::block_menu::BlockControl;
use super::{
    BlockCommand, BlockTransform, Editor, EditorEvent, EntityTagKind, EntityTagRequest,
    UndoSelectionSnapshot, ViewMode,
};
use crate::components::{
    Block, BlockKind, BlockRecord, BoldSelection, CodeSelection, ItalicSelection,
    SelectionLinkState, SelectionMarkState, StyleFlag, UnderlineSelection, UndoCaptureKind,
};
use crate::theme::Theme;

const TOOLBAR_WIDTH: f32 = 478.0;
const TOOLBAR_HEIGHT: f32 = 38.0;
const TOOLBAR_GAP: f32 = 10.0;
const BLOCK_MENU_WIDTH: f32 = 174.0;
const BLOCK_MENU_NATURAL_HEIGHT: f32 = 344.0;
const LINK_PANEL_WIDTH: f32 = 344.0;
const VIEWPORT_MARGIN: f32 = 8.0;
const MAX_LINK_BYTES: usize = 2_048;
const MAX_CUSTOM_KIND_BYTES: usize = 64;

const TOOLBAR_BG: u32 = 0x091310ff;
const TOOLBAR_BORDER: u32 = 0x245448ff;
const TOOLBAR_TEXT: u32 = 0xaac1baff;
const TOOLBAR_TEXT_ACTIVE: u32 = 0x78f5d3ff;
const TOOLBAR_ACTIVE_BG: u32 = 0x123d34ff;
const TOOLBAR_HOVER_BG: u32 = 0x102720ff;
const TOOLBAR_ERROR: u32 = 0xf48c83ff;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionIdentity {
    source: UndoSelectionSnapshot,
    document_revision: u64,
}

#[derive(Clone)]
struct SelectionSlice {
    block: Entity<Block>,
    current_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionCommand {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
    Link,
    Entity,
    Copy,
}

impl SelectionCommand {
    fn style_flag(self) -> Option<StyleFlag> {
        match self {
            Self::Bold => Some(StyleFlag::Bold),
            Self::Italic => Some(StyleFlag::Italic),
            Self::Underline => Some(StyleFlag::Underline),
            Self::Strikethrough => Some(StyleFlag::Strikethrough),
            Self::Code => Some(StyleFlag::Code),
            Self::Link | Self::Entity | Self::Copy => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SelectionFormatStates {
    bold: SelectionMarkState,
    italic: SelectionMarkState,
    underline: SelectionMarkState,
    strikethrough: SelectionMarkState,
    code: SelectionMarkState,
}

#[derive(Clone)]
struct SelectionLease {
    identity: SelectionIdentity,
    bounds: Bounds<Pixels>,
    formats: SelectionFormatStates,
    link_state: SelectionLinkState,
    can_link: bool,
    has_text_selection: bool,
    block_control: BlockControl,
}

#[derive(Default)]
pub(super) struct SelectionToolbarState {
    lease: Option<SelectionLease>,
    dismissed: Option<SelectionIdentity>,
    link_input: Option<Entity<Block>>,
    entity_panel_open: bool,
    block_menu_open: bool,
    custom_entity_input: Option<Entity<Block>>,
    entity_error: Option<SharedString>,
    pinned_block: Option<Entity<Block>>,
    link_error: Option<SharedString>,
}

impl Editor {
    fn clear_link_editor(&mut self, cx: &mut Context<Self>) -> bool {
        let had_input = self.selection_toolbar.link_input.take().is_some();
        let had_error = self.selection_toolbar.link_error.take().is_some();
        let pinned = self.selection_toolbar.pinned_block.take();
        if let Some(block) = pinned {
            block.update(cx, |block, cx| {
                if block.editor_selection_range.take().is_some() {
                    cx.notify();
                }
            });
        }
        had_input || had_error
    }

    fn clear_entity_editor(&mut self, _cx: &mut Context<Self>) -> bool {
        let changed = self.selection_toolbar.entity_panel_open
            || self.selection_toolbar.custom_entity_input.take().is_some()
            || self.selection_toolbar.entity_error.take().is_some();
        self.selection_toolbar.entity_panel_open = false;
        changed
    }

    fn clear_block_menu(&mut self) -> bool {
        std::mem::take(&mut self.selection_toolbar.block_menu_open)
    }

    pub(super) fn on_selection_changed(&mut self, cx: &mut Context<Self>) {
        let current_identity = self.current_selection_identity(cx);
        let preserve_entity_panel = self.selection_toolbar.entity_panel_open
            && self
                .selection_toolbar
                .lease
                .as_ref()
                .is_some_and(|lease| lease.identity == current_identity);
        let preserve_block_menu = self.selection_toolbar.block_menu_open
            && self
                .selection_toolbar
                .lease
                .as_ref()
                .is_some_and(|lease| lease.identity == current_identity);
        self.selection_toolbar.lease = None;
        self.selection_toolbar.dismissed = None;
        self.clear_link_editor(cx);
        if !preserve_block_menu {
            self.clear_block_menu();
        }
        if !preserve_entity_panel {
            self.clear_entity_editor(cx);
        }
        cx.notify();
    }

    pub(super) fn dismiss_selection_toolbar(&mut self, cx: &mut Context<Self>) {
        let identity = self
            .selection_toolbar
            .lease
            .as_ref()
            .map(|lease| lease.identity.clone());
        let changed = self.selection_toolbar.lease.take().is_some()
            || self.clear_link_editor(cx)
            || self.clear_entity_editor(cx)
            || self.clear_block_menu();
        self.selection_toolbar.dismissed = identity;
        self.sync_cross_block_selection_visuals(cx);
        if changed {
            cx.notify();
        }
    }

    fn current_selection_identity(&self, cx: &App) -> SelectionIdentity {
        SelectionIdentity {
            source: self.capture_source_selection_snapshot(cx),
            document_revision: self.document_revision,
        }
    }

    fn current_selection_slices(&self, cx: &App) -> Option<Vec<SelectionSlice>> {
        if self.view_mode != ViewMode::Rendered {
            return None;
        }

        if let Some(selection) = self.normalized_cross_block_selection(cx) {
            let visible = self.document.visible_blocks();
            let mut slices = Vec::with_capacity(selection.end_index - selection.start_index + 1);
            for index in selection.start_index..=selection.end_index {
                let block = visible.get(index)?.entity.clone();
                let block_ref = block.read(cx);
                if block_ref.uses_raw_text_editing() || block_ref.visible_len() == 0 {
                    return None;
                }
                let start = if index == selection.start_index {
                    selection.start.offset
                } else {
                    0
                };
                let end = if index == selection.end_index {
                    selection.end.offset
                } else {
                    block_ref.visible_len()
                };
                let range = start.min(end)..start.max(end);
                if !range.is_empty() {
                    slices.push(SelectionSlice {
                        block: block.clone(),
                        current_range: range,
                    });
                }
            }
            return (!slices.is_empty()).then_some(slices);
        }

        let block = self.current_edit_target_from_state(cx)?;
        let block_ref = block.read(cx);
        if block_ref.uses_raw_text_editing() || block_ref.selected_range.is_empty() {
            return None;
        }
        Some(vec![SelectionSlice {
            block: block.clone(),
            current_range: block_ref.selected_range.clone(),
        }])
    }

    fn current_toolbar_slices(&self, cx: &App) -> Option<(Vec<SelectionSlice>, bool)> {
        if let Some(slices) = self.current_selection_slices(cx) {
            return Some((slices, true));
        }
        if self.view_mode != ViewMode::Rendered || self.cross_block_selection.is_some() {
            return None;
        }

        let block = self.current_edit_target_from_state(cx)?;
        let block_ref = block.read(cx);
        if !block_ref.selected_range.is_empty() {
            return None;
        }
        Some((
            vec![SelectionSlice {
                block: block.clone(),
                current_range: block_ref.selected_range.clone(),
            }],
            false,
        ))
    }

    fn selection_bounds(slices: &[SelectionSlice], cx: &App) -> Option<Bounds<Pixels>> {
        let mut bounds: Option<Bounds<Pixels>> = None;
        for slice in slices {
            let block = slice.block.read(cx);
            let next = if slice.current_range.is_empty() {
                block.active_range_or_cursor_bounds()?
            } else {
                block.selection_bounds_for_range(slice.current_range.clone())?
            };
            bounds = Some(match bounds {
                None => next,
                Some(current) => Bounds::from_corners(
                    point(
                        current.left().min(next.left()),
                        current.top().min(next.top()),
                    ),
                    point(
                        current.right().max(next.right()),
                        current.bottom().max(next.bottom()),
                    ),
                ),
            });
        }
        bounds
    }

    fn merge_mark_state(
        aggregate: SelectionMarkState,
        next: SelectionMarkState,
    ) -> SelectionMarkState {
        match (aggregate, next) {
            (SelectionMarkState::Unavailable, state) => state,
            (state, SelectionMarkState::Unavailable) => state,
            (left, right) if left == right => left,
            _ => SelectionMarkState::Mixed,
        }
    }

    fn format_state(slices: &[SelectionSlice], flag: StyleFlag, cx: &App) -> SelectionMarkState {
        if slices.iter().any(|slice| slice.current_range.is_empty()) {
            return SelectionMarkState::Unavailable;
        }
        slices
            .iter()
            .fold(SelectionMarkState::Unavailable, |aggregate, slice| {
                let next = slice
                    .block
                    .read(cx)
                    .selection_style_state(slice.current_range.clone(), flag);
                Self::merge_mark_state(aggregate, next)
            })
    }

    fn selection_has_focus(&self, slices: &[SelectionSlice], window: &Window, cx: &App) -> bool {
        let block_focused = slices
            .iter()
            .any(|slice| slice.block.read(cx).focus_handle.is_focused(window));
        let link_focused = self
            .selection_toolbar
            .link_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle.is_focused(window));
        let entity_focused = self
            .selection_toolbar
            .custom_entity_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle.is_focused(window));
        block_focused || link_focused || entity_focused
    }

    fn build_selection_lease(
        &self,
        slices: &[SelectionSlice],
        has_text_selection: bool,
        bounds: Bounds<Pixels>,
        cx: &App,
    ) -> SelectionLease {
        let can_link = has_text_selection && slices.len() == 1;
        let link_state = if can_link {
            let slice = &slices[0];
            slice
                .block
                .read(cx)
                .selection_link_state(slice.current_range.clone())
        } else {
            SelectionLinkState::Unavailable
        };
        SelectionLease {
            identity: self.current_selection_identity(cx),
            bounds,
            formats: SelectionFormatStates {
                bold: Self::format_state(slices, StyleFlag::Bold, cx),
                italic: Self::format_state(slices, StyleFlag::Italic, cx),
                underline: Self::format_state(slices, StyleFlag::Underline, cx),
                strikethrough: Self::format_state(slices, StyleFlag::Strikethrough, cx),
                code: Self::format_state(slices, StyleFlag::Code, cx),
            },
            link_state,
            can_link,
            has_text_selection,
            block_control: self.block_control_for_slices(slices, cx),
        }
    }

    pub(super) fn sync_selection_toolbar(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((slices, has_text_selection)) = self.current_toolbar_slices(cx) else {
            self.selection_toolbar.lease = None;
            self.clear_link_editor(cx);
            self.clear_entity_editor(cx);
            self.clear_block_menu();
            self.selection_toolbar.dismissed = None;
            return;
        };
        if self.cross_block_drag.is_some()
            || slices
                .iter()
                .any(|slice| slice.block.read(cx).pointer_selection_active())
        {
            self.selection_toolbar.lease = None;
            return;
        }
        if !self.selection_has_focus(&slices, window, cx) {
            self.selection_toolbar.lease = None;
            self.clear_link_editor(cx);
            self.clear_entity_editor(cx);
            self.clear_block_menu();
            return;
        }

        let identity = self.current_selection_identity(cx);
        if self.selection_toolbar.dismissed.as_ref() == Some(&identity) {
            self.selection_toolbar.lease = None;
            return;
        }
        let Some(bounds) = Self::selection_bounds(&slices, cx) else {
            self.selection_toolbar.lease = None;
            return;
        };
        let viewport = self.scroll_handle.bounds();
        if bounds.bottom() < viewport.top()
            || bounds.top() > viewport.bottom()
            || bounds.right() < viewport.left()
            || bounds.left() > viewport.right()
        {
            self.selection_toolbar.lease = None;
            return;
        }

        let identity_changed = self
            .selection_toolbar
            .lease
            .as_ref()
            .is_some_and(|lease| lease.identity != identity);
        if identity_changed {
            self.clear_link_editor(cx);
            self.clear_entity_editor(cx);
            self.clear_block_menu();
        }
        self.selection_toolbar.lease =
            Some(self.build_selection_lease(&slices, has_text_selection, bounds, cx));
    }

    fn validated_selection_slices(
        &self,
        expected: &SelectionIdentity,
        cx: &App,
    ) -> Option<Vec<SelectionSlice>> {
        (self.current_selection_identity(cx) == *expected)
            .then(|| self.current_selection_slices(cx))
            .flatten()
    }

    fn execute_style_command(
        &mut self,
        command: SelectionCommand,
        expected: &SelectionIdentity,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(flag) = command.style_flag() else {
            return false;
        };
        let Some(slices) = self.validated_selection_slices(expected, cx) else {
            return false;
        };
        let state = Self::format_state(&slices, flag, cx);
        if state == SelectionMarkState::Unavailable {
            return false;
        }
        let enabled = state != SelectionMarkState::On;

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let mut changed = false;
        for slice in &slices {
            changed |= slice.block.update(cx, |block, cx| {
                let changed = block.set_selection_style(slice.current_range.clone(), flag, enabled);
                if changed {
                    cx.notify();
                }
                changed
            });
        }
        if changed {
            self.mark_dirty(cx);
            self.finalize_pending_undo_capture(cx);
            self.sync_cross_block_selection_visuals(cx);
            self.selection_toolbar.dismissed = None;
            self.selection_toolbar.link_error = None;
            cx.notify();
        } else {
            self.finalize_pending_undo_capture(cx);
        }
        changed
    }

    fn execute_link_command(
        &mut self,
        expected: &SelectionIdentity,
        destination: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(slices) = self.validated_selection_slices(expected, cx) else {
            return false;
        };
        let [slice] = slices.as_slice() else {
            return false;
        };

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let changed = slice.block.update(cx, |block, cx| {
            let changed = block.set_selection_link(slice.current_range.clone(), destination);
            if changed {
                cx.notify();
            }
            changed
        });
        if changed {
            self.mark_dirty(cx);
            self.finalize_pending_undo_capture(cx);
            self.clear_link_editor(cx);
            self.selection_toolbar.dismissed = None;
            self.sync_cross_block_selection_visuals(cx);
            cx.notify();
        } else {
            self.finalize_pending_undo_capture(cx);
        }
        changed
    }

    fn open_link_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lease) = self.selection_toolbar.lease.as_ref() else {
            return;
        };
        if !lease.can_link {
            return;
        }
        let initial = match &lease.link_state {
            SelectionLinkState::On(destination) => destination.clone(),
            _ => String::new(),
        };
        let input = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(initial)));
        input.read(cx).focus_handle.focus(window);
        self.selection_toolbar.link_input = Some(input);
        self.selection_toolbar.pinned_block = None;
        self.selection_toolbar.link_error = None;

        if let Some(slices) = self.current_selection_slices(cx)
            && let [slice] = slices.as_slice()
        {
            self.selection_toolbar.pinned_block = Some(slice.block.clone());
            slice.block.update(cx, |block, cx| {
                block.editor_selection_range = Some(slice.current_range.clone());
                cx.notify();
            });
        }
        cx.notify();
    }

    fn validate_link_destination(value: &str) -> Result<&str, &'static str> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("Enter a link destination.");
        }
        if trimmed.len() > MAX_LINK_BYTES {
            return Err("Link destination is too long.");
        }
        if trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err("Link destinations cannot contain whitespace.");
        }
        Ok(trimmed)
    }

    fn apply_link_editor(&mut self, cx: &mut Context<Self>) {
        let Some(lease) = self.selection_toolbar.lease.as_ref() else {
            return;
        };
        let identity = lease.identity.clone();
        let Some(input) = self.selection_toolbar.link_input.as_ref() else {
            return;
        };
        let value = input.read(cx).display_text().to_string();
        let destination = match Self::validate_link_destination(&value) {
            Ok(destination) => destination,
            Err(error) => {
                self.selection_toolbar.link_error = Some(error.into());
                cx.notify();
                return;
            }
        };
        let _ = self.execute_link_command(&identity, Some(destination), cx);
    }

    fn remove_selection_link(&mut self, cx: &mut Context<Self>) {
        let Some(identity) = self
            .selection_toolbar
            .lease
            .as_ref()
            .map(|lease| lease.identity.clone())
        else {
            return;
        };
        let _ = self.execute_link_command(&identity, None, cx);
    }

    fn copy_selection(&mut self, expected: &SelectionIdentity, cx: &mut Context<Self>) {
        if self.validated_selection_slices(expected, cx).is_none() {
            return;
        }
        if let Some(markdown) = self.selected_markdown_text(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        }
    }

    fn on_selection_toolbar_command(
        &mut self,
        command: SelectionCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(identity) = self
            .selection_toolbar
            .lease
            .as_ref()
            .map(|lease| lease.identity.clone())
        else {
            return;
        };
        if !self
            .selection_toolbar
            .lease
            .as_ref()
            .is_some_and(|lease| lease.has_text_selection)
        {
            return;
        }
        match command {
            SelectionCommand::Link => {
                self.clear_entity_editor(cx);
                cx.spawn_in(window, async move |editor, async_cx| {
                    Timer::after(Duration::from_millis(20)).await;
                    let _ = async_cx.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.current_selection_identity(cx) == identity {
                                editor.sync_selection_toolbar(window, cx);
                                editor.open_link_editor(window, cx);
                            }
                        })
                    });
                })
                .detach();
            }
            SelectionCommand::Entity => {
                self.clear_link_editor(cx);
                self.selection_toolbar.entity_panel_open = true;
                self.selection_toolbar.custom_entity_input = None;
                self.selection_toolbar.entity_error = None;
                cx.notify();
            }
            SelectionCommand::Copy => self.copy_selection(&identity, cx),
            _ => {
                let _ = self.execute_style_command(command, &identity, cx);
            }
        }
    }

    fn capture_cross_block_format(&mut self, command: SelectionCommand, cx: &mut Context<Self>) {
        if self.cross_block_selection.is_none() {
            cx.propagate();
            return;
        }
        let identity = self.current_selection_identity(cx);
        if self.execute_style_command(command, &identity, cx) {
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    pub(super) fn on_cross_block_bold(
        &mut self,
        _: &BoldSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_cross_block_format(SelectionCommand::Bold, cx);
    }

    pub(super) fn on_cross_block_italic(
        &mut self,
        _: &ItalicSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_cross_block_format(SelectionCommand::Italic, cx);
    }

    pub(super) fn on_cross_block_underline(
        &mut self,
        _: &UnderlineSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_cross_block_format(SelectionCommand::Underline, cx);
    }

    pub(super) fn on_cross_block_code(
        &mut self,
        _: &CodeSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_cross_block_format(SelectionCommand::Code, cx);
    }

    fn toolbar_position(
        lease: &SelectionLease,
        viewport: Bounds<Pixels>,
        panel_width: f32,
    ) -> Point<Pixels> {
        let selection_center =
            (f32::from(lease.bounds.left()) + f32::from(lease.bounds.right())) * 0.5;
        let min_x = f32::from(viewport.left()) + VIEWPORT_MARGIN;
        let max_x = (f32::from(viewport.right()) - panel_width - VIEWPORT_MARGIN).max(min_x);
        let x = (selection_center - panel_width * 0.5).clamp(min_x, max_x);
        let above = f32::from(lease.bounds.top()) - TOOLBAR_HEIGHT - TOOLBAR_GAP;
        let max_y = (f32::from(viewport.bottom()) - TOOLBAR_HEIGHT - VIEWPORT_MARGIN)
            .max(f32::from(viewport.top()) + VIEWPORT_MARGIN);
        let y = if above >= f32::from(viewport.top()) + VIEWPORT_MARGIN {
            above
        } else {
            f32::from(lease.bounds.bottom()) + TOOLBAR_GAP
        }
        .min(max_y);
        point(px(x), px(y))
    }

    fn toolbar_overlay_position(
        lease: &SelectionLease,
        viewport: Bounds<Pixels>,
        panel_width: f32,
        embedded: bool,
    ) -> Point<Pixels> {
        let window_position = Self::toolbar_position(lease, viewport, panel_width);
        if embedded {
            point(
                window_position.x - viewport.left(),
                window_position.y - viewport.top(),
            )
        } else {
            window_position
        }
    }

    pub(super) fn render_selection_toolbar_overlay(
        &self,
        _theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let lease = self.selection_toolbar.lease.as_ref()?;
        let viewport = self.scroll_handle.bounds();
        let available_width = (f32::from(viewport.size.width) - VIEWPORT_MARGIN * 2.0).max(1.0);
        let panel_width = TOOLBAR_WIDTH.min(available_width);
        let position =
            Self::toolbar_overlay_position(lease, viewport, panel_width, self.is_embedded());

        let link_state = if lease.can_link {
            match lease.link_state {
                SelectionLinkState::On(_) => SelectionMarkState::On,
                SelectionLinkState::Mixed => SelectionMarkState::Mixed,
                SelectionLinkState::Off => SelectionMarkState::Off,
                SelectionLinkState::Unavailable => SelectionMarkState::Unavailable,
            }
        } else {
            SelectionMarkState::Unavailable
        };

        let toolbar = div()
            .id("selection-toolbar")
            .absolute()
            .left(position.x)
            .top(position.y)
            .h(px(TOOLBAR_HEIGHT))
            .w(px(panel_width))
            .px(px(5.0))
            .flex()
            .items_center()
            .gap(px(3.0))
            .overflow_x_scroll()
            .occlude()
            .rounded(px(7.0))
            .border(px(1.0))
            .border_color(rgba(TOOLBAR_BORDER))
            .bg(rgba(TOOLBAR_BG))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.render_block_control(lease, cx))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(1.0))
                    .h(px(18.0))
                    .mx(px(2.0))
                    .bg(rgba(TOOLBAR_BORDER)),
            )
            .child(self.render_toolbar_button(
                "selection-bold",
                "B",
                SelectionCommand::Bold,
                lease.formats.bold,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-italic",
                "I",
                SelectionCommand::Italic,
                lease.formats.italic,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-underline",
                "U",
                SelectionCommand::Underline,
                lease.formats.underline,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-strike",
                "S",
                SelectionCommand::Strikethrough,
                lease.formats.strikethrough,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-code",
                "<>",
                SelectionCommand::Code,
                lease.formats.code,
                cx,
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(1.0))
                    .h(px(18.0))
                    .mx(px(2.0))
                    .bg(rgba(TOOLBAR_BORDER)),
            )
            .child(self.render_toolbar_button(
                "selection-link",
                "Link",
                SelectionCommand::Link,
                link_state,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-entity",
                "Tag",
                SelectionCommand::Entity,
                SelectionMarkState::Off,
                cx,
            ))
            .child(self.render_toolbar_button(
                "selection-copy",
                "Copy",
                SelectionCommand::Copy,
                SelectionMarkState::Off,
                cx,
            ));

        let root = div()
            .id("selection-toolbar-overlay")
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .left_0()
            .child(toolbar);
        let root = if let Some(link_panel) = self.render_link_panel(position, lease, cx) {
            root.child(link_panel)
        } else {
            root
        };
        let root = if let Some(entity_panel) = self.render_entity_panel(position, cx) {
            root.child(entity_panel)
        } else {
            root
        };
        let root = if let Some(block_menu) =
            self.render_block_menu(position, viewport, self.is_embedded(), cx)
        {
            root.child(block_menu)
        } else {
            root
        };
        Some(root.into_any_element())
    }
}

#[cfg(test)]
#[path = "selection_toolbar_tests.rs"]
mod tests;
